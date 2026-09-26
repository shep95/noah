//! asherin.chat's conversation tree: a left panel, hidden until ctrl-b,
//! listing chats by recency with their branches (↳), side chats (~) and
//! hand-offs to code (→) nested under where they came from.

use agent::ChatConversation;
use agent::chat_tree;
use agent_client_protocol::schema::v1 as acp;
use asherin_chat::tree::{ConversationKind, ConversationNode, TreeRow, build_tree};
use collections::HashMap;
use editor::{Editor, EditorEvent};
use gpui::{
    Action, App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent, Pixels,
    Subscription, Task, WeakEntity, Window, px,
};
use ui::{Tooltip, prelude::*};
use util::ResultExt as _;
use util::path_list::PathList;
use workspace::Workspace;
use workspace::dock::{DockPosition, Panel, PanelEvent};

use crate::asherin_chat::{self as room, ToggleTree};
use crate::{AgentPanel, NewThread};

const INDENT: f32 = 14.;

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
    let weak_workspace = workspace.weak_handle();
    let panel = cx.new(|cx| ChatTreePanel::new(weak_workspace, window, cx));
    workspace.add_panel(panel, window, cx);
    // ctrl-b toggles the left dock, so the tree is the panel it shows.
    workspace.left_dock().update(cx, |dock, cx| {
        if let Some(index) = dock.panel_index_for_type::<ChatTreePanel>() {
            dock.activate_panel(index, window, cx);
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
            _load: None,
            _subscriptions: subscriptions,
        };
        panel.reload(cx);
        panel
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.observe_agent_panel(cx);
        let rows = chat_tree::list_chat_conversations(cx);
        self._load = Some(cx.spawn(async move |this, cx| {
            let conversations = rows.await.log_err().unwrap_or_default();
            this.update(cx, |this, cx| {
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
        let Some(panel) = self
            .workspace
            .upgrade()
            .and_then(|workspace| workspace.read(cx).panel::<AgentPanel>(cx))
        else {
            return;
        };
        self.observing_agent_panel = true;
        self._subscriptions
            .push(cx.observe(&panel, |_, _, cx| cx.notify()));
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
