//! A project's history as time: what changed, who did it (the person or
//! shepherd), the evidence it shipped with, and how it held up. Everything
//! here is read from records the product already keeps (the provenance
//! chain, the evidence bundles, git); nothing is rendered that was not
//! recorded.

use crate::evidence::{Bundle, Grounding};
use crate::outcomes::Commit;
use crate::provenance::Entry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MomentKind {
    /// One or more edits in a row by the same hand.
    Edits,
    /// An evidence bundle was saved.
    Evidence,
    Commit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeldUp {
    /// A later commit reverted this one.
    Reverted,
    /// A later commit within a week said "fix" and touched the same files.
    FixedSoonAfter,
    Held,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moment {
    /// Unix seconds.
    pub time: i64,
    pub kind: MomentKind,
    /// "person", "shepherd", or a model name when one was recorded.
    pub actor: String,
    pub title: String,
    /// Paths for edits and commits, claims for evidence; capped.
    pub lines: Vec<String>,
    /// The evidence bundle id a moment refers to, when it does.
    pub evidence: Option<String>,
    /// Only for commits with enough history after them to judge.
    pub held: Option<HeldUp>,
}

const MAX_LINES: usize = 6;
/// Edits closer together than this, by the same hand in the same thread,
/// are one moment.
const EDIT_GAP_SECONDS: i64 = 15 * 60;
const WEEK: i64 = 7 * 24 * 60 * 60;

fn unix(rfc3339: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .ok()
        .map(|time| time.timestamp())
}

fn cap(mut lines: Vec<String>, total: usize) -> Vec<String> {
    if total > MAX_LINES {
        lines.truncate(MAX_LINES);
        lines.push(format!("… and {} more", total - MAX_LINES));
    }
    lines
}

fn edit_moments(entries: &[Entry]) -> Vec<Moment> {
    let mut moments: Vec<Moment> = Vec::new();
    let mut group: Vec<&Entry> = Vec::new();
    let flush = |group: &mut Vec<&Entry>, moments: &mut Vec<Moment>| {
        let Some(first) = group.first() else {
            return;
        };
        let Some(time) = unix(&first.time) else {
            group.clear();
            return;
        };
        let mut paths: Vec<String> = Vec::new();
        for entry in group.iter() {
            if let Some(path) = &entry.path
                && !paths.contains(path)
            {
                paths.push(path.clone());
            }
        }
        let count = paths.len();
        let actor = first
            .model
            .clone()
            .filter(|_| first.actor == "shepherd")
            .unwrap_or_else(|| first.actor.clone());
        let title = match count {
            0 => format!("{} {}", first.actor, first.action),
            1 => format!("{} edited {}", first.actor, paths[0]),
            count => format!("{} edited {count} files", first.actor),
        };
        let evidence = group.iter().find_map(|entry| entry.evidence.clone());
        moments.push(Moment {
            time,
            kind: MomentKind::Edits,
            actor,
            title,
            lines: if count > 1 { cap(paths, count) } else { Vec::new() },
            evidence,
            held: None,
        });
        group.clear();
    };
    for entry in entries {
        if entry.action == "evidence" {
            continue;
        }
        let starts_new = match group.last() {
            None => false,
            Some(last) => {
                last.actor != entry.actor
                    || last.thread != entry.thread
                    || match (unix(&last.time), unix(&entry.time)) {
                        (Some(then), Some(now)) => now - then > EDIT_GAP_SECONDS,
                        _ => true,
                    }
            }
        };
        if starts_new {
            flush(&mut group, &mut moments);
        }
        group.push(entry);
    }
    flush(&mut group, &mut moments);
    moments
}

fn evidence_moments(bundles: &[Bundle]) -> Vec<Moment> {
    bundles
        .iter()
        .filter_map(|bundle| {
            let time = unix(&bundle.created)?;
            let grounded = bundle
                .claims
                .iter()
                .filter(|claim| matches!(bundle.grounding(claim), Grounding::Grounded))
                .count();
            let passed = bundle.checks.iter().filter(|check| check.passed()).count();
            let title = format!(
                "evidence: {} ({grounded} of {} claims backed, {passed} of {} commands passed)",
                bundle.title,
                bundle.claims.len(),
                bundle.checks.len()
            );
            let lines: Vec<String> = bundle
                .not_verified
                .iter()
                .map(|item| format!("not verified: {item}"))
                .collect();
            let total = lines.len();
            Some(Moment {
                time,
                kind: MomentKind::Evidence,
                actor: bundle
                    .model
                    .clone()
                    .unwrap_or_else(|| "shepherd".to_string()),
                title,
                lines: cap(lines, total),
                evidence: Some(bundle.id.clone()),
                held: None,
            })
        })
        .collect()
}

fn commit_moments(commits: &[Commit], now: i64) -> Vec<Moment> {
    let reverted: Vec<&str> = commits.iter().filter_map(Commit::reverted_hash).collect();
    commits
        .iter()
        .map(|commit| {
            let model = commit.shepherd_model();
            let actor = model.clone().unwrap_or_else(|| "person".to_string());
            let was_reverted = reverted
                .iter()
                .any(|hash| commit.hash.starts_with(hash) || hash.starts_with(&commit.hash));
            let fixed_soon_after = commits.iter().any(|later| {
                later.time > commit.time
                    && later.time - commit.time <= WEEK
                    && later.subject.to_lowercase().contains("fix")
                    && later.files.iter().any(|file| commit.files.contains(file))
            });
            let held = if was_reverted {
                Some(HeldUp::Reverted)
            } else if fixed_soon_after {
                Some(HeldUp::FixedSoonAfter)
            } else if now - commit.time >= WEEK {
                Some(HeldUp::Held)
            } else {
                None
            };
            let short: String = commit.hash.chars().take(7).collect();
            let total = commit.files.len();
            Moment {
                time: commit.time,
                kind: MomentKind::Commit,
                actor,
                title: format!(
                    "{} committed {short}: {}",
                    if model.is_some() { "shepherd" } else { "person" },
                    commit.subject
                ),
                lines: cap(commit.files.clone(), total),
                evidence: commit.body.lines().find_map(|line| {
                    line.trim()
                        .strip_prefix("Noah-Evidence:")
                        .map(|rest| rest.trim().to_string())
                }),
                held,
            }
        })
        .collect()
}

/// Every moment, newest first.
pub fn build(entries: &[Entry], bundles: &[Bundle], commits: &[Commit], now: i64) -> Vec<Moment> {
    let mut moments = edit_moments(entries);
    moments.extend(evidence_moments(bundles));
    moments.extend(commit_moments(commits, now));
    moments.sort_by(|a, b| b.time.cmp(&a.time));
    moments
}

/// The day a moment belongs to, in the local zone, for grouping.
pub fn day_of(moment: &Moment) -> String {
    chrono::DateTime::from_timestamp(moment.time, 0)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_default()
}

pub fn clock_of(moment: &Moment) -> String {
    chrono::DateTime::from_timestamp(moment.time, 0)
        .map(|time| time.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(time: &str, actor: &str, path: &str, thread: &str) -> Entry {
        Entry {
            time: time.to_string(),
            actor: actor.to_string(),
            action: "edit_file".to_string(),
            path: Some(path.to_string()),
            lines: None,
            model: (actor == "shepherd").then(|| "gpt-x".to_string()),
            thread: Some(thread.to_string()),
            prompt_sha256: None,
            evidence: None,
            detail: None,
            previous: String::new(),
            hash: String::new(),
        }
    }

    #[test]
    fn close_edits_by_one_hand_are_one_moment() {
        let entries = vec![
            entry("2026-10-01T10:00:00Z", "shepherd", "a.rs", "t1"),
            entry("2026-10-01T10:05:00Z", "shepherd", "b.rs", "t1"),
            entry("2026-10-01T10:06:00Z", "person", "c.rs", "t1"),
            entry("2026-10-01T12:00:00Z", "shepherd", "a.rs", "t1"),
        ];
        let moments = build(&entries, &[], &[], 1_800_000_000);
        assert_eq!(moments.len(), 3, "{moments:?}");
        assert_eq!(moments[2].title, "shepherd edited 2 files");
        assert_eq!(moments[2].actor, "gpt-x");
        assert_eq!(moments[1].title, "person edited c.rs");
        assert!(moments[0].time > moments[1].time);
    }

    #[test]
    fn commits_say_how_they_held_up() {
        let commits = vec![
            Commit {
                hash: "aaa1111".into(),
                time: 1000,
                subject: "Add cache".into(),
                body: "Assisted-by: shepherd (gpt-x)\nNoah-Evidence: 20261001-cache".into(),
                files: vec!["src/cache.rs".into()],
            },
            Commit {
                hash: "bbb2222".into(),
                time: 2000,
                subject: "Revert \"Add cache\"".into(),
                body: "This reverts commit aaa1111.".into(),
                files: vec!["src/cache.rs".into()],
            },
            Commit {
                hash: "ccc3333".into(),
                time: 3000,
                subject: "Add login".into(),
                body: String::new(),
                files: vec!["src/login.rs".into()],
            },
        ];
        let moments = build(&[], &[], &commits, 3000 + WEEK);
        let cache = moments.iter().find(|m| m.title.contains("aaa1111")).unwrap();
        assert_eq!(cache.held, Some(HeldUp::Reverted));
        assert_eq!(cache.actor, "gpt-x");
        assert_eq!(cache.evidence.as_deref(), Some("20261001-cache"));
        let login = moments.iter().find(|m| m.title.contains("ccc3333")).unwrap();
        assert_eq!(login.held, Some(HeldUp::Held));
        assert_eq!(login.actor, "person");
    }
}
