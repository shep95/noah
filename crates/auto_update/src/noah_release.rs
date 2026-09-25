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
        } else if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(SET_ASIDE_SUFFIX))
        {
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
