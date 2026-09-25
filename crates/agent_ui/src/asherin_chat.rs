//! asherin.chat and asherin.pages: shepherd outside any code project. Each is
//! a folder under `~/.noah` that opens in its own window. Chat threads use the
//! chat profile, so they can talk, search and read but never edit or run;
//! pages is where shepherd makes documents, books and slideshows.

use std::path::PathBuf;

use collections::HashSet;
use gpui::{App, TaskExt as _};
use project::trusted_worktrees::{PathTrust, TrustedWorktrees};
use workspace::{AppState, MultiWorkspace, OpenMode, OpenOptions, Workspace};

use crate::AgentPanel;

const PAGES_GUIDE: &str = include_str!("../../../assets/shepherd/pages_guide.md");

pub fn init(cx: &mut App) {
    cx.on_action(|_: &zed_actions::OpenAsherinChat, cx| {
        open_room(paths::chat_directory(), None, cx)
    });
    cx.on_action(|_: &zed_actions::OpenAsherinPages, cx| {
        open_room(paths::pages_directory(), Some(PAGES_GUIDE), cx)
    });
}

fn is_room_workspace(workspace: &Workspace, folder: &PathBuf, cx: &App) -> bool {
    workspace
        .project()
        .read(cx)
        .visible_worktrees(cx)
        .any(|worktree| worktree.read(cx).abs_path().as_ref() == folder.as_path())
}

/// Focuses the room's window when one is open, and otherwise opens a window
/// on the room's folder with shepherd ready. `guide` becomes the folder's
/// AGENTS.md the first time, which shepherd reads as the room's instructions.
fn open_room(folder: PathBuf, guide: Option<&'static str>, cx: &mut App) {
    for window in cx.windows() {
        let Some(window) = window.downcast::<MultiWorkspace>() else {
            continue;
        };
        let focused = window
            .update(cx, |multi_workspace, window, cx| {
                let Some(workspace) = multi_workspace
                    .workspaces()
                    .find(|workspace| is_room_workspace(workspace.read(cx), &folder, cx))
                    .cloned()
                else {
                    return false;
                };
                window.activate_window();
                workspace.update(cx, |workspace, cx| {
                    workspace.focus_panel::<AgentPanel>(window, cx);
                });
                true
            })
            .unwrap_or(false);
        if focused {
            return;
        }
    }
    let Some(app_state) = AppState::try_global(cx) else {
        return;
    };
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
    workspace::open_new(
        OpenOptions {
            open_mode: OpenMode::NewWindow,
            ..OpenOptions::default()
        },
        app_state,
        cx,
        move |workspace, window, cx| {
            let project = workspace.project().clone();
            // The folder is noah's own, so it opens trusted rather than in
            // Restricted Mode, where shepherd would have no tools.
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
            // Panels load after the window opens, so wait for shepherd's
            // before showing it.
            cx.spawn_in(window, async move |workspace, cx| {
                for _ in 0..100 {
                    let shown = workspace.update_in(cx, |workspace, window, cx| {
                        workspace.panel::<AgentPanel>(cx).is_some()
                            && workspace.focus_panel::<AgentPanel>(window, cx).is_some()
                    })?;
                    if shown {
                        break;
                    }
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(50))
                        .await;
                }
                anyhow::Ok(())
            })
            .detach_and_log_err(cx);
        },
    )
    .detach_and_log_err(cx);
}
