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
        /// Opens the device room: this computer's security and health.
        EnterDeviceRoom,
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
    Device,
}

impl Room {
    const ALL: [Room; 8] = [
        Room::Write,
        Room::Shepherd,
        Room::Files,
        Room::Changes,
        Room::Terminal,
        Room::Browser,
        Room::Mission,
        Room::Device,
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
            Room::Device => Some("DevicePanel"),
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
            Room::Device => "device",
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
            Room::Device => IconName::Lock,
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
            Room::Device => Box::new(EnterDeviceRoom),
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
    .on_action(cx.listener(|workspace, _: &EnterDeviceRoom, window, cx| {
        workspace.enter_room(Room::Device, window, cx)
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
    .on_action(
        cx.listener(|workspace, _: &noah_capture::ToggleCameraInRecordings, _window, cx| {
            workspace.toggle_camera_in_recordings(cx)
        }),
    )
}

struct CaptureNotification;

/// The backgrounds shipped in `assets/images/noah/wallpapers`, as menu label
/// and file name; any image added there appears here.
fn bundled_backgrounds(cx: &App) -> Vec<(String, String)> {
    let directory = "images/noah/wallpapers/";
    let mut backgrounds: Vec<(String, String)> = cx
        .asset_source()
        .list(directory)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|path| {
            let name = path.strip_prefix(directory)?.to_string();
            let lowercase = name.to_lowercase();
            let is_image = [".jpg", ".jpeg", ".png", ".webp"]
                .iter()
                .any(|extension| lowercase.ends_with(extension));
            if !is_image || name.contains('/') {
                return None;
            }
            let stem = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem);
            let label = stem.replace(['-', '_'], " ");
            Some((label, name))
        })
        .collect();
    backgrounds.sort();
    backgrounds
}

/// The rail's settings menu: the things people reach for most, one click
/// away, with everything else behind "all settings".
fn settings_menu(window: &mut Window, cx: &mut App) -> Entity<ContextMenu> {
    let adapts = crate::WorkspaceSettings::get_global(cx).wallpaper_adapts_theme;
    let recording = noah_capture::is_recording(cx);
    let camera = noah_capture::camera_wanted(cx);
    ContextMenu::build(window, cx, move |menu, _window, cx| {
        let check_for_updates = cx.build_action("auto_update::Check", None).ok();
        let menu = menu
            .header("look")
            .action("appearance, theme and fonts", Box::new(zed_actions::OpenSettingsPage {
                page: "Appearance".into(),
                target: None,
            }))
            .header("background")
            .action(
                "noah default",
                Box::new(zed_actions::UseBundledBackground {
                    name: String::new(),
                }),
            );
        let menu = bundled_backgrounds(cx).into_iter().fold(menu, |menu, (label, name)| {
            menu.action(label, Box::new(zed_actions::UseBundledBackground { name }))
        });
        let menu = menu
            .action("upload your own…", Box::new(zed_actions::ChooseBackgroundImage))
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
            .action("device security", Box::new(EnterDeviceRoom))
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
            .toggleable_entry(
                "your camera in recordings",
                camera,
                IconPosition::Start,
                Some(Box::new(noah_capture::ToggleCameraInRecordings)),
                |window, cx| {
                    window.dispatch_action(Box::new(noah_capture::ToggleCameraInRecordings), cx)
                },
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
    fn toggle_camera_in_recordings(&mut self, cx: &mut Context<Self>) {
        let message = match noah_capture::toggle_camera(cx) {
            Ok(true) => {
                "your camera joins the next recording, as a small rounded window in the corner"
                    .to_string()
            }
            Ok(false) => "recordings are the screen only again".to_string(),
            Err(error) => format!("couldn't turn the camera on: {error:#}"),
        };
        self.show_capture_toast(message, None, cx);
    }

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
            .children(crate::rail_apps::apps(cx).into_iter().enumerate().map(|(index, app)| {
                let initial: SharedString = app
                    .name
                    .chars()
                    .find(|character| character.is_alphanumeric())
                    .map(|character| character.to_lowercase().to_string())
                    .unwrap_or_else(|| "·".to_string())
                    .into();
                let open_url = app.url.clone();
                let unpin_url = app.url.clone();
                let tooltip: SharedString =
                    format!("{} · right-click to unpin", app.name).into();
                h_flex().w_full().justify_center().child(
                    div()
                        .id(("rail-app", index))
                        .size(px(22.))
                        .rounded_md()
                        .border_1()
                        .border_color(colors.border_variant)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .text_color(colors.text_muted)
                        .cursor_pointer()
                        .hover(|style| style.text_color(colors.text).border_color(colors.border))
                        .child(initial)
                        .tooltip(Tooltip::text(tooltip))
                        .on_click(move |_, window, cx| {
                            window.dispatch_action(
                                Box::new(zed_actions::OpenInBrowserRoom {
                                    url: open_url.clone(),
                                }),
                                cx,
                            );
                        })
                        .on_mouse_down(gpui::MouseButton::Right, move |_, _, cx| {
                            crate::rail_apps::unpin(&unpin_url, cx);
                        }),
                )
            }))
            .child(
                h_flex().w_full().justify_center().child(
                    IconButton::new("asherin-chat", IconName::Chat)
                        .icon_size(IconSize::Small)
                        .icon_color(Color::Muted)
                        .tooltip(|_window, cx| {
                            Tooltip::for_action("asherin.chat", &zed_actions::OpenAsherinChat, cx)
                        })
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(zed_actions::OpenAsherinChat), cx)
                        }),
                ),
            )
            .child(
                h_flex().w_full().justify_center().child(
                    IconButton::new("asherin-pages", IconName::FileDoc)
                        .icon_size(IconSize::Small)
                        .icon_color(Color::Muted)
                        .tooltip(|_window, cx| {
                            Tooltip::for_action("asherin.pages", &zed_actions::OpenAsherinPages, cx)
                        })
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(zed_actions::OpenAsherinPages), cx)
                        }),
                ),
            )
            .child(
                h_flex().w_full().justify_center().child(
                    IconButton::new("asherin-search", IconName::MagnifyingGlass)
                        .icon_size(IconSize::Small)
                        .icon_color(Color::Muted)
                        .tooltip(|_window, cx| {
                            Tooltip::for_action("asherin.search", &zed_actions::OpenAsherinSearch, cx)
                        })
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(zed_actions::OpenAsherinSearch), cx)
                        }),
                ),
            )
            .child(
                h_flex().w_full().justify_center().child(
                    IconButton::new("notes", IconName::Notepad)
                        .icon_size(IconSize::Small)
                        .icon_color(Color::Muted)
                        .tooltip(|_window, cx| {
                            Tooltip::for_action(
                                "notes: a pad of your own, drag it anywhere",
                                &zed_actions::ToggleNotes,
                                cx,
                            )
                        })
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(zed_actions::ToggleNotes), cx)
                        }),
                ),
            )
            .child(
                h_flex().w_full().justify_center().child(
                    IconButton::new("asherin-board", IconName::Blocks)
                        .icon_size(IconSize::Small)
                        .icon_color(Color::Muted)
                        .tooltip(|_window, cx| {
                            Tooltip::for_action("asherin.board", &zed_actions::OpenAsherinBoard, cx)
                        })
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(zed_actions::OpenAsherinBoard), cx)
                        }),
                ),
            )
            .child(
                h_flex().w_full().justify_center().child(
                    IconButton::new("asherin-eye", IconName::Eye)
                        .icon_size(IconSize::Small)
                        .icon_color(Color::Muted)
                        .tooltip(|_window, cx| {
                            Tooltip::for_action("asherin.eye", &zed_actions::OpenAsherinEye, cx)
                        })
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(zed_actions::OpenAsherinEye), cx)
                        }),
                ),
            )
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
                    .child({
                        let camera = noah_capture::camera_wanted(cx);
                        IconButton::new("camera-bubble", IconName::Person)
                            .icon_size(IconSize::Small)
                            .icon_color(if camera { Color::Default } else { Color::Muted })
                            .toggle_state(camera)
                            .tooltip(move |_window, cx| {
                                Tooltip::for_action(
                                    if camera {
                                        "your camera is in recordings, as a rounded window in the corner"
                                    } else {
                                        "add your camera to recordings (needs ffmpeg)"
                                    },
                                    &noah_capture::ToggleCameraInRecordings,
                                    cx,
                                )
                            })
                            .on_click(cx.listener(|workspace, _, _window, cx| {
                                workspace.toggle_camera_in_recordings(cx)
                            }))
                    })
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
