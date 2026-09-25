use gpui::{
    Action, App, Context, Div, InteractiveElement, IntoElement, ParentElement, Styled, Window,
    actions, div, px,
};
use gpui::Entity;
use settings::Settings as _;
use ui::{
    ContextMenu, IconButton, IconName, IconPosition, IconSize, PopoverMenu, Tooltip, prelude::*,
};

use crate::Workspace;

actions!(
    workspace,
    [
        /// Closes every side room and returns to the code.
        EnterWriteRoom,
        /// Opens shepherd, the agent, and closes the other rooms.
        EnterShepherdRoom,
        /// Opens the project files and closes the other rooms.
        EnterFilesRoom,
        /// Opens the git changes and closes the other rooms.
        EnterChangesRoom,
        /// Opens the terminal and closes the other rooms.
        EnterTerminalRoom,
        /// Opens the browser shepherd shares with you and closes the other rooms.
        EnterBrowserRoom,
        /// Opens mission control, where shepherd's work is supervised.
        EnterMissionRoom,
    ]
);

/// Badges the agent panel reports through `Panel::icon_label`, so the rail can
/// show whether shepherd is busy or needs a decision without depending on it.
pub const SHEPHERD_WORKING: &str = "working";
pub const SHEPHERD_AWAITING_APPROVAL: &str = "awaiting approval";

/// The places a person works in noah. Only one is open at a time, so the
/// window reads as a room you are standing in rather than a wall of panels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Room {
    Write,
    Shepherd,
    Files,
    Changes,
    Terminal,
    Browser,
    Mission,
}

impl Room {
    const ALL: [Room; 7] = [
        Room::Write,
        Room::Shepherd,
        Room::Files,
        Room::Changes,
        Room::Terminal,
        Room::Browser,
        Room::Mission,
    ];

    // Must match each panel's `Panel::persistent_name`.
    fn panel_name(self) -> Option<&'static str> {
        match self {
            Room::Write => None,
            Room::Shepherd => Some("AgentPanel"),
            Room::Files => Some("Project Panel"),
            Room::Changes => Some("GitPanel"),
            Room::Terminal => Some("TerminalPanel"),
            Room::Browser => Some("BrowserPanel"),
            Room::Mission => Some("MissionControlPanel"),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Room::Write => "write",
            Room::Shepherd => "shepherd",
            Room::Files => "files",
            Room::Changes => "changes",
            Room::Terminal => "terminal",
            Room::Browser => "browser",
            Room::Mission => "mission control",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Room::Write => IconName::Pencil,
            Room::Shepherd => IconName::ZedAgent,
            Room::Files => IconName::Folder,
            Room::Changes => IconName::GitBranch,
            Room::Terminal => IconName::Terminal,
            Room::Browser => IconName::ToolWeb,
            Room::Mission => IconName::ListTodo,
        }
    }

    fn action(self) -> Box<dyn Action> {
        match self {
            Room::Write => Box::new(EnterWriteRoom),
            Room::Shepherd => Box::new(EnterShepherdRoom),
            Room::Files => Box::new(EnterFilesRoom),
            Room::Changes => Box::new(EnterChangesRoom),
            Room::Terminal => Box::new(EnterTerminalRoom),
            Room::Browser => Box::new(EnterBrowserRoom),
            Room::Mission => Box::new(EnterMissionRoom),
        }
    }
}

pub(crate) fn room_actions(div: Div, cx: &mut Context<Workspace>) -> Div {
    div.on_action(cx.listener(|workspace, _: &EnterWriteRoom, window, cx| {
        workspace.enter_room(Room::Write, window, cx)
    }))
    .on_action(cx.listener(|workspace, _: &EnterShepherdRoom, window, cx| {
        workspace.enter_room(Room::Shepherd, window, cx)
    }))
    .on_action(cx.listener(|workspace, _: &EnterFilesRoom, window, cx| {
        workspace.enter_room(Room::Files, window, cx)
    }))
    .on_action(cx.listener(|workspace, _: &EnterChangesRoom, window, cx| {
        workspace.enter_room(Room::Changes, window, cx)
    }))
    .on_action(cx.listener(|workspace, _: &EnterTerminalRoom, window, cx| {
        workspace.enter_room(Room::Terminal, window, cx)
    }))
    .on_action(cx.listener(|workspace, _: &EnterBrowserRoom, window, cx| {
        workspace.enter_room(Room::Browser, window, cx)
    }))
    .on_action(cx.listener(|workspace, _: &EnterMissionRoom, window, cx| {
        workspace.enter_room(Room::Mission, window, cx)
    }))
    .on_action(
        cx.listener(|workspace, _: &zed_actions::ToggleBackgroundColors, _window, cx| {
            let adapts = crate::WorkspaceSettings::get_global(cx).wallpaper_adapts_theme;
            let fs = workspace.app_state().fs.clone();
            settings::update_settings_file(fs, cx, move |settings, _| {
                settings.workspace.wallpaper_adapts_theme = Some(!adapts);
            });
        }),
    )
    .on_action(cx.listener(|workspace, _: &noah_capture::TakeScreenshot, window, cx| {
        workspace.take_screenshot(window, cx)
    }))
    .on_action(
        cx.listener(|workspace, _: &noah_capture::ToggleScreenRecording, window, cx| {
            workspace.toggle_screen_recording(window, cx)
        }),
    )
}

struct CaptureNotification;

/// The rail's settings menu: the things people reach for most, one click
/// away, with everything else behind "all settings".
fn settings_menu(window: &mut Window, cx: &mut App) -> Entity<ContextMenu> {
    let adapts = crate::WorkspaceSettings::get_global(cx).wallpaper_adapts_theme;
    let recording = noah_capture::is_recording(cx);
    ContextMenu::build(window, cx, move |menu, _window, cx| {
        let check_for_updates = cx.build_action("auto_update::Check", None).ok();
        let menu = menu
            .header("look")
            .action("appearance, theme and fonts", Box::new(zed_actions::OpenSettingsPage {
                page: "Appearance".into(),
                target: None,
            }))
            .action("upload a background image", Box::new(zed_actions::ChooseBackgroundImage))
            .toggleable_entry(
                "colors follow the background",
                adapts,
                IconPosition::Start,
                Some(Box::new(zed_actions::ToggleBackgroundColors)),
                |window, cx| {
                    window.dispatch_action(Box::new(zed_actions::ToggleBackgroundColors), cx)
                },
            )
            .action("language", Box::new(zed_actions::OpenSettingsAt {
                path: "language".into(),
                target: None,
            }))
            .separator()
            .header("shepherd")
            .action("API keys and models", Box::new(zed_actions::OpenSettingsAt {
                path: "llm_providers".into(),
                target: None,
            }))
            .action("shepherd settings", Box::new(zed_actions::OpenSettingsPage {
                page: "AI".into(),
                target: None,
            }))
            .action("mission control", Box::new(EnterMissionRoom))
            .separator()
            .header("tools")
            .action(
                "preview this file in the browser",
                Box::new(zed_actions::PreviewFileInBrowser),
            )
            .action("screenshot", Box::new(noah_capture::TakeScreenshot))
            .action(
                if recording { "stop recording" } else { "record the screen" },
                Box::new(noah_capture::ToggleScreenRecording),
            )
            .action("keyboard shortcuts", Box::new(zed_actions::OpenKeymap))
            .action("view logs", Box::new(crate::OpenLog))
            .separator()
            .action("all settings", Box::new(zed_actions::OpenSettings))
            .action("settings file (JSON)", Box::new(zed_actions::OpenSettingsFile));
        let menu = match check_for_updates {
            Some(action) => menu.action("check for updates", action),
            None => menu,
        };
        menu.action("about noah", Box::new(zed_actions::About))
    })
}

impl Workspace {
    fn take_screenshot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let task = noah_capture::take_screenshot(window, cx);
        cx.spawn(async move |workspace, cx| {
            let result = task.await;
            workspace.update(cx, |workspace, cx| match result {
                Ok(path) => workspace.show_capture_toast(
                    format!("screenshot saved to {} and copied", path.display()),
                    Some(path),
                    cx,
                ),
                Err(error) => workspace.show_capture_toast(
                    format!("couldn't take a screenshot: {error:#}"),
                    None,
                    cx,
                ),
            })
        })
        .detach_and_log_err(cx);
    }

    fn toggle_screen_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let task = noah_capture::toggle_recording(window, cx);
        cx.notify();
        cx.spawn(async move |workspace, cx| {
            let result = task.await;
            workspace.update(cx, |workspace, cx| {
                match result {
                    Ok(Some(path)) => workspace.show_capture_toast(
                        format!("recording saved to {}", path.display()),
                        Some(path),
                        cx,
                    ),
                    Ok(None) => {}
                    Err(error) => workspace.show_capture_toast(
                        format!("the recording didn't work: {error:#}"),
                        None,
                        cx,
                    ),
                }
                cx.notify();
            })
        })
        .detach_and_log_err(cx);
    }

    fn show_capture_toast(&mut self, message: String, path: Option<std::path::PathBuf>, cx: &mut Context<Self>) {
        let mut toast = crate::Toast::new(
            crate::notifications::NotificationId::unique::<CaptureNotification>(),
            message,
        );
        if let Some(path) = path {
            toast = toast.on_click("show in folder", move |_, cx| cx.reveal_path(&path));
        }
        self.show_toast(toast.autohide(), cx);
    }

    /// The room whose panel is showing, or the writing room when none is.
    pub fn active_room(&self, cx: &App) -> Room {
        for dock in self.all_docks() {
            let dock = dock.read(cx);
            if !dock.is_open() {
                continue;
            }
            let Some(panel) = dock.visible_panel() else {
                continue;
            };
            let name = panel.persistent_name();
            if let Some(room) = Room::ALL
                .into_iter()
                .find(|room| room.panel_name() == Some(name))
            {
                return room;
            }
        }
        Room::Write
    }

    /// Opens `room` and closes every other one. Entering the room you are
    /// already in, or the writing room, returns focus to the code.
    pub fn enter_room(&mut self, room: Room, window: &mut Window, cx: &mut Context<Self>) {
        let already_here = self.active_room(cx) == room;
        self.close_all_docks(window, cx);
        let Some(panel_name) = room.panel_name().filter(|_| !already_here) else {
            self.focus_center_pane(window, cx);
            return;
        };

        for dock in self.all_docks() {
            let Some(panel_index) = dock.read(cx).panel_index_for_persistent_name(panel_name, cx)
            else {
                continue;
            };
            let panel = dock.update(cx, |dock, cx| {
                dock.activate_panel(panel_index, window, cx);
                dock.set_open(true, window, cx);
                dock.active_panel().cloned()
            });
            if let Some(panel) = panel {
                panel.panel_focus_handle(cx).focus(window, cx);
            }
            break;
        }
        cx.notify();
        self.serialize_workspace(window, cx);
    }

    fn room_badge(&self, room: Room, window: &Window, cx: &App) -> Option<String> {
        let panel_name = room.panel_name()?;
        self.all_docks().into_iter().find_map(|dock| {
            let dock = dock.read(cx);
            let index = dock.panel_index_for_persistent_name(panel_name, cx)?;
            dock.panel_at(index)?.icon_label(window, cx)
        })
    }

    pub(crate) fn render_room_rail(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active_room = self.active_room(cx);
        let colors = cx.theme().colors();
        let status = cx.theme().status();

        div()
            .id("room-rail")
            .flex()
            .flex_col()
            .items_center()
            .flex_none()
            .w(px(44.))
            .h_full()
            .pt_3()
            .gap_2()
            .bg(colors.title_bar_background)
            .border_r_1()
            .border_color(colors.border_variant)
            .children(Room::ALL.into_iter().map(|room| {
                let is_active = room == active_room;
                let badge = self.room_badge(room, window, cx);
                // Only a decision waiting on the person earns the accent; work in
                // progress and counts stay in the neutral mist.
                let awaiting_approval = badge.as_deref() == Some(SHEPHERD_AWAITING_APPROVAL);
                let label = noah_i18n::t(cx, room.label());
                let tooltip_label: SharedString = match &badge {
                    Some(badge) => format!("{label} · {}", noah_i18n::t(cx, badge)).into(),
                    None => label.into(),
                };
                h_flex()
                    .relative()
                    .w_full()
                    .justify_center()
                    .when(is_active, |this| {
                        this.child(
                            div()
                                .absolute()
                                .left_0()
                                .top(px(6.))
                                .bottom(px(6.))
                                .w(px(2.))
                                .rounded_r_sm()
                                .bg(colors.text),
                        )
                    })
                    .when_some(badge, |this, _| {
                        this.child(
                            div()
                                .absolute()
                                .top(px(4.))
                                .right(px(9.))
                                .size(px(6.))
                                .rounded_full()
                                .bg(if awaiting_approval {
                                    status.success
                                } else {
                                    colors.text_muted
                                }),
                        )
                    })
                    .child(
                        IconButton::new(room.label(), room.icon())
                            .icon_size(IconSize::Small)
                            .icon_color(if is_active { Color::Default } else { Color::Muted })
                            .toggle_state(is_active)
                            .tooltip(move |_window, cx| {
                                Tooltip::for_action(tooltip_label.clone(), &*room.action(), cx)
                            })
                            .on_click(cx.listener(move |workspace, _, window, cx| {
                                workspace.enter_room(room, window, cx);
                            })),
                    )
            }))
            .child(div().flex_1())
            .child({
                let recording = noah_capture::recording_elapsed(cx);
                v_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .pb_2()
                    .child(
                        IconButton::new("screenshot", IconName::Image)
                            .icon_size(IconSize::Small)
                            .icon_color(Color::Muted)
                            .tooltip(|_window, cx| {
                                Tooltip::for_action(
                                    "screenshot (saved and copied)",
                                    &noah_capture::TakeScreenshot,
                                    cx,
                                )
                            })
                            .on_click(cx.listener(|workspace, _, window, cx| {
                                workspace.take_screenshot(window, cx)
                            })),
                    )
                    .child(
                        IconButton::new(
                            "screen-recording",
                            if recording.is_some() {
                                IconName::Stop
                            } else {
                                IconName::Circle
                            },
                        )
                        .icon_size(IconSize::Small)
                        .icon_color(if recording.is_some() {
                            Color::Error
                        } else {
                            Color::Muted
                        })
                        .toggle_state(recording.is_some())
                        .tooltip(move |_window, cx| {
                            let label: SharedString = match recording {
                                Some(elapsed) => format!(
                                    "recording {}:{:02}: click to stop and save",
                                    elapsed.as_secs() / 60,
                                    elapsed.as_secs() % 60
                                )
                                .into(),
                                None => "record the screen".into(),
                            };
                            Tooltip::for_action(label, &noah_capture::ToggleScreenRecording, cx)
                        })
                        .on_click(cx.listener(|workspace, _, window, cx| {
                            workspace.toggle_screen_recording(window, cx)
                        })),
                    )
            })
            .child(
                div().pb_3().child(
                    PopoverMenu::new("settings-menu")
                        .trigger_with_tooltip(
                            IconButton::new("settings", IconName::Settings)
                                .icon_size(IconSize::Small)
                                .icon_color(Color::Muted),
                            Tooltip::text(noah_i18n::t(cx, "settings")),
                        )
                        .anchor(gpui::Anchor::BottomLeft)
                        .menu(|window, cx| Some(settings_menu(window, cx))),
                ),
            )
    }
}
