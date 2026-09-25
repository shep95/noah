use std::path::PathBuf;
use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use gpui::{App, AppContext as _, AsyncApp, Entity, Task};
use settings::Settings as _;
use language_model::{
    CompletionIntent, LanguageModel, LanguageModelRegistry, LanguageModelRequest,
    LanguageModelRequestMessage, Role,
};
use noah_trust::project_files;
use project::Project;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ui::SharedString;

use crate::{AgentTool, ToolCallEventStream, ToolInput};

/// Diffs longer than this are cut so the review fits any model's context.
const MAX_DIFF_CHARS: usize = 60_000;

/// Has a different model review your uncommitted changes, so a mistake you
/// made isn't also missed by the same blind spots when you check it.
///
/// The reviewer sees the diff, the project's spec and your latest evidence
/// bundle, and returns: the behavior that changed (clustered by concern and
/// ranked by risk, with the callers affected), problems it found with
/// file:line, and which of your claims the evidence does and doesn't support.
/// Address every problem it reports, or explain to the person why not. Use it
/// before `evidence` `finish` on anything non-trivial.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct VerifyToolInput {
    /// What the change is meant to do, in a sentence or two.
    pub intent: String,
    /// Anything the reviewer should look at especially hard.
    #[serde(default)]
    pub focus: Option<String>,
}

pub struct VerifyTool {
    project: Entity<Project>,
}

impl VerifyTool {
    pub fn new(project: Entity<Project>) -> Self {
        Self { project }
    }
}

const REVIEW_INSTRUCTIONS: &str = "You are an independent code reviewer. Another AI model wrote the change below; your job is to catch what it missed, not to agree with it. Be specific and brief.

Reply in markdown with these sections:

## behavior changes
What a user or caller will notice, not which lines moved. Cluster by concern and order by risk, highest first. For each, name who is affected (callers, endpoints, stored data) when you can tell.

## problems
Each problem as `- [high|medium|low] path:line: what is wrong, and what would go wrong because of it`. Include missing error handling, broken edge cases, security issues, concurrency hazards, changes that contradict the spec or its invariants, and tests that don't test what they claim. If you find nothing, say so; don't invent problems.

## claims
For each claim in the evidence, whether the evidence shown supports it: `supported`, `not supported` or `contradicted`, with one line of reason. If there is no evidence, say that nothing was verified.

## tests to add
Up to five tests that would catch the most likely remaining bugs: property-based, boundary, failure injection (timeouts, partial writes, error responses with 200 status) or concurrency tests where they fit.";

/// The part of a model id that names its family, so `qwen3-coder-480b` and
/// `qwen-2.5-72b` count as the same family and a reviewer from another one is
/// preferred.
fn family(model_id: &str) -> String {
    let name = model_id.rsplit('/').next().unwrap_or(model_id).to_lowercase();
    let letters: String = name.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    if letters.is_empty() { name } else { letters }
}

/// Picks the reviewer: the configured verifier model when set, otherwise a
/// model from a different family than the author, preferring the same
/// provider (already authenticated) and larger, stronger models.
fn choose_reviewer(author: Option<&Arc<dyn LanguageModel>>, cx: &App) -> Option<Arc<dyn LanguageModel>> {
    let registry = LanguageModelRegistry::read_global(cx);
    let configured = agent_settings::AgentSettings::get_global(cx)
        .verifier_model
        .clone();
    let offline = agent_settings::AgentSettings::get_global(cx).offline;
    let available: Vec<Arc<dyn LanguageModel>> = registry
        .available_models(cx)
        .filter(|model| {
            !offline
                || crate::trust::LOCAL_PROVIDERS
                    .contains(&model.provider_id().0.to_lowercase().as_str())
        })
        .collect();
    if let Some(configured) = configured
        && let Some(model) = available.iter().find(|model| {
            model.provider_id().0.as_ref() == configured.provider.0.as_str()
                && model.id().0.as_ref() == configured.model.as_str()
        })
    {
        return Some(model.clone());
    }
    let author_family = author.map(|author| family(&author.id().0));
    let author_provider = author.map(|author| author.provider_id());
    let mut candidates: Vec<&Arc<dyn LanguageModel>> = available
        .iter()
        .filter(|model| Some(family(&model.id().0)) != author_family)
        .collect();
    candidates.sort_by_key(|model| {
        (
            Some(model.provider_id()) != author_provider,
            std::cmp::Reverse(model.max_token_count()),
        )
    });
    candidates
        .first()
        .map(|model| (*model).clone())
        .or_else(|| author.filter(|author| available.iter().any(|model| model.id() == author.id())).cloned())
}

impl AgentTool for VerifyTool {
    type Input = VerifyToolInput;
    type Output = String;

    const NAME: &'static str = "verify";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Think
    }

    fn allow_in_restricted_mode() -> bool {
        false
    }

    fn initial_title(
        &self,
        _input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        "verify: independent review".into()
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
            let (reviewer, same_family) = cx.update(|cx| {
                let author = event_stream.thread_model(cx);
                let reviewer = choose_reviewer(author.as_ref(), cx);
                let same_family = match (&author, &reviewer) {
                    (Some(author), Some(reviewer)) => {
                        family(&author.id().0) == family(&reviewer.id().0)
                    }
                    _ => false,
                };
                (reviewer, same_family)
            });
            let reviewer = reviewer.ok_or_else(|| "no model is available to review with".to_string())?;
            event_stream.update_fields(acp::ToolCallUpdateFields::new().title(format!(
                "verify: reviewing with {}",
                reviewer.name().0
            )));

            let material = gather_material(root, cx).await?;
            let mut prompt = format!("# what the change is meant to do\n\n{}\n\n", input.intent);
            if let Some(focus) = &input.focus {
                prompt.push_str(&format!("# look especially at\n\n{focus}\n\n"));
            }
            prompt.push_str(&material);

            let request = LanguageModelRequest {
                intent: Some(CompletionIntent::Subagent),
                messages: vec![
                    LanguageModelRequestMessage {
                        role: Role::System,
                        content: vec![REVIEW_INSTRUCTIONS.into()],
                        cache: false,
                        reasoning_details: None,
                    },
                    LanguageModelRequestMessage {
                        role: Role::User,
                        content: vec![prompt.into()],
                        cache: false,
                        reasoning_details: None,
                    },
                ],
                thinking_allowed: false,
                ..Default::default()
            };
            let review = review_with(&reviewer, request, cx)
                .await
                .map_err(|error| format!("the reviewer model failed: {error:#}"))?;
            let mut header = format!("reviewed by {} ({})", reviewer.name().0, reviewer.id().0);
            if same_family {
                header.push_str(
                    ". note: no model from another family is available, so the reviewer shares the author's blind spots; set agent.verifier_model to a different family",
                );
            }
            Ok(format!("{header}\n\n{}", review.trim()))
        })
    }
}

async fn review_with(
    model: &Arc<dyn LanguageModel>,
    request: LanguageModelRequest,
    cx: &mut AsyncApp,
) -> anyhow::Result<String> {
    let response = model.stream_completion_text(request, cx).await?;
    let mut text = String::new();
    let mut chunks = response.stream;
    while let Some(chunk) = futures::StreamExt::next(&mut chunks).await {
        text.push_str(&chunk?);
    }
    Ok(text)
}

async fn gather_material(root: PathBuf, cx: &mut AsyncApp) -> Result<String, String> {
    let diff_command = "git diff HEAD --stat && git diff HEAD && git status --short";
    let (code, diff, _) = super::evidence_tool::run_shell(diff_command, &root, cx).await;
    if code != Some(0) {
        return Err(format!(
            "couldn't read the changes with git (is this a git repository?): {}",
            diff.trim()
        ));
    }
    if diff.trim().is_empty() {
        return Err("there are no uncommitted changes to review".to_string());
    }
    cx.background_spawn(async move {
        let mut diff = noah_trust::secrets::redact(&diff, &[]).text;
        if diff.len() > MAX_DIFF_CHARS {
            let mut end = MAX_DIFF_CHARS;
            while !diff.is_char_boundary(end) {
                end -= 1;
            }
            diff.truncate(end);
            diff.push_str("\n… (diff cut; review what is shown)");
        }
        let mut material = format!("# the change\n\n```diff\n{diff}\n```\n\n");
        if let Ok(spec) = std::fs::read_to_string(project_files::path(&root, project_files::SPEC)) {
            material.push_str(&format!("# the project's spec\n\n{}\n\n", project_files::clip(&spec)));
        }
        let evidence_directory = project_files::path(&root, project_files::EVIDENCE);
        let latest = std::fs::read_dir(&evidence_directory)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
            .max();
        match latest.and_then(|path| std::fs::read_to_string(path).ok()) {
            Some(evidence) => material.push_str(&format!(
                "# the author's latest evidence\n\n{}\n",
                project_files::clip(&evidence)
            )),
            None => material.push_str("# the author's evidence\n\nnone was recorded.\n"),
        }
        Ok(material)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::family;

    #[test]
    fn families() {
        assert_eq!(family("qwen3-coder-480b-a35b-instruct"), "qwen");
        assert_eq!(family("venice/qwen-2.5-72b"), "qwen");
        assert_eq!(family("llama-3.3-70b"), "llama");
        assert_ne!(family("deepseek-r1"), family("mistral-large"));
    }
}
