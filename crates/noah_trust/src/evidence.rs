//! Evidence bundles: what shepherd hands over with a finished change. It
//! holds the checks that actually ran (collected automatically from the
//! terminal, not typed by the model), each claim with the checks it rests on,
//! the spec clauses the change serves, per-file confidence, and an explicit
//! list of what was not verified. Claims that don't rest on a real, passing
//! check are marked, so a reviewer checks evidence instead of taking the
//! agent's word for it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Check {
    /// Stable id such as `run-3`, which claims cite.
    pub id: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    /// The end of the output, redacted.
    pub output_tail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    /// For repeated runs: how many of the attempts passed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<RepeatResult>,
}

impl Check {
    pub fn passed(&self) -> bool {
        match &self.repeat {
            Some(repeat) => repeat.passed == repeat.attempts,
            None => self.exit_code == Some(0),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RepeatResult {
    pub attempts: u32,
    pub passed: u32,
}

impl RepeatResult {
    pub fn is_flaky(&self) -> bool {
        self.passed > 0 && self.passed < self.attempts
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub text: String,
    /// Check ids (`run-3`), or `doc:<url>` / `file:<path>` for claims resting
    /// on documentation or code that was read.
    #[serde(default)]
    pub grounds: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FileConfidence {
    pub path: String,
    /// 0.0 to 1.0: how sure shepherd is this file's change is right.
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Bundle {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub created: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub checks: Vec<Check>,
    #[serde(default)]
    pub claims: Vec<Claim>,
    #[serde(default)]
    pub not_verified: Vec<String>,
    #[serde(default)]
    pub spec_clauses: Vec<String>,
    #[serde(default)]
    pub files: Vec<FileConfidence>,
    #[serde(default)]
    pub screenshots: Vec<String>,
    #[serde(default)]
    pub behavior_changes: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Grounding {
    Grounded,
    /// No check or source was cited.
    Ungrounded,
    /// A cited check doesn't exist.
    UnknownCheck { id: String },
    /// The claim says something worked, but a cited check failed.
    Contradicted { id: String },
}

const SUCCESS_WORDS: &[&str] = &[
    "pass", "passes", "passed", "passing", "works", "working", "fixed", "succeeds", "succeeded",
    "green", "no errors", "no warnings", "compiles", "builds", "no breaking", "no regressions",
    "verified", "clean",
];

impl Bundle {
    pub fn check(&self, id: &str) -> Option<&Check> {
        self.checks.iter().find(|check| check.id == id)
    }

    pub fn grounding(&self, claim: &Claim) -> Grounding {
        if claim.grounds.is_empty() {
            return Grounding::Ungrounded;
        }
        let lowercase = claim.text.to_lowercase();
        let words: Vec<&str> = lowercase
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .collect();
        let denies_problems = words.iter().any(|word| matches!(*word, "no" | "zero" | "without"))
            && words.iter().any(|word| {
                ["error", "warning", "failure", "issue", "regression", "breaking", "problem"]
                    .iter()
                    .any(|problem| word.starts_with(problem))
            });
        let claims_success =
            denies_problems || SUCCESS_WORDS.iter().any(|word| lowercase.contains(word));
        for ground in &claim.grounds {
            if ground.starts_with("doc:") || ground.starts_with("file:") {
                continue;
            }
            let Some(check) = self.check(ground) else {
                return Grounding::UnknownCheck { id: ground.clone() };
            };
            if claims_success && !check.passed() {
                return Grounding::Contradicted { id: ground.clone() };
            }
        }
        Grounding::Grounded
    }

    pub fn ungrounded_claims(&self) -> Vec<(&Claim, Grounding)> {
        self.claims
            .iter()
            .map(|claim| (claim, self.grounding(claim)))
            .filter(|(_, grounding)| *grounding != Grounding::Grounded)
            .collect()
    }

    pub fn flaky_checks(&self) -> Vec<&Check> {
        self.checks
            .iter()
            .filter(|check| check.repeat.as_ref().is_some_and(RepeatResult::is_flaky))
            .collect()
    }

    /// Files ordered from least to most confident: what a person should look
    /// at first.
    pub fn review_order(&self) -> Vec<&FileConfidence> {
        let mut files: Vec<&FileConfidence> = self.files.iter().collect();
        files.sort_by(|a, b| a.confidence.total_cmp(&b.confidence));
        files
    }

    pub fn to_markdown(&self) -> String {
        let mut out = format!("# {}\n\n", self.title);
        out.push_str(&format!(
            "evidence `{}` · {}{}\n\n",
            self.id,
            self.created,
            self.model
                .as_ref()
                .map(|model| format!(" · written by {model}"))
                .unwrap_or_default()
        ));
        if !self.summary.is_empty() {
            out.push_str(&format!("{}\n\n", self.summary));
        }
        let problems = self.ungrounded_claims();
        if !problems.is_empty() || !self.warnings.is_empty() {
            out.push_str("## needs attention\n\n");
            for (claim, grounding) in &problems {
                let why = match grounding {
                    Grounding::Ungrounded => "no check or source backs this".to_string(),
                    Grounding::UnknownCheck { id } => format!("cites `{id}`, which never ran"),
                    Grounding::Contradicted { id } => format!("`{id}` failed"),
                    Grounding::Grounded => String::new(),
                };
                out.push_str(&format!("- ⚠ \"{}\": {why}\n", claim.text));
            }
            for warning in &self.warnings {
                out.push_str(&format!("- ⚠ {warning}\n"));
            }
            out.push('\n');
        }
        if !self.claims.is_empty() {
            out.push_str("## claims\n\n");
            for claim in &self.claims {
                let mark = if self.grounding(claim) == Grounding::Grounded { "✓" } else { "⚠" };
                let grounds = if claim.grounds.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", claim.grounds.join(", "))
                };
                out.push_str(&format!("- {mark} {}{grounds}\n", claim.text));
            }
            out.push('\n');
        }
        if !self.behavior_changes.is_empty() {
            out.push_str("## behavior changes\n\n");
            for change in &self.behavior_changes {
                out.push_str(&format!("- {change}\n"));
            }
            out.push('\n');
        }
        out.push_str("## not verified\n\n");
        if self.not_verified.is_empty() {
            out.push_str("- nothing listed. shepherd was asked to list what it didn't check; an empty list is itself a claim.\n");
        }
        for item in &self.not_verified {
            out.push_str(&format!("- {item}\n"));
        }
        out.push('\n');
        if !self.files.is_empty() {
            out.push_str("## review first (least confident first)\n\n");
            for file in self.review_order() {
                out.push_str(&format!("- `{}` {:.2}", file.path, file.confidence));
                if !file.note.is_empty() {
                    out.push_str(&format!(": {}", file.note));
                }
                out.push('\n');
            }
            out.push('\n');
        }
        if !self.spec_clauses.is_empty() {
            out.push_str(&format!("## spec\n\nserves {}\n\n", self.spec_clauses.join(", ")));
        }
        if !self.screenshots.is_empty() {
            out.push_str("## screenshots\n\n");
            for screenshot in &self.screenshots {
                out.push_str(&format!("![]({screenshot})\n"));
            }
            out.push('\n');
        }
        out.push_str("## checks that ran\n\n");
        if self.checks.is_empty() {
            out.push_str("none. nothing in this change was executed.\n");
        }
        for check in &self.checks {
            let status = match (&check.repeat, check.exit_code) {
                (Some(repeat), _) if repeat.is_flaky() => {
                    format!("flaky: passed {} of {}", repeat.passed, repeat.attempts)
                }
                (Some(repeat), _) => format!("passed {} of {}", repeat.passed, repeat.attempts),
                (None, Some(0)) => "passed".to_string(),
                (None, Some(code)) => format!("failed (exit {code})"),
                (None, None) => "didn't finish".to_string(),
            };
            out.push_str(&format!(
                "### `{}` {status}\n\n`{}` · {:.1}s\n\n```\n{}\n```\n\n",
                check.id,
                check.command,
                check.duration_ms as f64 / 1000.0,
                check.output_tail.trim_end()
            ));
        }
        out
    }
}

/// A file-name-safe slug for a bundle title.
pub fn slug(title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .take(8)
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() { "change".to_string() } else { slug }
}

/// Keeps the last `max_lines` lines of output, which is where test runners
/// put their summary.
pub fn tail(output: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    let mut kept = lines[start..].join("\n");
    if start > 0 {
        kept = format!("… {start} earlier lines not shown\n{kept}");
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle() -> Bundle {
        Bundle {
            id: "2026-09-25-fix-login".into(),
            title: "Fix login redirect".into(),
            checks: vec![
                Check {
                    id: "run-1".into(),
                    command: "cargo test -p auth".into(),
                    exit_code: Some(0),
                    output_tail: "test result: ok. 12 passed".into(),
                    ..Default::default()
                },
                Check {
                    id: "run-2".into(),
                    command: "cargo clippy".into(),
                    exit_code: Some(1),
                    output_tail: "error: unused variable".into(),
                    ..Default::default()
                },
                Check {
                    id: "run-3".into(),
                    command: "cargo test -p auth login".into(),
                    exit_code: Some(0),
                    repeat: Some(RepeatResult { attempts: 10, passed: 8 }),
                    ..Default::default()
                },
            ],
            claims: vec![
                Claim { text: "auth tests pass".into(), grounds: vec!["run-1".into()] },
                Claim { text: "no lint errors".into(), grounds: vec!["run-2".into()] },
                Claim { text: "the API exists in v2".into(), grounds: vec!["doc:https://x.dev/api".into()] },
                Claim { text: "no breaking changes".into(), grounds: vec![] },
                Claim { text: "e2e passes".into(), grounds: vec!["run-9".into()] },
            ],
            files: vec![
                FileConfidence { path: "src/a.rs".into(), confidence: 0.9, note: String::new() },
                FileConfidence { path: "src/b.rs".into(), confidence: 0.4, note: "timing".into() },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn grounds_claims_in_real_runs() {
        let bundle = bundle();
        let problems = bundle.ungrounded_claims();
        assert_eq!(problems.len(), 3);
        assert_eq!(problems[0].1, Grounding::Contradicted { id: "run-2".into() });
        assert_eq!(problems[1].1, Grounding::Ungrounded);
        assert_eq!(problems[2].1, Grounding::UnknownCheck { id: "run-9".into() });
    }

    #[test]
    fn flags_flaky_runs_and_orders_review() {
        let bundle = bundle();
        assert_eq!(bundle.flaky_checks().len(), 1);
        assert_eq!(bundle.review_order()[0].path, "src/b.rs");
        let markdown = bundle.to_markdown();
        assert!(markdown.contains("flaky: passed 8 of 10"));
        assert!(markdown.contains("## needs attention"));
        assert!(markdown.contains("## not verified"));
    }

    #[test]
    fn helpers() {
        assert_eq!(slug("Fix: login redirect (again)!"), "fix-login-redirect-again");
        assert_eq!(tail("a\nb\nc", 2), "… 1 earlier lines not shown\nb\nc");
    }
}
