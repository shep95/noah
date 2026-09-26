//! asherin.chat on top of the native agent: custom chat agents, branching a
//! conversation, bringing a branch's conclusion back to its parent, and the
//! summaries behind "take this to code". The rules themselves live in the
//! `asherin_chat` crate; this module applies them to threads.

use std::path::PathBuf;
use std::sync::Arc;

use acp_thread::ClientUserMessageId;
use agent_client_protocol::schema::v1 as acp;
use anyhow::{Context as _, Result, anyhow};
use asherin_chat::BranchPoint;
use asherin_chat::agents::ChatAgent;
use asherin_chat::handoff::Handoff;
use asherin_chat::tree::ConversationKind;
use chrono::Utc;
use collections::HashMap;
use futures::StreamExt as _;
use gpui::{App, AppContext as _, AsyncApp, Context, Entity, SharedString, Task};
use language_model::{
    LanguageModel, LanguageModelCompletionEvent, LanguageModelRegistry, LanguageModelRequest,
    LanguageModelRequestMessage, Role, SelectedModel,
};
use project::Project;
use util::path_list::PathList;

use crate::{ChatConversation, DbThread, Message, NativeAgent, ThreadsDatabase, UserMessage};

pub fn chat_agents_directory() -> PathBuf {
    paths::chat_directory().join(asherin_chat::agents::AGENTS_FOLDER)
}

pub fn chat_agent_path(id: &str) -> PathBuf {
    chat_agents_directory().join(asherin_chat::agents::agent_file_name(id))
}

/// Reads and checks a custom chat agent's file.
pub fn load_chat_agent(id: &str) -> Result<ChatAgent> {
    let path = chat_agent_path(id);
    let text = std::fs::read_to_string(&path).map_err(|error| {
        anyhow!(
            "asherin.{id} couldn't be loaded: {error}. pick another agent, or create {}",
            path.display()
        )
    })?;
    asherin_chat::agents::parse_agent_file(id, &text).map_err(|error| {
        anyhow!(
            "asherin.{id} couldn't be loaded: {error}. fix {} or pick another agent",
            path.display()
        )
    })
}

#[derive(Debug, Clone)]
pub struct ChatAgentEntry {
    pub id: String,
    /// The agent, or why its file couldn't be read.
    pub agent: Result<ChatAgent, String>,
}

/// Every `asherin.<name>.md` in the agents folder, sorted by id.
pub fn list_chat_agents() -> Vec<ChatAgentEntry> {
    let Ok(entries) = std::fs::read_dir(chat_agents_directory()) else {
        return Vec::new();
    };
    let mut agents: Vec<ChatAgentEntry> = entries
        .filter_map(|entry| {
            let file_name = entry.ok()?.file_name();
            let id = asherin_chat::agents::agent_id_from_file_name(file_name.to_str()?)?;
            Some(ChatAgentEntry {
                id: id.to_string(),
                agent: load_chat_agent(id).map_err(|error| format!("{error:#}")),
            })
        })
        .collect();
    agents.sort_by(|left, right| left.id.cmp(&right.id));
    agents
}

pub fn list_chat_conversations(cx: &mut App) -> Task<Result<Vec<ChatConversation>>> {
    let database = ThreadsDatabase::connect(cx);
    cx.background_spawn(async move {
        let database = database.await.map_err(|error| anyhow!(error))?;
        database.list_chat_conversations().await
    })
}

pub fn chat_conversation(
    id: acp::SessionId,
    cx: &mut App,
) -> Task<Result<Option<ChatConversation>>> {
    let database = ThreadsDatabase::connect(cx);
    cx.background_spawn(async move {
        let database = database.await.map_err(|error| anyhow!(error))?;
        database.chat_conversation(id).await
    })
}

/// What each turn of a chat conversation cost, by the id of the user
/// message that started it. Only models whose provider publishes prices
/// have costs.
pub fn chat_turn_costs(
    thread_id: acp::SessionId,
    cx: &mut App,
) -> Task<Result<HashMap<String, f64>>> {
    let database = ThreadsDatabase::connect(cx);
    cx.background_spawn(async move {
        let database = database.await.map_err(|error| anyhow!(error))?;
        Ok(database
            .chat_turn_costs(thread_id)
            .await?
            .into_iter()
            .collect())
    })
}

pub fn save_chat_conversation(conversation: ChatConversation, cx: &mut App) -> Task<Result<()>> {
    let database = ThreadsDatabase::connect(cx);
    cx.background_spawn(async move {
        let database = database.await.map_err(|error| anyhow!(error))?;
        database.save_chat_conversation(conversation).await
    })
}

/// Records a conversation's place in the tree, keeping the row's creation
/// time and chat agent when it already has one.
pub fn link_chat_conversation(conversation: ChatConversation, cx: &mut App) -> Task<Result<()>> {
    let database = ThreadsDatabase::connect(cx);
    cx.background_spawn(async move {
        let database = database.await.map_err(|error| anyhow!(error))?;
        let mut conversation = conversation;
        if let Some(existing) = database.chat_conversation(conversation.id.clone()).await? {
            conversation.created_at = existing.created_at;
            conversation.agent = conversation.agent.or(existing.agent);
        }
        database.save_chat_conversation(conversation).await
    })
}

fn user_message_id(message: &Arc<Message>) -> Option<&ClientUserMessageId> {
    match &**message {
        Message::User(UserMessage { id, .. }) => Some(id),
        Message::Agent(_) | Message::Resume | Message::Compaction(_) => None,
    }
}

fn chat_folder_paths() -> PathList {
    PathList::new(&[paths::chat_directory()])
}

/// The summary model, checked against offline mode and the budget.
fn summary_model(cx: &mut App) -> Result<Arc<dyn LanguageModel>> {
    let model = LanguageModelRegistry::read_global(cx)
        .thread_summary_model(cx)
        .map(|configured| configured.model)
        .context("no model is set up to write summaries. connect a provider in settings > ai")?;
    crate::trust::check_model_allowed(model.as_ref(), cx)?;
    Ok(model)
}

/// Runs `messages` followed by `prompt` on `model` and returns the reply.
async fn complete(
    model: Arc<dyn LanguageModel>,
    mut messages: Vec<LanguageModelRequestMessage>,
    prompt: String,
    thread_id: Option<String>,
    cx: &mut AsyncApp,
) -> Result<String> {
    messages.push(LanguageModelRequestMessage {
        role: Role::User,
        content: vec![prompt.into()],
        cache: false,
        reasoning_details: None,
    });
    let request = LanguageModelRequest {
        thread_id: thread_id.clone(),
        messages,
        ..Default::default()
    };
    let mut events = model.stream_completion(request, cx).await?;
    let mut reply = String::new();
    let mut usage = None;
    while let Some(event) = events.next().await {
        match event? {
            LanguageModelCompletionEvent::Text(text) => reply.push_str(&text),
            LanguageModelCompletionEvent::UsageUpdate(update) => usage = Some(update),
            _ => {}
        }
    }
    if let Some(usage) = usage {
        cx.update(|cx| {
            crate::trust::record_spend(
                model.as_ref(),
                usage.input_tokens
                    + usage.cache_creation_input_tokens
                    + usage.cache_read_input_tokens,
                usage.output_tokens,
                thread_id.unwrap_or_default(),
                cx,
            )
        });
    }
    Ok(reply)
}

fn request_messages(messages: &[Arc<Message>]) -> Vec<LanguageModelRequestMessage> {
    messages
        .iter()
        .flat_map(|message| message.to_request())
        .collect()
}

/// Asks the summary model to draft a custom agent's instructions from what
/// the person described.
pub fn draft_chat_agent_instructions(description: String, cx: &mut App) -> Task<Result<String>> {
    let model = match summary_model(cx) {
        Ok(model) => model,
        Err(error) => return Task::ready(Err(error)),
    };
    cx.spawn(async move |cx| {
        let prompt = asherin_chat::agents::draft_instructions_prompt(&description);
        let instructions = complete(model, Vec::new(), prompt, None, cx).await?;
        let instructions = instructions.trim().to_string();
        anyhow::ensure!(
            !instructions.is_empty(),
            "the model didn't draft any instructions; try describing the agent differently"
        );
        Ok(instructions)
    })
}

impl NativeAgent {
    /// A thread's saved form, from its open session when there is one so
    /// unsaved messages count, and otherwise from the database.
    fn chat_thread_snapshot(
        &self,
        id: &acp::SessionId,
        cx: &mut Context<Self>,
    ) -> Task<Result<DbThread>> {
        if let Some(session) = self.sessions.get(id) {
            let snapshot = session.thread.read(cx).to_db(cx);
            return cx.background_spawn(async move { Ok(snapshot.await) });
        }
        let database = ThreadsDatabase::connect(cx);
        let id = id.clone();
        cx.background_spawn(async move {
            let database = database.await.map_err(|error| anyhow!(error))?;
            database
                .load_thread(id.clone())
                .await?
                .with_context(|| format!("conversation {id} isn't saved yet"))
        })
    }

    /// Starts a new conversation that inherits `source`'s messages up to
    /// `point`, and records it as a branch of `source`. The parent is left
    /// as it was and can keep going on its own. Returns the branch's id.
    pub fn branch_chat_thread(
        &mut self,
        source: acp::SessionId,
        point: BranchPoint<ClientUserMessageId>,
        cx: &mut Context<Self>,
    ) -> Task<Result<acp::SessionId>> {
        let snapshot = self.chat_thread_snapshot(&source, cx);
        let database = ThreadsDatabase::connect(cx);
        let thread_store = self.thread_store.clone();
        cx.spawn(async move |_this, cx| {
            let database = database.await.map_err(|error| anyhow!(error))?;
            let conversations = database.list_chat_conversations().await?;
            let parents: HashMap<acp::SessionId, acp::SessionId> = conversations
                .iter()
                .filter_map(|conversation| {
                    Some((
                        conversation.id.clone(),
                        conversation.parent_conversation_id.clone()?,
                    ))
                })
                .collect();
            asherin_chat::new_branch_depth(&source, |id| parents.get(id).cloned())?;
            let parent_row = conversations
                .iter()
                .find(|conversation| conversation.id == source)
                .cloned();

            let mut thread = snapshot.await?;
            let inherited_count =
                asherin_chat::inherited_message_count(&thread.messages, &point, user_message_id)
                    .context("that message is no longer in the conversation")?;
            let parent_title = thread.title.clone();
            thread.messages.truncate(inherited_count);
            let last_inherited_user_message = thread
                .messages
                .iter()
                .rev()
                .find_map(|message| user_message_id(message).cloned());
            let kept_user_messages: Vec<ClientUserMessageId> = thread
                .messages
                .iter()
                .filter_map(|message| user_message_id(message).cloned())
                .collect();
            thread
                .request_token_usage
                .retain(|id, _| kept_user_messages.contains(id));
            // The branch gets its own title from the summary model after its
            // first reply; until then the tree shows where it came from.
            thread.title = SharedString::default();
            thread.detailed_summary = None;
            thread.cumulative_token_usage = Default::default();
            thread.draft_prompt = None;
            thread.ui_scroll_position = None;
            thread.subagent_context = None;
            thread.sandboxed_terminal_temp_dir = None;
            thread.sandbox_grants = Default::default();
            thread.updated_at = Utc::now();

            let branch_id = acp::SessionId::new(uuid::Uuid::new_v4().to_string());
            database
                .save_thread(branch_id.clone(), thread, chat_folder_paths())
                .await?;

            if parent_row.is_none() {
                database
                    .save_chat_conversation(ChatConversation::new(
                        source.clone(),
                        parent_title.clone(),
                        ConversationKind::Root,
                    ))
                    .await?;
            }
            let mut branch = ChatConversation::new(
                branch_id.clone(),
                format!("from {parent_title}").into(),
                ConversationKind::Branch,
            );
            branch.parent_conversation_id = Some(source);
            branch.parent_message_id = last_inherited_user_message.map(|id| id.to_string());
            branch.inherited_message_count = inherited_count;
            branch.agent = parent_row.and_then(|row| row.agent);
            database.save_chat_conversation(branch).await?;

            thread_store.update(cx, |store, cx| store.reload(cx));
            Ok(branch_id)
        })
    }

    /// Summarizes what a branch concluded and posts it into its parent as a
    /// single message, `⑂ from "<title>": <conclusion>`. Returns the parent's
    /// id and the message.
    pub fn bring_back_chat_branch(
        &mut self,
        branch_id: acp::SessionId,
        project: Entity<Project>,
        cx: &mut Context<Self>,
    ) -> Task<Result<(acp::SessionId, String)>> {
        let model = match summary_model(cx) {
            Ok(model) => model,
            Err(error) => return Task::ready(Err(error)),
        };
        let snapshot = self.chat_thread_snapshot(&branch_id, cx);
        let database = ThreadsDatabase::connect(cx);
        cx.spawn(async move |this, cx| {
            let database = database.await.map_err(|error| anyhow!(error))?;
            let row = database
                .chat_conversation(branch_id.clone())
                .await?
                .filter(|row| row.kind == ConversationKind::Branch)
                .context("only a branch can be brought back")?;
            let parent_id = row
                .parent_conversation_id
                .clone()
                .context("this branch's parent conversation is gone")?;

            let thread = snapshot.await?;
            let own_messages = thread
                .messages
                .get(row.inherited_message_count..)
                .filter(|messages| !messages.is_empty())
                .context("this branch has no messages of its own yet")?;
            let title = if thread.title.is_empty() {
                row.title.to_string()
            } else {
                thread.title.to_string()
            };
            let reply = complete(
                model,
                request_messages(own_messages),
                asherin_chat::BRING_BACK_PROMPT.to_string(),
                Some(branch_id.to_string()),
                cx,
            )
            .await?;
            let message = asherin_chat::bring_back_message(&title, &reply)
                .context("the summary model didn't say what the branch concluded")?;

            let open_parent = this.update(cx, |this, cx| {
                this.open_thread(parent_id.clone(), project, cx)
            })?;
            let parent_thread = open_parent.await?;
            this.update(cx, |this, cx| {
                anyhow::ensure!(
                    parent_thread.read(cx).status() == acp_thread::ThreadStatus::Idle,
                    "the parent conversation is still answering. bring this back when it's done"
                );
                let session = this
                    .sessions
                    .get(&parent_id)
                    .context("the parent conversation closed")?;
                let path_style = project_path_style(&parent_thread, cx);
                let block = acp::ContentBlock::Text(acp::TextContent::new(message.clone()));
                let message_id = ClientUserMessageId::new();
                session.thread.update(cx, |thread, cx| {
                    thread.push_acp_user_block(message_id.clone(), [block.clone()], path_style, cx);
                });
                parent_thread.update(cx, |thread, cx| {
                    thread.push_user_content_block(Some(message_id), block, cx);
                });
                anyhow::Ok(())
            })??;
            drop(parent_thread);
            Ok((parent_id, message))
        })
    }

    /// Summarizes a chat's decisions and drafts acceptance criteria for
    /// handing it to the shepherd room. Returns the chat's title too.
    pub fn summarize_chat_for_handoff(
        &mut self,
        id: acp::SessionId,
        cx: &mut Context<Self>,
    ) -> Task<Result<(String, Handoff)>> {
        let model = match summary_model(cx) {
            Ok(model) => model,
            Err(error) => return Task::ready(Err(error)),
        };
        let snapshot = self.chat_thread_snapshot(&id, cx);
        cx.spawn(async move |_this, cx| {
            let thread = snapshot.await?;
            anyhow::ensure!(
                !thread.messages.is_empty(),
                "there's nothing in this chat to take to code yet"
            );
            let reply = complete(
                model,
                request_messages(&thread.messages),
                asherin_chat::handoff::HANDOFF_PROMPT.to_string(),
                Some(id.to_string()),
                cx,
            )
            .await?;
            let handoff = asherin_chat::handoff::parse_handoff(&reply)
                .context("the summary model returned an empty summary")?;
            let title = if thread.title.is_empty() {
                "chat".to_string()
            } else {
                thread.title.to_string()
            };
            Ok((title, handoff))
        })
    }

    /// Picks the custom chat agent a conversation uses, or none, and
    /// switches to the agent's model when its file names one.
    pub fn set_chat_agent(
        &mut self,
        id: acp::SessionId,
        agent: Option<String>,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let definition = match agent.as_deref().map(load_chat_agent).transpose() {
            Ok(definition) => definition,
            Err(error) => return Task::ready(Err(error)),
        };
        let mut title = SharedString::default();
        if let Some(session) = self.sessions.get(&id) {
            let model = definition
                .as_ref()
                .and_then(|definition| definition.model.as_deref())
                .and_then(|model| model.parse::<SelectedModel>().ok())
                .and_then(|selected| {
                    LanguageModelRegistry::global(cx)
                        .update(cx, |registry, cx| registry.select_model(&selected, cx))
                });
            session.thread.update(cx, |thread, cx| {
                title = thread.title().unwrap_or_default();
                thread.set_chat_agent(agent.clone(), cx);
                if let Some(model) = model {
                    thread.set_model(model.model, cx);
                }
            });
        }
        let database = ThreadsDatabase::connect(cx);
        cx.background_spawn(async move {
            let database = database.await.map_err(|error| anyhow!(error))?;
            let mut row = database
                .chat_conversation(id.clone())
                .await?
                .unwrap_or_else(|| ChatConversation::new(id, title, ConversationKind::Root));
            row.agent = agent;
            row.updated_at = Utc::now();
            database.save_chat_conversation(row).await
        })
    }
}

fn project_path_style(thread: &Entity<acp_thread::AcpThread>, cx: &App) -> util::paths::PathStyle {
    thread.read(cx).project().read(cx).path_style(cx)
}
