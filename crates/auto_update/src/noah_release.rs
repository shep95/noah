//! Where noah's releases are published, how a newer one is recognized, and
//! the platform steps that install one in place.

use anyhow::{Context as _, Result, bail, ensure};
use futures_lite::AsyncReadExt as _;
use http_client::HttpClientWithUrl;
use release_channel::ReleaseChannel;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// The release manifest the website publishes next to the installers.
const DEFAULT_MANIFEST_URL: &str = "https://noah.asherin.com/downloads/latest.json";

/// Where people get noah by hand, for installs noah can't update itself.
pub const DOWNLOAD_PAGE: &str = "https://noah.asherin.com/download";

/// `NOAH_UPDATE_URL` points development builds at a test manifest. Release
/// builds ignore it: whoever can set a process's environment could otherwise
/// send noah's updater to an installer of their choosing.
pub fn manifest_url(release_channel: Option<ReleaseChannel>) -> String {
    manifest_url_from(release_channel, std::env::var("NOAH_UPDATE_URL").ok())
}

fn manifest_url_from(
    release_channel: Option<ReleaseChannel>,
    override_url: Option<String>,
) -> String {
    let override_allowed = cfg!(debug_assertions) || release_channel == Some(ReleaseChannel::Dev);
    match override_url {
        Some(url) if override_allowed && !url.trim().is_empty() => url,
        _ => DEFAULT_MANIFEST_URL.to_string(),
    }
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
    /// ed25519 signature by noah's release key over [`Manifest::signed_text`].
    #[serde(default)]
    pub signature: Option<String>,
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

    /// Any asset by its exact key, such as [`ASHERIN_EYE_ASSET`].
    pub fn asset(&self, key: &str) -> Option<&Asset> {
        self.assets.get(key)
    }

    /// What the release key signs; `script/write-release-manifest` builds the
    /// same text, with the asset lines in byte order.
    fn signed_text(&self) -> String {
        let mut lines: Vec<String> = self
            .assets
            .iter()
            .map(|(key, asset)| format!("{key} {} {}\n", asset.url, asset.sha256))
            .collect();
        lines.sort();
        format!("noah-release\n{}\n{}\n{}", self.build, self.version, lines.concat())
    }

    /// The checksums in a manifest only prove the download is intact. The
    /// signature proves noah's release key published them, so whoever
    /// controls the website alone can't ship an update.
    pub fn verify_signature(&self) -> Result<()> {
        self.verify_signature_with(&release_public_key()?)
    }

    fn verify_signature_with(&self, public_key: &[u8]) -> Result<()> {
        use base64::Engine as _;
        let unsigned = || {
            anyhow::anyhow!(
                "this update isn't signed by noah's release key, so it wasn't installed. \
                 install the new version from {DOWNLOAD_PAGE}"
            )
        };
        let signature = self
            .signature
            .as_deref()
            .and_then(|signature| {
                base64::engine::general_purpose::STANDARD
                    .decode(signature.trim())
                    .ok()
            })
            .ok_or_else(unsigned)?;
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public_key)
            .verify(self.signed_text().as_bytes(), &signature)
            .map_err(|_| unsigned())
    }
}

/// The manifest key of asherin.eye's source tarball, which
/// `script/package-asherin-eye` builds and `script/write-release-manifest`
/// lists and signs next to the installers.
pub const ASHERIN_EYE_ASSET: &str = "asherin-eye";

/// Reads the release manifest. Nothing in it is trustworthy until
/// [`Manifest::verify_signature`] passes.
pub async fn fetch_manifest(
    client: &HttpClientWithUrl,
    release_channel: Option<ReleaseChannel>,
) -> Result<Manifest> {
    let url = manifest_url(release_channel);
    let mut response = client
        .get(&url, Default::default(), true)
        .await
        .with_context(|| format!("couldn't reach {url}; check the internet connection"))?;
    let mut body = Vec::new();
    response.body_mut().read_to_end(&mut body).await?;
    ensure!(
        response.status().is_success(),
        "couldn't read noah's release list ({})",
        response.status()
    );
    serde_json::from_slice(&body).context("noah's release information couldn't be read")
}

/// asherin.eye's source tarball as noah's release key published it. The
/// checksum can be trusted because the signature covers every asset line.
pub async fn fetch_asherin_eye_asset(
    client: &HttpClientWithUrl,
    release_channel: Option<ReleaseChannel>,
) -> Result<Asset> {
    let manifest = fetch_manifest(client, release_channel).await?;
    asherin_eye_asset(&manifest, &release_public_key()?)
}

fn asherin_eye_asset(manifest: &Manifest, public_key: &[u8]) -> Result<Asset> {
    manifest.verify_signature_with(public_key).map_err(|_| {
        anyhow::anyhow!(
            "noah's release list isn't signed by noah's release key, so asherin.eye wasn't \
             downloaded"
        )
    })?;
    manifest.asset(ASHERIN_EYE_ASSET).cloned().ok_or_else(|| {
        anyhow::anyhow!(
            "noah {} doesn't publish asherin.eye yet; try again after the next release",
            manifest.version
        )
    })
}

const RELEASE_PUBLIC_KEY_HEX: &str = include_str!("../release_public_key.txt");

fn release_public_key() -> Result<Vec<u8>> {
    let hex = RELEASE_PUBLIC_KEY_HEX.trim();
    anyhow::ensure!(hex.len() == 64, "noah's release key is malformed");
    (0..hex.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&hex[index..index + 2], 16)
                .context("noah's release key is malformed")
        })
        .collect()
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

    // A throwaway key made with `openssl genpkey -algorithm ed25519`; the
    // signature is `openssl pkeyutl -sign -rawin` over the text below, as
    // script/write-release-manifest produces it.
    const TEST_PUBLIC_KEY: &str = "041055cf0a08728621b5ae3a307927d7dbe058b4c3efa3679049e3a1870eb021";
    const TEST_SIGNATURE: &str = "JLWdm+EyqtQirTOBtU/fA0aKtQVGNUX6OkmEFkLeGXz6yBufpdXVvItd0YMy/KUWC+Ep7FGM3u/FXc40KUqGBw==";

    fn signed_manifest() -> Manifest {
        serde_json::from_str(&format!(
            r#"{{"build": 42, "version": "2026.1.2", "assets": {{
                "windows-x86_64": {{ "url": "https://example.com/noah.exe", "sha256": "ab" }},
                "linux-x86_64": {{ "url": "https://example.com/noah.tar.xz", "sha256": "cd" }}
            }}, "signature": "{TEST_SIGNATURE}"}}"#
        ))
        .expect("manifest parses")
    }

    fn test_key() -> Vec<u8> {
        (0..TEST_PUBLIC_KEY.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&TEST_PUBLIC_KEY[index..index + 2], 16).expect("hex"))
            .collect()
    }

    #[test]
    fn signed_manifests_verify() {
        signed_manifest()
            .verify_signature_with(&test_key())
            .expect("signature matches");
    }

    #[test]
    fn tampered_or_unsigned_manifests_are_rejected() {
        let mut tampered = signed_manifest();
        if let Some(asset) = tampered.assets.get_mut("windows-x86_64") {
            asset.sha256 = "ff".into();
        }
        assert!(tampered.verify_signature_with(&test_key()).is_err());

        let mut unsigned = signed_manifest();
        unsigned.signature = None;
        assert!(unsigned.verify_signature_with(&test_key()).is_err());

        // Signed by a different key than the one noah trusts.
        assert!(signed_manifest().verify_signature().is_err());
    }

    // Written by script/write-release-manifest itself (NOAH_BUILD=42) for a
    // downloads directory holding a Linux installer and asherin-eye.tar.gz,
    // signed with another throwaway key.
    const EYE_PUBLIC_KEY: &str = "a03b4b37461df07451933138854b8de8c98defe6dac7b57e74ec6b8c1c10a9bb";
    const EYE_MANIFEST: &str = r#"{
      "build": 42,
      "version": "1970.1.1",
      "assets": {
        "linux-x86_64": { "url": "https://example.com/downloads/noah-linux-x86_64.tar.xz", "sha256": "cf3a3bbe331c3950d16a8e9917c5bb8340e7c0ef917da25d4a96f92d074bce05" },
        "asherin-eye": { "url": "https://example.com/downloads/asherin-eye.tar.gz", "sha256": "47c215b5f70eb9c9b4bcb2c027007d6cf38a899f40d1d1da6922e49308b15b69" }
      },
      "signature": "Mo9KjKNKjMTy4laMzN4fhajfFmF7My/0izFrIDLNgbnQyny4ZGfklZ8a8b/jDx5n+idR6BkB5h/oP0eJwFXKBQ=="
    }"#;

    fn key_from_hex(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).expect("hex"))
            .collect()
    }

    #[test]
    fn the_eye_asset_is_signed_and_harmless_to_the_updater() {
        let manifest: Manifest = serde_json::from_str(EYE_MANIFEST).expect("parses");
        let key = key_from_hex(EYE_PUBLIC_KEY);
        manifest
            .verify_signature_with(&key)
            .expect("the script's signature covers the eye asset");

        let eye = asherin_eye_asset(&manifest, &key).expect("signed eye asset");
        assert_eq!(eye.url, "https://example.com/downloads/asherin-eye.tar.gz");
        assert_eq!(
            eye.sha256,
            "47c215b5f70eb9c9b4bcb2c027007d6cf38a899f40d1d1da6922e49308b15b69"
        );

        // The updater looks installers up by `std::env::consts::{OS, ARCH}`,
        // none of which is `asherin`/`eye`, so the extra key never stands in
        // for an installer.
        assert_eq!(
            manifest
                .asset_for("linux", "x86_64")
                .map(|asset| asset.url.as_str()),
            Some("https://example.com/downloads/noah-linux-x86_64.tar.xz")
        );
        for (os, arch) in [
            ("windows", "x86_64"),
            ("linux", "aarch64"),
            ("macos", "aarch64"),
        ] {
            assert!(manifest.asset_for(os, arch).is_none(), "{os}-{arch}");
        }
    }

    #[test]
    fn a_tampered_or_missing_eye_asset_is_refused() {
        let key = key_from_hex(EYE_PUBLIC_KEY);

        let mut tampered: Manifest = serde_json::from_str(EYE_MANIFEST).expect("parses");
        if let Some(asset) = tampered.assets.get_mut(ASHERIN_EYE_ASSET) {
            asset.sha256 = "00".repeat(32);
        }
        assert!(tampered.verify_signature_with(&key).is_err());
        let error = asherin_eye_asset(&tampered, &key).expect_err("tampered");
        assert!(error.to_string().contains("isn't signed"), "{error}");

        let mut redirected: Manifest = serde_json::from_str(EYE_MANIFEST).expect("parses");
        if let Some(asset) = redirected.assets.get_mut(ASHERIN_EYE_ASSET) {
            asset.url = "https://attacker.example/asherin-eye.tar.gz".into();
        }
        assert!(asherin_eye_asset(&redirected, &key).is_err());

        let mut removed: Manifest = serde_json::from_str(EYE_MANIFEST).expect("parses");
        removed.assets.remove(ASHERIN_EYE_ASSET);
        assert!(
            asherin_eye_asset(&removed, &key).is_err(),
            "dropping an asset breaks the signature too"
        );

        let without_eye = signed_manifest();
        let error = asherin_eye_asset(&without_eye, &test_key()).expect_err("no eye");
        assert!(error.to_string().contains("doesn't publish"), "{error}");
    }

    #[test]
    fn the_release_key_is_well_formed() {
        assert_eq!(release_public_key().expect("key parses").len(), 32);
    }

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
    fn only_development_builds_honor_the_manifest_override() {
        let custom = || Some("http://localhost:8000/latest.json".to_string());
        assert_eq!(
            manifest_url_from(Some(ReleaseChannel::Dev), custom()),
            "http://localhost:8000/latest.json"
        );
        assert_eq!(
            manifest_url_from(Some(ReleaseChannel::Dev), None),
            DEFAULT_MANIFEST_URL
        );
        assert_eq!(
            manifest_url_from(Some(ReleaseChannel::Dev), Some(" ".to_string())),
            DEFAULT_MANIFEST_URL
        );
        if !cfg!(debug_assertions) {
            for channel in [
                Some(ReleaseChannel::Stable),
                Some(ReleaseChannel::Preview),
                Some(ReleaseChannel::Nightly),
                None,
            ] {
                assert_eq!(manifest_url_from(channel, custom()), DEFAULT_MANIFEST_URL);
            }
        }
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
            "bin/noah.exe",
            "updates/setup.exe",
        ] {
            std::fs::write(root.join(file), file).expect("write");
        }
        let moved = set_aside_program_files(root).expect("sets aside");
        assert_eq!(moved.len(), 3);
        assert!(root.join("noah.exe.old").exists());
        assert!(root.join("bin/noah.exe.old").exists());
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
