//! Windows doesn't come with Git and noah's installer can't carry it, so when
//! a machine has none, noah downloads MinGit, the portable build of Git for
//! Windows (with Git Credential Manager, so private repositories can sign in),
//! into its data directory.

use anyhow::{Context as _, Result};
use futures::AsyncReadExt as _;
use http_client::{AsyncBody, HttpClient};
use sha2::{Digest as _, Sha256};
use std::path::PathBuf;
use std::sync::Arc;

const MINGIT_URL: &str = "https://github.com/git-for-windows/git/releases/download/v2.55.0.windows.1/MinGit-2.55.0-64-bit.zip";
const MINGIT_SHA256: &str = "31497e7968196332263459ee319d2524e3ebc5786ab895e2abad34ffdd4f4ebf";

fn install_directory() -> PathBuf {
    paths::downloaded_git_directory()
}

/// The Git noah downloaded earlier, if there is one.
pub fn installed_git_binary() -> Option<PathBuf> {
    let git = install_directory().join("cmd").join("git.exe");
    git.is_file().then_some(git)
}

pub async fn install_git(http_client: Arc<dyn HttpClient>) -> Result<PathBuf> {
    let mut response = http_client
        .get(MINGIT_URL, AsyncBody::default(), true)
        .await
        .context("couldn't download Git")?;
    anyhow::ensure!(
        response.status().is_success(),
        "couldn't download Git: the server answered {}",
        response.status()
    );
    let mut archive = Vec::new();
    response
        .body_mut()
        .read_to_end(&mut archive)
        .await
        .context("couldn't download Git")?;
    let digest = format!("{:x}", Sha256::digest(&archive));
    anyhow::ensure!(
        digest == MINGIT_SHA256,
        "the downloaded Git didn't match its checksum, so it wasn't installed"
    );

    let destination = install_directory();
    let staging = destination.with_extension("partial");
    remove_directory_if_present(&staging)?;
    util::archive::extract_zip(&staging, futures::io::Cursor::new(archive))
        .await
        .context("couldn't unpack Git")?;
    remove_directory_if_present(&destination)?;
    std::fs::rename(&staging, &destination)
        .with_context(|| format!("couldn't move Git into {}", destination.display()))?;
    installed_git_binary().context("Git was unpacked, but git.exe is missing")
}

fn remove_directory_if_present(directory: &std::path::Path) -> Result<()> {
    match std::fs::remove_dir_all(directory) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("couldn't remove {}", directory.display()))
        }
    }
}
