//! asherin.eye: the person's own copy of ADAM, a live 3D globe of aircraft,
//! vessels, satellites and cameras. noah downloads its source once, as listed
//! in the signed release manifest, into `~/noah-lab/asherin.eye`. From then on
//! it is an ordinary project the person and shepherd edit: opening the room
//! opens that folder, runs its dev server in a terminal and shows the page in
//! the browser room.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context as _, Result, bail};
use asherin_eye_install::{
    FOLDER_NAME, InstallStep, dev_server_port, npm_environment, port_answers,
};
use auto_update::noah_release;
use collections::{HashMap, HashSet};
use futures::channel::oneshot;
use gpui::{
    App, AppContext as _, AsyncWindowContext, Focusable as _, Global, SharedString, TaskExt as _,
    WeakEntity,
};
use node_runtime::NodeRuntime;
use project::trusted_worktrees::{PathTrust, TrustedWorktrees};
use release_channel::ReleaseChannel;
use task::{HideStrategy, RevealStrategy, RevealTarget, Shell, SpawnInTerminal, TaskId};
use util::ResultExt as _;
use workspace::{
    AppState, MultiWorkspace, OpenMode, OpenOptions, Workspace,
    notifications::{
        NotificationId, dismiss_app_notification, show_app_notification,
        simple_message_notification::MessageNotification,
    },
};

/// Must match `TerminalPanel`'s `Panel::persistent_name`.
const TERMINAL_PANEL: &str = "TerminalPanel";
const SERVER_START_TIMEOUT: Duration = Duration::from_secs(120);

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
            asherin_eye_install::install(&http, &asset.url, &asset.sha256, &folder, |step| {
                let message = match step {
                    InstallStep::Downloading { percent } => {
                        format!("downloading asherin.eye… {percent}%")
                    }
                    InstallStep::Unpacking => "unpacking asherin.eye…".to_string(),
                };
                cx.update(|cx| show_status(message, cx));
            })
            .await
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
    let environment: HashMap<String, String> =
        npm_environment(node_directory.as_deref(), std::env::var_os("PATH"))?
            .into_iter()
            .collect();

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
    let dev_server = npm_task(
        &["run", "dev"],
        folder,
        &environment,
        RevealStrategy::NoFocus,
    );
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
