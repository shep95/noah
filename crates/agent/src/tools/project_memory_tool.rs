use std::path::Path;
use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use futures::FutureExt as _;
use gpui::{App, AppContext as _, Entity, Task};
use noah_trust::{
    project_files,
    spec::{ClauseKind, Spec},
};
use project::Project;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ui::SharedString;

use crate::{AgentTool, ToolCallEventStream, ToolInput};

/// The project's long-term knowledge, kept as plain files in `.noah/` that
/// the person can read and edit. It is shown to you at the start of every
/// conversation.
///
/// - `memory`: facts and conventions worth remembering between conversations
///   (how to run the tests, where things live, gotchas).
/// - `intent`: what the project is for and what matters in it. Check your
///   work against it and say so when a request drifts away from it.
/// - `spec`: acceptance criteria, invariants and non-goals, each with an id
///   (AC-1, INV-1, NG-1). Add clauses with `add` and `clause_kind`.
/// - `why`: why the code is the way it is: decisions, incidents and their
///   lessons. Before removing code that looks odd, check here and in git
///   history. After an incident, record what happened plus the regression
///   test, lint rule and instruction that would have prevented it.
/// - `preferences`: the person's style and review preferences, learned from
///   what they keep, reject and correct. Record them openly here; never adapt
///   silently.
///
/// Actions: `read` shows a file; `add` appends an entry; `remove` deletes
/// entries containing `text`; `replace` rewrites the whole file (for intent).
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProjectMemoryToolInput {
    pub action: MemoryAction,
    pub file: MemoryFile,
    /// The entry to add, the text to remove, or the new contents.
    #[serde(default)]
    pub text: Option<String>,
    /// For `add` to the spec: which kind of clause.
    #[serde(default)]
    pub clause_kind: Option<SpecClauseKind>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryAction {
    Read,
    Add,
    Remove,
    Replace,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryFile {
    Memory,
    Intent,
    Spec,
    Why,
    Preferences,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpecClauseKind {
    Acceptance,
    Invariant,
    NonGoal,
}

impl MemoryFile {
    fn file_name(self) -> &'static str {
        match self {
            Self::Memory => project_files::MEMORY,
            Self::Intent => project_files::INTENT,
            Self::Spec => project_files::SPEC,
            Self::Why => project_files::WHY,
            Self::Preferences => project_files::PREFERENCES,
        }
    }

    fn heading(self) -> &'static str {
        match self {
            Self::Memory => "# memory\n\nWhat shepherd keeps in mind for this project. Edit freely.\n",
            Self::Intent => "# intent\n",
            Self::Spec => "# spec\n",
            Self::Why => "# why\n\nDecisions, incidents and their lessons.\n",
            Self::Preferences => {
                "# preferences\n\nWhat shepherd has learned about how you like things done. Edit or delete anything here.\n"
            }
        }
    }
}

pub struct ProjectMemoryTool {
    project: Entity<Project>,
}

impl ProjectMemoryTool {
    pub fn new(project: Entity<Project>) -> Self {
        Self { project }
    }
}

impl AgentTool for ProjectMemoryTool {
    type Input = ProjectMemoryToolInput;
    type Output = String;

    const NAME: &'static str = "project_memory";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn allow_in_restricted_mode() -> bool {
        false
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(input) => {
                let verb = match input.action {
                    MemoryAction::Read => "read",
                    MemoryAction::Add => "add to",
                    MemoryAction::Remove => "remove from",
                    MemoryAction::Replace => "rewrite",
                };
                format!("{verb} .noah/{}", input.file.file_name()).into()
            }
            Err(_) => "project memory".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        let project = self.project.clone();
        cx.spawn(async move |cx| {
            let input = input.recv().await.map_err(|error| error.to_string())?;
            let root = cx
                .update(|cx| super::evidence_tool::first_root(&project, cx))
                .ok_or_else(|| "open a project folder first".to_string())?;
            let path = project_files::path(&root, input.file.file_name());
            if input.action == MemoryAction::Read {
                return Ok(std::fs::read_to_string(&path).unwrap_or_else(|_| {
                    format!(".noah/{} is empty", input.file.file_name())
                }));
            }
            let text = input
                .text
                .clone()
                .filter(|text| !text.trim().is_empty())
                .ok_or_else(|| "give the `text`".to_string())?;
            let description = match input.action {
                MemoryAction::Add => format!("remember in .noah/{}: {text}", input.file.file_name()),
                MemoryAction::Remove => format!(
                    "forget from .noah/{} entries containing: {text}",
                    input.file.file_name()
                ),
                _ => format!("rewrite .noah/{}", input.file.file_name()),
            };
            let authorize = cx.update(|cx| {
                let context = crate::ToolPermissionContext::new(Self::NAME, vec![description.clone()]);
                event_stream.authorize(description.clone(), context, cx)
            });
            futures::select! {
                result = authorize.fuse() => result.map_err(|error| error.to_string())?,
                _ = event_stream.cancelled_by_user().fuse() => return Err("cancelled".to_string()),
            }
            cx.background_spawn(async move { apply(&input, &path, text) })
                .await
                .map_err(|error| format!("{error:#}"))
        })
    }
}

fn apply(input: &ProjectMemoryToolInput, path: &Path, text: String) -> anyhow::Result<String> {
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let updated = match (input.action, input.file) {
        (MemoryAction::Replace, _) => text.trim_end().to_string() + "\n",
        (MemoryAction::Add, MemoryFile::Spec) => {
            let mut spec = Spec::parse(&existing);
            let kind = match input.clause_kind.unwrap_or(SpecClauseKind::Acceptance) {
                SpecClauseKind::Acceptance => ClauseKind::Acceptance,
                SpecClauseKind::Invariant => ClauseKind::Invariant,
                SpecClauseKind::NonGoal => ClauseKind::NonGoal,
            };
            let id = spec.add(kind, &text);
            std::fs::write(path, spec.to_markdown())?;
            return Ok(format!("added {id} to .noah/spec.md"));
        }
        (MemoryAction::Add, file) => {
            let mut updated = if existing.trim().is_empty() {
                file.heading().to_string()
            } else {
                existing.trim_end().to_string() + "\n"
            };
            let date = chrono::Local::now().format("%Y-%m-%d");
            let entry = text.trim().replace('\n', "\n  ");
            if file == MemoryFile::Why {
                updated.push_str(&format!("\n- {date}: {entry}\n"));
            } else {
                updated.push_str(&format!("\n- {entry}\n"));
            }
            updated
        }
        (MemoryAction::Remove, _) => {
            let needle = text.trim().to_lowercase();
            let kept: Vec<&str> = existing
                .lines()
                .filter(|line| !(line.trim_start().starts_with("- ") && line.to_lowercase().contains(&needle)))
                .collect();
            let removed = existing.lines().count() - kept.len();
            if removed == 0 {
                return Ok(format!("nothing in .noah/{} contains that", input.file.file_name()));
            }
            std::fs::write(path, kept.join("\n") + "\n")?;
            return Ok(format!("removed {removed} entr{}", if removed == 1 { "y" } else { "ies" }));
        }
        (MemoryAction::Read, _) => existing.clone(),
    };
    std::fs::write(path, &updated)?;
    Ok(format!("saved .noah/{}", input.file.file_name()))
}
