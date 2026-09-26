//! asherin.chat's conversation tree: a left panel, hidden until ctrl-b,
//! listing chats by recency with their branches (↳), side chats (~) and
//! hand-offs to code (→) nested under where they came from.

use agent::ChatConversation;
use agent::chat_tree;
use agent_client_protocol::schema::v1 as acp;
use asherin_chat::tree::{ConversationKind, ConversationNode, TreeRow, build_tree};
use collections::HashMap;
use editor::{Editor, EditorEvent};
use std::time::Duration;

use gpui::{
    Action, Animation, AnimationExt as _, AnyElement, App, Context, Entity, EntityId, EventEmitter,
    FocusHandle, Focusable, KeyDownEvent, MouseButton, Pixels, Subscription, Task, WeakEntity,
    Window, ease_out_quint, px, relative,
};
use ui::{ContextMenu, Tooltip, prelude::*, right_click_menu};
use util::ResultExt as _;
use util::path_list::PathList;
use workspace::Workspace;
use workspace::dock::{DockPosition, Panel, PanelEvent};

use crate::asherin_chat::{
    self as room, ActivateChatTab, CloseChatTab, NewChatTab, NextChatTab, OpenChatBeside,
    PreviousChatTab, ReopenChatTab, ToggleTree,
};
use crate::{AgentPanel, NewThread};

const INDENT: f32 = 14.;
/// Past this many unpinned tabs, the oldest collapse into the overflow menu.
const MAX_VISIBLE_TABS: usize = 8;

#[derive(Clone, PartialEq)]
struct ChatTab {
    id: acp::SessionId,
    pinned: bool,
}

/// Adds the tree to asherin.chat's window, now or once the chat folder is
/// its project. Other windows never get it.
pub(crate) fn attach_to_chat_room(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if room::is_chat_workspace(workspace, cx) {
        add_tree_panel(workspace, window, cx);
        return;
    }
    let project = workspace.project().clone();
    cx.subscribe_in(&project, window, |workspace, _, event, window, cx| {
        if matches!(event, project::Event::WorktreeAdded(_))
            && workspace.panel::<ChatTreePanel>(cx).is_none()
            && room::is_chat_workspace(workspace, cx)
        {
            add_tree_panel(workspace, window, cx);
        }
    })
    .detach();
}

fn add_tree_panel(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    // Like a chat app: the conversations down one side, the conversation
    // filling the rest. shepherd's panel fills the centre in asherin.chat, so
    // the tree takes whichever side that panel's dock is not on.
    let agent_side = workspace
        .all_docks()
        .into_iter()
        .find(|dock| {
            dock.read(cx)
                .panel_index_for_persistent_name("AgentPanel", cx)
                .is_some()
        })
        .map(|dock| dock.read(cx).position());
    let tree_side = match agent_side {
        Some(DockPosition::Left) => DockPosition::Right,
        _ => DockPosition::Left,
    };
    let weak_workspace = workspace.weak_handle();
    let panel = cx.new(|cx| {
        let mut panel = ChatTreePanel::new(weak_workspace, window, cx);
        panel.position = tree_side;
        panel
    });
    workspace.add_panel(panel, window, cx);
    workspace.dock_at_position(tree_side).update(cx, |dock, cx| {
        if let Some(index) = dock.panel_index_for_type::<ChatTreePanel>() {
            dock.activate_panel(index, window, cx);
            dock.set_open(true, window, cx);
        }
    });
}

pub struct ChatTreePanel {
    workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    list_focus_handle: FocusHandle,
    filter_editor: Entity<Editor>,
    position: DockPosition,
    nodes: Vec<ConversationNode<acp::SessionId>>,
    conversations: HashMap<acp::SessionId, ChatConversation>,
    rows: Vec<TreeRow<acp::SessionId>>,
    selected: Option<usize>,
    observing_agent_panel: bool,
    /// Conversations open as tabs in this chat window, pinned ones first.
    tabs: Vec<ChatTab>,
    /// Recently closed tabs, newest last, for reopening.
    closed_tabs: Vec<acp::SessionId>,
    /// Watches the window until its shepherd panel arrives.
    _agent_panel_watch: Option<Subscription>,
    /// Watches the conversation on screen, keyed by its entity.
    _active_view_watch: Option<(EntityId, Subscription)>,
    _load: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl ChatTreePanel {
    fn new(workspace: WeakEntity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter_editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("search conversations", window, cx);
            editor
        });
        let mut subscriptions = vec![cx.subscribe(&filter_editor, |this, _, event, cx| {
            if matches!(event, EditorEvent::BufferEdited) {
                this.rebuild_rows(cx);
            }
        })];
        if let Some(thread_store) = agent::ThreadStore::try_global(cx) {
            subscriptions.push(cx.observe(&thread_store, |this, _, cx| this.reload(cx)));
        }
        let revision = room::chat_tree_revision(cx);
        subscriptions.push(cx.observe(&revision, |this, _, cx| this.reload(cx)));
        let mut panel = Self {
            workspace,
            focus_handle: cx.focus_handle(),
            list_focus_handle: cx.focus_handle(),
            filter_editor,
            position: DockPosition::Left,
            nodes: Vec::new(),
            conversations: HashMap::default(),
            rows: Vec::new(),
            selected: None,
            observing_agent_panel: false,
            tabs: Vec::new(),
            closed_tabs: Vec::new(),
            _agent_panel_watch: None,
            _active_view_watch: None,
            _load: None,
            _subscriptions: subscriptions,
        };
        panel.reload(cx);
        panel
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let rows = chat_tree::list_chat_conversations(cx);
        self._load = Some(cx.spawn(async move |this, cx| {
            let conversations = rows.await.log_err().unwrap_or_default();
            this.update(cx, |this, cx| {
                // Reading the workspace waits until here: reload also runs
                // while the workspace is mid-update (adding or activating
                // this panel), where reading it would panic.
                this.observe_agent_panel(cx);
                this.conversations = conversations
                    .into_iter()
                    .map(|conversation| (conversation.id.clone(), conversation))
                    .collect();
                this.nodes = this.collect_nodes(cx);
                this.rebuild_rows(cx);
            })
            .log_err();
        }));
    }

    /// Re-renders when the chat window's shepherd panel switches threads, so
    /// the open conversation stays marked.
    fn observe_agent_panel(&mut self, cx: &mut Context<Self>) {
        if self.observing_agent_panel {
            return;
        }
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let Some(panel) = workspace.read(cx).panel::<AgentPanel>(cx) else {
            // The shepherd panel can join the chat window after this tree
            // has loaded; look again whenever the window changes, or a new
            // chat would get no tab until the tree next reloads.
            if self._agent_panel_watch.is_none() {
                self._agent_panel_watch = Some(
                    cx.observe(&workspace, |this, _, cx| this.observe_agent_panel(cx)),
                );
            }
            return;
        };
        self._agent_panel_watch = None;
        self.observing_agent_panel = true;
        self._subscriptions.push(cx.observe(&panel, |this, _, cx| {
            this.track_active_tab(cx);
            cx.notify();
        }));
        self.track_active_tab(cx);
    }

    fn collect_nodes(&self, cx: &App) -> Vec<ConversationNode<acp::SessionId>> {
        let chat_directory = paths::chat_directory();
        let mut nodes = Vec::new();
        if let Some(thread_store) = agent::ThreadStore::try_global(cx) {
            for thread in thread_store.read(cx).entries() {
                let in_chat = !thread.folder_paths.paths().is_empty()
                    && thread
                        .folder_paths
                        .paths()
                        .iter()
                        .all(|path| path == &chat_directory);
                if !in_chat || thread.parent_session_id.is_some() {
                    continue;
                }
                let row = self.conversations.get(&thread.id);
                let title = if !thread.title.is_empty() {
                    thread.title.to_string()
                } else if let Some(row) = row.filter(|row| !row.title.is_empty()) {
                    row.title.to_string()
                } else {
                    "new chat".to_string()
                };
                nodes.push(ConversationNode {
                    id: thread.id.clone(),
                    parent: row.and_then(|row| row.parent_conversation_id.clone()),
                    kind: row.map_or(ConversationKind::Root, |row| row.kind),
                    title,
                    agent: row.and_then(|row| row.agent.clone()),
                    created_at: thread.created_at.unwrap_or(thread.updated_at),
                    updated_at: thread.updated_at,
                });
            }
        }
        for row in self.conversations.values() {
            if row.kind == ConversationKind::Handoff {
                nodes.push(ConversationNode {
                    id: row.id.clone(),
                    parent: row.parent_conversation_id.clone(),
                    kind: ConversationKind::Handoff,
                    title: format!("handed to code in {}", row.title),
                    agent: None,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                });
            }
        }
        nodes
    }

    fn rebuild_rows(&mut self, cx: &mut Context<Self>) {
        let query = self.filter_editor.read(cx).text(cx);
        let selected_id = self.selected_id();
        self.rows = build_tree(
            self.nodes.clone(),
            &chrono::Local::now(),
            Some(query.as_str()),
        );
        self.selected = selected_id.and_then(|id| {
            self.rows.iter().position(
                |row| matches!(row, TreeRow::Conversation { id: row_id, .. } if *row_id == id),
            )
        });
        cx.notify();
    }

    fn selected_id(&self) -> Option<acp::SessionId> {
        match self.rows.get(self.selected?)? {
            TreeRow::Conversation { id, .. } => Some(id.clone()),
            TreeRow::Group(_) => None,
        }
    }

    fn active_session(&self, cx: &App) -> Option<acp::SessionId> {
        self.workspace
            .upgrade()?
            .read(cx)
            .panel::<AgentPanel>(cx)?
            .read(cx)
            .active_conversation_view()?
            .read(cx)
            .root_session_id
            .clone()
    }

    fn move_selection(&mut self, forward: bool, cx: &mut Context<Self>) {
        let conversation_rows: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| matches!(row, TreeRow::Conversation { .. }))
            .map(|(index, _)| index)
            .collect();
        let next = match (self.selected, forward) {
            (None, true) => conversation_rows.first().copied(),
            (None, false) => conversation_rows.last().copied(),
            (Some(current), true) => conversation_rows
                .iter()
                .copied()
                .find(|index| *index > current)
                .or(Some(current)),
            (Some(current), false) => conversation_rows
                .iter()
                .rev()
                .copied()
                .find(|index| *index < current)
                .or(Some(current)),
        };
        self.selected = next;
        cx.notify();
    }

    fn open_row(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(TreeRow::Conversation { id, kind, .. }) = self.rows.get(index).cloned() else {
            return;
        };
        self.selected = Some(index);
        cx.notify();
        if kind == ConversationKind::Handoff {
            let Some(project_path) = self
                .conversations
                .get(&id)
                .and_then(|row| row.project_path.clone())
            else {
                return;
            };
            cx.defer(move |cx| room::open_handoff_thread(project_path, id, cx));
            return;
        }
        self.open_session(id, window, cx);
    }

    fn open_session(&self, id: acp::SessionId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        window.defer(cx, move |window, cx| {
            workspace.update(cx, |workspace, cx| {
                if let Some(panel) = workspace.focus_panel::<AgentPanel>(window, cx) {
                    panel.update(cx, |panel, cx| {
                        panel.open_thread(
                            id,
                            Some(PathList::new(&[paths::chat_directory()])),
                            None,
                            window,
                            cx,
                        )
                    });
                }
            });
        });
    }

    fn new_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        window.defer(cx, move |window, cx| {
            workspace.update(cx, |workspace, cx| {
                if let Some(panel) = workspace.focus_panel::<AgentPanel>(window, cx) {
                    panel.update(cx, |panel, cx| panel.new_thread(&NewThread, window, cx));
                }
            });
        });
    }

    fn handle_list_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        match keystroke.key.as_str() {
            "j" | "down" => self.move_selection(true, cx),
            "k" | "up" => self.move_selection(false, cx),
            "enter" => {
                if let Some(index) = self.selected {
                    self.open_row(index, window, cx);
                }
            }
            "/" => {
                self.filter_editor.focus_handle(cx).focus(window, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn render_row(
        &self,
        index: usize,
        row: &TreeRow<acp::SessionId>,
        active_session: Option<&acp::SessionId>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match row {
            TreeRow::Group(group) => div()
                .px_2()
                .pt_3()
                .pb_1()
                .child(
                    Label::new(group.label())
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
                .into_any_element(),
            TreeRow::Conversation {
                id,
                depth,
                kind,
                title,
                agent,
            } => {
                let is_open = active_session == Some(id);
                let is_selected = self.selected == Some(index);
                let row_info = self.conversations.get(id);
                let tooltip: Option<SharedString> = match kind {
                    ConversationKind::Branch => row_info.map(|row| {
                        let from = row
                            .parent_conversation_id
                            .as_ref()
                            .and_then(|parent| {
                                self.nodes
                                    .iter()
                                    .find(|node| node.id == *parent)
                                    .map(|node| node.title.clone())
                            })
                            .unwrap_or_else(|| row.title.to_string());
                        format!(
                            "branched from \"{from}\" after {} messages",
                            row.inherited_message_count
                        )
                        .into()
                    }),
                    ConversationKind::Side => row_info.map(|row| row.title.clone()),
                    ConversationKind::Handoff => {
                        Some("opens the shepherd-room thread this chat was handed to".into())
                    }
                    ConversationKind::Root => None,
                };
                h_flex()
                    .id(("chat-tree-row", index))
                    .w_full()
                    .gap_1()
                    .py_0p5()
                    .pr_2()
                    .pl(px(8. + INDENT * *depth as f32))
                    .rounded_sm()
                    .cursor_pointer()
                    .when(is_selected, |this| {
                        this.bg(cx.theme().colors().ghost_element_selected)
                    })
                    .hover(|style| style.bg(cx.theme().colors().ghost_element_hover))
                    .children(
                        kind.glyph().map(|glyph| {
                            Label::new(glyph).size(LabelSize::Small).color(Color::Muted)
                        }),
                    )
                    .child(
                        Label::new(title.to_lowercase())
                            .size(LabelSize::Small)
                            .color(if is_open {
                                Color::Default
                            } else {
                                Color::Muted
                            })
                            .truncate(),
                    )
                    .children(agent.as_ref().map(|agent| {
                        Label::new(format!("· {agent}"))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted)
                    }))
                    .when(is_open, |this| {
                        this.child(Label::new("◦").size(LabelSize::XSmall).color(Color::Muted))
                    })
                    .when_some(tooltip, |this, tooltip| {
                        this.tooltip(Tooltip::text(tooltip))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_row(index, window, cx);
                    }))
                    .into_any_element()
            }
        }
    }
}

impl EventEmitter<PanelEvent> for ChatTreePanel {}

impl Focusable for ChatTreePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ChatTreePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_session = self.active_session(cx);
        let rows: Vec<AnyElement> = self
            .rows
            .clone()
            .iter()
            .enumerate()
            .map(|(index, row)| self.render_row(index, row, active_session.as_ref(), cx))
            .collect();
        let is_empty = rows.is_empty();
        let has_query = !self.filter_editor.read(cx).text(cx).trim().is_empty();
        v_flex()
            .key_context("AsherinChatTree")
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().colors().panel_background)
            .child(
                h_flex()
                    .px_2()
                    .py_1p5()
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().colors().border)
                    .child(
                        Label::new("asherin.chat")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .child(
                        IconButton::new("chat-tree-new", IconName::Plus)
                            .icon_size(IconSize::Small)
                            .icon_color(Color::Muted)
                            .tooltip(Tooltip::text("new chat"))
                            .on_click(cx.listener(|this, _, window, cx| this.new_chat(window, cx))),
                    ),
            )
            .child(
                div()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().colors().border)
                    .child(self.filter_editor.clone()),
            )
            .child(
                v_flex()
                    .id("chat-tree-rows")
                    .track_focus(&self.list_focus_handle)
                    .on_key_down(cx.listener(Self::handle_list_key))
                    .flex_1()
                    .min_h_0()
                    .px_1()
                    .pb_2()
                    .overflow_y_scroll()
                    .when(is_empty, |this| {
                        this.child(
                            div().px_2().pt_3().child(
                                Label::new(if has_query {
                                    "nothing matches"
                                } else {
                                    "no conversations yet"
                                })
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                            ),
                        )
                    })
                    .children(rows),
            )
    }
}

impl Panel for ChatTreePanel {
    fn persistent_name() -> &'static str {
        "AsherinChatTree"
    }

    fn panel_key() -> &'static str {
        "AsherinChatTree"
    }

    fn activation_focus_handle(&self, _cx: &App) -> FocusHandle {
        self.list_focus_handle.clone()
    }

    fn position(&self, _window: &Window, _cx: &App) -> DockPosition {
        self.position
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Left | DockPosition::Right)
    }

    fn set_position(&mut self, position: DockPosition, _: &mut Window, cx: &mut Context<Self>) {
        self.position = position;
        cx.notify();
    }

    fn default_size(&self, _window: &Window, _cx: &App) -> Pixels {
        px(260.)
    }

    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        Some(IconName::ListTree)
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("asherin.chat conversations")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleTree)
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if active {
            self.reload(cx);
        }
    }

    fn activation_priority(&self) -> u32 {
        23
    }
}

/// The chat window's active conversation. Takes the workspace rather than
/// reading it, so actions that run while it's being updated can call it.
fn active_session_in(workspace: &Workspace, cx: &App) -> Option<acp::SessionId> {
    workspace
        .panel::<AgentPanel>(cx)?
        .read(cx)
        .active_conversation_view()?
        .read(cx)
        .root_session_id
        .clone()
}

impl ChatTreePanel {
    fn track_active_tab(&mut self, cx: &mut Context<Self>) {
        self.watch_active_view(cx);
        let Some(active) = self.active_session(cx) else {
            return;
        };
        if self.tabs.iter().any(|tab| tab.id == active) {
            return;
        }
        self.tabs.push(ChatTab {
            id: active.clone(),
            pinned: false,
        });
        self.closed_tabs.retain(|id| *id != active);
        cx.notify();
        // The strip is drawn by the shepherd panel, which reads this panel
        // but does not observe it; without a nudge the new tab shows up one
        // render late, after whatever next happens to redraw the chat.
        if let Some(panel) = self
            .workspace
            .upgrade()
            .and_then(|workspace| workspace.read(cx).panel::<AgentPanel>(cx))
        {
            panel.update(cx, |_, cx| cx.notify());
        }
    }

    /// A conversation loaded from storage, like a reopened tab, learns its
    /// session only once it has loaded, and it tells no one but itself; the
    /// shepherd panel has already announced the switch by then. Watching
    /// the conversation on screen is what gives it its tab.
    fn watch_active_view(&mut self, cx: &mut Context<Self>) {
        let view = self
            .workspace
            .upgrade()
            .and_then(|workspace| workspace.read(cx).panel::<AgentPanel>(cx))
            .and_then(|panel| panel.read(cx).active_conversation_view().cloned());
        let watched = self._active_view_watch.as_ref().map(|(id, _)| *id);
        if watched == view.as_ref().map(|view| view.entity_id()) {
            return;
        }
        self._active_view_watch = view.map(|view| {
            (
                view.entity_id(),
                cx.observe(&view, |this, _, cx| this.track_active_tab(cx)),
            )
        });
    }

    fn tab_title(&self, id: &acp::SessionId) -> (SharedString, ConversationKind) {
        self.nodes
            .iter()
            .find(|node| &node.id == id)
            .map(|node| (SharedString::from(node.title.clone()), node.kind))
            .unwrap_or_else(|| ("new chat".into(), ConversationKind::Root))
    }

    /// Pinned tabs first, each group in the order it was opened.
    fn ordered_tabs(&self) -> Vec<ChatTab> {
        let (mut pinned, unpinned): (Vec<ChatTab>, Vec<ChatTab>) =
            self.tabs.iter().cloned().partition(|tab| tab.pinned);
        pinned.extend(unpinned);
        pinned
    }

    /// The tabs to draw, and the oldest unpinned ones that collapse into the
    /// overflow menu. The active tab always stays visible.
    fn visible_tabs(&self, active: Option<&acp::SessionId>) -> (Vec<ChatTab>, Vec<ChatTab>) {
        let ordered = self.ordered_tabs();
        let unpinned = ordered.iter().filter(|tab| !tab.pinned).count();
        let mut excess = unpinned.saturating_sub(MAX_VISIBLE_TABS);
        let mut visible = Vec::new();
        let mut overflow = Vec::new();
        for tab in ordered {
            if excess > 0 && !tab.pinned && Some(&tab.id) != active {
                excess -= 1;
                overflow.push(tab);
            } else {
                visible.push(tab);
            }
        }
        (visible, overflow)
    }

    fn activate_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.ordered_tabs().get(index) {
            self.open_session(tab.id.clone(), window, cx);
        }
    }

    fn cycle_tab(
        &mut self,
        forward: bool,
        active: Option<acp::SessionId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ordered = self.ordered_tabs();
        if ordered.is_empty() {
            return;
        }
        let current = active
            .and_then(|active| ordered.iter().position(|tab| tab.id == active))
            .unwrap_or(0);
        let next = if forward {
            (current + 1) % ordered.len()
        } else {
            (current + ordered.len() - 1) % ordered.len()
        };
        if let Some(tab) = ordered.get(next) {
            self.open_session(tab.id.clone(), window, cx);
        }
    }

    fn close_tab(
        &mut self,
        id: &acp::SessionId,
        active: Option<&acp::SessionId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ordered = self.ordered_tabs();
        let Some(position) = ordered.iter().position(|tab| &tab.id == id) else {
            return;
        };
        self.tabs.retain(|tab| &tab.id != id);
        self.closed_tabs.push(id.clone());
        if active == Some(id) {
            // Show the neighbour that takes its place, like closing an editor
            // tab; with no tabs left, start a new chat.
            let neighbour = ordered
                .get(position + 1)
                .or_else(|| position.checked_sub(1).and_then(|index| ordered.get(index)));
            match neighbour {
                Some(tab) => self.open_session(tab.id.clone(), window, cx),
                None => self.new_chat(window, cx),
            }
        }
        cx.notify();
    }

    fn reopen_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.closed_tabs.pop() {
            // The tab is known before the conversation has loaded, so it goes
            // back on the strip now rather than when the load reports in.
            if !self.tabs.iter().any(|tab| tab.id == id) {
                self.tabs.push(ChatTab {
                    id: id.clone(),
                    pinned: false,
                });
            }
            self.open_session(id, window, cx);
            cx.notify();
            // This runs inside the workspace's own update (it is a workspace
            // action), so the panel is looked up once that update is over.
            cx.defer_in(window, |this, _, cx| {
                if let Some(panel) = this
                    .workspace
                    .upgrade()
                    .and_then(|workspace| workspace.read(cx).panel::<AgentPanel>(cx))
                {
                    panel.update(cx, |_, cx| cx.notify());
                }
            });
        }
    }

    fn toggle_pin(&mut self, id: &acp::SessionId, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| &tab.id == id) {
            tab.pinned = !tab.pinned;
            cx.notify();
        }
    }

    /// Opens `id` beside the chat, in the window's centre, so two
    /// conversations can be read side by side.
    fn open_beside(&self, id: acp::SessionId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let title = self.tab_title(&id).0;
        window.defer(cx, move |window, cx| {
            workspace.update(cx, |workspace, cx| {
                let Some(panel) = workspace.panel::<AgentPanel>(cx) else {
                    return;
                };
                let Some(conversation_view) = panel.update(cx, |panel, cx| {
                    panel.conversation_for_split(
                        id,
                        Some(PathList::new(&[paths::chat_directory()])),
                        window,
                        cx,
                    )
                }) else {
                    return;
                };
                let item = cx.new(|_| ChatSplitItem {
                    conversation_view,
                    title,
                });
                workspace.add_item_to_active_pane(Box::new(item), None, true, window, cx);
            });
        });
    }

    /// The tab strip drawn above an asherin.chat conversation.
    pub(crate) fn render_tab_strip(
        panel: &Entity<Self>,
        active: Option<&acp::SessionId>,
        cx: &App,
    ) -> Option<AnyElement> {
        let this = panel.read(cx);
        let (visible, overflow) = this.visible_tabs(active);
        if visible.is_empty() {
            return None;
        }
        let colors = cx.theme().colors();
        let border = colors.border;
        let underline_color = colors.text;
        let tabs: Vec<AnyElement> = visible
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let (title, kind) = this.tab_title(&tab.id);
                let glyph = match kind {
                    ConversationKind::Root => "",
                    ConversationKind::Branch => "↳ ",
                    ConversationKind::Side => "~ ",
                    ConversationKind::Handoff => "→ ",
                };
                let is_active = Some(&tab.id) == active;
                let id = tab.id.clone();
                let pinned = tab.pinned;
                let underline_id = SharedString::from(format!("chat-tab-line-{}", tab.id.0));
                let fade_id = SharedString::from(format!("chat-tab-in-{}", tab.id.0));
                let trigger = {
                    let panel = panel.clone();
                    let id = id.clone();
                    let close_panel = panel.clone();
                    let close_id = id.clone();
                    let middle_panel = panel.clone();
                    let middle_id = id.clone();
                    let active = active.cloned();
                    let middle_active = active.clone();
                    v_flex()
                        .id(("chat-tab", index))
                        .max_w(px(200.))
                        .cursor_pointer()
                        .child(
                            h_flex()
                                .gap_1()
                                .py_1()
                                .when(pinned, |row| {
                                    row.child(
                                        Icon::new(IconName::Pin)
                                            .size(IconSize::XSmall)
                                            .color(Color::Muted),
                                    )
                                })
                                .child(
                                    Label::new(format!("{glyph}{title}"))
                                        .size(LabelSize::Small)
                                        .color(if is_active {
                                            Color::Default
                                        } else {
                                            Color::Muted
                                        })
                                        .truncate(),
                                )
                                .child(
                                    IconButton::new(("chat-tab-close", index), IconName::Close)
                                        .icon_size(IconSize::XSmall)
                                        .icon_color(Color::Muted)
                                        .tooltip(Tooltip::text("close tab"))
                                        .on_click(move |_, window, cx| {
                                            cx.stop_propagation();
                                            close_panel.update(cx, |panel, cx| {
                                                panel.close_tab(
                                                    &close_id,
                                                    active.as_ref(),
                                                    window,
                                                    cx,
                                                )
                                            });
                                        }),
                                ),
                        )
                        .child(if is_active {
                            // The underline draws itself across the tab each
                            // time the tab becomes the active one.
                            div()
                                .h(px(1.))
                                .bg(underline_color)
                                .with_animation(
                                    underline_id,
                                    Animation::new(Duration::from_millis(180))
                                        .with_easing(ease_out_quint()),
                                    |line, delta| line.w(relative(delta)),
                                )
                                .into_any_element()
                        } else {
                            div().h(px(1.)).into_any_element()
                        })
                        .on_click(move |_, window, cx| {
                            panel
                                .update(cx, |panel, cx| panel.open_session(id.clone(), window, cx));
                        })
                        .on_mouse_down(MouseButton::Middle, move |_, window, cx| {
                            middle_panel.update(cx, |panel, cx| {
                                panel.close_tab(&middle_id, middle_active.as_ref(), window, cx)
                            });
                        })
                        .with_animation(
                            fade_id,
                            Animation::new(Duration::from_millis(180))
                                .with_easing(ease_out_quint()),
                            |tab, delta| tab.opacity(delta),
                        )
                };
                let menu_panel = panel.clone();
                let menu_id = id.clone();
                let menu_active = active.cloned();
                right_click_menu(("chat-tab-menu", index))
                    .trigger(move |_, _, _| trigger)
                    .menu(move |window, cx| {
                        let panel = menu_panel.clone();
                        let id = menu_id.clone();
                        let active = menu_active.clone();
                        ContextMenu::build(window, cx, move |menu, _, _| {
                            let pin_panel = panel.clone();
                            let pin_id = id.clone();
                            let beside_panel = panel.clone();
                            let beside_id = id.clone();
                            let close_panel = panel.clone();
                            let close_id = id.clone();
                            let active = active.clone();
                            menu.entry(
                                if pinned { "unpin tab" } else { "pin tab" },
                                None,
                                move |_, cx| {
                                    pin_panel.update(cx, |panel, cx| panel.toggle_pin(&pin_id, cx));
                                },
                            )
                            .entry("open beside", None, move |window, cx| {
                                beside_panel.update(cx, |panel, cx| {
                                    panel.open_beside(beside_id.clone(), window, cx)
                                });
                            })
                            .entry(
                                "close tab",
                                None,
                                move |window, cx| {
                                    close_panel.update(cx, |panel, cx| {
                                        panel.close_tab(&close_id, active.as_ref(), window, cx)
                                    });
                                },
                            )
                        })
                    })
                    .into_any_element()
            })
            .collect();
        let overflow_menu = (!overflow.is_empty()).then(|| {
            let panel = panel.clone();
            let entries: Vec<(acp::SessionId, SharedString)> = overflow
                .iter()
                .map(|tab| (tab.id.clone(), this.tab_title(&tab.id).0))
                .collect();
            let count = entries.len();
            ui::PopoverMenu::new("chat-tab-overflow")
                .trigger(
                    Button::new("chat-tab-overflow-trigger", format!("+{count}"))
                        .label_size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .menu(move |window, cx| {
                    let panel = panel.clone();
                    let entries = entries.clone();
                    Some(ContextMenu::build(window, cx, move |mut menu, _, _| {
                        for (id, title) in entries.iter().cloned() {
                            let panel = panel.clone();
                            menu = menu.entry(title, None, move |window, cx| {
                                panel.update(cx, |panel, cx| {
                                    panel.open_session(id.clone(), window, cx)
                                });
                            });
                        }
                        menu
                    }))
                })
                .into_any_element()
        });
        let new_panel = panel.clone();
        Some(
            h_flex()
                .w_full()
                .px_2()
                .gap_3()
                .border_b_1()
                .border_color(border)
                .overflow_hidden()
                .children(tabs)
                .children(overflow_menu)
                .child(
                    IconButton::new("chat-tab-new", IconName::Plus)
                        .icon_size(IconSize::XSmall)
                        .icon_color(Color::Muted)
                        .tooltip(Tooltip::text("new chat"))
                        .on_click(move |_, window, cx| {
                            new_panel.update(cx, |panel, cx| panel.new_chat(window, cx));
                        }),
                )
                .into_any_element(),
        )
    }
}

/// The chat window's tab shortcuts. They're bound only inside asherin.chat's
/// conversation, so they don't change the editor's own tab keys.
pub(crate) fn register_tab_actions(workspace: &mut Workspace) {
    fn with_tabs(
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
        run: impl FnOnce(
            &mut ChatTreePanel,
            Option<acp::SessionId>,
            &mut Window,
            &mut Context<ChatTreePanel>,
        ),
    ) {
        let Some(panel) = workspace.panel::<ChatTreePanel>(cx) else {
            return;
        };
        let active = active_session_in(workspace, cx);
        panel.update(cx, |panel, cx| run(panel, active, window, cx));
    }
    workspace.register_action(|workspace, _: &NewChatTab, window, cx| {
        with_tabs(workspace, window, cx, |panel, _, window, cx| {
            panel.new_chat(window, cx)
        });
    });
    workspace.register_action(|workspace, _: &CloseChatTab, window, cx| {
        with_tabs(workspace, window, cx, |panel, active, window, cx| {
            if let Some(active) = active {
                panel.close_tab(&active, Some(&active), window, cx);
            }
        });
    });
    workspace.register_action(|workspace, _: &NextChatTab, window, cx| {
        with_tabs(workspace, window, cx, |panel, active, window, cx| {
            panel.cycle_tab(true, active, window, cx)
        });
    });
    workspace.register_action(|workspace, _: &PreviousChatTab, window, cx| {
        with_tabs(workspace, window, cx, |panel, active, window, cx| {
            panel.cycle_tab(false, active, window, cx)
        });
    });
    workspace.register_action(|workspace, _: &ReopenChatTab, window, cx| {
        with_tabs(workspace, window, cx, |panel, _, window, cx| {
            panel.reopen_tab(window, cx)
        });
    });
    workspace.register_action(|workspace, action: &ActivateChatTab, window, cx| {
        let index = action.0.saturating_sub(1);
        with_tabs(workspace, window, cx, |panel, _, window, cx| {
            panel.activate_tab(index, window, cx)
        });
    });
    workspace.register_action(|workspace, _: &OpenChatBeside, window, cx| {
        with_tabs(workspace, window, cx, |panel, active, window, cx| {
            if let Some(active) = active {
                panel.open_beside(active, window, cx);
            }
        });
    });
}

/// A conversation opened beside the chat to compare it with another.
pub(crate) struct ChatSplitItem {
    conversation_view: Entity<crate::ConversationView>,
    title: SharedString,
}

impl EventEmitter<()> for ChatSplitItem {}

impl Focusable for ChatSplitItem {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.conversation_view.focus_handle(cx)
    }
}

impl Render for ChatSplitItem {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.conversation_view.clone())
    }
}

impl workspace::Item for ChatSplitItem {
    type Event = ();

    fn include_in_nav_history() -> bool {
        false
    }

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        self.title.clone()
    }
}
