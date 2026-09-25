use gpui::{
    Action, App, Context, Div, InteractiveElement, IntoElement, ParentElement, Styled, Window,
    actions, div, px,
};
use ui::{IconButton, IconName, IconSize, Tooltip, prelude::*};

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
}

impl Room {
    const ALL: [Room; 6] = [
        Room::Write,
        Room::Shepherd,
        Room::Files,
        Room::Changes,
        Room::Terminal,
        Room::Browser,
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
}

impl Workspace {
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
                let tooltip_label: SharedString = match &badge {
                    Some(badge) if room == Room::Shepherd => {
                        format!("{} · {badge}", room.label()).into()
                    }
                    Some(count) => format!("{} · {count}", room.label()).into(),
                    None => room.label().into(),
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
            .child(
                div().pb_3().child(
                    IconButton::new("settings", IconName::Settings)
                        .icon_size(IconSize::Small)
                        .icon_color(Color::Muted)
                        .tooltip(|_window, cx| {
                            Tooltip::for_action("settings", &zed_actions::OpenSettings, cx)
                        })
                        .on_click(|_, window, cx| {
                            window.dispatch_action(zed_actions::OpenSettings.boxed_clone(), cx);
                        }),
                ),
            )
    }
}
