//! Installing asherin.eye's source and preparing its dev server, kept free of
//! noah's UI so it can be tested on its own. The `asherin_eye` crate drives
//! these steps from the room.

use std::{
    collections::HashMap,
    ffi::OsString,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream},
    path::{Component, Path, PathBuf},
    time::Duration,
};

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use async_compression::futures::bufread::GzipDecoder;
use async_tar::EntryType;
use futures::{AsyncRead, AsyncReadExt as _, AsyncWriteExt as _, StreamExt as _, io::BufReader};
use http_client::{HttpClient as _, HttpClientWithUrl};
use sha2::{Digest as _, Sha256};

/// The folder the tarball unpacks to and the name of the person's copy.
pub const FOLDER_NAME: &str = "asherin.eye";
/// ADAM's Vite config falls back to this port when `PORT` isn't set.
pub const DEFAULT_PORT: u16 = 4173;
/// Becomes the copy's AGENTS.md, which shepherd reads as the project's
/// instructions.
pub const GUIDE: &str = include_str!("../../../assets/shepherd/asherin_eye_guide.md");

pub enum InstallStep {
    Downloading { percent: u64 },
    Unpacking,
}

/// Downloads the tarball at `url`, checks it against `sha256` (which the
/// caller took from the signed release manifest) and unpacks it as `folder`.
/// An existing folder is the person's edited copy, so it is never replaced.
pub async fn install(
    http: &HttpClientWithUrl,
    url: &str,
    sha256: &str,
    folder: &Path,
    mut on_step: impl FnMut(InstallStep),
) -> Result<()> {
    ensure!(
        !folder.exists(),
        "{} already exists, so noah left it as it is",
        folder.display()
    );
    let lab = folder
        .parent()
        .context("asherin.eye's folder has no parent folder")?;
    let staging = lab.join(".asherin.eye-download");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)
            .with_context(|| format!("couldn't clear {}", staging.display()))?;
    }
    let unpacked = staging.join("unpacked");
    std::fs::create_dir_all(&unpacked)
        .with_context(|| format!("couldn't create {}", unpacked.display()))?;

    let result = async {
        let tarball = staging.join("asherin-eye.tar.gz");
        let mut last_percent = None;
        download(http, url, sha256, &tarball, |received, total| {
            let Some(total) = total.filter(|total| *total > 0) else {
                return;
            };
            let percent = (received.min(total) * 100 / total / 10) * 10;
            if last_percent != Some(percent) {
                last_percent = Some(percent);
                on_step(InstallStep::Downloading { percent });
            }
        })
        .await?;
        on_step(InstallStep::Unpacking);
        extract_tarball(&tarball, &unpacked).await?;
        move_into_place(&unpacked.join(FOLDER_NAME), folder)
    }
    .await;
    if let Err(error) = std::fs::remove_dir_all(&staging) {
        log::error!("couldn't remove {}: {error}", staging.display());
    }
    result
}

/// Unpacks a tarball already on this machine (the copy noah's installer
/// ships) as `folder`, so the first open of asherin.eye needs no network.
/// An existing folder is the person's edited copy, so it is never replaced.
pub async fn install_from_tarball(
    tarball: &Path,
    folder: &Path,
    mut on_step: impl FnMut(InstallStep),
) -> Result<()> {
    ensure!(
        !folder.exists(),
        "{} already exists, so noah left it as it is",
        folder.display()
    );
    let lab = folder
        .parent()
        .context("asherin.eye's folder has no parent folder")?;
    let staging = lab.join(".asherin.eye-unpack");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)
            .with_context(|| format!("couldn't clear {}", staging.display()))?;
    }
    std::fs::create_dir_all(&staging)
        .with_context(|| format!("couldn't create {}", staging.display()))?;
    let result = async {
        on_step(InstallStep::Unpacking);
        extract_tarball(tarball, &staging).await?;
        move_into_place(&staging.join(FOLDER_NAME), folder)
    }
    .await;
    if let Err(error) = std::fs::remove_dir_all(&staging) {
        log::error!("couldn't remove {}: {error}", staging.display());
    }
    result
}

/// The tarball noah's installer put beside the editor, when it did: in the
/// same folder as the editor (Linux, Windows), or in Resources (macOS).
pub fn bundled_tarball() -> Option<PathBuf> {
    let directory = std::env::current_exe().ok()?.parent()?.to_path_buf();
    [
        directory.join("asherin-eye.tar.gz"),
        directory.join("../libexec/asherin-eye.tar.gz"),
        directory.join("../Resources/asherin-eye.tar.gz"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

async fn download(
    http: &HttpClientWithUrl,
    url: &str,
    sha256: &str,
    destination: &Path,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    let mut response = http
        .get(url, Default::default(), true)
        .await
        .with_context(|| format!("couldn't reach {url}; check the internet connection"))?;
    ensure!(
        response.status().is_success(),
        "couldn't download asherin.eye ({})",
        response.status()
    );
    let total = response
        .headers()
        .get(http_client::http::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());

    let mut file = smol::fs::File::create(destination)
        .await
        .with_context(|| format!("couldn't write {}", destination.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut received = 0u64;
    let body = response.body_mut();
    loop {
        let read = body
            .read(&mut buffer)
            .await
            .context("the asherin.eye download was interrupted; check the internet connection")?;
        if read == 0 {
            break;
        }
        let chunk = buffer
            .get(..read)
            .context("the download reported more bytes than it read")?;
        hasher.update(chunk);
        file.write_all(chunk)
            .await
            .with_context(|| format!("couldn't write {}", destination.display()))?;
        received += read as u64;
        on_progress(received, total);
    }
    file.flush().await?;

    let actual: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    ensure!(
        actual.eq_ignore_ascii_case(sha256.trim()),
        "the asherin.eye download doesn't match the checksum in noah's signed release list, \
         so it wasn't installed"
    );
    Ok(())
}

async fn extract_tarball(tarball: &Path, destination: &Path) -> Result<()> {
    let file = smol::fs::File::open(tarball)
        .await
        .with_context(|| format!("couldn't read {}", tarball.display()))?;
    unpack(GzipDecoder::new(BufReader::new(file)), destination).await
}

/// Unpacks only plain files and folders, and only inside `asherin.eye/`, so a
/// tampered archive can't write anywhere else or plant links.
async fn unpack(reader: impl AsyncRead + Unpin, destination: &Path) -> Result<()> {
    let archive = async_tar::ArchiveBuilder::new(reader)
        .set_preserve_mtime(false)
        .build();
    let mut entries = archive
        .entries()
        .context("asherin.eye's download is damaged")?;
    while let Some(entry) = entries.next().await {
        let mut entry = entry.context("asherin.eye's download is damaged")?;
        let entry_type = entry.header().entry_type();
        if matches!(entry_type, EntryType::XGlobalHeader | EntryType::XHeader) {
            continue;
        }
        let path = PathBuf::from(
            entry
                .path()
                .context("asherin.eye's download is damaged")?
                .as_os_str(),
        );
        let target = destination.join(checked_entry_path(&path)?);
        match entry_type {
            EntryType::Directory => smol::fs::create_dir_all(&target)
                .await
                .with_context(|| format!("couldn't create {}", target.display()))?,
            EntryType::Regular | EntryType::Continuous => {
                if let Some(parent) = target.parent() {
                    smol::fs::create_dir_all(parent)
                        .await
                        .with_context(|| format!("couldn't create {}", parent.display()))?;
                }
                entry
                    .unpack(&target)
                    .await
                    .with_context(|| format!("couldn't write {}", target.display()))?;
            }
            other => bail!(
                "asherin.eye's download contains {} ({other:?}), which isn't a plain file, \
                 so it wasn't installed",
                path.display()
            ),
        }
    }
    Ok(())
}

/// The entry's path relative to the unpack folder, refusing anything that
/// could land outside `asherin.eye/`.
fn checked_entry_path(path: &Path) -> Result<PathBuf> {
    let outside = || {
        anyhow!(
            "asherin.eye's download contains {}, which points outside its folder, so it \
             wasn't installed",
            path.display()
        )
    };
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(outside());
            }
        }
    }
    if !relative.starts_with(FOLDER_NAME) {
        return Err(outside());
    }
    Ok(relative)
}

fn move_into_place(unpacked: &Path, folder: &Path) -> Result<()> {
    ensure!(
        unpacked.is_dir(),
        "asherin.eye's download has no {FOLDER_NAME} folder, so it wasn't installed"
    );
    ensure!(
        !folder.exists(),
        "{} already exists, so noah left it as it is",
        folder.display()
    );
    std::fs::rename(unpacked, folder)
        .with_context(|| format!("couldn't move asherin.eye into {}", folder.display()))?;
    let guide = folder.join("AGENTS.md");
    if !guide.exists() {
        std::fs::write(&guide, GUIDE)
            .with_context(|| format!("couldn't write {}", guide.display()))?;
    }
    Ok(())
}

/// The environment for the copy's npm commands. `node_directory` is set when
/// there is no `npm` on PATH, to the folder of the Node.js noah manages,
/// whose `npm` sits next to `node`.
pub fn npm_environment(
    node_directory: Option<&Path>,
    inherited_path: Option<OsString>,
) -> Result<HashMap<String, String>> {
    let mut environment = HashMap::new();
    // ADAM's QA scripts use puppeteer, which otherwise downloads a whole
    // Chromium during npm install.
    environment.insert("PUPPETEER_SKIP_DOWNLOAD".to_string(), "1".to_string());
    if let Some(node_directory) = node_directory {
        let directories = std::iter::once(node_directory.to_path_buf())
            .chain(inherited_path.iter().flat_map(std::env::split_paths));
        let path =
            std::env::join_paths(directories).context("couldn't put noah's Node.js on PATH")?;
        environment.insert("PATH".to_string(), path.to_string_lossy().into_owned());
    }
    Ok(environment)
}

/// Vite listens on `localhost`, which is IPv4 on some systems and IPv6 on
/// others. This blocks for up to half a second.
pub fn port_answers(port: u16) -> bool {
    [
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
    ]
    .into_iter()
    .any(|address| {
        TcpStream::connect_timeout(&SocketAddr::new(address, port), Duration::from_millis(250))
            .is_ok()
    })
}

/// ADAM's Vite config reads `PORT` from the dotenv files Vite loads for
/// `npm run dev`, where later files in this list win.
pub fn dev_server_port(folder: &Path) -> u16 {
    [
        ".env",
        ".env.local",
        ".env.development",
        ".env.development.local",
    ]
    .into_iter()
    .filter_map(|name| std::fs::read_to_string(folder.join(name)).ok())
    .filter_map(|contents| port_from_dotenv(&contents))
    .last()
    .unwrap_or(DEFAULT_PORT)
}

fn port_from_dotenv(contents: &str) -> Option<u16> {
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let line = line.strip_prefix("export ").unwrap_or(line);
            let value = line.strip_prefix("PORT")?.trim_start().strip_prefix('=')?;
            let value = value.split('#').next()?.trim().trim_matches(['"', '\'']);
            value.parse::<u16>().ok().filter(|port| *port != 0)
        })
        .last()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_tar::{Builder, Header};
    use futures::io::Cursor;
    use http_client::{FakeHttpClient, Response};
    use std::sync::Arc;

    fn header(path: &[u8], entry_type: EntryType, size: u64) -> Header {
        let mut header = Header::new_gnu();
        let name = &mut header.as_old_mut().name;
        if let Some(prefix) = name.get_mut(..path.len()) {
            prefix.copy_from_slice(path);
        }
        header.set_entry_type(entry_type);
        header.set_size(size);
        header.set_mode(0o644);
        header.set_cksum();
        header
    }

    fn tar(entries: &[(&str, EntryType, &str)]) -> Vec<u8> {
        smol::block_on(async {
            let mut builder = Builder::new(Vec::new());
            for (path, entry_type, contents) in entries {
                let header = header(path.as_bytes(), *entry_type, contents.len() as u64);
                builder
                    .append(&header, contents.as_bytes())
                    .await
                    .expect("append");
            }
            builder.into_inner().await.expect("finish")
        })
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        smol::block_on(async {
            let mut encoder = async_compression::futures::write::GzipEncoder::new(Vec::new());
            encoder.write_all(bytes).await.expect("compress");
            encoder.close().await.expect("close");
            encoder.into_inner()
        })
    }

    fn unpack_bytes(bytes: Vec<u8>, destination: &Path) -> Result<()> {
        smol::block_on(unpack(Cursor::new(bytes), destination))
    }

    #[test]
    fn entry_paths_stay_inside_the_eye_folder() {
        assert_eq!(
            checked_entry_path(Path::new("asherin.eye/src/main.js")).expect("inside"),
            Path::new("asherin.eye/src/main.js")
        );
        assert_eq!(
            checked_entry_path(Path::new("./asherin.eye/")).expect("inside"),
            Path::new("asherin.eye")
        );
        for outside in [
            "../evil",
            "asherin.eye/../../evil",
            "asherin.eye/src/../../../evil",
            "/etc/passwd",
            "other/file",
            "asherin.eyes/file",
            "",
        ] {
            assert!(
                checked_entry_path(Path::new(outside)).is_err(),
                "{outside} must be refused"
            );
        }
    }

    #[test]
    fn unpacks_files_and_folders() {
        let directory = tempfile::tempdir().expect("tempdir");
        let bytes = tar(&[
            ("asherin.eye/", EntryType::Directory, ""),
            ("asherin.eye/package.json", EntryType::Regular, "{}"),
            (
                "asherin.eye/src/layers/a.js",
                EntryType::Regular,
                "export {}",
            ),
        ]);
        unpack_bytes(bytes, directory.path()).expect("unpacks");
        let root = directory.path().join(FOLDER_NAME);
        assert_eq!(
            std::fs::read_to_string(root.join("package.json"))
                .ok()
                .as_deref(),
            Some("{}")
        );
        assert!(root.join("src/layers/a.js").is_file());
    }

    #[test]
    fn refuses_entries_that_escape_the_folder() {
        let directory = tempfile::tempdir().expect("tempdir");
        let destination = directory.path().join("unpacked");
        std::fs::create_dir_all(&destination).expect("mkdir");
        for path in [
            "asherin.eye/../../escaped.txt",
            "../escaped.txt",
            "/tmp/asherin-eye-escaped.txt",
            "somewhere-else/escaped.txt",
        ] {
            let bytes = tar(&[
                ("asherin.eye/ok.txt", EntryType::Regular, "ok"),
                (path, EntryType::Regular, "evil"),
            ]);
            let error = unpack_bytes(bytes, &destination).expect_err(path);
            assert!(error.to_string().contains("outside"), "{path}: {error}");
        }
        assert!(!directory.path().join("escaped.txt").exists());
        assert!(!destination.join("escaped.txt").exists());
        assert!(!destination.join("somewhere-else").exists());
        assert!(!Path::new("/tmp/asherin-eye-escaped.txt").exists());
    }

    #[test]
    fn refuses_links() {
        let directory = tempfile::tempdir().expect("tempdir");
        for entry_type in [EntryType::Symlink, EntryType::Link] {
            let bytes = tar(&[("asherin.eye/link", entry_type, "")]);
            let error = unpack_bytes(bytes, directory.path()).expect_err("links are refused");
            assert!(error.to_string().contains("isn't a plain file"), "{error}");
            assert!(!directory.path().join("asherin.eye/link").exists());
        }
    }

    #[test]
    fn extracts_a_gzipped_tarball() {
        let directory = tempfile::tempdir().expect("tempdir");
        let tarball = directory.path().join("asherin-eye.tar.gz");
        std::fs::write(
            &tarball,
            gzip(&tar(&[(
                "asherin.eye/README.md",
                EntryType::Regular,
                "# ADAM",
            )])),
        )
        .expect("write");
        let destination = directory.path().join("unpacked");
        smol::block_on(extract_tarball(&tarball, &destination)).expect("extracts");
        assert!(destination.join("asherin.eye/README.md").is_file());
    }

    #[test]
    fn moving_into_place_never_overwrites_and_adds_the_guide() {
        let directory = tempfile::tempdir().expect("tempdir");
        let unpacked = directory.path().join("unpacked").join(FOLDER_NAME);
        std::fs::create_dir_all(&unpacked).expect("mkdir");
        std::fs::write(unpacked.join("package.json"), "{}").expect("write");

        let existing = directory.path().join("existing");
        std::fs::create_dir_all(&existing).expect("mkdir");
        std::fs::write(existing.join("mine.js"), "edited").expect("write");
        assert!(move_into_place(&unpacked, &existing).is_err());
        assert_eq!(
            std::fs::read_to_string(existing.join("mine.js"))
                .ok()
                .as_deref(),
            Some("edited")
        );
        assert!(!existing.join("package.json").exists());

        let folder = directory.path().join(FOLDER_NAME);
        move_into_place(&unpacked, &folder).expect("moves");
        assert!(folder.join("package.json").is_file());
        assert_eq!(
            std::fs::read_to_string(folder.join("AGENTS.md"))
                .ok()
                .as_deref(),
            Some(GUIDE)
        );

        let with_guide = directory.path().join("with-guide").join(FOLDER_NAME);
        std::fs::create_dir_all(&with_guide).expect("mkdir");
        std::fs::write(with_guide.join("AGENTS.md"), "ADAM's own").expect("write");
        let target = directory.path().join("second");
        move_into_place(&with_guide, &target).expect("moves");
        assert_eq!(
            std::fs::read_to_string(target.join("AGENTS.md"))
                .ok()
                .as_deref(),
            Some("ADAM's own")
        );
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn serving(bytes: Vec<u8>) -> Arc<HttpClientWithUrl> {
        let bytes = Arc::new(bytes);
        FakeHttpClient::create(move |request| {
            let bytes = bytes.clone();
            async move {
                if request.uri().path() == "/downloads/asherin-eye.tar.gz" {
                    Ok(Response::builder()
                        .status(200)
                        .body(bytes.as_ref().clone().into())?)
                } else {
                    Ok(Response::builder().status(404).body("".into())?)
                }
            }
        })
    }

    #[test]
    fn installs_only_a_verified_download_and_never_over_a_copy() {
        let tarball = gzip(&tar(&[
            ("asherin.eye/", EntryType::Directory, ""),
            ("asherin.eye/package.json", EntryType::Regular, "{}"),
        ]));
        let http = serving(tarball.clone());
        let directory = tempfile::tempdir().expect("tempdir");
        let folder = directory.path().join(FOLDER_NAME);
        let url = "https://noah.asherin.com/downloads/asherin-eye.tar.gz";
        let checksum = sha256_hex(&tarball);
        let attempt =
            |url: &str, sha256: &str| smol::block_on(install(&http, url, sha256, &folder, |_| {}));

        let error = attempt(url, &"00".repeat(32)).expect_err("a checksum mismatch is refused");
        assert!(error.to_string().contains("doesn't match"), "{error}");
        assert!(!folder.exists(), "nothing is installed");
        assert!(!directory.path().join(".asherin.eye-download").exists());

        let error = attempt(
            "https://noah.asherin.com/downloads/missing.tar.gz",
            &checksum,
        )
        .expect_err("a missing download is refused");
        assert!(error.to_string().contains("404"), "{error}");
        assert!(!folder.exists());

        attempt(url, &checksum.to_uppercase()).expect("installs");
        assert!(folder.join("package.json").is_file());
        assert!(folder.join("AGENTS.md").is_file());
        assert!(!directory.path().join(".asherin.eye-download").exists());

        std::fs::write(folder.join("package.json"), "edited").expect("write");
        let error = attempt(url, &checksum).expect_err("an existing copy is never replaced");
        assert!(error.to_string().contains("already exists"), "{error}");
        assert_eq!(
            std::fs::read_to_string(folder.join("package.json"))
                .ok()
                .as_deref(),
            Some("edited")
        );
    }

    #[test]
    fn reads_the_port_like_vite() {
        assert_eq!(port_from_dotenv("PORT=5000\n"), Some(5000));
        assert_eq!(
            port_from_dotenv("export PORT = \"5001\" # dev\n"),
            Some(5001)
        );
        assert_eq!(port_from_dotenv("PORT=\nHOST=0.0.0.0\n"), None);
        assert_eq!(port_from_dotenv("VITE_PORT=5002\nPORTAL=1\n"), None);
        assert_eq!(port_from_dotenv("PORT=99999\n"), None);

        let directory = tempfile::tempdir().expect("tempdir");
        assert_eq!(dev_server_port(directory.path()), DEFAULT_PORT);
        std::fs::write(directory.path().join(".env"), "PORT=5000\n").expect("write");
        std::fs::write(directory.path().join(".env.local"), "PORT=5001\n").expect("write");
        assert_eq!(dev_server_port(directory.path()), 5001);
        std::fs::write(
            directory.path().join(".env.development.local"),
            "PORT=5002\n",
        )
        .expect("write");
        assert_eq!(dev_server_port(directory.path()), 5002);
    }

    #[test]
    fn puts_noahs_node_first_on_path_only_when_needed() {
        let system = npm_environment(None, Some(OsString::from("/usr/bin"))).expect("env");
        assert_eq!(
            system.get("PUPPETEER_SKIP_DOWNLOAD").map(String::as_str),
            Some("1")
        );
        assert!(!system.contains_key("PATH"));

        let node_directory = Path::new("/home/me/.local/share/noah/node/bin");
        let inherited =
            std::env::join_paths([Path::new("/usr/bin"), Path::new("/bin")]).expect("joins");
        let managed = npm_environment(Some(node_directory), Some(inherited)).expect("env");
        let path = managed.get("PATH").expect("PATH is set");
        let directories: Vec<PathBuf> = std::env::split_paths(path).collect();
        assert_eq!(
            directories,
            vec![
                node_directory.to_path_buf(),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin")
            ]
        );
    }

    #[test]
    fn detects_a_listening_port() {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
        let port = listener.local_addr().expect("address").port();
        assert!(port_answers(port));
        drop(listener);
        assert!(!port_answers(port));
    }
}
