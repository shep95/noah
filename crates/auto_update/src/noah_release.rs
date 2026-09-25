//! Where noah's releases are published, how a newer one is recognized, and
//! the platform steps that install one in place.

use anyhow::{Context as _, Result, bail, ensure};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// The release manifest the website publishes next to the installers.
const DEFAULT_MANIFEST_URL: &str = "https://unlocket.vercel.app/downloads/latest.json";

/// Where people get noah by hand, for installs noah can't update itself.
pub const DOWNLOAD_PAGE: &str = "https://unlocket.vercel.app/download";

pub fn manifest_url() -> String {
    std::env::var("NOAH_UPDATE_URL").unwrap_or_else(|_| DEFAULT_MANIFEST_URL.to_string())
}

/// The build this copy of noah is: the release scripts stamp the unix time of
/// its commit. Builds made any other way have none and never update.
pub fn installed_build() -> Option<u64> {
    option_env!("NOAH_BUILD").and_then(|build| build.parse().ok())
}

#[derive(Debug, Deserialize)]
pub struct Manifest {
    /// Increases with every release; compared against [`installed_build`].
    pub build: u64,
    /// The release's name for people, such as `2026.9.25`.
    pub version: String,
    pub assets: HashMap<String, Asset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub url: String,
    pub sha256: String,
}

impl Manifest {
    pub fn version(&self) -> Version {
        self.version
            .parse()
            .unwrap_or_else(|_| Version::new(0, 0, self.build))
    }

    /// The installer for this system, keyed like `windows-x86_64`.
    pub fn asset_for(&self, os: &str, arch: &str) -> Option<&Asset> {
        self.assets.get(&format!("{os}-{arch}"))
    }
}

/// Whether a release should be downloaded: it must be newer than both the
/// running build and any update already downloaded this session.
pub fn is_newer(release_build: u64, installed_build: u64, downloaded_build: Option<u64>) -> bool {
    release_build > installed_build.max(downloaded_build.unwrap_or(0))
}

pub fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    ensure!(
        actual.eq_ignore_ascii_case(expected.trim()),
        "the downloaded update doesn't match its published checksum, so it wasn't installed"
    );
    Ok(())
}

const SET_ASIDE_SUFFIX: &str = ".old";

/// Renames noah's programs and libraries to `<name>.old` so the installer can
/// write new copies while noah is still running; Windows allows renaming a
/// file that is in use, not overwriting it. Returns what was moved, so a
/// failed install can be undone.
pub fn set_aside_program_files(install_directory: &Path) -> Result<Vec<(PathBuf, PathBuf)>> {
    let mut moved = Vec::new();
    let result = set_aside_in(install_directory, &mut moved);
    if let Err(error) = result {
        restore_set_aside(&moved);
        return Err(error);
    }
    Ok(moved)
}

fn set_aside_in(directory: &Path, moved: &mut Vec<(PathBuf, PathBuf)>) -> Result<()> {
    for entry in std::fs::read_dir(directory)
        .with_context(|| format!("couldn't read {}", directory.display()))?
    {
        let path = entry?.path();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if path.is_dir() {
            if name != "updates" {
                set_aside_in(&path, moved)?;
            }
            continue;
        }
        if name.ends_with(".exe") || name.ends_with(".dll") {
            let mut aside = path.clone().into_os_string();
            aside.push(SET_ASIDE_SUFFIX);
            let aside = PathBuf::from(aside);
            std::fs::remove_file(&aside).ok();
            std::fs::rename(&path, &aside)
                .with_context(|| format!("couldn't move {} aside", path.display()))?;
            moved.push((path, aside));
        }
    }
    Ok(())
}

pub fn restore_set_aside(moved: &[(PathBuf, PathBuf)]) {
    for (original, aside) in moved.iter().rev() {
        if !original.exists()
            && let Err(error) = std::fs::rename(aside, original)
        {
            log::error!("couldn't restore {}: {error}", original.display());
        }
    }
}

/// Deletes the files a previous update moved aside; they are no longer in use
/// once the new noah has started.
#[cfg(any(target_os = "windows", test))]
pub fn remove_set_aside_files(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            remove_set_aside_files(&path);
        } else if path.file_name().is_some_and(|name| {
            let name = name.to_string_lossy().to_lowercase();
            name.ends_with(&format!(".exe{SET_ASIDE_SUFFIX}"))
                || name.ends_with(&format!(".dll{SET_ASIDE_SUFFIX}"))
        }) {
            std::fs::remove_file(&path).ok();
        }
    }
}

/// The folder holding `noah.app` for an install made from the Linux archive.
/// The .deb installs system-wide and is updated through the package instead.
pub fn linux_install_prefix(running_editor: &Path) -> Result<PathBuf> {
    let expected = Path::new("noah.app").join("libexec").join("zed-editor");
    let Some(prefix) = running_editor
        .to_str()
        .and_then(|path| path.strip_suffix(&*expected.to_string_lossy()))
    else {
        bail!(
            "this copy of noah was installed with the .deb package. install the new \
             version from {DOWNLOAD_PAGE}"
        );
    };
    let prefix = PathBuf::from(prefix);
    let probe = prefix.join("noah.app").join(".noah-update-probe");
    std::fs::write(&probe, b"").with_context(|| {
        format!(
            "noah can't write to {}; install the new version from {DOWNLOAD_PAGE}",
            prefix.display()
        )
    })?;
    std::fs::remove_file(&probe).ok();
    Ok(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"{
        "build": 1790400000,
        "version": "2026.9.25",
        "assets": {
            "windows-x86_64": { "url": "https://example.com/noah.exe", "sha256": "ab" },
            "linux-x86_64": { "url": "https://example.com/noah.tar.xz", "sha256": "cd" }
        }
    }"#;

    #[test]
    fn reads_the_manifest() {
        let manifest: Manifest = serde_json::from_str(MANIFEST).expect("parses");
        assert_eq!(manifest.version(), Version::new(2026, 9, 25));
        assert_eq!(
            manifest
                .asset_for("linux", "x86_64")
                .map(|asset| asset.url.as_str()),
            Some("https://example.com/noah.tar.xz")
        );
        assert!(manifest.asset_for("macos", "aarch64").is_none());
    }

    #[test]
    fn only_newer_builds_update() {
        assert!(is_newer(20, 10, None));
        assert!(!is_newer(10, 10, None));
        assert!(!is_newer(9, 10, None));
        assert!(!is_newer(20, 10, Some(20)), "already downloaded");
        assert!(is_newer(30, 10, Some(20)));
    }

    #[test]
    fn checks_the_download() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("update");
        std::fs::write(&path, b"noah").expect("write");
        verify_sha256(
            &path,
            "3f7bd0d6d5e5d21f1bd4a15d8bc76f2e7a0ab18d6d43e9f0e0b3b4c3a7b1c7b2",
        )
        .expect_err("a wrong checksum is refused");
        let actual: String = Sha256::digest(b"noah")
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        verify_sha256(&path, &actual.to_uppercase()).expect("the right checksum passes");
    }

    #[test]
    fn sets_program_files_aside_and_restores_them() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path();
        std::fs::create_dir_all(root.join("bin")).expect("mkdir");
        std::fs::create_dir_all(root.join("updates")).expect("mkdir");
        for file in [
            "noah.exe",
            "conpty.dll",
            "noah.ico",
            "bin/zed.exe",
            "updates/setup.exe",
        ] {
            std::fs::write(root.join(file), file).expect("write");
        }
        let moved = set_aside_program_files(root).expect("sets aside");
        assert_eq!(moved.len(), 3);
        assert!(root.join("noah.exe.old").exists());
        assert!(root.join("bin/zed.exe.old").exists());
        assert!(
            root.join("noah.ico").exists(),
            "only programs and libraries move"
        );
        assert!(
            root.join("updates/setup.exe").exists(),
            "the download stays put"
        );

        restore_set_aside(&moved);
        assert!(root.join("noah.exe").exists() && !root.join("noah.exe.old").exists());

        set_aside_program_files(root).expect("sets aside");
        std::fs::write(root.join("noah.exe"), "new").expect("write");
        remove_set_aside_files(root);
        assert!(!root.join("noah.exe.old").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("noah.exe"))
                .ok()
                .as_deref(),
            Some("new")
        );
    }

    #[test]
    fn newer_build_edge_cases() {
        assert!(is_newer(1, 0, None), "any release beats build zero");
        assert!(!is_newer(0, 0, None));
        assert!(!is_newer(0, 0, Some(0)));
        assert!(!is_newer(u64::MAX, u64::MAX, None));
        assert!(is_newer(15, 10, Some(5)), "a stale download doesn't block");
        assert!(!is_newer(15, 10, Some(15)), "equal to the download");
        assert!(!is_newer(15, 10, Some(16)), "older than the download");
        assert!(!is_newer(10, 10, Some(5)), "equal to the installed build");
    }

    #[test]
    fn manifest_without_this_platform_has_no_asset() {
        let manifest: Manifest =
            serde_json::from_str(r#"{ "build": 5, "version": "1.2.3", "assets": {} }"#)
                .expect("parses");
        assert!(manifest.asset_for("windows", "x86_64").is_none());
        assert!(manifest.asset_for("linux", "x86_64").is_none());

        let manifest: Manifest = serde_json::from_str(MANIFEST).expect("parses");
        assert!(manifest.asset_for("windows", "aarch64").is_none());
        assert!(manifest.asset_for("linux", "aarch64").is_none());
        assert!(manifest.asset_for("Linux", "x86_64").is_none(), "keys are exact");
        assert!(manifest.asset_for("", "").is_none());
        assert_eq!(
            manifest
                .asset_for("windows", "x86_64")
                .map(|asset| asset.sha256.as_str()),
            Some("ab")
        );
    }

    #[test]
    fn malformed_manifests_are_rejected() {
        assert!(
            serde_json::from_str::<Manifest>(r#"{ "build": 5, "version": "1.2.3" }"#).is_err(),
            "assets are required"
        );
        assert!(
            serde_json::from_str::<Manifest>(r#"{ "version": "1.2.3", "assets": {} }"#).is_err(),
            "build is required"
        );
        assert!(
            serde_json::from_str::<Manifest>(
                r#"{ "build": 5, "version": "1.2.3", "assets": { "linux-x86_64": { "url": "u" } } }"#
            )
            .is_err(),
            "every asset needs a checksum"
        );
        assert!(
            serde_json::from_str::<Manifest>(r#"{ "build": -1, "version": "1", "assets": {} }"#)
                .is_err(),
            "builds are unsigned"
        );
    }

    #[test]
    fn unparseable_versions_fall_back_to_the_build() {
        let manifest: Manifest =
            serde_json::from_str(r#"{ "build": 42, "version": "nightly", "assets": {} }"#)
                .expect("parses");
        assert_eq!(manifest.version(), Version::new(0, 0, 42));
    }

    #[test]
    fn set_aside_matches_extensions_in_any_case_and_skips_updates() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path();
        std::fs::create_dir_all(root.join("Updates")).expect("mkdir");
        std::fs::create_dir_all(root.join("lib/deeper")).expect("mkdir");
        for file in [
            "NOAH.EXE",
            "Helper.Dll",
            "readme.txt",
            "noah.exe.config",
            "lib/deeper/core.dll",
            "Updates/noah-setup.exe",
        ] {
            std::fs::write(root.join(file), file).expect("write");
        }

        let moved = set_aside_program_files(root).expect("sets aside");
        let mut moved_names: Vec<String> = moved
            .iter()
            .map(|(original, aside)| {
                let mut expected_aside = original.clone().into_os_string();
                expected_aside.push(".old");
                assert_eq!(aside, &PathBuf::from(expected_aside));
                original
                    .strip_prefix(root)
                    .expect("inside the install directory")
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        moved_names.sort();
        assert_eq!(
            moved_names,
            vec!["Helper.Dll", "NOAH.EXE", "lib/deeper/core.dll"]
        );
        assert!(root.join("readme.txt").exists());
        assert!(root.join("noah.exe.config").exists());
        assert!(root.join("Updates/noah-setup.exe").exists());
        assert!(!root.join("Updates/noah-setup.exe.old").exists());
        assert!(root.join("lib/deeper/core.dll.old").exists());

        restore_set_aside(&moved);
        for (original, aside) in &moved {
            assert!(original.exists(), "{}", original.display());
            assert!(!aside.exists(), "{}", aside.display());
        }
        assert_eq!(
            std::fs::read_to_string(root.join("NOAH.EXE")).ok().as_deref(),
            Some("NOAH.EXE")
        );
    }

    #[test]
    fn set_aside_replaces_a_stale_copy() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path();
        std::fs::write(root.join("noah.exe"), "current").expect("write");
        std::fs::write(root.join("noah.exe.old"), "stale").expect("write");

        let moved = set_aside_program_files(root).expect("sets aside");
        assert_eq!(moved.len(), 1);
        assert!(!root.join("noah.exe").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("noah.exe.old")).ok().as_deref(),
            Some("current")
        );
    }

    #[test]
    fn restore_keeps_newly_installed_files() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path();
        std::fs::write(root.join("noah.exe"), "old").expect("write");
        std::fs::write(root.join("conpty.dll"), "old").expect("write");

        let moved = set_aside_program_files(root).expect("sets aside");
        std::fs::write(root.join("noah.exe"), "new").expect("write");
        restore_set_aside(&moved);

        assert_eq!(
            std::fs::read_to_string(root.join("noah.exe")).ok().as_deref(),
            Some("new")
        );
        assert!(root.join("noah.exe.old").exists(), "the old copy stays aside");
        assert_eq!(
            std::fs::read_to_string(root.join("conpty.dll")).ok().as_deref(),
            Some("old")
        );
        assert!(!root.join("conpty.dll.old").exists());
    }

    #[test]
    fn set_aside_of_a_missing_directory_fails() {
        let directory = tempfile::tempdir().expect("tempdir");
        let error = set_aside_program_files(&directory.path().join("missing"))
            .expect_err("nothing to read");
        assert!(error.to_string().contains("couldn't read"), "{error}");
    }

    #[test]
    fn removes_only_set_aside_files() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path();
        std::fs::create_dir_all(root.join("bin/nested")).expect("mkdir");
        for file in [
            "noah.exe",
            "noah.exe.old",
            "notes.old.txt",
            "settings.json.old",
            "bin/zed.dll.old",
            "bin/nested/tool.exe.old",
            "bin/nested/tool.exe",
        ] {
            std::fs::write(root.join(file), file).expect("write");
        }

        remove_set_aside_files(root);

        for kept in [
            "noah.exe",
            "notes.old.txt",
            "settings.json.old",
            "bin/nested/tool.exe",
        ] {
            assert!(root.join(kept).exists(), "{kept}");
        }
        for removed in ["noah.exe.old", "bin/zed.dll.old", "bin/nested/tool.exe.old"] {
            assert!(!root.join(removed).exists(), "{removed}");
        }

        remove_set_aside_files(&root.join("missing"));
    }

    #[test]
    fn deb_installs_are_not_touched() {
        let error = linux_install_prefix(Path::new("/opt/noah/libexec/zed-editor"))
            .expect_err("deb layout");
        assert!(error.to_string().contains(".deb"));

        let directory = tempfile::tempdir().expect("tempdir");
        let app = directory.path().join("noah.app").join("libexec");
        std::fs::create_dir_all(&app).expect("mkdir");
        let prefix = linux_install_prefix(&app.join("zed-editor")).expect("archive layout");
        assert_eq!(prefix, directory.path());
    }
}
