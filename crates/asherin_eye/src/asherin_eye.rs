//! asherin.eye: the person's own copy of ADAM, a live 3D globe of aircraft,
//! vessels, satellites and cameras. noah downloads its source once, as listed
//! in the signed release manifest, into `~/noah-lab/asherin.eye`. From then on
//! it is an ordinary project the person and shepherd edit: opening the room
//! opens that folder, runs its dev server in a terminal and shows the page in
//! the browser room.

use std::{
    ffi::OsString,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream},
    path::{Component, Path, PathBuf},
    time::Duration,
};

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use async_compression::futures::bufread::GzipDecoder;
use async_tar::EntryType;
use auto_update::noah_release::{self, Asset};
use collections::{HashMap, HashSet};
use futures::{
    AsyncRead, AsyncReadExt as _, AsyncWriteExt as _, StreamExt as _, channel::oneshot,
    io::BufReader,
};
use gpui::{
    App, AppContext as _, AsyncApp, AsyncWindowContext, Focusable as _, Global, SharedString,
    TaskExt as _, WeakEntity,
};
use http_client::{HttpClient as _, HttpClientWithUrl};
use node_runtime::NodeRuntime;
use project::trusted_worktrees::{PathTrust, TrustedWorktrees};
use release_channel::ReleaseChannel;
use sha2::{Digest as _, Sha256};
use task::{HideStrategy, RevealStrategy, RevealTarget, Shell, SpawnInTerminal, TaskId};
use util::ResultExt as _;
use workspace::{
    AppState, MultiWorkspace, OpenMode, OpenOptions, Workspace,
    notifications::{
        NotificationId, dismiss_app_notification, show_app_notification,
        simple_message_notification::MessageNotification,
    },
};

/// The folder the tarball unpacks to and the name of the person's copy.
const FOLDER_NAME: &str = "asherin.eye";
/// ADAM's Vite config falls back to this port when `PORT` isn't set.
const DEFAULT_PORT: u16 = 4173;
/// Must match `TerminalPanel`'s `Panel::persistent_name`.
const TERMINAL_PANEL: &str = "TerminalPanel";
const SERVER_START_TIMEOUT: Duration = Duration::from_secs(120);
const GUIDE: &str = include_str!("../../../assets/shepherd/asherin_eye_guide.md");

pub fn init(cx: &mut App) {
    cx.set_global(EyeState::default());
    cx.on_action(|_: &zed_actions::OpenAsherinEye, cx| open(cx));
}

/// `~/noah-lab/asherin.eye`, next to what people make in noah lab.
pub fn eye_directory() -> PathBuf {
    paths::home_dir().join("noah-lab").join(FOLDER_NAME)
}

#[derive(Default)]
struct EyeState {
    installing: bool,
    /// The window whose terminal runs `npm run dev`, until that command exits.
    server: Option<WeakEntity<Workspace>>,
}

impl Global for EyeState {}

struct EyeNotification;

fn open(cx: &mut App) {
    let folder = eye_directory();
    if focus_open_room(&folder, cx) {
        return;
    }
    if cx.default_global::<EyeState>().installing {
        return;
    }
    if folder.exists() {
        open_room_window(folder, cx);
    } else {
        install(folder, cx);
    }
}

fn show_status(message: impl Into<SharedString>, cx: &mut App) {
    let message = message.into();
    show_app_notification(NotificationId::unique::<EyeNotification>(), cx, move |cx| {
        let message = message.clone();
        cx.new(|cx| MessageNotification::new(message, cx).with_title("asherin.eye"))
    });
}

fn dismiss_status(cx: &mut App) {
    dismiss_app_notification(&NotificationId::unique::<EyeNotification>(), cx);
}

fn show_error(error: &anyhow::Error, cx: &mut App) {
    log::error!("asherin.eye: {error:#}");
    let message: SharedString = format!("{error:#}").into();
    show_app_notification(NotificationId::unique::<EyeNotification>(), cx, move |cx| {
        let message = message.clone();
        cx.new(|cx| {
            MessageNotification::new(message, cx)
                .with_title("asherin.eye couldn't open")
                .primary_message("try again")
                // Deferred so the new attempt's status outlives this
                // notification's own dismissal.
                .primary_on_click(|_window, cx| App::defer(cx, open))
        })
    });
}

fn report(result: Result<()>, cx: &mut AsyncWindowContext) {
    if let Err(error) = result {
        cx.update(|_, cx| show_error(&error, cx)).log_err();
    }
}

fn is_eye_workspace(workspace: &Workspace, folder: &Path, cx: &App) -> bool {
    workspace
        .project()
        .read(cx)
        .visible_worktrees(cx)
        .any(|worktree| worktree.read(cx).abs_path().as_ref() == folder)
}

/// Brings an open asherin.eye window forward and shows its page, starting the
/// dev server again if it has stopped.
fn focus_open_room(folder: &Path, cx: &mut App) -> bool {
    for window in cx.windows() {
        let Some(window) = window.downcast::<MultiWorkspace>() else {
            continue;
        };
        let focused = window
            .update(cx, |multi_workspace, window, cx| {
                let Some(workspace) = multi_workspace
                    .workspaces()
                    .find(|workspace| is_eye_workspace(workspace.read(cx), folder, cx))
                    .cloned()
                else {
                    return false;
                };
                window.activate_window();
                multi_workspace.activate(workspace.clone(), None, window, cx);
                let workspace = workspace.downgrade();
                let folder = folder.to_path_buf();
                window
                    .spawn(cx, async move |cx| {
                        let result = show_or_start_server(workspace, folder, cx).await;
                        report(result, cx);
                    })
                    .detach();
                true
            })
            .unwrap_or(false);
        if focused {
            return true;
        }
    }
    false
}

fn open_room_window(folder: PathBuf, cx: &mut App) {
    let Some(app_state) = AppState::try_global(cx) else {
        return;
    };
    workspace::open_new(
        OpenOptions {
            open_mode: OpenMode::NewWindow,
            ..OpenOptions::default()
        },
        app_state,
        cx,
        move |workspace, window, cx| {
            let project = workspace.project().clone();
            // noah put the folder there itself, so it opens trusted rather
            // than in Restricted Mode, where shepherd could neither edit nor
            // run anything.
            if let Some(trusted_worktrees) = TrustedWorktrees::try_get_global(cx) {
                let worktree_store = project.read(cx).worktree_store();
                trusted_worktrees.update(cx, |trusted_worktrees, cx| {
                    trusted_worktrees.trust(
                        &worktree_store,
                        HashSet::from_iter([PathTrust::AbsPath(folder.clone())]),
                        cx,
                    );
                });
            }
            project
                .update(cx, |project, cx| {
                    project.find_or_create_worktree(&folder, true, cx)
                })
                .detach_and_log_err(cx);
            cx.spawn_in(window, async move |workspace, cx| {
                let result = show_or_start_server(workspace, folder, cx).await;
                report(result, cx);
            })
            .detach();
        },
    )
    .detach_and_log_err(cx);
}

fn install(folder: PathBuf, cx: &mut App) {
    let Some(app_state) = AppState::try_global(cx) else {
        return;
    };
    cx.default_global::<EyeState>().installing = true;
    show_status("getting asherin.eye's source…", cx);
    let http = app_state.client.http_client();
    let release_channel = ReleaseChannel::try_global(cx);
    cx.spawn(async move |cx| {
        let result = async {
            let asset = noah_release::fetch_asherin_eye_asset(&http, release_channel).await?;
            install_from_asset(&http, &asset, &folder, cx).await
        }
        .await;
        cx.update(|cx| {
            cx.default_global::<EyeState>().installing = false;
            match result {
                Ok(()) => {
                    dismiss_status(cx);
                    open_room_window(folder, cx);
                }
                Err(error) => show_error(&error, cx),
            }
        });
    })
    .detach();
}

/// Downloads the tarball the signed manifest lists, checks it against the
/// signed checksum and unpacks it as `folder`. An existing folder is the
/// person's edited copy, so it is never replaced.
async fn install_from_asset(
    http: &HttpClientWithUrl,
    asset: &Asset,
    folder: &Path,
    cx: &mut AsyncApp,
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
        download(http, asset, &tarball, |received, total| {
            let Some(total) = total.filter(|total| *total > 0) else {
                return;
            };
            let percent = (received.min(total) * 100 / total / 10) * 10;
            if last_percent != Some(percent) {
                last_percent = Some(percent);
                cx.update(|cx| show_status(format!("downloading asherin.eye… {percent}%"), cx));
            }
        })
        .await?;
        cx.update(|cx| show_status("unpacking asherin.eye…", cx));
        cx.background_spawn({
            let unpacked = unpacked.clone();
            async move { extract_tarball(&tarball, &unpacked).await }
        })
        .await?;
        move_into_place(&unpacked.join(FOLDER_NAME), folder)
    }
    .await;
    std::fs::remove_dir_all(&staging)
        .with_context(|| format!("couldn't remove {}", staging.display()))
        .log_err();
    result
}

async fn download(
    http: &HttpClientWithUrl,
    asset: &Asset,
    destination: &Path,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    let mut response = http
        .get(&asset.url, Default::default(), true)
        .await
        .with_context(|| format!("couldn't reach {}; check the internet connection", asset.url))?;
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
        actual.eq_ignore_ascii_case(asset.sha256.trim()),
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

async fn show_or_start_server(
    workspace: WeakEntity<Workspace>,
    folder: PathBuf,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    let port = cx
        .background_spawn({
            let folder = folder.clone();
            async move { dev_server_port(&folder) }
        })
        .await;
    let url = format!("http://localhost:{port}");
    if server_answers(port, cx).await {
        return show_in_browser_room(url, cx);
    }
    let already_starting = cx.update(|_, cx| {
        cx.default_global::<EyeState>()
            .server
            .as_ref()
            .is_some_and(|server| server.upgrade().is_some())
    })?;
    // The start already under way shows the page once the port answers.
    if already_starting {
        return Ok(());
    }
    cx.update(|_, cx| cx.default_global::<EyeState>().server = Some(workspace.clone()))?;
    let result = start_server(&workspace, &folder, port, &url, cx).await;
    if result.is_err() {
        cx.update(|_, cx| forget_server(&workspace, cx)).log_err();
    }
    result
}

fn forget_server(workspace: &WeakEntity<Workspace>, cx: &mut App) {
    let state = cx.default_global::<EyeState>();
    if state
        .server
        .as_ref()
        .is_some_and(|server| server.entity_id() == workspace.entity_id())
    {
        state.server = None;
    }
}

async fn start_server(
    workspace: &WeakEntity<Workspace>,
    folder: &Path,
    port: u16,
    url: &str,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    wait_for_terminal(workspace, cx).await?;

    let node_runtime = cx
        .update(|_, cx| AppState::try_global(cx).map(|state| state.node_runtime.clone()))?
        .context("noah isn't ready to start asherin.eye yet")?;
    let node_directory = node_directory_for_npm(node_runtime, cx).await?;
    let environment = npm_environment(node_directory.as_deref(), std::env::var_os("PATH"))?;

    if !folder.join("node_modules").is_dir() {
        cx.update(|_, cx| {
            show_status(
                "installing asherin.eye's packages with npm install; the first time takes a \
                 minute",
                cx,
            )
        })?;
        let install = npm_task(&["install"], folder, &environment, RevealStrategy::Always);
        let install = workspace.update_in(cx, |workspace, window, cx| {
            workspace.spawn_in_terminal(install, window, cx)
        })?;
        match install.await {
            Some(Ok(status)) if status.success() => {}
            Some(Ok(status)) => bail!("npm install failed ({status}); the terminal shows why"),
            Some(Err(error)) => return Err(error.context("couldn't run npm install")),
            None => bail!("npm install stopped before it finished"),
        }
    }

    cx.update(|_, cx| show_status(format!("starting asherin.eye at {url}…"), cx))?;
    let dev_server = npm_task(&["run", "dev"], folder, &environment, RevealStrategy::NoFocus);
    let dev_server = workspace.update_in(cx, |workspace, window, cx| {
        workspace.spawn_in_terminal(dev_server, window, cx)
    })?;
    let (exited_sender, mut exited) = oneshot::channel::<()>();
    cx.spawn({
        let workspace = workspace.clone();
        async move |cx| {
            if let Some(Err(error)) = dev_server.await {
                log::error!("asherin.eye's dev server couldn't run: {error:#}");
            }
            cx.update(|_, cx| forget_server(&workspace, cx)).log_err();
            exited_sender.send(()).ok();
        }
    })
    .detach();

    let poll_interval = Duration::from_millis(500);
    let attempts = SERVER_START_TIMEOUT.as_millis() / poll_interval.as_millis();
    for _ in 0..attempts {
        if server_answers(port, cx).await {
            cx.update(|_, cx| dismiss_status(cx))?;
            return show_in_browser_room(url.to_string(), cx);
        }
        if !matches!(exited.try_recv(), Ok(None)) {
            bail!("asherin.eye's dev server stopped before it started; the terminal shows why");
        }
        cx.background_executor().timer(poll_interval).await;
    }
    bail!(
        "asherin.eye's dev server didn't answer at {url} within {} seconds; the terminal \
         shows what it's doing",
        SERVER_START_TIMEOUT.as_secs()
    )
}

/// A new window's panels load after it opens, and tasks can't run before the
/// terminal panel exists.
async fn wait_for_terminal(
    workspace: &WeakEntity<Workspace>,
    cx: &mut AsyncWindowContext,
) -> Result<()> {
    for _ in 0..200 {
        let ready = workspace.read_with(cx, |workspace, cx| {
            workspace.all_docks().iter().any(|dock| {
                dock.read(cx)
                    .panel_index_for_persistent_name(TERMINAL_PANEL, cx)
                    .is_some()
            })
        })?;
        if ready {
            return Ok(());
        }
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
    }
    bail!("noah's terminal didn't open, so asherin.eye's dev server couldn't start")
}

/// `None` when `npm` is already on PATH; otherwise the folder of the Node.js
/// noah manages, whose `npm` sits next to `node`.
async fn node_directory_for_npm(
    node_runtime: NodeRuntime,
    cx: &mut AsyncWindowContext,
) -> Result<Option<PathBuf>> {
    let system_npm = cx
        .background_spawn(async { which::which("npm").is_ok() })
        .await;
    if system_npm {
        return Ok(None);
    }
    cx.update(|_, cx| show_status("getting Node.js for asherin.eye…", cx))?;
    let node = node_runtime
        .binary_path()
        .await
        .context("asherin.eye needs Node.js, and noah couldn't find or download it")?;
    Ok(node.parent().map(Path::to_path_buf))
}

fn npm_environment(
    node_directory: Option<&Path>,
    inherited_path: Option<OsString>,
) -> Result<HashMap<String, String>> {
    let mut environment = HashMap::default();
    // ADAM's QA scripts use puppeteer, which otherwise downloads a whole
    // Chromium during npm install.
    environment.insert("PUPPETEER_SKIP_DOWNLOAD".to_string(), "1".to_string());
    if let Some(node_directory) = node_directory {
        let directories = std::iter::once(node_directory.to_path_buf())
            .chain(inherited_path.iter().flat_map(std::env::split_paths));
        let path = std::env::join_paths(directories)
            .context("couldn't put noah's Node.js on PATH")?;
        environment.insert("PATH".to_string(), path.to_string_lossy().into_owned());
    }
    Ok(environment)
}

fn npm_task(
    args: &[&str],
    folder: &Path,
    environment: &HashMap<String, String>,
    reveal: RevealStrategy,
) -> SpawnInTerminal {
    let command_label = format!("npm {}", args.join(" "));
    let label = format!("asherin.eye: {command_label}");
    SpawnInTerminal {
        id: TaskId(format!("asherin-eye-{}", args.join("-"))),
        full_label: label.clone(),
        label,
        command: Some("npm".to_string()),
        args: args.iter().map(|arg| arg.to_string()).collect(),
        command_label,
        cwd: Some(folder.to_path_buf()),
        env: environment.clone(),
        use_new_terminal: true,
        allow_concurrent_runs: false,
        reveal,
        reveal_target: RevealTarget::Dock,
        hide: HideStrategy::Never,
        shell: Shell::System,
        show_summary: true,
        show_command: true,
        show_rerun: true,
        ..SpawnInTerminal::default()
    }
}

fn show_in_browser_room(url: String, cx: &mut AsyncWindowContext) -> Result<()> {
    cx.update(|window, cx| {
        window.activate_window();
        let Some(workspace) = window
            .root::<MultiWorkspace>()
            .flatten()
            .map(|multi_workspace| multi_workspace.read(cx).workspace().clone())
        else {
            return;
        };
        // Dispatched from inside the workspace so its browser room handles
        // it, and outside any workspace update, which the handler needs.
        workspace.focus_handle(cx).dispatch_action(
            &zed_actions::OpenInBrowserRoom { url },
            window,
            cx,
        );
    })
}

async fn server_answers(port: u16, cx: &AsyncWindowContext) -> bool {
    cx.background_spawn(async move { port_answers(port) }).await
}

/// Vite listens on `localhost`, which is IPv4 on some systems and IPv6 on
/// others.
fn port_answers(port: u16) -> bool {
    [IpAddr::V4(Ipv4Addr::LOCALHOST), IpAddr::V6(Ipv6Addr::LOCALHOST)]
        .into_iter()
        .any(|address| {
            TcpStream::connect_timeout(
                &SocketAddr::new(address, port),
                Duration::from_millis(250),
            )
            .is_ok()
        })
}

/// ADAM's Vite config reads `PORT` from the dotenv files Vite loads for
/// `npm run dev`, where later files in this list win.
fn dev_server_port(folder: &Path) -> u16 {
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
            ("asherin.eye/src/layers/a.js", EntryType::Regular, "export {}"),
        ]);
        unpack_bytes(bytes, directory.path()).expect("unpacks");
        let root = directory.path().join(FOLDER_NAME);
        assert_eq!(
            std::fs::read_to_string(root.join("package.json")).ok().as_deref(),
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
            "/tmp/escaped.txt",
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
            gzip(&tar(&[("asherin.eye/README.md", EntryType::Regular, "# ADAM")])),
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
            std::fs::read_to_string(existing.join("mine.js")).ok().as_deref(),
            Some("edited")
        );
        assert!(!existing.join("package.json").exists());

        let folder = directory.path().join(FOLDER_NAME);
        move_into_place(&unpacked, &folder).expect("moves");
        assert!(folder.join("package.json").is_file());
        assert_eq!(
            std::fs::read_to_string(folder.join("AGENTS.md")).ok().as_deref(),
            Some(GUIDE)
        );

        let with_guide = directory.path().join("with-guide").join(FOLDER_NAME);
        std::fs::create_dir_all(&with_guide).expect("mkdir");
        std::fs::write(with_guide.join("AGENTS.md"), "ADAM's own").expect("write");
        let target = directory.path().join("second");
        move_into_place(&with_guide, &target).expect("moves");
        assert_eq!(
            std::fs::read_to_string(target.join("AGENTS.md")).ok().as_deref(),
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

    #[gpui::test]
    async fn installs_a_verified_download(cx: &mut gpui::TestAppContext) {
        let tarball = gzip(&tar(&[
            ("asherin.eye/", EntryType::Directory, ""),
            ("asherin.eye/package.json", EntryType::Regular, "{}"),
        ]));
        let http = serving(tarball.clone());
        let directory = tempfile::tempdir().expect("tempdir");
        let folder = directory.path().join(FOLDER_NAME);
        let asset = Asset {
            url: "https://noah.asherin.com/downloads/asherin-eye.tar.gz".to_string(),
            sha256: sha256_hex(&tarball),
        };
        let mut async_cx = cx.to_async();

        let tampered = Asset {
            sha256: "00".repeat(32),
            ..asset.clone()
        };
        let error = install_from_asset(&http, &tampered, &folder, &mut async_cx)
            .await
            .expect_err("a checksum mismatch is refused");
        assert!(error.to_string().contains("doesn't match"), "{error}");
        assert!(!folder.exists(), "nothing is installed");
        assert!(!directory.path().join(".asherin.eye-download").exists());

        let missing = Asset {
            url: "https://noah.asherin.com/downloads/missing.tar.gz".to_string(),
            ..asset.clone()
        };
        assert!(
            install_from_asset(&http, &missing, &folder, &mut async_cx)
                .await
                .is_err()
        );
        assert!(!folder.exists());

        install_from_asset(&http, &asset, &folder, &mut async_cx)
            .await
            .expect("installs");
        assert!(folder.join("package.json").is_file());
        assert!(folder.join("AGENTS.md").is_file());
        assert!(!directory.path().join(".asherin.eye-download").exists());

        std::fs::write(folder.join("package.json"), "edited").expect("write");
        assert!(
            install_from_asset(&http, &asset, &folder, &mut async_cx)
                .await
                .is_err(),
            "an existing copy is never replaced"
        );
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
        assert_eq!(port_from_dotenv("export PORT = \"5001\" # dev\n"), Some(5001));
        assert_eq!(port_from_dotenv("PORT=\nHOST=0.0.0.0\n"), None);
        assert_eq!(port_from_dotenv("VITE_PORT=5002\nPORTAL=1\n"), None);
        assert_eq!(port_from_dotenv("PORT=99999\n"), None);

        let directory = tempfile::tempdir().expect("tempdir");
        assert_eq!(dev_server_port(directory.path()), DEFAULT_PORT);
        std::fs::write(directory.path().join(".env"), "PORT=5000\n").expect("write");
        std::fs::write(directory.path().join(".env.local"), "PORT=5001\n").expect("write");
        assert_eq!(dev_server_port(directory.path()), 5001);
        std::fs::write(directory.path().join(".env.development.local"), "PORT=5002\n")
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
        let inherited = std::env::join_paths([Path::new("/usr/bin"), Path::new("/bin")])
            .expect("joins");
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

    #[test]
    fn npm_tasks_run_in_the_eye_folder() {
        let folder = Path::new("/home/me/noah-lab/asherin.eye");
        let environment = npm_environment(None, None).expect("env");
        let task = npm_task(&["run", "dev"], folder, &environment, RevealStrategy::NoFocus);
        assert_eq!(task.command.as_deref(), Some("npm"));
        assert_eq!(task.args, vec!["run".to_string(), "dev".to_string()]);
        assert_eq!(task.cwd.as_deref(), Some(folder));
        assert_eq!(task.label, "asherin.eye: npm run dev");
        assert_eq!(
            task.env.get("PUPPETEER_SKIP_DOWNLOAD").map(String::as_str),
            Some("1")
        );
    }
}
