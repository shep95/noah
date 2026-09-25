use anyhow::{Result, bail};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};
use walkdir::{DirEntry, WalkDir};

const PREFIX_BYTES: u64 = 64 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Directory names skipped anywhere: dependency and build output folders that are full of
/// legitimately identical files.
const SKIPPED_DIRECTORY_NAMES: [&str; 3] = ["node_modules", "target", "__pycache__"];

/// Operating system folders skipped when they sit directly at the top of a drive.
const SKIPPED_SYSTEM_DIRECTORIES: [&str; 6] = [
    "Windows",
    "Program Files",
    "Program Files (x86)",
    "ProgramData",
    "$Recycle.Bin",
    "System Volume Information",
];

#[cfg(unix)]
const SKIPPED_UNIX_PATHS: [&str; 15] = [
    "/proc", "/sys", "/dev", "/run", "/boot", "/bin", "/sbin", "/lib", "/lib64", "/usr", "/etc",
    "/var", "/System", "/Library", "/private",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateFile {
    pub path: PathBuf,
    pub modified: SystemTime,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateGroup {
    pub size: u64,
    /// Sorted newest first; `files[0]` is the one to keep.
    pub files: Vec<DuplicateFile>,
}

impl DuplicateGroup {
    fn new(size: u64, mut files: Vec<DuplicateFile>) -> Self {
        files.sort_by(|left, right| {
            right
                .modified
                .cmp(&left.modified)
                .then_with(|| left.path.cmp(&right.path))
        });
        Self { size, files }
    }

    pub fn keep(&self) -> &DuplicateFile {
        // `find_duplicates` only builds groups with at least two files.
        &self.files[0]
    }

    pub fn older(&self) -> &[DuplicateFile] {
        self.files.get(1..).unwrap_or_default()
    }

    pub fn reclaimable_bytes(&self) -> u64 {
        self.size.saturating_mul(self.older().len() as u64)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanProgress {
    pub files_seen: usize,
    pub bytes_hashed: u64,
}

/// Blocking. Walks `roots` without following symlinks, skipping hidden folders, dependency and
/// build folders, and operating system folders. Files smaller than `min_size` (and empty files)
/// are ignored. Candidates are grouped by size, then by the SHA-256 of their first 64 KiB, then by
/// their full SHA-256. When `cancel` is set, returns promptly with the groups confirmed so far.
/// Groups are sorted by reclaimable bytes, largest first.
pub fn find_duplicates(
    roots: &[PathBuf],
    min_size: u64,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(ScanProgress),
) -> Result<Vec<DuplicateGroup>> {
    let mut scanner = Scanner {
        cancel,
        progress,
        state: ScanProgress::default(),
        last_report: Instant::now(),
    };

    let by_size = scanner.collect_candidates(roots, min_size.max(1))?;
    let mut size_groups: Vec<(u64, Vec<DuplicateFile>)> = by_size
        .into_iter()
        .filter(|(_, files)| files.len() > 1)
        .collect();
    // Biggest potential savings first, so a cancelled scan still returns the most useful groups.
    size_groups.sort_by(|left, right| {
        let left_potential = left.0.saturating_mul(left.1.len() as u64 - 1);
        let right_potential = right.0.saturating_mul(right.1.len() as u64 - 1);
        right_potential
            .cmp(&left_potential)
            .then(left.0.cmp(&right.0))
    });

    let mut groups = Vec::new();
    'sizes: for (size, files) in size_groups {
        let Some(prefix_groups) = scanner.group_by_hash(files, Some(PREFIX_BYTES)) else {
            break;
        };
        for candidates in prefix_groups {
            let confirmed = if size <= PREFIX_BYTES {
                vec![candidates]
            } else {
                match scanner.group_by_hash(candidates, None) {
                    Some(confirmed) => confirmed,
                    None => break 'sizes,
                }
            };
            groups.extend(
                confirmed
                    .into_iter()
                    .map(|files| DuplicateGroup::new(size, files)),
            );
        }
    }

    groups.sort_by(|left, right| {
        right
            .reclaimable_bytes()
            .cmp(&left.reclaimable_bytes())
            .then_with(|| left.keep().path.cmp(&right.keep().path))
    });
    scanner.report(true);
    Ok(groups)
}

struct Scanner<'a> {
    cancel: &'a AtomicBool,
    progress: &'a mut dyn FnMut(ScanProgress),
    state: ScanProgress,
    last_report: Instant,
}

impl Scanner<'_> {
    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn report(&mut self, force: bool) {
        if force || self.last_report.elapsed() >= PROGRESS_INTERVAL {
            self.last_report = Instant::now();
            (self.progress)(self.state);
        }
    }

    fn collect_candidates(
        &mut self,
        roots: &[PathBuf],
        min_size: u64,
    ) -> Result<HashMap<u64, Vec<DuplicateFile>>> {
        let roots = outermost_roots(roots);
        let mut by_size: HashMap<u64, Vec<DuplicateFile>> = HashMap::new();
        let mut seen_paths = HashSet::new();
        #[cfg(unix)]
        let mut seen_inodes = HashSet::new();
        let mut readable_roots = 0;

        for root in &roots {
            if !root.is_dir() {
                log::warn!("skipping {}: not a readable folder", root.display());
                continue;
            }
            readable_roots += 1;

            let walker = WalkDir::new(root)
                .follow_links(false)
                .into_iter()
                .filter_entry(|entry| entry.depth() == 0 || !is_skipped_directory(entry));
            for entry in walker {
                if self.is_cancelled() {
                    return Ok(by_size);
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        log::debug!("skipping unreadable entry: {error}");
                        continue;
                    }
                };
                if !entry.file_type().is_file() {
                    continue;
                }
                self.state.files_seen += 1;
                self.report(false);

                let metadata = match entry.metadata() {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        log::debug!("skipping {}: {error}", entry.path().display());
                        continue;
                    }
                };
                let size = metadata.len();
                if size < min_size {
                    continue;
                }
                // Hard links share their data, so deleting one of them frees nothing.
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt as _;
                    if !seen_inodes.insert((metadata.dev(), metadata.ino())) {
                        continue;
                    }
                }
                if !seen_paths.insert(entry.path().to_path_buf()) {
                    continue;
                }
                by_size.entry(size).or_default().push(DuplicateFile {
                    path: entry.into_path(),
                    modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    size,
                });
            }
        }

        if readable_roots == 0 && !roots.is_empty() {
            bail!("none of the folders to scan could be read");
        }
        Ok(by_size)
    }

    /// Splits files into groups with identical hashes of their first `limit` bytes (or whole
    /// contents), dropping groups of one. Returns `None` if cancelled.
    fn group_by_hash(
        &mut self,
        files: Vec<DuplicateFile>,
        limit: Option<u64>,
    ) -> Option<Vec<Vec<DuplicateFile>>> {
        let mut by_hash: HashMap<[u8; 32], Vec<DuplicateFile>> = HashMap::new();
        for file in files {
            match self.hash_file(&file.path, limit) {
                Ok(Some(hash)) => by_hash.entry(hash).or_default().push(file),
                Ok(None) => return None,
                Err(error) => {
                    log::debug!("skipping {}: {error}", file.path.display());
                }
            }
        }
        Some(
            by_hash
                .into_values()
                .filter(|group| group.len() > 1)
                .collect(),
        )
    }

    /// Returns `Ok(None)` if cancelled while reading.
    fn hash_file(&mut self, path: &Path, limit: Option<u64>) -> std::io::Result<Option<[u8; 32]>> {
        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0; PREFIX_BYTES as usize];
        let mut remaining = limit.unwrap_or(u64::MAX);
        while remaining > 0 {
            if self.is_cancelled() {
                return Ok(None);
            }
            let wanted = remaining.min(buffer.len() as u64) as usize;
            let Some(chunk) = buffer.get_mut(..wanted) else {
                break;
            };
            let read = file.read(chunk)?;
            if read == 0 {
                break;
            }
            hasher.update(chunk.get(..read).unwrap_or_default());
            remaining -= read as u64;
            self.state.bytes_hashed += read as u64;
            self.report(false);
        }
        Ok(Some(hasher.finalize().into()))
    }
}

/// Drops roots nested inside other roots so no file is visited twice.
fn outermost_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut sorted: Vec<&PathBuf> = roots.iter().collect();
    sorted.sort_by_key(|root| root.components().count());
    let mut kept: Vec<PathBuf> = Vec::new();
    for root in sorted {
        if !kept.iter().any(|outer| root.starts_with(outer)) {
            kept.push(root.clone());
        }
    }
    kept
}

fn is_skipped_directory(entry: &DirEntry) -> bool {
    if !entry.file_type().is_dir() {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    if name.starts_with('.') || SKIPPED_DIRECTORY_NAMES.contains(&name.as_ref()) {
        return true;
    }
    let at_drive_top = entry
        .path()
        .parent()
        .is_some_and(|parent| parent.parent().is_none());
    if at_drive_top
        && SKIPPED_SYSTEM_DIRECTORIES
            .iter()
            .any(|system| system.eq_ignore_ascii_case(&name))
    {
        return true;
    }
    #[cfg(unix)]
    if SKIPPED_UNIX_PATHS
        .iter()
        .any(|skipped| entry.path() == Path::new(skipped))
    {
        return true;
    }
    #[cfg(target_os = "macos")]
    if name == "Library" && entry.path().parent() == dirs::home_dir().as_deref() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        if entry
            .metadata()
            .is_ok_and(|metadata| metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0)
        {
            return true;
        }
    }
    false
}

/// The user's Downloads, Documents, Desktop, Pictures, Videos and Music folders that exist.
pub fn default_scan_roots() -> Vec<PathBuf> {
    let home = dirs::home_dir();
    // Linux desktops without `user-dirs.dirs` report none of these folders,
    // so fall back to their usual names in the home folder.
    let candidates = [
        (dirs::download_dir(), "Downloads"),
        (dirs::document_dir(), "Documents"),
        (dirs::desktop_dir(), "Desktop"),
        (dirs::picture_dir(), "Pictures"),
        (dirs::video_dir(), "Videos"),
        (dirs::audio_dir(), "Music"),
    ];
    let mut roots: Vec<PathBuf> = Vec::new();
    for candidate in candidates.into_iter().filter_map(|(folder, usual_name)| {
        folder.or_else(|| home.as_ref().map(|home| home.join(usual_name)))
    }) {
        // Unconfigured XDG folders can point at the home folder itself, which would turn a quick
        // scan into a scan of everything.
        if Some(&candidate) == home.as_ref() || !candidate.is_dir() || roots.contains(&candidate) {
            continue;
        }
        roots.push(candidate);
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, contents: &[u8], modified_seconds: u64) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
        let file = File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(modified_seconds))
            .unwrap();
    }

    fn pattern(length: usize, seed: u8) -> Vec<u8> {
        (0..length)
            .map(|index| (index as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    fn scan(roots: &[PathBuf], min_size: u64) -> Vec<DuplicateGroup> {
        let cancel = AtomicBool::new(false);
        find_duplicates(roots, min_size, &cancel, &mut |_| {}).unwrap()
    }

    fn names(group: &DuplicateGroup, root: &Path) -> Vec<String> {
        group
            .files
            .iter()
            .map(|file| {
                file.path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    #[test]
    fn finds_identical_files_and_orders_newest_first() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let big = pattern(200 * 1024, 1);
        write(&root.join("a/big.bin"), &big, 1_000);
        write(&root.join("b/big copy.bin"), &big, 3_000);
        write(&root.join("c/big again.bin"), &big, 2_000);

        // Same size and same first 64 KiB, different ending: only the full hash tells them apart.
        let mut late_difference = big.clone();
        if let Some(last) = late_difference.last_mut() {
            *last ^= 0xff;
        }
        write(&root.join("d/almost.bin"), &late_difference, 5_000);

        // Same size, different from the first byte.
        write(&root.join("d/other.bin"), &pattern(200 * 1024, 7), 5_000);

        let small = pattern(5_000, 3);
        write(&root.join("small1.txt"), &small, 10);
        write(&root.join("nested/deeper/small2.txt"), &small, 20);

        write(&root.join("unique.bin"), &pattern(12_345, 9), 1);

        let groups = scan(&[root.to_path_buf()], 1);
        assert_eq!(groups.len(), 2);

        let first = &groups[0];
        assert_eq!(first.size, big.len() as u64);
        assert_eq!(
            names(first, root),
            vec!["b/big copy.bin", "c/big again.bin", "a/big.bin"]
        );
        assert_eq!(first.keep().path, root.join("b/big copy.bin"));
        assert_eq!(first.older().len(), 2);
        assert_eq!(first.reclaimable_bytes(), 2 * big.len() as u64);

        let second = &groups[1];
        assert_eq!(
            names(second, root),
            vec!["nested/deeper/small2.txt", "small1.txt"]
        );
        assert_eq!(second.reclaimable_bytes(), small.len() as u64);
    }

    #[test]
    fn respects_min_size_and_ignores_empty_files() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        write(&root.join("one.txt"), b"hello world", 1);
        write(&root.join("two.txt"), b"hello world", 2);
        write(&root.join("empty1"), b"", 1);
        write(&root.join("empty2"), b"", 2);

        assert!(scan(&[root.to_path_buf()], 1024).is_empty());
        let groups = scan(&[root.to_path_buf()], 0);
        assert_eq!(groups.len(), 1);
        assert_eq!(names(&groups[0], root), vec!["two.txt", "one.txt"]);
    }

    #[test]
    fn skips_hidden_and_dependency_folders() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let contents = pattern(4_096, 5);
        write(&root.join("keep.dat"), &contents, 1);
        write(&root.join(".git/objects/copy.dat"), &contents, 2);
        write(&root.join(".hidden/copy.dat"), &contents, 3);
        write(
            &root.join("project/node_modules/lib/copy.dat"),
            &contents,
            4,
        );
        write(&root.join("project/target/debug/copy.dat"), &contents, 5);
        assert!(scan(&[root.to_path_buf()], 1).is_empty());

        write(&root.join("project/src/copy.dat"), &contents, 6);
        let groups = scan(&[root.to_path_buf()], 1);
        assert_eq!(groups.len(), 1);
        assert_eq!(
            names(&groups[0], root),
            vec!["project/src/copy.dat", "keep.dat"]
        );
    }

    #[test]
    fn scans_a_hidden_root_itself() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join(".config");
        write(&root.join("a"), b"same contents", 1);
        write(&root.join("b"), b"same contents", 2);
        assert_eq!(scan(&[root], 1).len(), 1);
    }

    #[test]
    fn overlapping_roots_do_not_duplicate_files() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        write(&root.join("sub/only.txt"), b"only one copy", 1);
        write(&root.join("other.txt"), b"different text", 1);
        let groups = scan(&[root.join("sub"), root.to_path_buf(), root.join("sub")], 1);
        assert!(groups.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn ignores_hard_links_and_symlinks() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        write(&root.join("original.txt"), b"linked contents", 1);
        fs::hard_link(root.join("original.txt"), root.join("hard.txt")).unwrap();
        std::os::unix::fs::symlink(root.join("original.txt"), root.join("soft.txt")).unwrap();
        std::os::unix::fs::symlink(root, root.join("loop")).unwrap();
        assert!(scan(&[root.to_path_buf()], 1).is_empty());
    }

    #[test]
    fn cancelled_scan_returns_promptly() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        write(&root.join("a"), b"same contents", 1);
        write(&root.join("b"), b"same contents", 2);
        let cancel = AtomicBool::new(true);
        let groups = find_duplicates(&[root.to_path_buf()], 1, &cancel, &mut |_| {}).unwrap();
        assert!(groups.is_empty());
    }

    #[test]
    fn cancelling_from_progress_callback_is_safe() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        write(&root.join("big1"), &pattern(300 * 1024, 1), 1);
        write(&root.join("big2"), &pattern(300 * 1024, 1), 2);
        write(&root.join("small1"), &pattern(1_000, 2), 1);
        write(&root.join("small2"), &pattern(1_000, 2), 2);

        let cancel = AtomicBool::new(false);
        let mut updates = 0;
        let groups = find_duplicates(&[root.to_path_buf()], 1, &cancel, &mut |progress| {
            updates += 1;
            // Cancel once the first (largest) group has been fully hashed.
            if progress.bytes_hashed >= 2 * 300 * 1024 + 2 * PREFIX_BYTES {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();
        assert!(updates >= 1);
        assert!(groups.len() <= 2);
    }

    #[test]
    fn reports_progress() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        write(&root.join("a"), b"same contents", 1);
        write(&root.join("b"), b"same contents", 2);
        let cancel = AtomicBool::new(false);
        let mut last = ScanProgress::default();
        find_duplicates(&[root.to_path_buf()], 1, &cancel, &mut |progress| {
            last = progress
        })
        .unwrap();
        assert_eq!(last.files_seen, 2);
        assert_eq!(last.bytes_hashed, 2 * "same contents".len() as u64);
    }

    #[test]
    fn missing_roots_are_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let result = find_duplicates(
            &[directory.path().join("does-not-exist")],
            1,
            &cancel,
            &mut |_| {},
        );
        assert!(result.is_err());
        assert!(
            find_duplicates(&[], 1, &cancel, &mut |_| {})
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn outermost_roots_drops_nested() {
        let roots = outermost_roots(&[
            PathBuf::from("/home/me/Documents/work"),
            PathBuf::from("/home/me/Documents"),
            PathBuf::from("/home/me/Downloads"),
            PathBuf::from("/home/me/Documents"),
        ]);
        assert_eq!(
            roots,
            vec![
                PathBuf::from("/home/me/Documents"),
                PathBuf::from("/home/me/Downloads"),
            ]
        );
    }

    #[test]
    fn group_accessors() {
        let file = |name: &str, seconds: u64| DuplicateFile {
            path: PathBuf::from(name),
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(seconds),
            size: 10,
        };
        let group = DuplicateGroup::new(10, vec![file("old", 1), file("new", 9), file("mid", 5)]);
        assert_eq!(group.keep().path, PathBuf::from("new"));
        let older: Vec<&Path> = group
            .older()
            .iter()
            .map(|file| file.path.as_path())
            .collect();
        assert_eq!(older, vec![Path::new("mid"), Path::new("old")]);
        assert_eq!(group.reclaimable_bytes(), 20);
    }

    #[test]
    fn default_roots_exist() {
        for root in default_scan_roots() {
            assert!(root.is_dir());
            assert_ne!(Some(root), dirs::home_dir());
        }
    }
}
