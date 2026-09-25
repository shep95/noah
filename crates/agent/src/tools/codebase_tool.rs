use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use gpui::{App, AppContext as _, AsyncApp, Entity, Task};
use settings::Settings as _;
use noah_trust::{
    calibration, outcomes,
    packages::{Dependency, Ecosystem},
    planning, project_files, provenance, rules_check,
};
use project::Project;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ui::SharedString;

use crate::{AgentTool, ToolCallEventStream, ToolInput};

/// Understanding the codebase as a whole, and checking the things around it.
///
/// - `map`: build or refresh `.noah/map.md`, a structural model of the
///   project: its areas and what's in them, languages, entry points,
///   manifests, the files that change most, and files that always change
///   together. Notes you add under "## notes" survive a refresh. The map is a
///   summary: confirm against the code before relying on a detail.
/// - `why`: why a line is the way it is: its git history, who or what wrote
///   it (the provenance log) and what `.noah/why.md` says about the file.
///   Check this before removing code that looks unnecessary.
/// - `tour`: write `.noah/tour.md`, a reading order for someone new to the
///   project, from the map.
/// - `rules`: check the project's agent instruction files (AGENTS.md,
///   CLAUDE.md, .rules, ...) for contradictions, duplicates and stale paths.
/// - `plan`: split parallel work into waves that won't conflict, given each
///   task's files; overlap and history of files changing together are
///   predicted conflicts.
/// - `packages`: check dependency names against their registries (exists,
///   age, look-alikes of popular packages) before adding them.
/// - `outcomes`: how shepherd's past work held up here: commits landed,
///   reverted and fixed soon after, per model, and whether its stated
///   confidence has been accurate.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CodebaseToolInput {
    pub action: CodebaseAction,
    /// For `why`: the file, relative to the project root.
    #[serde(default)]
    pub path: Option<String>,
    /// For `why`: the 1-based line.
    #[serde(default)]
    pub line: Option<u32>,
    /// For `plan`: the tasks and the paths each will touch (a path ending in
    /// `/` covers a folder).
    #[serde(default)]
    pub tasks: Vec<PlanTask>,
    /// For `packages`: `crates`, `npm` or `pypi`.
    #[serde(default)]
    pub ecosystem: Option<String>,
    /// For `packages`: the package names.
    #[serde(default)]
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PlanTask {
    pub name: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CodebaseAction {
    Map,
    Why,
    Tour,
    Rules,
    Plan,
    Packages,
    Outcomes,
}

pub struct CodebaseTool {
    project: Entity<Project>,
}

impl CodebaseTool {
    pub fn new(project: Entity<Project>) -> Self {
        Self { project }
    }
}

impl AgentTool for CodebaseTool {
    type Input = CodebaseToolInput;
    type Output = String;

    const NAME: &'static str = "codebase";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Search
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
                CodebaseAction::Map => "codebase: map the project".into(),
                CodebaseAction::Why => format!(
                    "codebase: why is {}:{} like this",
                    input.path.as_deref().unwrap_or("?"),
                    input.line.unwrap_or(1)
                )
                .into(),
                CodebaseAction::Tour => "codebase: write a tour".into(),
                CodebaseAction::Rules => "codebase: check agent instructions".into(),
                CodebaseAction::Plan => "codebase: plan parallel work".into(),
                CodebaseAction::Packages => {
                    format!("codebase: check packages {}", input.names.join(", ")).into()
                }
                CodebaseAction::Outcomes => "codebase: outcomes of shepherd's work".into(),
            },
            Err(_) => "codebase".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        let project = self.project.clone();
        cx.spawn(async move |cx| {
            let input = input.recv().await.map_err(|error| error.to_string())?;
            let (root, http_client, offline) = cx.update(|cx| {
                (
                    super::evidence_tool::first_root(&project, cx),
                    project.read(cx).client().http_client(),
                    agent_settings::AgentSettings::get_global(cx).offline,
                )
            });
            let root = root.ok_or_else(|| "open a project folder first".to_string())?;
            match input.action {
                CodebaseAction::Map => map(root, cx).await,
                CodebaseAction::Why => why(input, root, cx).await,
                CodebaseAction::Tour => tour(root, cx).await,
                CodebaseAction::Rules => Ok(cx.background_spawn(async move { rules(&root) }).await),
                CodebaseAction::Plan => plan(input, root, cx).await,
                CodebaseAction::Packages => {
                    if offline {
                        return Err("noah is in offline mode, so registries can't be checked".to_string());
                    }
                    let ecosystem = input
                        .ecosystem
                        .as_deref()
                        .and_then(Ecosystem::parse)
                        .ok_or_else(|| "give the `ecosystem`: crates, npm or pypi".to_string())?;
                    let dependencies = input
                        .names
                        .into_iter()
                        .map(|name| Dependency { ecosystem, name })
                        .collect();
                    let assessments = crate::trust::check_dependencies(http_client, dependencies).await;
                    Ok(assessments
                        .iter()
                        .map(|assessment| {
                            format!(
                                "- {:?} `{}`: {}",
                                assessment.verdict,
                                assessment.dependency.name,
                                if assessment.notes.is_empty() {
                                    "exists, established".to_string()
                                } else {
                                    assessment.notes.join("; ")
                                }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n"))
                }
                CodebaseAction::Outcomes => outcomes_report(root, cx).await,
            }
        })
    }
}

async fn git(arguments: &str, root: &Path, cx: &mut AsyncApp) -> Option<String> {
    let (code, output, _) =
        super::evidence_tool::run_shell(&format!("git {arguments}"), root, cx).await;
    (code == Some(0)).then_some(output)
}

const SKIPPED_FOLDERS: &[&str] = &[
    ".git", "node_modules", "target", "dist", "build", ".next", "vendor", "__pycache__", ".venv",
    "venv", ".noah",
];

fn walk_files(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if !SKIPPED_FOLDERS.contains(&name.as_str()) && !name.starts_with('.') {
                    stack.push(path);
                }
            } else if let Ok(relative) = path.strip_prefix(root) {
                files.push(relative.to_string_lossy().replace('\\', "/"));
            }
            if files.len() > 200_000 {
                return files;
            }
        }
    }
    files
}

fn language_of(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.')?.1.to_lowercase();
    Some(match extension.as_str() {
        "rs" => "Rust",
        "ts" | "tsx" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "py" => "Python",
        "go" => "Go",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "swift" => "Swift",
        "c" | "h" => "C",
        "cc" | "cpp" | "cxx" | "hpp" => "C++",
        "cs" => "C#",
        "rb" => "Ruby",
        "php" => "PHP",
        "scala" => "Scala",
        "ex" | "exs" => "Elixir",
        "hs" => "Haskell",
        "lua" => "Lua",
        "dart" => "Dart",
        "sql" => "SQL",
        "sh" | "bash" | "zsh" => "Shell",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "zig" => "Zig",
        _ => return None,
    })
}

const MANIFESTS: &[&str] = &[
    "Cargo.toml", "package.json", "pyproject.toml", "requirements.txt", "go.mod", "pom.xml",
    "build.gradle", "build.gradle.kts", "Gemfile", "composer.json", "mix.exs", "pubspec.yaml",
    "Package.swift", "CMakeLists.txt", "Makefile", "Dockerfile", "docker-compose.yml",
];

const ENTRY_POINTS: &[&str] = &[
    "main.rs", "lib.rs", "main.go", "main.py", "__main__.py", "app.py", "index.ts", "index.tsx",
    "index.js", "main.ts", "main.tsx", "server.ts", "server.js", "App.tsx", "Program.cs",
    "Main.java", "main.c", "main.cpp",
];

const NOTES_HEADING: &str = "## notes (kept when the map is refreshed)";

async fn map(root: PathBuf, cx: &mut AsyncApp) -> Result<String, String> {
    let commit = git("rev-parse --short HEAD", &root, cx).await;
    let history = git(
        "log -n 400 --name-only --format=%x1e",
        &root,
        cx,
    )
    .await
    .unwrap_or_default();
    let tracked = git("ls-files", &root, cx).await;
    cx.background_spawn(async move {
        let files: Vec<String> = match tracked {
            Some(listing) if !listing.trim().is_empty() => {
                listing.lines().map(str::to_string).collect()
            }
            _ => walk_files(&root),
        };
        let mut areas: BTreeMap<String, (usize, BTreeMap<&'static str, usize>)> = BTreeMap::new();
        let mut languages: BTreeMap<&'static str, usize> = BTreeMap::new();
        let mut manifests = Vec::new();
        let mut entry_points = Vec::new();
        for file in &files {
            let parts: Vec<&str> = file.split('/').collect();
            let area = match parts.len() {
                1 => "(root)".to_string(),
                2 => format!("{}/", parts[0]),
                _ => format!("{}/{}/", parts[0], parts[1]),
            };
            let language = language_of(file);
            let entry = areas.entry(area).or_default();
            entry.0 += 1;
            if let Some(language) = language {
                *entry.1.entry(language).or_insert(0) += 1;
                *languages.entry(language).or_insert(0) += 1;
            }
            let name = parts.last().copied().unwrap_or_default();
            if MANIFESTS.contains(&name) && parts.len() <= 3 {
                manifests.push(file.clone());
            }
            if ENTRY_POINTS.contains(&name) && parts.len() <= 4 {
                entry_points.push(file.clone());
            }
        }
        let mut change_counts: BTreeMap<&str, usize> = BTreeMap::new();
        for line in history.lines() {
            let line = line.trim_matches('\u{1e}').trim();
            if !line.is_empty() {
                *change_counts.entry(line).or_insert(0) += 1;
            }
        }
        let mut hot: Vec<(&str, usize)> = change_counts.into_iter().collect();
        hot.sort_by(|a, b| b.1.cmp(&a.1));
        let co_changes = planning::co_changes(&history, 4);
        let mut coupled: Vec<(&(String, String), &usize)> = co_changes.iter().collect();
        coupled.sort_by(|a, b| b.1.cmp(a.1));

        let map_path = project_files::path(&root, project_files::MAP);
        let kept_notes = std::fs::read_to_string(&map_path)
            .ok()
            .and_then(|text| {
                text.split_once(NOTES_HEADING)
                    .map(|(_, notes)| notes.trim().to_string())
            })
            .unwrap_or_default();

        let mut out = format!(
            "# map\n\ngenerated {}{}. A summary for orientation: confirm details in the code before relying on them.\n\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M"),
            commit
                .map(|commit| format!(" at commit {}", commit.trim()))
                .unwrap_or_default()
        );
        out.push_str(&format!("{} files.\n\n## languages\n\n", files.len()));
        let mut language_list: Vec<_> = languages.into_iter().collect();
        language_list.sort_by(|a, b| b.1.cmp(&a.1));
        for (language, count) in language_list.iter().take(10) {
            out.push_str(&format!("- {language}: {count} files\n"));
        }
        out.push_str("\n## areas\n\n");
        let mut area_list: Vec<_> = areas.into_iter().collect();
        area_list.sort_by(|a, b| b.1.0.cmp(&a.1.0));
        for (area, (count, area_languages)) in area_list.iter().take(60) {
            let main_language = area_languages
                .iter()
                .max_by_key(|(_, count)| **count)
                .map(|(language, _)| format!(", mostly {language}"))
                .unwrap_or_default();
            out.push_str(&format!("- `{area}` {count} files{main_language}\n"));
        }
        if !manifests.is_empty() {
            out.push_str("\n## manifests\n\n");
            for manifest in manifests.iter().take(40) {
                out.push_str(&format!("- `{manifest}`\n"));
            }
        }
        if !entry_points.is_empty() {
            out.push_str("\n## entry points\n\n");
            for entry_point in entry_points.iter().take(40) {
                out.push_str(&format!("- `{entry_point}`\n"));
            }
        }
        if !hot.is_empty() {
            out.push_str("\n## most changed (last 400 commits)\n\n");
            for (file, count) in hot.iter().take(15) {
                out.push_str(&format!("- `{file}` {count} commits\n"));
            }
        }
        if !coupled.is_empty() {
            out.push_str("\n## change together (edit one, check the other)\n\n");
            for ((first, second), count) in coupled.iter().take(15) {
                out.push_str(&format!("- `{first}` + `{second}`: {count} commits\n"));
            }
        }
        out.push_str(&format!("\n{NOTES_HEADING}\n\n"));
        if kept_notes.is_empty() {
            out.push_str("Module responsibilities, invariants and data flows go here. Add them as you learn them.\n");
        } else {
            out.push_str(&kept_notes);
            out.push('\n');
        }
        if let Some(directory) = map_path.parent() {
            std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        }
        std::fs::write(&map_path, &out).map_err(|error| error.to_string())?;
        Ok(format!("saved .noah/map.md\n\n{out}"))
    })
    .await
}

async fn why(input: CodebaseToolInput, root: PathBuf, cx: &mut AsyncApp) -> Result<String, String> {
    let path = input
        .path
        .ok_or_else(|| "give the `path`".to_string())?
        .replace('\\', "/");
    let line = input.line.unwrap_or(1).max(1);
    if path.contains(['"', '\'', '`', '$', ';', '&', '|']) {
        return Err("that path has characters noah won't pass to git".to_string());
    }
    let history = git(
        &format!("log -n 6 --format=%h%x20%ad%x20%an%n%B%n--- -L {line},{line}:\"{path}\" --date=short --no-patch"),
        &root,
        cx,
    )
    .await;
    let blame = git(
        &format!("blame -L {line},{line} --date=short -- \"{path}\""),
        &root,
        cx,
    )
    .await;
    cx.background_spawn(async move {
        let mut out = format!("# why `{path}:{line}`\n\n");
        match blame {
            Some(blame) => out.push_str(&format!("## last written\n\n```\n{}\n```\n\n", blame.trim())),
            None => out.push_str("## last written\n\nnot in git, or not committed yet.\n\n"),
        }
        if let Some(history) = history.filter(|history| !history.trim().is_empty()) {
            out.push_str(&format!("## commits that touched it\n\n{}\n\n", history.trim()));
        }
        let entries = provenance::read(&project_files::path(&root, project_files::PROVENANCE))
            .unwrap_or_default();
        let authored = provenance::history_of_line(&entries, &path, line);
        if !authored.is_empty() {
            out.push_str("## written by shepherd\n\n");
            for entry in authored.iter().take(5) {
                out.push_str(&format!(
                    "- {} {} with {}{}\n",
                    entry.time,
                    entry.action,
                    entry.model.as_deref().unwrap_or("an unknown model"),
                    entry
                        .evidence
                        .as_ref()
                        .map(|evidence| format!(", evidence {evidence}"))
                        .unwrap_or_default()
                ));
            }
            out.push('\n');
        }
        let file_name = path.rsplit('/').next().unwrap_or(&path).to_string();
        if let Ok(why_log) = std::fs::read_to_string(project_files::path(&root, project_files::WHY)) {
            let mentions: Vec<&str> = why_log
                .lines()
                .filter(|entry| entry.contains(&path) || entry.contains(&file_name))
                .collect();
            if !mentions.is_empty() {
                out.push_str("## from .noah/why.md\n\n");
                for mention in mentions {
                    out.push_str(mention);
                    out.push('\n');
                }
            }
        }
        Ok(out)
    })
    .await
}

async fn tour(root: PathBuf, cx: &mut AsyncApp) -> Result<String, String> {
    let map_path = project_files::path(&root, project_files::MAP);
    if !map_path.exists() {
        map(root.clone(), cx).await?;
    }
    cx.background_spawn(async move {
        let map = std::fs::read_to_string(&map_path).map_err(|error| error.to_string())?;
        let section = |heading: &str| -> Vec<String> {
            map.split(&format!("## {heading}"))
                .nth(1)
                .map(|rest| {
                    rest.lines()
                        .skip(1)
                        .take_while(|line| !line.starts_with("## "))
                        .filter(|line| line.starts_with("- "))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        };
        let readme = ["README.md", "readme.md", "README", "README.rst"]
            .iter()
            .find(|name| root.join(name).exists());
        let mut out = String::from(
            "# tour\n\nA reading order for getting oriented, generated from `.noah/map.md`. shepherd can expand any stop on request.\n\n",
        );
        let mut stop = 1;
        if let Some(readme) = readme {
            out.push_str(&format!("{stop}. `{readme}`: what the project says about itself.\n"));
            stop += 1;
        }
        for manifest in section("manifests").iter().take(3) {
            out.push_str(&format!(
                "{stop}. {}: dependencies and how it's built.\n",
                manifest.trim_start_matches("- ")
            ));
            stop += 1;
        }
        for entry in section("entry points").iter().take(4) {
            out.push_str(&format!(
                "{stop}. {}: where execution starts.\n",
                entry.trim_start_matches("- ")
            ));
            stop += 1;
        }
        for area in section("areas").iter().take(6) {
            out.push_str(&format!("{stop}. {}\n", area.trim_start_matches("- ")));
            stop += 1;
        }
        let hot = section("most changed");
        if !hot.is_empty() {
            out.push_str("\nWhere the action is (changed most recently):\n\n");
            for file in hot.iter().take(5) {
                out.push_str(file);
                out.push('\n');
            }
        }
        let tour_path = project_files::path(&root, project_files::TOUR);
        std::fs::write(&tour_path, &out).map_err(|error| error.to_string())?;
        Ok(format!("saved .noah/tour.md\n\n{out}"))
    })
    .await
}

pub(crate) fn rules(root: &Path) -> String {
    let mut files = Vec::new();
    for name in rules_check::INSTRUCTION_FILES {
        if let Ok(text) = std::fs::read_to_string(root.join(name)) {
            files.push((name.to_string(), text));
        }
    }
    if let Ok(entries) = std::fs::read_dir(root.join(".cursor").join("rules")) {
        for entry in entries.flatten() {
            if let Ok(text) = std::fs::read_to_string(entry.path()) {
                files.push((
                    format!(".cursor/rules/{}", entry.file_name().to_string_lossy()),
                    text,
                ));
            }
        }
    }
    if files.is_empty() {
        return "no agent instruction files (AGENTS.md, CLAUDE.md, .rules, ...) in this project".to_string();
    }
    let rule_count: usize = files
        .iter()
        .map(|(name, text)| rules_check::extract_rules(name, text).len())
        .sum();
    let findings = rules_check::check(&files, |path| root.join(path).exists());
    rules_check::report(&findings, files.len(), rule_count)
}

async fn plan(input: CodebaseToolInput, root: PathBuf, cx: &mut AsyncApp) -> Result<String, String> {
    if input.tasks.len() < 2 {
        return Err("give at least two `tasks` with the paths each will touch".to_string());
    }
    let history = git("log -n 500 --name-only --format=%x1e", &root, cx)
        .await
        .unwrap_or_default();
    let tasks: Vec<planning::Task> = input
        .tasks
        .into_iter()
        .map(|task| planning::Task {
            name: task.name,
            paths: task.paths,
        })
        .collect();
    let co_changes = planning::co_changes(&history, 3);
    let plan = planning::plan(&tasks, &co_changes);
    let mut out = String::from("# parallel plan\n\n");
    for (index, wave) in plan.waves.iter().enumerate() {
        out.push_str(&format!(
            "wave {}: {} (can run at the same time)\n",
            index + 1,
            wave.join(", ")
        ));
    }
    if plan.conflicts.is_empty() {
        out.push_str("\nno conflicts predicted.\n");
    } else {
        out.push_str("\npredicted conflicts:\n");
        for conflict in &plan.conflicts {
            out.push_str(&format!(
                "- {} ↔ {}: {}\n",
                conflict.first, conflict.second, conflict.reason
            ));
        }
    }
    out.push_str("\nFor changes across services or repositories, order the waves: backward-compatible changes first, then consumers, then clean-up.");
    Ok(out)
}

async fn outcomes_report(root: PathBuf, cx: &mut AsyncApp) -> Result<String, String> {
    let log = git(
        &format!("log -n 2000 --name-only {}", outcomes::GIT_LOG_FORMAT),
        &root,
        cx,
    )
    .await
    .ok_or_else(|| "couldn't read git history".to_string())?;
    cx.background_spawn(async move {
        let measured = outcomes::measure(&outcomes::parse_git_log(&log));
        let calibration = calibration::calibrate(&calibration::read(&project_files::path(
            &root,
            project_files::CALIBRATION,
        )));
        Ok(format!(
            "{}\n## confidence\n\n{}\n",
            measured.to_markdown(),
            calibration.summary()
        ))
    })
    .await
}
