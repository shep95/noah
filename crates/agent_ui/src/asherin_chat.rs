//! asherin.chat and asherin.pages: shepherd outside any code project. Each is
//! a folder under `~/.noah` that opens in its own window. Chat threads use the
//! chat profile, so they can talk, search and read but never edit or run;
//! pages is where shepherd makes documents, books and slideshows.
//!
//! This module also holds the bridges between asherin.chat and the shepherd
//! room ("take this to code" and "discuss in chat"), custom chat agents, and
//! the actions behind them. The conversation tree lives in
//! `asherin_chat_tree`, and the chat room's parts of the thread view in
//! `conversation_view::thread_view::chat_room`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use agent::ChatConversation;
use agent::chat_tree;
use agent_client_protocol::schema::v1 as acp;
use asherin_chat::tree::ConversationKind;
use collections::HashSet;
use editor::Editor;
use fuzzy::{StringMatch, StringMatchCandidate};
use gpui::{
    App, AsyncApp, Context, DismissEvent, Entity, Global, SharedString, Task, TaskExt as _,
    WeakEntity, Window, WindowHandle, actions,
};
use picker::{Picker, PickerDelegate};
use project::trusted_worktrees::{PathTrust, TrustedWorktrees};
use ui::{ContextMenu, ListItem, ListItemSpacing, prelude::*};
use util::ResultExt as _;
use workspace::notifications::NotificationId;
use workspace::{
    MultiWorkspace, OpenOptions, OpenVisible, Toast, Workspace, WorkspaceDb,
};

use crate::conversation_view::ThreadView;
use crate::{AgentInitialContent, AgentPanel, ConversationView};

const PAGES_GUIDE: &str = include_str!("../../../assets/shepherd/pages_guide.md");
const SEARCH_GUIDE: &str = include_str!("../../../assets/shepherd/search_guide.md");
const BOARD_GUIDE: &str = include_str!("../../../assets/shepherd/board_guide.md");
const PROMPT_PROJECT_GUIDE: &str =
    include_str!("../../../assets/shepherd/prompt_project_guide.md");

actions!(
    asherin_chat,
    [
        /// Shows or hides asherin.chat's conversation tree. Outside
        /// asherin.chat this toggles the left dock.
        ToggleTree,
        /// Takes this asherin.chat conversation to the shepherd room: its
        /// decisions become a summary and draft spec clauses, and a shepherd
        /// thread starts from them.
        TakeToCode,
        /// Opens a side conversation in asherin.chat about this shepherd
        /// thread, so questions don't fill its context.
        DiscussInChat,
        /// Posts what this branch concluded into the conversation it came from.
        BringBack,
        /// Creates a custom asherin.chat agent from a template.
        NewChatAgent,
        /// Starts a new asherin.chat conversation in a new tab.
        NewChatTab,
        /// Closes the current asherin.chat tab. The conversation stays in
        /// the tree.
        CloseChatTab,
        /// Switches to the next asherin.chat tab.
        NextChatTab,
        /// Switches to the previous asherin.chat tab.
        PreviousChatTab,
        /// Reopens the asherin.chat tab closed most recently.
        ReopenChatTab,
        /// Opens the current asherin.chat conversation beside the chat, to
        /// compare two conversations.
        OpenChatBeside,
    ]
);

/// Switches to the asherin.chat tab at this position, counting from 1.
#[derive(
    Clone, PartialEq, Debug, serde::Deserialize, schemars::JsonSchema, Default, gpui::Action,
)]
#[action(namespace = asherin_chat)]
pub struct ActivateChatTab(pub usize);

pub fn init(cx: &mut App) {
    cx.on_action(|_: &zed_actions::OpenAsherinChat, cx| {
        open_room(paths::chat_directory(), None, None, cx)
    });
    cx.on_action(|_: &zed_actions::OpenAsherinPages, cx| {
        open_room(paths::pages_directory(), Some(PAGES_GUIDE), None, cx)
    });
    cx.on_action(|_: &zed_actions::OpenAsherinSearch, cx| {
        open_room(paths::search_directory(), Some(SEARCH_GUIDE), None, cx)
    });
    cx.on_action(|_: &zed_actions::OpenAsherinBoard, cx| {
        open_room(
            paths::board_directory(),
            Some(BOARD_GUIDE),
            Some(Box::new(|workspace, _panel, window, cx| {
                workspace.close_all_docks(window, cx);
                noah_board::open_in(workspace, paths::board_directory(), window, cx);
            })),
            cx,
        )
    });
    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        let Some(window) = window else {
            return;
        };
        workspace.register_action(|workspace, _: &zed_actions::StartProjectFromPrompt, window, cx| {
            start_project_from_prompt(workspace, window, cx);
        });
        workspace.register_action(|workspace, _: &ToggleTree, window, cx| {
            toggle_tree(workspace, window, cx);
        });
        workspace.register_action(|workspace, _: &TakeToCode, window, cx| {
            if let Some(thread_view) = active_thread_view(workspace, cx) {
                thread_view.update(cx, |thread_view, cx| {
                    thread_view.take_to_code(String::new(), window, cx)
                });
            }
        });
        workspace.register_action(|workspace, _: &DiscussInChat, window, cx| {
            if let Some(thread_view) = active_thread_view(workspace, cx) {
                thread_view.update(cx, |thread_view, cx| {
                    thread_view.discuss_in_chat(window, cx)
                });
            }
        });
        workspace.register_action(|workspace, _: &BringBack, window, cx| {
            if let Some(thread_view) = active_thread_view(workspace, cx) {
                thread_view.update(cx, |thread_view, cx| thread_view.bring_back(window, cx));
            }
        });
        workspace.register_action(|workspace, _: &NewChatAgent, window, cx| {
            open_new_chat_agent(workspace, None, window, cx);
        });
        crate::asherin_chat_tree::register_tab_actions(workspace);
        crate::asherin_chat_tree::attach_to_chat_room(workspace, window, cx);
        crate::asherin_search::attach(workspace, window, cx);
    })
    .detach();
}

pub(crate) fn is_chat_workspace(workspace: &Workspace, cx: &App) -> bool {
    agent::Thread::is_chat_project(workspace.project(), cx)
}

/// The next free `untitled-N` under `base`, created.
fn next_untitled_folder(base: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(base)?;
    for index in 1..10_000 {
        let candidate = base.join(format!("untitled-{index}"));
        if !candidate.exists() {
            std::fs::create_dir(&candidate)?;
            return Ok(candidate);
        }
    }
    anyhow::bail!("too many untitled projects in {}", base.display())
}

/// The welcome page's way in for someone with an idea and no folder: makes
/// a project under ~/noah-projects, opens it in this window, and starts a
/// shepherd thread there with the composer waiting for the prompt.
fn start_project_from_prompt(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let folder = match next_untitled_folder(&paths::prompt_projects_directory()) {
        Ok(folder) => folder,
        Err(error) => {
            workspace.show_toast(
                Toast::new(
                    NotificationId::named("prompt-project".into()),
                    format!("couldn't make a project folder: {error:#}"),
                ),
                cx,
            );
            return;
        }
    };
    if let Err(error) = std::fs::write(folder.join("AGENTS.md"), PROMPT_PROJECT_GUIDE) {
        log::error!("couldn't write the project guide in {}: {error}", folder.display());
    }
    let project = workspace.project().clone();
    // A folder noah just made is the person's own; opening it trusted keeps
    // shepherd's tools available from the first message.
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
    let added = project.update(cx, |project, cx| {
        project.find_or_create_worktree(&folder, true, cx)
    });
    cx.spawn_in(window, async move |workspace, cx| {
        added.await?;
        workspace.update_in(cx, |workspace, window, cx| {
            if let Some(panel) = workspace.focus_panel::<AgentPanel>(window, cx) {
                panel.update(cx, |panel, cx| {
                    panel.new_thread_with_content(
                        AgentInitialContent::ContentBlock {
                            blocks: Vec::new(),
                            auto_submit: false,
                        },
                        window,
                        cx,
                    );
                });
            }
            workspace.show_toast(
                Toast::new(
                    NotificationId::named("prompt-project".into()),
                    format!(
                        "your project lives in {}. tell shepherd what to build.",
                        folder.display()
                    ),
                )
                .autohide(),
                cx,
            );
        })?;
        anyhow::Ok(())
    })
    .detach_and_log_err(cx);
}

fn active_thread_view(workspace: &Workspace, cx: &App) -> Option<Entity<ThreadView>> {
    workspace
        .panel::<AgentPanel>(cx)?
        .read(cx)
        .active_conversation_view()?
        .read(cx)
        .root_thread_view()
}

fn toggle_tree(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let Some(panel) = workspace
        .panel::<crate::asherin_chat_tree::ChatTreePanel>(cx)
        .filter(|_| is_chat_workspace(workspace, cx))
    else {
        window.dispatch_action(Box::new(workspace::ToggleLeftDock), cx);
        return;
    };
    let dock = workspace.left_dock().read(cx);
    let tree_is_showing = dock.is_open()
        && dock
            .visible_panel()
            .is_some_and(|visible| visible.panel_id() == panel.entity_id());
    if tree_is_showing {
        workspace.close_panel::<crate::asherin_chat_tree::ChatTreePanel>(window, cx);
    } else {
        workspace.focus_panel::<crate::asherin_chat_tree::ChatTreePanel>(window, cx);
    }
}

/// Adds asherin.chat's entries to the agent panel's thread menu.
pub(crate) fn extend_thread_menu(
    menu: ContextMenu,
    thread_view: Entity<ThreadView>,
    cx: &App,
) -> ContextMenu {
    let view = thread_view.read(cx);
    if view.is_subagent_view() {
        return menu;
    }
    if view.is_chat_room(cx) {
        let is_branch = view.is_chat_branch();
        let menu = menu.entry("take this to code", None, {
            let thread_view = thread_view.clone();
            move |window, cx| {
                thread_view.update(cx, |thread_view, cx| {
                    thread_view.take_to_code(String::new(), window, cx)
                });
            }
        });
        if is_branch {
            menu.entry("bring back to parent", None, move |window, cx| {
                thread_view.update(cx, |thread_view, cx| thread_view.bring_back(window, cx));
            })
        } else {
            menu
        }
    } else {
        menu.entry("discuss in asherin.chat", None, move |window, cx| {
            thread_view.update(cx, |thread_view, cx| {
                thread_view.discuss_in_chat(window, cx)
            });
        })
    }
}

pub(crate) fn is_room_workspace(workspace: &Workspace, folder: &PathBuf, cx: &App) -> bool {
    workspace
        .project()
        .read(cx)
        .visible_worktrees(cx)
        .any(|worktree| worktree.read(cx).abs_path().as_ref() == folder.as_path())
}

/// Runs in the room's window once shepherd's panel is ready.
pub(crate) type RoomReady = Box<
    dyn FnOnce(&mut Workspace, Entity<AgentPanel>, &mut Window, &mut Context<Workspace>) + Send,
>;

/// Focuses the room's window when one is open, and otherwise opens a window
/// on the room's folder with shepherd ready. `guide` becomes the folder's
/// AGENTS.md the first time, which shepherd reads as the room's instructions.
pub(crate) fn open_room(
    folder: PathBuf,
    guide: Option<&'static str>,
    then: Option<RoomReady>,
    cx: &mut App,
) {
    if let Err(error) = std::fs::create_dir_all(&folder) {
        log::error!("couldn't create {}: {error}", folder.display());
        return;
    }
    if let Some(guide) = guide {
        let guide_path = folder.join("AGENTS.md");
        if !guide_path.exists()
            && let Err(error) = std::fs::write(&guide_path, guide)
        {
            log::error!("couldn't write {}: {error}", guide_path.display());
        }
    }
    // The room is a workspace in the window the person is using, beside
    // their project, never a window of its own. Panels load after the
    // workspace opens, so shepherd's is awaited before it is shown.
    let init: Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + Send> =
        Box::new(move |_workspace, window, cx| {
            cx.spawn_in(window, async move |workspace, cx| {
                let mut then = then;
                for _ in 0..100 {
                    let shown = workspace.update_in(cx, |workspace, window, cx| {
                        if workspace.panel::<AgentPanel>(cx).is_none() {
                            return false;
                        }
                        let Some(panel) = workspace.focus_panel::<AgentPanel>(window, cx) else {
                            return false;
                        };
                        if let Some(then) = then.take() {
                            then(workspace, panel, window, cx);
                        }
                        true
                    })?;
                    if shown {
                        break;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(50))
                        .await;
                }
                anyhow::Ok(())
            })
            .detach_and_log_err(cx);
        });
    workspace::open_noah_folder_in_active_window(folder, init, cx).detach_and_log_err(cx);
}

/// Bumped whenever a conversation's place in the tree changes in a way the
/// thread list doesn't show, such as a new branch link or a chosen agent.
pub(crate) struct ChatTreeRevision;

struct GlobalChatTreeRevision(Entity<ChatTreeRevision>);

impl Global for GlobalChatTreeRevision {}

pub(crate) fn chat_tree_revision(cx: &mut App) -> Entity<ChatTreeRevision> {
    if let Some(global) = cx.try_global::<GlobalChatTreeRevision>() {
        return global.0.clone();
    }
    let revision = cx.new(|_| ChatTreeRevision);
    cx.set_global(GlobalChatTreeRevision(revision.clone()));
    revision
}

pub(crate) fn notify_chat_tree_changed(cx: &mut App) {
    chat_tree_revision(cx).update(cx, |_, cx| cx.notify());
}

fn toast_id() -> NotificationId {
    NotificationId::named("asherin-chat".into())
}

pub(crate) fn show_chat_toast(
    workspace: &WeakEntity<Workspace>,
    message: impl Into<SharedString>,
    autohide: bool,
    cx: &mut App,
) {
    let message: SharedString = message.into();
    workspace
        .update(cx, |workspace, cx| {
            let toast = Toast::new(toast_id(), message.to_string());
            let toast = if autohide { toast.autohide() } else { toast };
            workspace.show_toast(toast, cx);
        })
        .log_err();
}

/// Waits until a new conversation view has a session, which happens once
/// its agent connects.
pub(crate) async fn wait_for_session(
    conversation_view: &Entity<ConversationView>,
    cx: &mut AsyncApp,
) -> anyhow::Result<acp::SessionId> {
    for _ in 0..300 {
        let session_id = conversation_view.read_with(cx, |conversation_view, _| {
            conversation_view.root_session_id.clone()
        });
        if let Some(session_id) = session_id {
            return Ok(session_id);
        }
        cx.background_executor()
            .timer(Duration::from_millis(100))
            .await;
    }
    anyhow::bail!("the new shepherd thread didn't start")
}

/// Opens a side conversation in asherin.chat about the shepherd-room thread
/// `source`, with `seed` in its composer, and links it in the tree.
pub(crate) fn open_side_chat(
    source: acp::SessionId,
    source_title: SharedString,
    seed: String,
    cx: &mut App,
) {
    open_room(
        paths::chat_directory(),
        None,
        Some(Box::new(move |_workspace, panel, window, cx| {
            let conversation_view = panel.update(cx, |panel, cx| {
                panel.new_thread_with_content(
                    AgentInitialContent::ContentBlock {
                        blocks: vec![acp::ContentBlock::Text(acp::TextContent::new(seed))],
                        auto_submit: false,
                    },
                    window,
                    cx,
                )
            });
            cx.spawn(async move |_, cx| {
                let session_id = wait_for_session(&conversation_view, cx).await?;
                let mut side =
                    ChatConversation::new(session_id, source_title, ConversationKind::Side);
                side.agent_thread_id = Some(source);
                cx.update(|cx| chat_tree::link_chat_conversation(side, cx))
                    .await?;
                cx.update(notify_chat_tree_changed);
                anyhow::Ok(())
            })
            .detach_and_log_err(cx);
        })),
        cx,
    );
}

/// A project "take this to code" can go to.
#[derive(Clone, Debug)]
struct CodeProject {
    name: SharedString,
    path: PathBuf,
}

fn is_noah_room(path: &Path) -> bool {
    path.starts_with(paths::home_dir().join(".noah"))
}

fn project_name(path: &Path) -> SharedString {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
        .into()
}

/// The root folders of the projects open in any window.
fn open_project_roots(cx: &App) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for window in cx.windows() {
        let Some(window) = window.downcast::<MultiWorkspace>() else {
            continue;
        };
        let Ok(multi_workspace) = window.read(cx) else {
            continue;
        };
        for workspace in multi_workspace.workspaces() {
            let project = workspace.read(cx).project().read(cx);
            if !project.is_local() {
                continue;
            }
            if let Some(worktree) = project.visible_worktrees(cx).next() {
                let root = worktree.read(cx).abs_path().to_path_buf();
                if !is_noah_room(&root) && !roots.contains(&root) {
                    roots.push(root);
                }
            }
        }
    }
    roots
}

struct CodeProjectDelegate {
    projects: Vec<CodeProject>,
    matches: Vec<StringMatch>,
    selected_index: usize,
    on_pick: Option<Box<dyn FnOnce(CodeProject, &mut App)>>,
}

impl PickerDelegate for CodeProjectDelegate {
    type ListItem = ListItem;

    fn name() -> &'static str {
        "asherin.chat code project picker"
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn selected_index(&self) -> usize {
        self.selected_index
    }

    fn set_selected_index(&mut self, ix: usize, _: &mut Window, _: &mut Context<Picker<Self>>) {
        self.selected_index = ix;
    }

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> Arc<str> {
        "take this to which project?".into()
    }

    fn no_matches_text(&self, _window: &mut Window, _cx: &mut App) -> Option<SharedString> {
        Some("no projects yet. open one in noah first".into())
    }

    fn update_matches(
        &mut self,
        query: String,
        _window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Task<()> {
        let candidates: Vec<StringMatchCandidate> = self
            .projects
            .iter()
            .enumerate()
            .map(|(index, project)| {
                StringMatchCandidate::new(
                    index,
                    &format!("{} {}", project.name, project.path.display()),
                )
            })
            .collect();
        let query = query.trim().to_string();
        if query.is_empty() {
            self.matches = candidates
                .iter()
                .map(|candidate| StringMatch {
                    candidate_id: candidate.id,
                    score: 0.0,
                    positions: Vec::new(),
                    string: candidate.string.clone(),
                })
                .collect();
            self.selected_index = 0;
            return Task::ready(());
        }
        let executor = cx.background_executor().clone();
        cx.spawn(async move |picker, cx| {
            let matches = fuzzy::match_strings(
                &candidates,
                &query,
                false,
                true,
                100,
                &Default::default(),
                executor,
            )
            .await;
            picker
                .update(cx, |picker, cx| {
                    picker.delegate.matches = matches;
                    picker.delegate.selected_index = 0;
                    cx.notify();
                })
                .log_err();
        })
    }

    fn confirm(&mut self, _secondary: bool, _window: &mut Window, cx: &mut Context<Picker<Self>>) {
        let Some(project) = self
            .matches
            .get(self.selected_index)
            .and_then(|found| self.projects.get(found.candidate_id))
            .cloned()
        else {
            return;
        };
        if let Some(on_pick) = self.on_pick.take() {
            on_pick(project, cx);
        }
        cx.emit(DismissEvent);
    }

    fn dismissed(&mut self, _window: &mut Window, _cx: &mut Context<Picker<Self>>) {}

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        let found = self.matches.get(ix)?;
        let project = self.projects.get(found.candidate_id)?;
        Some(
            ListItem::new(ix)
                .inset(true)
                .spacing(ListItemSpacing::Sparse)
                .toggle_state(selected)
                .child(
                    h_flex()
                        .gap_2()
                        .child(Label::new(project.name.clone()))
                        .child(
                            Label::new(project.path.display().to_string())
                                .size(LabelSize::Small)
                                .color(Color::Muted)
                                .truncate(),
                        ),
                ),
        )
    }
}

/// Asks which project to take a chat to, then runs `on_pick` with it.
pub(crate) fn pick_code_project(
    workspace: &mut Workspace,
    on_pick: Box<dyn FnOnce(CodeProjectChoice, &mut App)>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let fs = workspace.app_state().fs.clone();
    let delegate = CodeProjectDelegate {
        projects: Vec::new(),
        matches: Vec::new(),
        selected_index: 0,
        on_pick: Some(Box::new(move |project, cx| {
            on_pick(
                CodeProjectChoice {
                    name: project.name,
                    path: project.path,
                },
                cx,
            )
        })),
    };
    workspace.toggle_modal(window, cx, |window, cx| {
        Picker::uniform_list(delegate, window, cx)
    });
    let Some(picker) = workspace.active_modal::<Picker<CodeProjectDelegate>>(cx) else {
        return;
    };
    let database = WorkspaceDb::global(cx);
    cx.spawn_in(window, async move |_, cx| {
        let mut roots = cx.update(|_, cx| open_project_roots(cx))?;
        let recent = database
            .recent_project_workspaces(fs.as_ref())
            .await
            .log_err()
            .unwrap_or_default();
        for workspace in recent {
            if !matches!(
                workspace.location,
                workspace::SerializedWorkspaceLocation::Local
            ) {
                continue;
            }
            if let Some(root) = workspace.paths.ordered_paths().next().cloned()
                && !is_noah_room(&root)
                && !roots.contains(&root)
            {
                roots.push(root);
            }
        }
        picker.update_in(cx, |picker, window, cx| {
            picker.delegate.projects = roots
                .into_iter()
                .map(|path| CodeProject {
                    name: project_name(&path),
                    path,
                })
                .collect();
            picker.refresh(window, cx);
        })
    })
    .detach_and_log_err(cx);
}

pub(crate) struct CodeProjectChoice {
    pub name: SharedString,
    pub path: PathBuf,
}

/// Opens (or focuses) the window for the project at `root`, waits for
/// shepherd's panel there, and runs `then` in it.
async fn with_project_panel<R: 'static>(
    root: PathBuf,
    then: impl FnOnce(&mut Workspace, Entity<AgentPanel>, &mut Window, &mut Context<Workspace>) -> R
    + 'static,
    cx: &mut AsyncApp,
) -> anyhow::Result<R> {
    let existing = cx.update(|cx| {
        cx.windows().into_iter().find_map(|window| {
            let window = window.downcast::<MultiWorkspace>()?;
            let workspace = window
                .read(cx)
                .ok()?
                .workspaces()
                .find(|workspace| is_room_workspace(workspace.read(cx), &root, cx))
                .cloned()?;
            Some((window, workspace))
        })
    });
    let (window, workspace) = match existing {
        Some(found) => found,
        None => {
            // The project joins the window the person is already in, as a
            // workspace of its own, instead of opening another window.
            let workspace = cx
                .update(|cx| {
                    workspace::open_noah_folder_in_active_window(
                        root.clone(),
                        Box::new(|_, _, _| {}),
                        cx,
                    )
                })
                .await?;
            let window = cx
                .update(|cx| window_for_workspace(&workspace, cx))
                .ok_or_else(|| anyhow::anyhow!("the project's window closed while it opened"))?;
            (window, workspace)
        }
    };
    let mut then = Some(then);
    for _ in 0..200 {
        let result = window.update(cx, |_, window, cx| {
            window.activate_window();
            workspace.update(cx, |workspace, cx| {
                workspace.panel::<AgentPanel>(cx)?;
                let panel = workspace.focus_panel::<AgentPanel>(window, cx)?;
                let then = then.take()?;
                Some(then(workspace, panel, window, cx))
            })
        })?;
        if let Some(result) = result {
            return Ok(result);
        }
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
    }
    anyhow::bail!("shepherd's panel didn't open in {}", root.display())
}

/// The window holding `workspace`.
pub(crate) fn window_for_workspace(
    workspace: &Entity<Workspace>,
    cx: &App,
) -> Option<WindowHandle<MultiWorkspace>> {
    cx.windows().into_iter().find_map(|window| {
        let window = window.downcast::<MultiWorkspace>()?;
        window
            .read(cx)
            .ok()?
            .workspaces()
            .any(|candidate| candidate == workspace)
            .then_some(window)
    })
}

/// "take this to code": summarizes the chat's decisions, drafts acceptance
/// criteria into the project's `.noah/spec.md` for review, and opens a
/// shepherd thread there seeded with the summary.
pub(crate) fn run_handoff(
    chat_workspace: WeakEntity<Workspace>,
    native_agent: Entity<agent::NativeAgent>,
    chat_session: acp::SessionId,
    note: String,
    project: CodeProjectChoice,
    cx: &mut App,
) {
    show_chat_toast(
        &chat_workspace,
        format!("summarizing this chat for {}…", project.name),
        false,
        cx,
    );
    let summarize = native_agent.update(cx, |agent, cx| {
        agent.summarize_chat_for_handoff(chat_session.clone(), cx)
    });
    let toast_workspace = chat_workspace.clone();
    cx.spawn(async move |cx| {
        let result: anyhow::Result<String> = async {
            let (title, handoff) = summarize.await?;
            let spec_path = noah_trust::project_files::path(
                &project.path,
                noah_trust::project_files::SPEC,
            );
            let clauses = handoff.clauses.clone();
            let spec_title = title.clone();
            let clause_ids = cx
                .background_spawn(async move {
                    let existing = match std::fs::read_to_string(&spec_path) {
                        Ok(text) => text,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            String::new()
                        }
                        Err(error) => return Err(anyhow::Error::from(error)),
                    };
                    let (text, ids) =
                        asherin_chat::handoff::append_clauses(&existing, &clauses, &spec_title);
                    if !ids.is_empty() {
                        if let Some(folder) = spec_path.parent() {
                            std::fs::create_dir_all(folder)?;
                        }
                        std::fs::write(&spec_path, text)?;
                    }
                    Ok(ids)
                })
                .await?;
            let seed = asherin_chat::handoff::handoff_seed(&title, &handoff, &clause_ids, &note);
            let spec_path =
                noah_trust::project_files::path(&project.path, noah_trust::project_files::SPEC);
            let open_spec = !clause_ids.is_empty();
            let conversation_view = with_project_panel(
                project.path.clone(),
                move |workspace, panel, window, cx| {
                    if open_spec {
                        workspace
                            .open_abs_path(
                                spec_path,
                                OpenOptions {
                                    visible: Some(OpenVisible::None),
                                    ..OpenOptions::default()
                                },
                                window,
                                cx,
                            )
                            .detach_and_log_err(cx);
                    }
                    panel.update(cx, |panel, cx| {
                        panel.new_thread_with_content(
                            AgentInitialContent::ContentBlock {
                                blocks: vec![acp::ContentBlock::Text(acp::TextContent::new(
                                    seed,
                                ))],
                                auto_submit: false,
                            },
                            window,
                            cx,
                        )
                    })
                },
                cx,
            )
            .await?;
            let session_id = wait_for_session(&conversation_view, cx).await?;
            let mut handoff_node = ChatConversation::new(
                session_id.clone(),
                project.name.clone(),
                ConversationKind::Handoff,
            );
            handoff_node.parent_conversation_id = Some(chat_session);
            handoff_node.agent_thread_id = Some(session_id);
            handoff_node.project_path = Some(project.path.clone());
            cx.update(|cx| chat_tree::link_chat_conversation(handoff_node, cx))
                .await?;
            cx.update(notify_chat_tree_changed);
            Ok(match clause_ids.as_slice() {
                [] => format!("taken to code in {}", project.name),
                [only] => format!(
                    "taken to code in {}. {only} is drafted in .noah/spec.md for review",
                    project.name
                ),
                [first, .., last] => format!(
                    "taken to code in {}. {first} to {last} are drafted in .noah/spec.md for review",
                    project.name
                ),
            })
        }
        .await;
        cx.update(|cx| match result {
            Ok(message) => show_chat_toast(&toast_workspace, message, true, cx),
            Err(error) => show_chat_toast(
                &toast_workspace,
                format!("couldn't take this to code: {error:#}"),
                false,
                cx,
            ),
        });
    })
    .detach();
}

/// Opens the shepherd-room thread a chat was handed to.
pub(crate) fn open_handoff_thread(project_root: PathBuf, session_id: acp::SessionId, cx: &mut App) {
    cx.spawn(async move |cx| {
        with_project_panel(
            project_root.clone(),
            move |_workspace, panel, window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.open_thread(
                        session_id,
                        Some(util::path_list::PathList::new(&[project_root])),
                        None,
                        window,
                        cx,
                    )
                });
            },
            cx,
        )
        .await
    })
    .detach_and_log_err(cx);
}

/// A free agent id based on `base`, so a new agent never overwrites one.
fn unused_agent_id(base: &str) -> String {
    let base =
        asherin_chat::agents::sanitize_agent_name(base).unwrap_or_else(|| "new-agent".into());
    let mut id = base.clone();
    let mut number = 2;
    while chat_tree::chat_agent_path(&id).exists() {
        id = format!("{base}-{number}");
        number += 1;
    }
    id
}

/// Opens a new agent file for review: `instructions` drafted by shepherd,
/// or a template to fill in. Nothing is written until the person saves.
pub(crate) fn open_new_chat_agent(
    workspace: &mut Workspace,
    draft: Option<(String, String)>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let (id, text) = match draft {
        Some((description, instructions)) => {
            let base = asherin_chat::agents::agent_name_from_description(&description)
                .unwrap_or_else(|| "new-agent".to_string());
            let id = unused_agent_id(&base);
            let text = asherin_chat::agents::agent_file_template(&id, &description, &instructions);
            (id, text)
        }
        None => {
            let id = unused_agent_id("new-agent");
            let text = asherin_chat::agents::agent_file_template(&id, "", "");
            (id, text)
        }
    };
    if let Err(error) = std::fs::create_dir_all(chat_tree::chat_agents_directory()) {
        workspace.show_toast(
            Toast::new(
                toast_id(),
                format!("couldn't create the agents folder: {error}"),
            ),
            cx,
        );
        return;
    }
    let path = chat_tree::chat_agent_path(&id);
    let open = workspace.open_abs_path(
        path,
        OpenOptions {
            visible: Some(OpenVisible::None),
            ..OpenOptions::default()
        },
        window,
        cx,
    );
    workspace.show_toast(
        Toast::new(
            toast_id(),
            format!("review asherin.{id} and save it to use it. rename the file and its name: to rename the agent"),
        )
        .autohide(),
        cx,
    );
    cx.spawn_in(window, async move |_, cx| {
        let item = open.await?;
        if let Some(editor) = item.downcast::<Editor>() {
            editor.update_in(cx, |editor, window, cx| {
                if editor.text(cx).trim().is_empty() {
                    editor.set_text(text, window, cx);
                }
            })?;
        }
        anyhow::Ok(())
    })
    .detach_and_log_err(cx);
}

/// `/agent new <description>`: shepherd drafts the instructions, then the
/// file opens for review.
pub(crate) fn draft_chat_agent(
    workspace: WeakEntity<Workspace>,
    description: String,
    cx: &mut App,
) {
    show_chat_toast(&workspace, "drafting the agent's instructions…", false, cx);
    let draft = chat_tree::draft_chat_agent_instructions(description.clone(), cx);
    cx.spawn(async move |cx| {
        let instructions = match draft.await {
            Ok(instructions) => instructions,
            Err(error) => {
                cx.update(|cx| {
                    show_chat_toast(
                        &workspace,
                        format!("couldn't draft the agent: {error:#}"),
                        false,
                        cx,
                    )
                });
                return;
            }
        };
        let opened = cx.update(|cx| {
            let workspace = workspace.upgrade()?;
            let window_handle = window_for_workspace(&workspace, cx)?;
            window_handle
                .update(cx, |_, window, cx| {
                    workspace.update(cx, |workspace, cx| {
                        open_new_chat_agent(
                            workspace,
                            Some((description, instructions)),
                            window,
                            cx,
                        );
                    })
                })
                .ok()
        });
        if opened.is_none() {
            log::error!("the asherin.chat window closed before the agent draft was ready");
        }
    })
    .detach();
}

#[cfg(test)]
mod prompt_project_tests {
    use super::next_untitled_folder;

    #[test]
    fn untitled_folders_count_up_and_are_created() {
        let base = tempfile::tempdir().expect("a temp dir");
        let first = next_untitled_folder(base.path()).expect("first");
        let second = next_untitled_folder(base.path()).expect("second");
        assert_eq!(first, base.path().join("untitled-1"));
        assert_eq!(second, base.path().join("untitled-2"));
        assert!(first.is_dir() && second.is_dir());
    }
}
