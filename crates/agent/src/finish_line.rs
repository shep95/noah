//! The finish line of a turn: what noah itself does after the model stops.
//!
//! * A model that cannot call tools writes files through fenced blocks in its
//!   reply; noah applies them here.
//! * When code changed, noah runs the project's own check and hands a failure
//!   back to the model to fix, up to [`MAX_VERIFY_ROUNDS`] times.
//! * When interface code changed, noah opens the canvas: the project served
//!   and shown in the browser room.

use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// One failing check comes back to the model at most this many times per
/// user message, so a stubborn failure ends with the transcript, not a bill.
pub const MAX_VERIFY_ROUNDS: usize = 3;

/// How long the project's check may run before noah gives up on it.
const CHECK_TIME_LIMIT: Duration = Duration::from_secs(300);

/// A file the model asked noah to write, from a fenced block whose info line
/// carries `path=`.
#[derive(Debug, PartialEq)]
pub struct FileBlock {
    pub path: String,
    pub contents: String,
}

/// Parses `path=` fenced blocks out of a reply from a model without tools.
/// The fence's info line looks like ```` ```html path=myapp/index.html ````;
/// everything to the closing fence is the file.
pub fn parse_file_blocks(text: &str) -> Vec<FileBlock> {
    let mut blocks = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        let Some(info) = trimmed.strip_prefix("```") else {
            continue;
        };
        let fence_indent = line.len() - trimmed.len();
        let Some(path) = info
            .split_whitespace()
            .find_map(|word| word.strip_prefix("path="))
        else {
            // Not a file block; skip to this fence's end so its contents
            // can't be mistaken for one.
            for inner in lines.by_ref() {
                if inner.trim_start().starts_with("```") {
                    break;
                }
            }
            continue;
        };
        let path = path.trim_matches(['"', '\'', '`']);
        if !block_path_is_plain(path) {
            continue;
        }
        let mut contents = String::new();
        for inner in lines.by_ref() {
            if inner.trim_start() == "```" || inner.trim_start().starts_with("``` ") {
                break;
            }
            // The model may indent the whole block (a list item); strip the
            // fence's own indentation, not the file's.
            let line = if fence_indent > 0
                && inner.len() >= fence_indent
                && inner[..fence_indent].trim().is_empty()
            {
                &inner[fence_indent..]
            } else {
                inner
            };
            contents.push_str(line);
            contents.push('\n');
        }
        blocks.push(FileBlock {
            path: path.to_string(),
            contents,
        });
    }
    blocks
}

/// A block path is a plain relative path: no root, no prefix (drive or UNC),
/// no `..`, no backslashes, and nothing under `.git`, whose hooks run.
pub fn block_path_is_plain(path: &str) -> bool {
    if path.is_empty() || path.contains('\\') || path.contains(':') {
        return false;
    }
    let mut components = Path::new(path).components();
    components.all(|component| match component {
        std::path::Component::Normal(name) => {
            let name = name.to_string_lossy();
            !name.is_empty() && name != ".git" && !name.eq_ignore_ascii_case(".git")
        }
        std::path::Component::CurDir => true,
        _ => false,
    })
}

/// The real place a write lands, with every existing ancestor resolved, so
/// a symlinked folder inside the project cannot carry the file outside it.
fn contained_target(root: &Path, target: &Path) -> Result<PathBuf> {
    let root = root
        .canonicalize()
        .with_context(|| format!("couldn't resolve {}", root.display()))?;
    // Walk up without following links: a symlink anywhere on the path, a
    // dangling one included, could carry the write elsewhere, so none is
    // allowed on the part that exists.
    let mut existing = target.to_path_buf();
    let mut tail = Vec::new();
    loop {
        match std::fs::symlink_metadata(&existing) {
            Ok(metadata) if metadata.file_type().is_symlink() => anyhow::bail!(
                "the path `{}` goes through a symlink, which noah does not write through",
                target.display()
            ),
            Ok(_) => break,
            Err(_) => {
                let Some(name) = existing.file_name().map(|name| name.to_owned()) else {
                    anyhow::bail!(
                        "the path `{}` has no parent inside the project",
                        target.display()
                    );
                };
                tail.push(name);
                existing.pop();
            }
        }
    }
    let resolved = existing
        .canonicalize()
        .with_context(|| format!("couldn't resolve {}", existing.display()))?;
    anyhow::ensure!(
        resolved.starts_with(&root),
        "the path `{}` leads outside the project",
        target.display()
    );
    anyhow::ensure!(
        !resolved
            .components()
            .any(|component| component.as_os_str() == ".git"),
        "the path `{}` is inside .git",
        target.display()
    );
    let mut full = resolved;
    for name in tail.into_iter().rev() {
        full.push(name);
    }
    Ok(full)
}

/// Resolves a block's path against the project's roots: `myapp/index.html`
/// lands inside the root named `myapp`, or inside the only root when the
/// name matches none.
pub fn resolve_block_path(path: &str, roots: &[(String, PathBuf)]) -> Option<PathBuf> {
    let relative = Path::new(path);
    let mut parts = relative.components();
    let first = parts.next()?.as_os_str().to_string_lossy().into_owned();
    let rest: PathBuf = parts.as_path().to_path_buf();
    if let Some((_, root)) = roots.iter().find(|(name, _)| *name == first) {
        if rest.as_os_str().is_empty() {
            return None;
        }
        return Some(root.join(rest));
    }
    let (_, only) = roots.first()?;
    if roots.len() == 1 {
        return Some(only.join(relative));
    }
    None
}

/// Writes the blocks to disk and returns what was written, in words.
pub fn apply_file_blocks(
    blocks: &[FileBlock],
    roots: &[(String, PathBuf)],
) -> Result<Vec<PathBuf>> {
    let mut written = Vec::new();
    for block in blocks {
        let target = resolve_block_path(&block.path, roots)
            .with_context(|| format!("the path `{}` matches no project root", block.path))?;
        let root = roots
            .iter()
            .map(|(_, root)| root)
            .find(|root| target.starts_with(root))
            .context("the path matches no project root")?;
        let target = contained_target(root, &target)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("couldn't create {}", parent.display()))?;
        }
        std::fs::write(&target, &block.contents)
            .with_context(|| format!("couldn't write {}", target.display()))?;
        written.push(target);
    }
    Ok(written)
}

/// The project's own check, found the way a contributor would: the language's
/// standard build or test entry point.
pub fn project_check_command(root: &Path) -> Option<(String, Vec<String>)> {
    if root.join("Cargo.toml").is_file() {
        return Some((
            "cargo check".into(),
            vec![
                "cargo".into(),
                "check".into(),
                "--workspace".into(),
                "--quiet".into(),
            ],
        ));
    }
    if root.join("package.json").is_file() {
        let scripts = std::fs::read_to_string(root.join("package.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|json| json.get("scripts").cloned());
        if let Some(scripts) = scripts.as_ref().and_then(|value| value.as_object()) {
            for (script, label) in [
                ("test", "npm test"),
                ("build", "npm run build"),
                ("lint", "npm run lint"),
            ] {
                // A placeholder test script fails every project that never
                // wrote tests; skip it rather than fail every turn.
                if let Some(body) = scripts.get(script).and_then(|value| value.as_str())
                    && !body.contains("no test specified")
                {
                    let mut command = vec!["npm".to_string()];
                    if script != "test" {
                        command.push("run".into());
                    }
                    command.push(script.into());
                    if script == "test" {
                        command.push("--".into());
                        command.push("--watch=false".into());
                    }
                    return Some((label.into(), command));
                }
            }
        }
        if root.join("tsconfig.json").is_file() {
            return Some((
                "tsc".into(),
                vec![
                    "npx".into(),
                    "--no-install".into(),
                    "tsc".into(),
                    "--noEmit".into(),
                ],
            ));
        }
        return None;
    }
    if root.join("go.mod").is_file() {
        return Some((
            "go build".into(),
            vec!["go".into(), "build".into(), "./...".into()],
        ));
    }
    if root.join("pyproject.toml").is_file()
        || root.join("pytest.ini").is_file()
        || root.join("setup.py").is_file()
    {
        return Some((
            "pytest".into(),
            vec![
                "python3".into(),
                "-m".into(),
                "pytest".into(),
                "-x".into(),
                "-q".into(),
                "--no-header".into(),
            ],
        ));
    }
    if root.join("tsconfig.json").is_file() {
        return Some((
            "tsc".into(),
            vec![
                "npx".into(),
                "--no-install".into(),
                "tsc".into(),
                "--noEmit".into(),
            ],
        ));
    }
    None
}

/// Runs the check and returns whether it passed with the output's tail.
pub async fn run_check(root: PathBuf, command: Vec<String>) -> (bool, String) {
    let mut process = util::command::new_command(&command[0]);
    process
        .args(&command[1..])
        .current_dir(&root)
        .env("CI", "true")
        .env("NO_COLOR", "1")
        .stdin(std::process::Stdio::null());
    let output = futures::future::select(
        Box::pin(process.output()),
        Box::pin(smol::Timer::after(CHECK_TIME_LIMIT)),
    )
    .await;
    match output {
        futures::future::Either::Left((Ok(output), _)) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            (output.status.success(), tail(&text, 4000).to_string())
        }
        futures::future::Either::Left((Err(error), _)) => {
            (false, format!("the check could not run: {error:#}"))
        }
        futures::future::Either::Right(_) => (
            false,
            format!(
                "the check ran past {} seconds and was abandoned",
                CHECK_TIME_LIMIT.as_secs()
            ),
        ),
    }
}

/// Whether a path is interface code the canvas should show.
pub fn is_interface_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [
        ".html", ".htm", ".css", ".scss", ".jsx", ".tsx", ".vue", ".svelte", ".astro",
    ]
    .iter()
    .any(|extension| lower.ends_with(extension))
        || ((lower.ends_with(".js") || lower.ends_with(".ts")) && !lower.ends_with(".d.ts"))
}

fn tail(text: &str, keep: usize) -> &str {
    let start = text.len().saturating_sub(keep);
    let start = text
        .char_indices()
        .map(|(index, _)| index)
        .find(|index| *index >= start)
        .unwrap_or(0);
    &text[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_blocks_come_out_of_a_reply() {
        let reply = "I made the page.\n\n```html path=myapp/index.html\n<!doctype html>\n<p>hi</p>\n```\n\nand a style:\n\n```css path=myapp/style.css\nbody { margin: 0 }\n```\n\n```rust\nfn not_a_file() {}\n```\n";
        let blocks = parse_file_blocks(reply);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].path, "myapp/index.html");
        assert!(blocks[0].contents.contains("<!doctype html>"));
        assert_eq!(blocks[1].path, "myapp/style.css");
    }

    #[test]
    fn dangerous_paths_are_refused() {
        assert!(parse_file_blocks("```js path=../evil.js\nx\n```").is_empty());
        assert!(parse_file_blocks("```js path=/etc/passwd\nx\n```").is_empty());
        let roots = vec![("myapp".to_string(), PathBuf::from("/tmp/myapp"))];
        assert_eq!(
            resolve_block_path("myapp/src/a.js", &roots),
            Some(PathBuf::from("/tmp/myapp/src/a.js"))
        );
        assert_eq!(
            resolve_block_path("other/a.js", &roots),
            Some(PathBuf::from("/tmp/myapp/other/a.js"))
        );
    }

    #[test]
    fn interface_files_are_recognized() {
        assert!(is_interface_file("src/App.tsx"));
        assert!(is_interface_file("index.html"));
        assert!(!is_interface_file("src/types.d.ts"));
        assert!(!is_interface_file("main.rs"));
    }
}
