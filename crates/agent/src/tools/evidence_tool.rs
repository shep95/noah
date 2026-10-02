use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_client_protocol::schema::v1 as acp;
use futures::FutureExt as _;
use gpui::{App, AppContext as _, AsyncApp, Entity, Task};
use noah_trust::{
    evidence::{self, Bundle, Check, Claim, FileConfidence, RepeatResult, RunFingerprint},
    mutation, project_files, provenance,
    spec::Spec,
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
/// - `replay`: run every command a saved bundle recorded, in the same
///   folders, and compare each exit code and output tail with what was
///   recorded then: one call answers "did this stay true". Give `bundle`
///   (an id from `.noah/evidence/`, or `latest`).
/// - `contract`: turns the spec's invariants (`INV-n` in `.noah/spec.md`)
///   into executable truth. It returns each invariant with what to do: write
///   one property-based test per invariant (proptest, hypothesis,
///   fast-check, or the project's own), named after its id, run them, then
///   `mutate` the code each one guards to show the test catches a break.
///   Cite the runs for each `INV-n` in `finish`.
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
    /// For `replay`: the bundle id (the file name under `.noah/evidence/`
    /// without `.json`), or `latest`.
    #[serde(default)]
    pub bundle: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceAction {
    Finish,
    Repeat,
    Mutate,
    Replay,
    Contract,
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
                EvidenceAction::Finish => {
                    format!("evidence: {}", input.title.as_deref().unwrap_or("finish")).into()
                }
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
                EvidenceAction::Contract => "evidence: the spec's invariants as tests".into(),
                EvidenceAction::Replay => format!(
                    "evidence: replay {}",
                    MarkdownInlineCode(input.bundle.as_deref().unwrap_or("latest"))
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
                EvidenceAction::Replay => replay(input, root, &event_stream, cx).await,
                EvidenceAction::Contract => contract(&root),
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
                    bundle.warnings.push(format!(
                        "cites spec clause {unknown}, which the spec doesn't have"
                    ));
                }
            }
            if bundle.checks.is_empty() {
                bundle.warnings.push(
                    "no commands ran in this conversation, so nothing here was executed"
                        .to_string(),
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
        id.map(|id| format!("\n\n[evidence {id}]"))
            .unwrap_or_default()
    ))
}

/// What the spec promises always holds, with the instructions to make each
/// promise a test that runs.
fn contract(root: &Path) -> Result<String, String> {
    let path = project_files::path(root, project_files::SPEC);
    let text = std::fs::read_to_string(&path).map_err(|_| {
        "this project has no spec yet (.noah/spec.md); write its invariants first".to_string()
    })?;
    let spec = Spec::parse(&text);
    let invariants: Vec<_> = spec
        .clauses
        .iter()
        .filter(|clause| clause.kind == noah_trust::spec::ClauseKind::Invariant)
        .collect();
    if invariants.is_empty() {
        return Err(
            "the spec lists no invariants (`INV-n` under an \"invariants\" heading); add the things that must always hold, then ask again"
                .to_string(),
        );
    }
    let mut out = format!(
        "{} invariant{} to turn into tests:\n",
        invariants.len(),
        if invariants.len() == 1 { "" } else { "s" }
    );
    for clause in &invariants {
        out.push_str(&format!("\n- {}: {}", clause.id, clause.text));
    }
    out.push_str(
        "\n\nfor each one: write a property-based test named after its id (for example `inv_1_...`) that generates many inputs and asserts the invariant, using the project's property-testing library or adding one; run it in the terminal; then `mutate` the code it guards to show the test fails when the invariant breaks. an invariant you can't express as a test goes under not_verified in `finish`, with why.",
    );
    Ok(out)
}

/// A bundle by id, or the newest one; the id is a file name, so it must be
/// a plain name and nothing that walks out of the evidence folder.
fn load_bundle(root: &Path, wanted: &str) -> Result<Bundle, String> {
    let directory = project_files::path(root, project_files::EVIDENCE);
    let wanted = wanted.trim().trim_end_matches(".json").trim_end_matches(".md");
    let path = if wanted.is_empty() || wanted == "latest" {
        let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
        let entries = std::fs::read_dir(&directory)
            .map_err(|_| "no evidence has been saved in this project yet".to_string())?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == "json")
                && let Ok(modified) = entry.metadata().and_then(|metadata| metadata.modified())
                && newest.as_ref().is_none_or(|(when, _)| modified > *when)
            {
                newest = Some((modified, path));
            }
        }
        newest
            .map(|(_, path)| path)
            .ok_or_else(|| "no evidence has been saved in this project yet".to_string())?
    } else {
        if wanted
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
            || wanted.contains("..")
        {
            return Err(format!("`{wanted}` is not a bundle id"));
        }
        directory.join(format!("{wanted}.json"))
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("couldn't read {}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{} is not a bundle: {error}", path.display()))
}

/// The folder a recorded check ran in, when it is still inside the project;
/// anything else replays from the root.
fn replay_directory(root: &Path, recorded: Option<&str>) -> PathBuf {
    let Some(recorded) = recorded else {
        return root.to_path_buf();
    };
    let recorded = PathBuf::from(recorded);
    if recorded.is_dir() && recorded.starts_with(root) {
        recorded
    } else {
        root.to_path_buf()
    }
}

/// Lines in `then` that are not in `now` and the reverse, ignoring the
/// "earlier lines not shown" marker and timing-looking noise (durations,
/// timestamps), capped so a changed log doesn't flood the reply.
pub(crate) fn tail_difference(then: &str, now: &str) -> Vec<String> {
    const MAX_LINES: usize = 20;
    fn normalise(line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() || line.starts_with('…') {
            return None;
        }
        let mut out = String::with_capacity(line.len());
        let mut in_number = false;
        for c in line.chars() {
            if c.is_ascii_digit() || (in_number && c == '.') {
                if !in_number {
                    out.push('#');
                    in_number = true;
                }
            } else {
                in_number = false;
                out.push(c);
            }
        }
        Some(out)
    }
    let then_lines: Vec<String> = then.lines().filter_map(normalise).collect();
    let now_lines: Vec<String> = now.lines().filter_map(normalise).collect();
    let mut difference = Vec::new();
    for line in &then_lines {
        if !now_lines.contains(line) {
            difference.push(format!("- {line}"));
        }
    }
    for line in &now_lines {
        if !then_lines.contains(line) {
            difference.push(format!("+ {line}"));
        }
    }
    if difference.len() > MAX_LINES {
        let hidden = difference.len() - MAX_LINES;
        difference.truncate(MAX_LINES);
        difference.push(format!("… {hidden} more changed lines"));
    }
    difference
}

async fn replay(
    input: EvidenceToolInput,
    root: PathBuf,
    event_stream: &ToolCallEventStream,
    cx: &mut AsyncApp,
) -> Result<String, String> {
    let bundle = load_bundle(&root, input.bundle.as_deref().unwrap_or("latest"))?;
    if bundle.checks.is_empty() {
        return Err(format!("bundle {} recorded no commands to replay", bundle.id));
    }
    let known_secrets = cx.update(|cx| crate::trust::brokered_secrets(cx));
    let total = bundle.checks.len();
    let mut held = 0usize;
    let mut lines: Vec<String> = Vec::new();
    let mut ids: Vec<String> = Vec::new();
    for (index, recorded) in bundle.checks.iter().enumerate() {
        if event_stream.was_cancelled_by_user() {
            return Err("cancelled".to_string());
        }
        event_stream.update_fields(acp::ToolCallUpdateFields::new().title(format!(
            "evidence: replay {} ({} of {total}, {held} held)",
            bundle.id,
            index + 1
        )));
        if let Err(reason) =
            authorize_command(&recorded.command, "replay", event_stream, cx).await
        {
            lines.push(format!(
                "· `{}` ({}): skipped, {reason}",
                recorded.command, recorded.id
            ));
            continue;
        }
        let directory = replay_directory(&root, recorded.working_directory.as_deref());
        let (code, output, elapsed) = run_shell(&recorded.command, &directory, cx).await;
        let output = noah_trust::secrets::redact(&output, &known_secrets).text;
        let now_tail = evidence::tail(&output, 40);
        let same_exit = code == recorded.exit_code;
        let difference = tail_difference(&recorded.output_tail, &now_tail);
        let stayed = same_exit && difference.is_empty();
        if stayed {
            held += 1;
        }
        let id = cx.update(|cx| {
            event_stream.thread_entity_id().map(|thread| {
                crate::trust::record_check(
                    thread,
                    Check {
                        id: String::new(),
                        command: noah_trust::secrets::redact(&recorded.command, &known_secrets)
                            .text,
                        exit_code: code,
                        duration_ms: elapsed.as_millis() as u64,
                        output_tail: now_tail.clone(),
                        working_directory: Some(directory.display().to_string()),
                        repeat: None,
                    },
                    cx,
                )
            })
        });
        if let Some(id) = &id {
            ids.push(id.clone());
        }
        let exit_text = |code: Option<i32>| code.map_or("none".to_string(), |code| code.to_string());
        let mut line = format!(
            "{} `{}` ({} → {}): exit {} then, {} now",
            if stayed { "✓" } else { "✗" },
            recorded.command,
            recorded.id,
            id.as_deref().unwrap_or("not recorded"),
            exit_text(recorded.exit_code),
            exit_text(code)
        );
        if !difference.is_empty() {
            line.push_str("\n  output changed:\n  ");
            line.push_str(&difference.join("\n  "));
        }
        lines.push(line);
    }
    let verdict = if held == total {
        format!("every one of the {total} recorded commands still ends the same way")
    } else {
        format!("{held} of {total} recorded commands still end the same way; the rest changed")
    };
    Ok(format!(
        "replayed bundle {} ({}): {verdict}\n\n{}",
        bundle.id,
        bundle.title,
        lines.join("\n")
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
        id.map(|id| format!("\n\n[evidence {id}]"))
            .unwrap_or_default()
    ))
}

#[cfg(test)]
mod replay_tests {
    use super::*;

    #[test]
    fn tail_difference_ignores_numbers_and_the_elided_marker() {
        let then = "… 3 earlier lines not shown\ntest result: ok. 12 passed in 0.31s\n";
        let now = "test result: ok. 14 passed in 1.02s\n";
        assert!(tail_difference(then, now).is_empty());
        let broken = "test result: FAILED. 1 failed\n";
        let difference = tail_difference(then, broken);
        assert_eq!(difference.len(), 2, "{difference:?}");
    }

    #[test]
    fn bundle_ids_are_file_names_only() {
        let root = tempfile::tempdir().expect("tempdir");
        assert!(load_bundle(root.path(), "../outside").is_err());
        assert!(load_bundle(root.path(), "latest").is_err());
    }

    #[test]
    fn recorded_folders_outside_the_project_replay_from_the_root() {
        let root = tempfile::tempdir().expect("tempdir");
        assert_eq!(replay_directory(root.path(), Some("/")), root.path());
        assert_eq!(replay_directory(root.path(), None), root.path());
        let inside = root.path().join("sub");
        std::fs::create_dir(&inside).expect("mkdir");
        assert_eq!(replay_directory(root.path(), inside.to_str()), inside);
    }
}
