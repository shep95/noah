//! Honest outcome numbers from git history instead of "lines generated":
//! how many of shepherd's commits landed, how many were later reverted, and
//! how soon follow-up fixes touched the same files, per model.
//!
//! shepherd marks its commits with an `Assisted-by: shepherd (<model>)`
//! trailer, which is how its work is told apart from the person's.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `git log` format these functions read: fields separated by 0x1f, commits
/// ending with 0x1e, then the changed files.
pub const GIT_LOG_FORMAT: &str = "--format=%x1e%H%x1f%ct%x1f%s%x1f%b%x1f";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Commit {
    pub hash: String,
    pub time: i64,
    pub subject: String,
    pub body: String,
    pub files: Vec<String>,
}

impl Commit {
    /// The model named in the commit's `Assisted-by: shepherd (...)` trailer.
    pub fn shepherd_model(&self) -> Option<String> {
        self.body.lines().find_map(|line| {
            let rest = line.trim().strip_prefix("Assisted-by:")?.trim();
            let rest = rest.strip_prefix("shepherd")?.trim();
            Some(
                rest.trim_start_matches('(')
                    .trim_end_matches(')')
                    .trim()
                    .to_string(),
            )
            .map(|model| if model.is_empty() { "unknown model".to_string() } else { model })
        })
    }

    fn reverted_hash(&self) -> Option<&str> {
        let marker = "This reverts commit ";
        let start = self.body.find(marker)? + marker.len();
        let hash = self.body[start..].split(|c: char| !c.is_ascii_hexdigit()).next()?;
        (hash.len() >= 7).then_some(hash)
    }
}

/// Reads `git log --name-only` output written with [`GIT_LOG_FORMAT`].
pub fn parse_git_log(output: &str) -> Vec<Commit> {
    output
        .split('\u{1e}')
        .filter_map(|record| {
            let mut fields = record.splitn(5, '\u{1f}');
            let hash = fields.next()?.trim().to_string();
            if hash.is_empty() {
                return None;
            }
            let time = fields.next()?.trim().parse().ok()?;
            let subject = fields.next()?.to_string();
            let body = fields.next()?.to_string();
            let files = fields
                .next()
                .unwrap_or_default()
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect();
            Some(Commit {
                hash,
                time,
                subject,
                body,
                files,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelOutcomes {
    pub commits: usize,
    pub reverted: usize,
    /// Commits followed within a week by a "fix" commit touching the same files.
    pub fixed_soon_after: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Outcomes {
    pub total_commits: usize,
    pub shepherd_commits: usize,
    pub by_model: BTreeMap<String, ModelOutcomes>,
}

impl Outcomes {
    pub fn to_markdown(&self) -> String {
        let mut out = format!(
            "# outcomes\n\n{} of {} commits were written with shepherd.\n\n",
            self.shepherd_commits, self.total_commits
        );
        if self.by_model.is_empty() {
            out.push_str("No shepherd commits yet. shepherd adds an `Assisted-by: shepherd (<model>)` trailer to commits it makes, which is how they're counted.\n");
            return out;
        }
        out.push_str("| model | commits | reverted | fixed within a week |\n|---|---|---|---|\n");
        for (model, outcomes) in &self.by_model {
            let percent = |count: usize| {
                if outcomes.commits == 0 {
                    0.0
                } else {
                    count as f64 * 100.0 / outcomes.commits as f64
                }
            };
            out.push_str(&format!(
                "| {model} | {} | {} ({:.0}%) | {} ({:.0}%) |\n",
                outcomes.commits,
                outcomes.reverted,
                percent(outcomes.reverted),
                outcomes.fixed_soon_after,
                percent(outcomes.fixed_soon_after)
            ));
        }
        out
    }
}

pub fn measure(commits: &[Commit]) -> Outcomes {
    const WEEK: i64 = 7 * 24 * 60 * 60;
    let reverted: Vec<&str> = commits.iter().filter_map(Commit::reverted_hash).collect();
    let mut outcomes = Outcomes {
        total_commits: commits.len(),
        ..Default::default()
    };
    for commit in commits {
        let Some(model) = commit.shepherd_model() else {
            continue;
        };
        outcomes.shepherd_commits += 1;
        let entry = outcomes.by_model.entry(model).or_default();
        entry.commits += 1;
        if reverted
            .iter()
            .any(|hash| commit.hash.starts_with(hash) || hash.starts_with(&commit.hash))
        {
            entry.reverted += 1;
        }
        let fixed_soon_after = commits.iter().any(|later| {
            later.time > commit.time
                && later.time - commit.time <= WEEK
                && later.subject.to_lowercase().contains("fix")
                && later.files.iter().any(|file| commit.files.contains(file))
        });
        if fixed_soon_after {
            entry.fixed_soon_after += 1;
        }
    }
    outcomes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_landed_reverted_and_fixed_work() {
        let log = "\u{1e}aaa1111\u{1f}1000\u{1f}Add login\u{1f}Assisted-by: shepherd (venice/qwen3-coder)\n\u{1f}\nsrc/login.rs\n\
\u{1e}bbb2222\u{1f}2000\u{1f}Fix login redirect\u{1f}\u{1f}\nsrc/login.rs\n\
\u{1e}ccc3333\u{1f}3000\u{1f}Add cache\u{1f}Assisted-by: shepherd (venice/qwen3-coder)\n\u{1f}\nsrc/cache.rs\n\
\u{1e}ddd4444\u{1f}4000\u{1f}Revert \"Add cache\"\u{1f}This reverts commit ccc3333.\n\u{1f}\nsrc/cache.rs\n\
\u{1e}eee5555\u{1f}5000\u{1f}Docs\u{1f}\u{1f}\nREADME.md\n";
        let commits = parse_git_log(log);
        assert_eq!(commits.len(), 5);
        let outcomes = measure(&commits);
        assert_eq!(outcomes.shepherd_commits, 2);
        let model = &outcomes.by_model["venice/qwen3-coder"];
        assert_eq!(model.reverted, 1);
        assert_eq!(model.fixed_soon_after, 1);
        assert!(outcomes.to_markdown().contains("| venice/qwen3-coder | 2 | 1 (50%) | 1 (50%) |"));
    }
}
