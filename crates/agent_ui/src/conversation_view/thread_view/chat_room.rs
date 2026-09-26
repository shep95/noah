//! The thread view's asherin.chat parts: slash commands the room handles
//! itself, redaction before sending, branching (including editing a sent
//! message), bring back, the hand-off to code, custom chat agents and the
//! per-turn controls. Threads outside the chat room only get "discuss in
//! chat".

use agent::ChatConversation;
use agent::chat_tree;
use asherin_chat::tree::ConversationKind;
use asherin_chat::{BranchPoint, SlashCommand};
use ui::{ContextMenu, PopoverMenu, Tooltip};
use util::path_list::PathList;

use super::*;
use crate::asherin_chat::{self as room, CodeProjectChoice};
use crate::message_editor::MessageEditor;
use crate::{AgentInitialContent, AgentPanel};
use gpui::AsyncApp;
use workspace::Workspace;

#[derive(Default)]
pub(crate) struct ChatRoomState {
    /// This conversation's row in the tree, when it has one.
    conversation: Option<ChatConversation>,
    /// Branches split from this conversation.
    branches: Vec<ChatConversation>,
    parent_title: Option<SharedString>,
    redaction: Option<PendingRedaction>,
    send_unredacted_once: bool,
    status: Option<SharedString>,
    agent_menu_handle: PopoverMenuHandle<ContextMenu>,
    /// What each turn cost, by the id of the user message that started it.
    turn_costs: collections::HashMap<String, f64>,
    _load: Option<Task<()>>,
    _costs: Option<Task<()>>,
    _work: Option<Task<()>>,
}

struct PendingRedaction {
    original: Vec<acp::ContentBlock>,
    count: usize,
}

fn chat_work_dirs() -> Option<PathList> {
    Some(PathList::new(&[paths::chat_directory()]))
}

impl ThreadView {
    pub(crate) fn is_chat_room(&self, cx: &App) -> bool {
        self.project
            .upgrade()
            .is_some_and(|project| agent::Thread::is_chat_project(&project, cx))
    }

    pub(crate) fn is_subagent_view(&self) -> bool {
        self.is_subagent()
    }

    pub(crate) fn is_chat_branch(&self) -> bool {
        self.chat_room
            .conversation
            .as_ref()
            .is_some_and(|conversation| conversation.kind == ConversationKind::Branch)
    }

    fn native_agent(&self, cx: &App) -> Option<Entity<agent::NativeAgent>> {
        self.as_native_connection(cx)
            .map(|connection| connection.0.clone())
    }

    /// Loads this conversation's place in the tree and what its turns cost,
    /// and keeps both current. The chat folder can finish opening after the
    /// first thread view exists, so the checks happen when each update comes.
    pub(super) fn start_chat_room(&mut self, cx: &mut Context<Self>) {
        if self.is_subagent() {
            return;
        }
        let revision = room::chat_tree_revision(cx);
        self._subscriptions
            .push(cx.observe(&revision, |this, _, cx| {
                if this.is_chat_room(cx) {
                    this.load_chat_room_state(cx);
                }
            }));
        self._subscriptions.push(cx.subscribe(
            &self.thread,
            |this, _, event: &acp_thread::AcpThreadEvent, cx| {
                if matches!(
                    event,
                    acp_thread::AcpThreadEvent::Stopped(_) | acp_thread::AcpThreadEvent::Error
                ) && this.is_chat_room(cx)
                {
                    this.load_turn_costs(cx);
                }
            },
        ));
        if self.is_chat_room(cx) {
            self.load_chat_room_state(cx);
            self.load_turn_costs(cx);
        }
    }

    fn load_turn_costs(&mut self, cx: &mut Context<Self>) {
        let session_id = self.session_id.clone();
        self.chat_room._costs = Some(cx.spawn(async move |this, cx| {
            // Costs are written as usage arrives; give the last write of a
            // turn a moment to land.
            cx.background_executor()
                .timer(std::time::Duration::from_millis(300))
                .await;
            let costs = cx.update(|cx| chat_tree::chat_turn_costs(session_id, cx));
            let Some(costs) = costs.await.log_err() else {
                return;
            };
            this.update(cx, |this, cx| {
                this.chat_room.turn_costs = costs;
                cx.notify();
            })
            .log_err();
        }));
    }

    fn load_chat_room_state(&mut self, cx: &mut Context<Self>) {
        let session_id = self.session_id.clone();
        let conversations = chat_tree::list_chat_conversations(cx);
        self.chat_room._load = Some(cx.spawn(async move |this, cx| {
            let Some(conversations) = conversations.await.log_err() else {
                return;
            };
            this.update(cx, |this, cx| {
                let conversation = conversations
                    .iter()
                    .find(|conversation| conversation.id == session_id)
                    .cloned();
                let parent_title = conversation
                    .as_ref()
                    .and_then(|conversation| conversation.parent_conversation_id.as_ref())
                    .and_then(|parent_id| {
                        agent::ThreadStore::try_global(cx)?
                            .read(cx)
                            .thread_from_session_id(parent_id)
                            .map(|thread| thread.title.clone())
                    });
                this.chat_room.branches = conversations
                    .into_iter()
                    .filter(|conversation| {
                        conversation.kind == ConversationKind::Branch
                            && conversation.parent_conversation_id.as_ref() == Some(&session_id)
                    })
                    .collect();
                this.chat_room.conversation = conversation;
                this.chat_room.parent_title = parent_title;
                cx.notify();
            })
            .log_err();
        }));
    }

    fn set_chat_status(&mut self, status: Option<SharedString>, cx: &mut Context<Self>) {
        self.chat_room.status = status;
        cx.notify();
    }

    /// Handles what the chat room does before a message goes out. Returns
    /// true when it took care of the send itself.
    pub(super) fn intercept_chat_room_send(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.is_subagent() || !self.is_chat_room(cx) {
            return false;
        }
        let text = self.message_editor.read(cx).text(cx);
        if let Some(parsed) = asherin_chat::parse_slash_command(&text) {
            match parsed.command {
                SlashCommand::Code => {
                    let note = parsed.argument.to_string();
                    self.message_editor
                        .update(cx, |editor, cx| editor.clear(window, cx));
                    self.take_to_code(note, window, cx);
                    return true;
                }
                SlashCommand::Agent => {
                    let argument = parsed.argument.to_string();
                    self.message_editor
                        .update(cx, |editor, cx| editor.clear(window, cx));
                    self.run_agent_command(&argument, window, cx);
                    return true;
                }
                SlashCommand::Research
                | SlashCommand::Draft
                | SlashCommand::Explain
                | SlashCommand::Compare => {}
            }
        }

        if std::mem::take(&mut self.chat_room.send_unredacted_once) {
            self.chat_room.redaction = None;
            return false;
        }
        let blocks = self
            .message_editor
            .read(cx)
            .draft_content_blocks_snapshot(cx);
        let mut count = 0;
        let redacted: Vec<acp::ContentBlock> = blocks
            .iter()
            .map(|block| match block {
                acp::ContentBlock::Text(text) => match asherin_chat::redact_outgoing(&text.text) {
                    Some(redaction) => {
                        count += redaction.count;
                        acp::ContentBlock::Text(acp::TextContent::new(redaction.text))
                    }
                    None => block.clone(),
                },
                other => other.clone(),
            })
            .collect();
        if count == 0 {
            self.chat_room.redaction = None;
            return false;
        }
        self.message_editor.update(cx, |editor, cx| {
            editor.set_message(redacted, window, cx);
        });
        self.chat_room.redaction = Some(PendingRedaction {
            original: blocks,
            count,
        });
        cx.notify();
        true
    }

    fn send_unredacted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(redaction) = self.chat_room.redaction.take() else {
            return;
        };
        self.message_editor.update(cx, |editor, cx| {
            editor.set_message(redaction.original, window, cx);
        });
        self.chat_room.send_unredacted_once = true;
        self.send(window, cx);
    }

    pub(super) fn render_chat_redaction_notice(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let redaction = self.chat_room.redaction.as_ref()?;
        Some(
            h_flex()
                .w_full()
                .gap_1()
                .px_1()
                .child(
                    Label::new(asherin_chat::redaction_notice(redaction.count))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(Label::new("·").size(LabelSize::Small).color(Color::Muted))
                .child(
                    Button::new("chat-send-unredacted", "send anyway")
                        .label_size(LabelSize::Small)
                        .color(Color::Muted)
                        .tooltip(Tooltip::text(
                            "sends the message exactly as you wrote it, secrets included",
                        ))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.send_unredacted(window, cx);
                        })),
                )
                .into_any_element(),
        )
    }

    /// Editing a sent message in the chat room starts a branch from just
    /// before it instead of rewriting the conversation.
    pub(super) fn branch_from_edit(
        &mut self,
        entry_ix: usize,
        editor: Entity<MessageEditor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(client_id) = self
            .thread
            .read(cx)
            .entries()
            .get(entry_ix)
            .and_then(|entry| entry.user_message())
            .and_then(|message| message.client_id.clone())
        else {
            return;
        };
        let blocks = editor.read(cx).draft_content_blocks_snapshot(cx);
        self.cancel_editing(&Default::default(), window, cx);
        self.create_branch(BranchPoint::BeforeMessage(client_id), Some(blocks), cx);
    }

    fn branch_after_turn(&mut self, user_message_index: Option<usize>, cx: &mut Context<Self>) {
        let Some(client_id) = user_message_index
            .and_then(|index| self.thread.read(cx).entries().get(index))
            .and_then(|entry| entry.user_message())
            .and_then(|message| message.client_id.clone())
        else {
            return;
        };
        self.create_branch(BranchPoint::AfterTurn(client_id), None, cx);
    }

    fn create_branch(
        &mut self,
        point: BranchPoint<acp_thread::ClientUserMessageId>,
        first_message: Option<Vec<acp::ContentBlock>>,
        cx: &mut Context<Self>,
    ) {
        let Some(native_agent) = self.native_agent(cx) else {
            return;
        };
        let source = self.session_id.clone();
        let branch =
            native_agent.update(cx, |agent, cx| agent.branch_chat_thread(source, point, cx));
        let workspace = self.workspace.clone();
        self.set_chat_status(Some("branching…".into()), cx);
        self.chat_room._work = Some(cx.spawn(async move |this, cx| {
            let result = branch.await;
            this.update(cx, |this, cx| this.set_chat_status(None, cx))
                .log_err();
            let branch_id = match result {
                Ok(branch_id) => branch_id,
                Err(error) => {
                    this.update(cx, |this, cx| this.handle_thread_error(error, cx))
                        .log_err();
                    return;
                }
            };
            cx.update(room::notify_chat_tree_changed);
            open_chat_conversation(workspace, branch_id, first_message, cx);
        }));
    }

    pub(crate) fn bring_back(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_chat_branch() {
            self.handle_thread_error(
                anyhow::anyhow!("only a branch can be brought back to its parent"),
                cx,
            );
            return;
        }
        let (Some(native_agent), Some(project)) = (self.native_agent(cx), self.project.upgrade())
        else {
            return;
        };
        let branch_id = self.session_id.clone();
        let bring_back = native_agent.update(cx, |agent, cx| {
            agent.bring_back_chat_branch(branch_id, project, cx)
        });
        let workspace = self.workspace.clone();
        self.set_chat_status(Some("bringing back…".into()), cx);
        self.chat_room._work = Some(cx.spawn(async move |this, cx| {
            let result = bring_back.await;
            this.update(cx, |this, cx| this.set_chat_status(None, cx))
                .log_err();
            match result {
                Ok((parent_id, _message)) => {
                    cx.update(room::notify_chat_tree_changed);
                    open_chat_conversation(workspace, parent_id, None, cx);
                }
                Err(error) => {
                    this.update(cx, |this, cx| this.handle_thread_error(error, cx))
                        .log_err();
                }
            }
        }));
    }

    pub(crate) fn take_to_code(
        &mut self,
        note: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_chat_room(cx) {
            self.handle_thread_error(
                anyhow::anyhow!("take this to code works from asherin.chat"),
                cx,
            );
            return;
        }
        let Some(native_agent) = self.native_agent(cx) else {
            return;
        };
        if self.thread.read(cx).entries().is_empty() {
            self.handle_thread_error(
                anyhow::anyhow!("there's nothing in this chat to take to code yet"),
                cx,
            );
            return;
        }
        let chat_session = self.session_id.clone();
        let chat_workspace = self.workspace.clone();
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let on_pick: Box<dyn FnOnce(CodeProjectChoice, &mut App)> = Box::new(move |project, cx| {
            room::run_handoff(
                chat_workspace,
                native_agent,
                chat_session,
                note,
                project,
                cx,
            );
        });
        window.defer(cx, move |window, cx| {
            workspace.update(cx, |workspace, cx| {
                room::pick_code_project(workspace, on_pick, window, cx);
            });
        });
    }

    /// Opens a side conversation in asherin.chat about this shepherd-room
    /// thread, starting from where the task stands.
    pub(crate) fn discuss_in_chat(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.is_chat_room(cx) || self.is_subagent() {
            return;
        }
        let thread = self.thread.read(cx);
        let title = thread.title().unwrap_or_default();
        let entries = thread.entries();
        let last_request = entries
            .iter()
            .rev()
            .find_map(|entry| entry.user_message())
            .map(|message| message.content.to_markdown(cx).to_string());
        let latest_answer = entries
            .iter()
            .rposition(|entry| matches!(entry, AgentThreadEntry::AssistantMessage(_)))
            .and_then(|index| Self::get_agent_message_content(entries, index, cx));
        let project_name = self
            .project
            .upgrade()
            .and_then(|project| {
                project
                    .read(cx)
                    .visible_worktrees(cx)
                    .next()
                    .map(|worktree| worktree.read(cx).root_name_str().to_string())
            })
            .unwrap_or_else(|| "this project".to_string());
        let seed = asherin_chat::side_chat_seed(
            &title,
            &project_name,
            last_request.as_deref(),
            latest_answer.as_deref(),
        );
        let source = self.session_id.clone();
        let source_title: SharedString = format!("from shepherd: {title}").into();
        cx.defer(move |cx| room::open_side_chat(source, source_title, seed, cx));
    }

    /// `/agent new <description>` drafts an agent, `/agent <name>` picks
    /// one for this conversation, `/agent none` goes back to plain shepherd.
    fn run_agent_command(&mut self, argument: &str, window: &mut Window, cx: &mut Context<Self>) {
        let argument = argument.trim();
        let (first, rest) = argument
            .split_once(char::is_whitespace)
            .map(|(first, rest)| (first, rest.trim()))
            .unwrap_or((argument, ""));
        match first {
            "" | "new" => {
                let workspace = self.workspace.clone();
                if rest.is_empty() {
                    window.defer(cx, move |window, cx| {
                        workspace
                            .update(cx, |workspace, cx| {
                                room::open_new_chat_agent(workspace, None, window, cx)
                            })
                            .log_err();
                    });
                } else {
                    let description = rest.to_string();
                    cx.defer(move |cx| room::draft_chat_agent(workspace, description, cx));
                }
            }
            "none" | "off" | "shepherd" => self.pick_chat_agent(None, cx),
            name => match asherin_chat::agents::sanitize_agent_name(name) {
                Some(id) => self.pick_chat_agent(Some(id), cx),
                None => self.handle_thread_error(
                    anyhow::anyhow!("`{name}` isn't an agent name. try /agent new <description>"),
                    cx,
                ),
            },
        }
    }

    fn pick_chat_agent(&mut self, agent: Option<String>, cx: &mut Context<Self>) {
        let Some(native_agent) = self.native_agent(cx) else {
            return;
        };
        let session_id = self.session_id.clone();
        let task = native_agent.update(cx, |native_agent, cx| {
            native_agent.set_chat_agent(session_id, agent, cx)
        });
        self.chat_room._work = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.thread_error = None;
                        room::notify_chat_tree_changed(cx);
                    }
                    Err(error) => this.handle_thread_error(error, cx),
                }
                cx.notify();
            })
            .log_err();
        }));
    }

    fn current_chat_agent(&self, cx: &App) -> Option<String> {
        self.as_native_thread(cx)
            .and_then(|thread| thread.read(cx).chat_agent().map(str::to_string))
    }

    /// The agent picker beside the model picker: which custom agent this
    /// conversation uses.
    /// The tabs of the conversations open in the chat window.
    pub(super) fn render_chat_tabs(&self, cx: &App) -> Option<AnyElement> {
        if self.is_subagent() || !self.is_chat_room(cx) {
            return None;
        }
        let workspace = self.workspace.upgrade()?;
        let panel = workspace
            .read(cx)
            .panel::<crate::asherin_chat_tree::ChatTreePanel>(cx)?;
        let session_id = self.thread.read(cx).session_id().clone();
        crate::asherin_chat_tree::ChatTreePanel::render_tab_strip(&panel, Some(&session_id), cx)
    }

    pub(super) fn render_chat_agent_picker(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.is_subagent() || !self.is_chat_room(cx) {
            return None;
        }
        let current = self.current_chat_agent(cx);
        let label: SharedString = match &current {
            Some(id) => format!("asherin.{id}").into(),
            None => "shepherd".into(),
        };
        let this = cx.weak_entity();
        Some(
            PopoverMenu::new("chat-agent-picker")
                .trigger_with_tooltip(
                    Button::new("chat-agent-picker-button", label)
                        .label_size(LabelSize::Small)
                        .color(Color::Muted),
                    Tooltip::text("which agent answers in this conversation"),
                )
                .with_handle(self.chat_room.agent_menu_handle.clone())
                .anchor(gpui::Anchor::BottomRight)
                .menu(move |window, cx| {
                    let this = this.clone();
                    let current = current.clone();
                    let agents = chat_tree::list_chat_agents();
                    Some(ContextMenu::build(window, cx, move |mut menu, _, _| {
                        menu = menu.toggleable_entry(
                            "shepherd",
                            current.is_none(),
                            IconPosition::End,
                            None,
                            {
                                let this = this.clone();
                                move |_, cx| {
                                    this.update(cx, |this, cx| this.pick_chat_agent(None, cx))
                                        .log_err();
                                }
                            },
                        );
                        for entry in agents {
                            match entry.agent {
                                Ok(agent) => {
                                    let selected = current.as_deref() == Some(agent.id.as_str());
                                    let label = if agent.description.is_empty() {
                                        format!("asherin.{}", agent.id)
                                    } else {
                                        format!("asherin.{} · {}", agent.id, agent.description)
                                    };
                                    let this = this.clone();
                                    let id = agent.id.clone();
                                    menu = menu.toggleable_entry(
                                        label,
                                        selected,
                                        IconPosition::End,
                                        None,
                                        move |_, cx| {
                                            this.update(cx, |this, cx| {
                                                this.pick_chat_agent(Some(id.clone()), cx)
                                            })
                                            .log_err();
                                        },
                                    );
                                }
                                Err(error) => {
                                    menu = menu
                                        .label(format!("asherin.{} can't load: {error}", entry.id));
                                }
                            }
                        }
                        menu.separator()
                            .action("new agent…", Box::new(room::NewChatAgent))
                            .entry("open the agents folder", None, |_, cx| {
                                let folder = chat_tree::chat_agents_directory();
                                if let Err(error) = std::fs::create_dir_all(&folder) {
                                    log::error!("couldn't create {}: {error}", folder.display());
                                    return;
                                }
                                cx.reveal_path(&folder);
                            })
                    }))
                })
                .into_any_element(),
        )
    }

    /// The chat room's additions to a turn's controls: branch from here, the
    /// branches already split here, and at the bottom the room's actions.
    /// Outside the room, the bottom row gets "discuss in chat".
    pub(super) fn render_chat_turn_controls(
        &self,
        entry_ix: usize,
        user_message_index: Option<usize>,
        is_thread_bottom: bool,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        if self.is_subagent() {
            return Vec::new();
        }
        let mut controls = Vec::new();
        if !self.is_chat_room(cx) {
            if is_thread_bottom {
                controls.push(
                    IconButton::new("discuss-in-chat", IconName::Chat)
                        .icon_size(IconSize::Small)
                        .icon_color(Color::Muted)
                        .tooltip(Tooltip::text("discuss in asherin.chat"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.discuss_in_chat(window, cx);
                        }))
                        .into_any_element(),
                );
            }
            return controls;
        }

        if let Some(status) = self.chat_room.status.clone().filter(|_| is_thread_bottom) {
            controls.push(
                Label::new(status)
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .into_any_element(),
            );
        }

        let turn_message_id = user_message_index
            .and_then(|index| self.thread.read(cx).entries().get(index))
            .and_then(|entry| entry.user_message())
            .and_then(|message| message.client_id.clone());
        if let Some(turn_message_id) = turn_message_id {
            let turn_message_id = turn_message_id.to_string();
            if let Some(cost) = self.chat_room.turn_costs.get(&turn_message_id) {
                controls.push(
                    Label::new(noah_trust::cost::format_usd(*cost))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                        .into_any_element(),
                );
            }
            let branches: Vec<(acp::SessionId, SharedString)> = self
                .chat_room
                .branches
                .iter()
                .filter(|branch| {
                    branch.parent_message_id.as_deref() == Some(turn_message_id.as_str())
                })
                .map(|branch| {
                    let title = agent::ThreadStore::try_global(cx)
                        .and_then(|store| {
                            store
                                .read(cx)
                                .thread_from_session_id(&branch.id)
                                .map(|thread| thread.title.clone())
                        })
                        .filter(|title| !title.is_empty())
                        .unwrap_or_else(|| branch.title.clone());
                    (branch.id.clone(), title)
                })
                .collect();
            if !branches.is_empty() {
                let workspace = self.workspace.clone();
                controls.push(
                    PopoverMenu::new(("chat-branches", entry_ix))
                        .trigger_with_tooltip(
                            Button::new(
                                ("chat-branch-count", entry_ix),
                                format!("⑂ {}", branches.len()),
                            )
                            .label_size(LabelSize::Small)
                            .color(Color::Muted),
                            Tooltip::text("branches from this message"),
                        )
                        .anchor(gpui::Anchor::BottomRight)
                        .menu(move |window, cx| {
                            let branches = branches.clone();
                            let workspace = workspace.clone();
                            Some(ContextMenu::build(window, cx, move |mut menu, _, _| {
                                for (branch_id, title) in branches {
                                    let workspace = workspace.clone();
                                    menu =
                                        menu.entry(format!("↳ {title}"), None, move |_, cx| {
                                            let workspace = workspace.clone();
                                            let branch_id = branch_id.clone();
                                            cx.spawn(async move |cx| {
                                                open_chat_conversation(
                                                    workspace, branch_id, None, cx,
                                                )
                                            })
                                            .detach();
                                        });
                                }
                                menu
                            }))
                        })
                        .into_any_element(),
                );
            }
            controls.push(
                IconButton::new(("chat-branch", entry_ix), IconName::GitBranch)
                    .icon_size(IconSize::Small)
                    .icon_color(Color::Muted)
                    .tooltip(Tooltip::text("branch from here"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.branch_after_turn(user_message_index, cx);
                    }))
                    .into_any_element(),
            );
        }

        if is_thread_bottom {
            let total: f64 = self.chat_room.turn_costs.values().sum();
            if total > 0.0 {
                controls.push(
                    Label::new(format!("{} this chat", noah_trust::cost::format_usd(total)))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                        .into_any_element(),
                );
            }
            if self.is_chat_branch() {
                let tooltip: SharedString = match &self.chat_room.parent_title {
                    Some(title) if !title.is_empty() => {
                        format!("post what this branch concluded into \"{title}\"").into()
                    }
                    _ => "post what this branch concluded into its parent".into(),
                };
                controls.push(
                    Button::new("chat-bring-back", "bring back")
                        .label_size(LabelSize::Small)
                        .color(Color::Muted)
                        .tooltip(Tooltip::text(tooltip))
                        .on_click(cx.listener(|this, _, window, cx| this.bring_back(window, cx)))
                        .into_any_element(),
                );
            }
            controls.push(
                Button::new("chat-take-to-code", "take this to code")
                    .label_size(LabelSize::Small)
                    .color(Color::Muted)
                    .tooltip(Tooltip::text(
                        "summarize the decisions, draft spec clauses and open a shepherd thread in a project",
                    ))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.take_to_code(String::new(), window, cx)
                    }))
                    .into_any_element(),
            );
        }
        controls
    }
}

/// Opens a chat conversation in the chat room's panel, optionally sending
/// `first_message` in it right away.
fn open_chat_conversation(
    workspace: WeakEntity<Workspace>,
    session_id: acp::SessionId,
    first_message: Option<Vec<acp::ContentBlock>>,
    cx: &mut AsyncApp,
) {
    cx.update(|cx| {
        let Some(workspace) = workspace.upgrade() else {
            return;
        };
        let Some(window) = room::window_for_workspace(&workspace, cx) else {
            return;
        };
        window
            .update(cx, |_, window, cx| {
                workspace.update(cx, |workspace, cx| {
                    let Some(panel) = workspace.focus_panel::<AgentPanel>(window, cx) else {
                        return;
                    };
                    panel.update(cx, |panel, cx| match first_message {
                        Some(blocks) => panel.open_thread_with_content(
                            session_id,
                            chat_work_dirs(),
                            AgentInitialContent::ContentBlock {
                                blocks,
                                auto_submit: true,
                            },
                            window,
                            cx,
                        ),
                        None => panel.open_thread(session_id, chat_work_dirs(), None, window, cx),
                    });
                });
            })
            .log_err();
    });
}
