use std::path::Path;
use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use gpui::{App, Entity, SharedString, Task};
use project::Project;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{AgentTool, ToolCallEventStream, ToolInput};

/// Explains a change in plain words, one hunk at a time, and keeps the
/// explanation with the commit.
///
/// - `read`: returns the hunks of the staged change (or, when nothing is
///   staged, of the last commit), numbered. Read every hunk.
/// - `attach`: give one plain-words explanation per hunk, in the order
///   `read` returned them, written in the person's own language and for
///   someone who doesn't read code: what the hunk does for a user or
///   caller, not which lines moved. They are attached to the last commit
///   as a git note (`git notes --ref=noah-explain`), so `git log
///   --notes=noah-explain` shows them beside the commit. Use it after a
///   commit, before the person pushes.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExplainDiffToolInput {
    pub action: ExplainDiffAction,
    /// For `attach`: one explanation per hunk, in `read` order.
    #[serde(default)]
    pub explanations: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExplainDiffAction {
    Read,
    Attach,
}

pub struct ExplainDiffTool {
    project: Entity<Project>,
}

impl ExplainDiffTool {
    pub fn new(project: Entity<Project>) -> Self {
        Self { project }
    }
}

const MAX_DIFF_CHARS: usize = 60_000;
const NOTES_REF: &str = "noah-explain";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hunk {
    pub file: String,
    pub header: String,
    pub body: String,
}

/// Splits unified diff output into hunks, each with the file it belongs to.
pub(crate) fn hunks(diff: &str) -> Vec<Hunk> {
    let mut hunks = Vec::new();
    let mut file = String::new();
    let mut current: Option<Hunk> = None;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(hunk) = current.take() {
                hunks.push(hunk);
            }
            file = rest
                .split(" b/")
                .nth(1)
                .unwrap_or(rest)
                .to_string();
            continue;
        }
        if line.starts_with("@@") {
            if let Some(hunk) = current.take() {
                hunks.push(hunk);
            }
            current = Some(Hunk {
                file: file.clone(),
                header: line.to_string(),
                body: String::new(),
            });
            continue;
        }
        if let Some(hunk) = current.as_mut() {
            hunk.body.push_str(line);
            hunk.body.push('\n');
        }
    }
    if let Some(hunk) = current.take() {
        hunks.push(hunk);
    }
    hunks
}

pub(crate) fn note_text(hunks: &[Hunk], explanations: &[String]) -> String {
    let mut note = String::from("what this commit does, in plain words (written by shepherd):\n");
    for (index, (hunk, explanation)) in hunks.iter().zip(explanations).enumerate() {
        note.push_str(&format!(
            "\n{}. {} {}\n   {}\n",
            index + 1,
            hunk.file,
            hunk.header.split("@@").nth(1).unwrap_or("").trim(),
            explanation.trim().replace('\n', "\n   ")
        ));
    }
    note
}

async fn diff_for(root: &Path, cx: &mut gpui::AsyncApp) -> Result<(String, &'static str), String> {
    let (code, staged, _) =
        super::evidence_tool::run_shell("git --no-pager diff --cached --no-color", root, cx).await;
    if code != Some(0) {
        return Err(format!("git couldn't read the change: {}", staged.trim()));
    }
    if !staged.trim().is_empty() {
        return Ok((staged, "the staged change"));
    }
    let (code, last, _) =
        super::evidence_tool::run_shell("git --no-pager show --no-color --format= HEAD", root, cx)
            .await;
    if code != Some(0) {
        return Err(format!("git couldn't read the last commit: {}", last.trim()));
    }
    Ok((last, "the last commit"))
}

impl AgentTool for ExplainDiffTool {
    type Input = ExplainDiffToolInput;
    type Output = String;

    const NAME: &'static str = "explain_diff";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Read
    }

    fn allow_in_restricted_mode() -> bool {
        false
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input.map(|input| input.action) {
            Ok(ExplainDiffAction::Attach) => "explain diff: attach the explanation".into(),
            _ => "explain diff: read the change".into(),
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
            let (diff, what) = diff_for(&root, cx).await?;
            let hunks = hunks(&diff);
            if hunks.is_empty() {
                return Err(format!("{what} changes no lines"));
            }
            match input.action {
                ExplainDiffAction::Read => {
                    let mut out = format!(
                        "{} hunks in {what}; explain each, in order, with `attach`:\n",
                        hunks.len()
                    );
                    for (index, hunk) in hunks.iter().enumerate() {
                        out.push_str(&format!(
                            "\n### {}. {} {}\n```diff\n{}```\n",
                            index + 1,
                            hunk.file,
                            hunk.header,
                            hunk.body
                        ));
                        if out.len() > MAX_DIFF_CHARS {
                            out.push_str(&format!(
                                "\n[cut after hunk {} of {}: the change is too long to read whole; explain what is shown and say the rest was not read]",
                                index + 1,
                                hunks.len()
                            ));
                            break;
                        }
                    }
                    Ok(out)
                }
                ExplainDiffAction::Attach => {
                    if input.explanations.len() != hunks.len() {
                        return Err(format!(
                            "{what} has {} hunks and {} explanations were given; give one per hunk, in `read` order",
                            hunks.len(),
                            input.explanations.len()
                        ));
                    }
                    if what == "the staged change" {
                        return Err(
                            "the change is staged but not committed; commit it first, then attach"
                                .to_string(),
                        );
                    }
                    let note = note_text(&hunks, &input.explanations);
                    let authorize = cx.update(|cx| {
                        let context = crate::ToolPermissionContext::new(
                            "terminal",
                            vec![format!("git notes --ref={NOTES_REF} add -f HEAD")],
                        );
                        event_stream.authorize("attach the explanation to the last commit", context, cx)
                    });
                    authorize.await.map_err(|error| error.to_string())?;
                    let note_file = root.join(".git").join("NOAH_EXPLAIN_NOTE");
                    std::fs::write(&note_file, &note)
                        .map_err(|error| format!("couldn't write the note: {error}"))?;
                    let command = format!(
                        "git notes --ref={NOTES_REF} add -f -F .git/NOAH_EXPLAIN_NOTE HEAD"
                    );
                    let (code, output, _) =
                        super::evidence_tool::run_shell(&command, &root, cx).await;
                    if let Err(error) = std::fs::remove_file(&note_file) {
                        log::warn!("couldn't remove {}: {error}", note_file.display());
                    }
                    if code != Some(0) {
                        return Err(format!("git couldn't attach the note: {}", output.trim()));
                    }
                    Ok(format!(
                        "attached to HEAD as a git note; `git log --notes={NOTES_REF}` shows it.\n\n{note}"
                    ))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "diff --git a/src/a.rs b/src/a.rs\nindex 1..2 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,2 +1,2 @@ fn main\n-old\n+new\n@@ -10 +10 @@\n-x\n+y\ndiff --git a/b.md b/b.md\n@@ -1 +1 @@\n-a\n+b\n";

    #[test]
    fn hunks_carry_their_file() {
        let hunks = hunks(DIFF);
        assert_eq!(hunks.len(), 3);
        assert_eq!(hunks[0].file, "src/a.rs");
        assert_eq!(hunks[1].file, "src/a.rs");
        assert_eq!(hunks[2].file, "b.md");
        assert_eq!(hunks[0].body, "-old\n+new\n");
    }

    #[test]
    fn the_note_numbers_each_hunk() {
        let hunks = hunks(DIFF);
        let note = note_text(
            &hunks,
            &["renames it".into(), "fixes y".into(), "docs".into()],
        );
        assert!(note.contains("1. src/a.rs -1,2 +1,2\n   renames it"));
        assert!(note.contains("3. b.md"));
    }
}
