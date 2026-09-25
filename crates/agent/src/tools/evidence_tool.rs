use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_client_protocol::schema::v1 as acp;
use futures::FutureExt as _;
use gpui::{App, AppContext as _, AsyncApp, Entity, Task};
use noah_trust::{
    evidence::{self, Bundle, Check, Claim, FileConfidence, RepeatResult, RunFingerprint},
    mutation, project_files, provenance, spec::Spec,
};
use project::Project;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ui::SharedString;
use util::ResultExt as _;
use util::markdown::MarkdownInlineCode;

use crate::{AgentTool, ToolCallEventStream, ToolInput};

/// Proof for your work. The person checks evidence, not your word.
///
/// - `finish`: call this when a change is done, before telling the person it
///   works. Every command you ran in the terminal this conversation is
///   already attached with its real exit code and output (each terminal
///   result ends with its id, such as `[evidence run-3]`). List each claim
///   you are making ("the login tests pass", "no public API changed") with
///   the run ids or sources (`doc:<url>`, `file:<path>`) that back it. Claims
///   without backing, or citing a run that failed, are flagged to the person.
///   List honestly what you did not verify, your confidence per changed file
///   (0.0 to 1.0), the spec clauses (AC-1...) the change serves, and the
///   behavior that changed (what a user or caller will notice, not which
///   lines moved). A claim citing a run the code has changed since is
///   flagged as stale; re-run that command and cite the new run. The bundle
///   is saved in `.noah/evidence/`.
/// - `repeat`: run a test command several times to find flaky tests. The
///   result is attached as evidence.
/// - `mutate`: put small deliberate bugs into lines you changed (flipped
///   comparisons and booleans, off-by-one boundaries, replaced return
///   values, deleted calls, swapped arguments, skipped error branches), one
///   at a time, and run the tests after each. A bug the tests don't catch
///   shows behavior no test checks; add a test for it. The file is always
///   restored.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceToolInput {
    pub action: EvidenceAction,
    /// For `finish`: a short title for the change.
    #[serde(default)]
    pub title: Option<String>,
    /// For `finish`: what changed and why, in a few sentences.
    #[serde(default)]
    pub summary: Option<String>,
    /// For `finish`: each claim and what backs it.
    #[serde(default)]
    pub claims: Vec<ClaimInput>,
    /// For `finish`: everything you did not check. Be specific.
    #[serde(default)]
    pub not_verified: Vec<String>,
    /// For `finish`: spec clause ids this change serves.
    #[serde(default)]
    pub spec_clauses: Vec<String>,
    /// For `finish`: confidence per changed file.
    #[serde(default)]
    pub files: Vec<FileConfidenceInput>,
    /// For `finish`: behavior a user or caller will notice, one per item.
    #[serde(default)]
    pub behavior_changes: Vec<String>,
    /// For `finish`: screenshot paths (for example from the browser tool).
    #[serde(default)]
    pub screenshots: Vec<String>,
    /// For `repeat` and `mutate`: the test command to run.
    #[serde(default)]
    pub command: Option<String>,
    /// For `repeat`: how many times (default 5, at most 50).
    #[serde(default)]
    pub times: Option<u32>,
    /// For `mutate`: the file whose lines you changed, relative to the project
    /// root.
    #[serde(default)]
    pub path: Option<String>,
    /// For `mutate`: first and last changed line (1-based).
    #[serde(default)]
    pub start_line: Option<usize>,
    #[serde(default)]
    pub end_line: Option<usize>,
    /// For `mutate`: most mutants to try (default 8, at most 30).
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceAction {
    Finish,
    Repeat,
    Mutate,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ClaimInput {
    pub text: String,
    /// Run ids (`run-2`), `doc:<url>` or `file:<path>`.
    #[serde(default)]
    pub grounds: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct FileConfidenceInput {
    pub path: String,
    pub confidence: f32,
    #[serde(default)]
    pub note: String,
}

pub struct EvidenceTool {
    project: Entity<Project>,
}

impl EvidenceTool {
    pub fn new(project: Entity<Project>) -> Self {
        Self { project }
    }
}

const COMMAND_TIMEOUT: Duration = Duration::from_secs(600);

impl AgentTool for EvidenceTool {
    type Input = EvidenceToolInput;
    type Output = String;

    const NAME: &'static str = "evidence";

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
            Ok(input) => match input.action {
                EvidenceAction::Finish => format!(
                    "evidence: {}",
                    input.title.as_deref().unwrap_or("finish")
                )
                .into(),
                EvidenceAction::Repeat => format!(
                    "evidence: run {} ×{}",
                    MarkdownInlineCode(input.command.as_deref().unwrap_or("")),
                    input.times.unwrap_or(5)
                )
                .into(),
                EvidenceAction::Mutate => format!(
                    "evidence: mutation test {}",
                    MarkdownInlineCode(input.path.as_deref().unwrap_or(""))
                )
                .into(),
            },
            Err(_) => "evidence".into(),
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
                .update(|cx| first_root(&project, cx))
                .ok_or_else(|| "open a project folder first".to_string())?;
            match input.action {
                EvidenceAction::Finish => finish(input, root, &event_stream, cx).await,
                EvidenceAction::Repeat => repeat(input, root, &event_stream, cx).await,
                EvidenceAction::Mutate => mutate(input, root, &event_stream, cx).await,
            }
        })
    }
}

pub(crate) fn first_root(project: &Entity<Project>, cx: &App) -> Option<PathBuf> {
    project
        .read(cx)
        .visible_worktrees(cx)
        .next()
        .map(|worktree| worktree.read(cx).abs_path().to_path_buf())
}

async fn finish(
    input: EvidenceToolInput,
    root: PathBuf,
    event_stream: &ToolCallEventStream,
    cx: &mut AsyncApp,
) -> Result<String, String> {
    let (checks, run_trees, model) = cx.update(|cx| {
        let thread = event_stream.thread_entity_id();
        let checks = thread
            .map(|thread| crate::trust::thread_checks(thread, cx))
            .unwrap_or_default();
        let run_trees = thread
            .map(|thread| crate::trust::thread_run_trees(thread, cx))
            .unwrap_or_default();
        (checks, run_trees, event_stream.thread_model_name(cx))
    });
    let mut fingerprints_at_run = Vec::with_capacity(run_trees.len());
    for (run, tree) in run_trees {
        let at_run = tree.fingerprint.await;
        fingerprints_at_run.push((run, tree.directory, at_run));
    }
    let title = input.title.unwrap_or_else(|| "change".to_string());
    let now = chrono::Utc::now();
    let id = format!("{}-{}", now.format("%Y%m%d-%H%M%S"), evidence::slug(&title));
    let mut bundle = Bundle {
        id: id.clone(),
        title,
        summary: input.summary.unwrap_or_default(),
        created: now.to_rfc3339(),
        model,
        checks,
        claims: input
            .claims
            .into_iter()
            .map(|claim| Claim {
                text: claim.text,
                grounds: claim.grounds,
                stale_grounds: Vec::new(),
            })
            .collect(),
        not_verified: input.not_verified,
        spec_clauses: input.spec_clauses,
        files: input
            .files
            .into_iter()
            .map(|file| FileConfidence {
                path: file.path,
                confidence: file.confidence.clamp(0.0, 1.0),
                note: file.note,
            })
            .collect(),
        screenshots: input.screenshots,
        behavior_changes: input.behavior_changes,
        warnings: Vec::new(),
    };
    let spec_path = project_files::path(&root, project_files::SPEC);
    let root_for_write = root.clone();
    let result = cx
        .background_spawn(async move {
            let mut current_fingerprints: Vec<(PathBuf, Option<String>)> = Vec::new();
            let mut runs = Vec::with_capacity(fingerprints_at_run.len());
            for (run, directory, at_run) in fingerprints_at_run {
                let current = match directory {
                    Some(directory) => {
                        let known = current_fingerprints
                            .iter()
                            .find(|(known_directory, _)| *known_directory == directory)
                            .map(|(_, fingerprint)| fingerprint.clone());
                        match known {
                            Some(fingerprint) => fingerprint,
                            None => {
                                let fingerprint =
                                    crate::trust::working_tree_fingerprint(&directory).await;
                                current_fingerprints.push((directory, fingerprint.clone()));
                                fingerprint
                            }
                        }
                    }
                    None => None,
                };
                runs.push(RunFingerprint {
                    run,
                    at_run,
                    now: current,
                });
            }
            bundle.mark_stale(&evidence::stale_runs(&runs));
            let stale_notes = bundle.stale_claim_notes();

            if let Ok(spec_text) = std::fs::read_to_string(&spec_path) {
                let spec = Spec::parse(&spec_text);
                for unknown in spec.unknown_ids(&bundle.spec_clauses) {
                    bundle
                        .warnings
                        .push(format!("cites spec clause {unknown}, which the spec doesn't have"));
                }
            }
            if bundle.checks.is_empty() {
                bundle.warnings.push(
                    "no commands ran in this conversation, so nothing here was executed".to_string(),
                );
            }
            if bundle.not_verified.is_empty() {
                bundle
                    .warnings
                    .push("nothing is listed as unverified".to_string());
            }
            let flaky: Vec<String> = bundle
                .flaky_checks()
                .iter()
                .map(|check| format!("`{}` is flaky", check.command))
                .collect();
            bundle.warnings.extend(flaky);

            let directory = project_files::path(&root_for_write, project_files::EVIDENCE);
            std::fs::create_dir_all(&directory)?;
            let markdown = bundle.to_markdown();
            let markdown_path = directory.join(format!("{}.md", bundle.id));
            std::fs::write(&markdown_path, &markdown)?;
            std::fs::write(
                directory.join(format!("{}.json", bundle.id)),
                serde_json::to_string_pretty(&bundle)?,
            )?;
            crate::trust::record_provenance(
                &root_for_write,
                provenance::Entry {
                    time: bundle.created.clone(),
                    actor: "shepherd".into(),
                    action: "evidence".into(),
                    path: Some(format!(".noah/evidence/{}.md", bundle.id)),
                    model: bundle.model.clone(),
                    evidence: Some(bundle.id.clone()),
                    detail: Some(bundle.title.clone()),
                    ..Default::default()
                },
            );
            let problems = bundle.ungrounded_claims().len() + bundle.warnings.len();
            anyhow::Ok((markdown, markdown_path, problems, stale_notes))
        })
        .await
        .map_err(|error| format!("couldn't save the evidence: {error:#}"))?;
    let (markdown, path, problems, stale_notes) = result;
    let mut response = format!("saved {}\n\n{markdown}", path.display());
    if !stale_notes.is_empty() {
        response.push('\n');
        for note in &stale_notes {
            response.push_str(&format!("\n{note}"));
        }
        response.push_str(
            "\nRe-run those commands in the terminal, then call `finish` again citing the new run ids.",
        );
    }
    if problems > 0 {
        response.push_str(
            "\nThe person will see the items under \"needs attention\". Mention them plainly in your reply; don't describe the change as verified where the evidence doesn't show it.",
        );
    }
    Ok(response)
}

async fn authorize_command(
    command: &str,
    purpose: &str,
    event_stream: &ToolCallEventStream,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    if let Some(irreversible) = noah_trust::blast_radius::classify(command) {
        return Err(format!(
            "`{command}` is irreversible ({}), so it can't be repeated automatically",
            irreversible.kind
        ));
    }
    let authorize = cx.update(|cx| {
        let context = crate::ToolPermissionContext::new("terminal", vec![command.to_string()]);
        event_stream.authorize(format!("{purpose}: {command}"), context, cx)
    });
    futures::select! {
        result = authorize.fuse() => result.map_err(|error| error.to_string()),
        _ = event_stream.cancelled_by_user().fuse() => Err("cancelled".to_string()),
    }
}

/// Runs a command through the person's shell, outside the terminal, and
/// returns its exit code and combined output.
pub(crate) async fn run_shell(
    command: &str,
    directory: &Path,
    cx: &mut AsyncApp,
) -> (Option<i32>, String, Duration) {
    let started = Instant::now();
    let path_with_git = path_with_git_for(command);
    #[cfg(windows)]
    let output = {
        use std::os::windows::process::CommandExt as _;
        let mut std_command = util::command::new_std_command("cmd");
        // Rust would quote the command as one argument and escape inner quotes
        // as `\"`, which cmd doesn't understand. With `/S`, cmd strips only the
        // outermost quotes and runs the rest verbatim.
        std_command.raw_arg(format!("/S /C \"{command}\""));
        std_command.current_dir(directory);
        if let Some(path) = &path_with_git {
            std_command.env("PATH", path);
        }
        let mut process = smol::process::Command::from(std_command);
        process.kill_on_drop(true);
        async move { process.output().await }
    };
    #[cfg(not(windows))]
    let output = {
        let mut process = util::command::new_command("sh");
        process
            .arg("-c")
            .arg(command)
            .current_dir(directory)
            .kill_on_drop(true);
        if let Some(path) = &path_with_git {
            process.env("PATH", path);
        }
        async move { process.output().await }
    };
    let output = output.fuse();
    let timeout = cx.background_executor().timer(COMMAND_TIMEOUT).fuse();
    futures::pin_mut!(output, timeout);
    futures::select! {
        output = output => match output {
            Ok(output) => {
                let mut text = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.trim().is_empty() {
                    text.push_str("\n");
                    text.push_str(&stderr);
                }
                (output.status.code(), text, started.elapsed())
            }
            Err(error) => (None, format!("couldn't start the command: {error}"), started.elapsed()),
        },
        _ = timeout => {
            (None, format!("timed out after {} seconds", COMMAND_TIMEOUT.as_secs()), started.elapsed())
        }
    }
}

/// The `PATH` for a `git` command when the git noah uses isn't on the
/// person's `PATH`, such as the MinGit noah downloads on Windows.
fn path_with_git_for(command: &str) -> Option<OsString> {
    if !command.trim_start().starts_with("git ") {
        return None;
    }
    let git = paths::git_program();
    if !git.is_absolute() {
        return None;
    }
    let git_directory = git.parent()?;
    let current_path = std::env::var_os("PATH").unwrap_or_default();
    if std::env::split_paths(&current_path).any(|entry| entry.as_path() == git_directory) {
        return None;
    }
    let entries =
        std::iter::once(git_directory.to_path_buf()).chain(std::env::split_paths(&current_path));
    std::env::join_paths(entries).log_err()
}

async fn repeat(
    input: EvidenceToolInput,
    root: PathBuf,
    event_stream: &ToolCallEventStream,
    cx: &mut AsyncApp,
) -> Result<String, String> {
    let command = input
        .command
        .ok_or_else(|| "give the test `command` to repeat".to_string())?;
    let times = input.times.unwrap_or(5).clamp(2, 50);
    authorize_command(&command, &format!("run {times} times"), event_stream, cx).await?;

    let known_secrets = cx.update(|cx| crate::trust::brokered_secrets(cx));
    let started = Instant::now();
    let mut passed = 0;
    let mut failures: Vec<(u32, Option<i32>, String)> = Vec::new();
    let mut last_output = String::new();
    for attempt in 1..=times {
        if event_stream.was_cancelled_by_user() {
            return Err("cancelled".to_string());
        }
        let (code, output, _) = run_shell(&command, &root, cx).await;
        let output = noah_trust::secrets::redact(&output, &known_secrets).text;
        if code == Some(0) {
            passed += 1;
        } else {
            failures.push((attempt, code, evidence::tail(&output, 25)));
        }
        last_output = output;
        event_stream.update_fields(acp::ToolCallUpdateFields::new().title(format!(
            "evidence: run {} ×{times} ({attempt} done, {passed} passed)",
            MarkdownInlineCode(&command)
        )));
    }
    let result = RepeatResult {
        attempts: times,
        passed,
    };
    let output_tail = match failures.first() {
        Some((attempt, code, output)) => format!(
            "attempt {attempt} failed (exit {}):\n{output}",
            code.map_or("none".to_string(), |code| code.to_string())
        ),
        None => evidence::tail(&last_output, 25),
    };
    let id = cx.update(|cx| {
        event_stream.thread_entity_id().map(|thread| {
            crate::trust::record_check(
                thread,
                Check {
                    id: String::new(),
                    command: noah_trust::secrets::redact(&command, &known_secrets).text,
                    exit_code: Some(if passed == times { 0 } else { 1 }),
                    duration_ms: started.elapsed().as_millis() as u64,
                    output_tail: output_tail.clone(),
                    working_directory: Some(root.display().to_string()),
                    repeat: Some(result.clone()),
                },
                cx,
            )
        })
    });
    let verdict = if passed == times {
        "passed every time".to_string()
    } else if passed == 0 {
        "failed every time: this is a real failure, not a flake".to_string()
    } else {
        format!(
            "FLAKY: passed {passed} of {times}. Failing attempts: {}",
            failures
                .iter()
                .map(|(attempt, _, _)| attempt.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    Ok(format!(
        "`{command}` ×{times}: {verdict}\n\n```\n{output_tail}\n```{}",
        id.map(|id| format!("\n\n[evidence {id}]")).unwrap_or_default()
    ))
}

/// Puts the original file back when dropped, so a crash or cancellation in
/// the middle of mutation testing never leaves a bug behind.
struct RestoreOnDrop {
    path: PathBuf,
    original: String,
}

impl Drop for RestoreOnDrop {
    fn drop(&mut self) {
        if let Err(error) = std::fs::write(&self.path, &self.original) {
            log::error!(
                "couldn't restore {} after mutation testing: {error}",
                self.path.display()
            );
        }
    }
}

async fn mutate(
    input: EvidenceToolInput,
    root: PathBuf,
    event_stream: &ToolCallEventStream,
    cx: &mut AsyncApp,
) -> Result<String, String> {
    let command = input
        .command
        .ok_or_else(|| "give the test `command` to run against each mutant".to_string())?;
    let relative = input
        .path
        .ok_or_else(|| "give the `path` of the file you changed".to_string())?;
    let relative_path = Path::new(&relative);
    if relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!(
            "`{relative}` must be a path inside the project, relative to its root, without `..`"
        ));
    }
    let path = root.join(relative_path);
    let original = std::fs::read_to_string(&path)
        .map_err(|error| format!("couldn't read {}: {error}", path.display()))?;
    let first = input.start_line.unwrap_or(1);
    let last = input.end_line.unwrap_or(original.lines().count());
    let limit = input
        .limit
        .unwrap_or(mutation::DEFAULT_LIMIT)
        .clamp(1, mutation::MAX_LIMIT);
    let mutants = mutation::mutants_for_path(&relative, &original, first, last, limit);
    if mutants.is_empty() {
        return Ok(format!(
            "nothing to mutate in lines {first}–{last} (no comparisons, booleans, arithmetic, returns, plain calls or two-argument calls)"
        ));
    }
    authorize_command(
        &command,
        &format!(
            "mutation test {} ({} temporary edits, restored after each)",
            path.display(),
            mutants.len()
        ),
        event_stream,
        cx,
    )
    .await?;

    let (baseline, _, _) = run_shell(&command, &root, cx).await;
    if baseline != Some(0) {
        return Err(format!(
            "`{command}` fails before any mutation, so mutation testing can't tell anything. Fix the tests first."
        ));
    }

    let started = Instant::now();
    let mut survived = Vec::new();
    let mut caught = 0;
    for (index, mutant) in mutants.iter().enumerate() {
        if event_stream.was_cancelled_by_user() {
            break;
        }
        let Some(mutated) = mutation::apply(&original, mutant) else {
            continue;
        };
        let restore = RestoreOnDrop {
            path: path.clone(),
            original: original.clone(),
        };
        std::fs::write(&path, mutated)
            .map_err(|error| format!("couldn't write {}: {error}", path.display()))?;
        let (code, _, _) = run_shell(&command, &root, cx).await;
        drop(restore);
        if code == Some(0) {
            survived.push(mutant.clone());
        } else {
            caught += 1;
        }
        event_stream.update_fields(acp::ToolCallUpdateFields::new().title(format!(
            "evidence: mutation test {} ({}/{} done)",
            MarkdownInlineCode(&relative),
            index + 1,
            mutants.len()
        )));
    }
    let tried = caught + survived.len();
    let mut report = format!("{caught} of {tried} mutants were caught by `{command}`.\n");
    if !survived.is_empty() {
        report.push_str("\nNot caught (behavior no test checks):\n");
        for mutant in &survived {
            report.push_str(&format!("- {}\n", mutation::survivor_line(mutant)));
        }
        report.push_str("\nAdd tests that fail for these, then run the mutation test again.");
    }
    let id = cx.update(|cx| {
        event_stream.thread_entity_id().map(|thread| {
            crate::trust::record_check(
                thread,
                Check {
                    id: String::new(),
                    command: format!("mutation test of {relative} with `{command}`"),
                    exit_code: Some(if survived.is_empty() { 0 } else { 1 }),
                    duration_ms: started.elapsed().as_millis() as u64,
                    output_tail: report.clone(),
                    working_directory: Some(root.display().to_string()),
                    repeat: None,
                },
                cx,
            )
        })
    });
    Ok(format!(
        "{report}{}",
        id.map(|id| format!("\n\n[evidence {id}]")).unwrap_or_default()
    ))
}
