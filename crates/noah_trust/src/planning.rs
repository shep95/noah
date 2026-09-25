//! Splits parallel work so agents don't collide. Each task names the files or
//! folders it expects to touch; tasks that overlap, directly or through files
//! that history shows always change together, are predicted to conflict and
//! are put in different waves. Tasks within a wave can run at the same time.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub name: String,
    /// Paths relative to the project root; a path ending in `/` covers the
    /// whole folder.
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conflict {
    pub first: String,
    pub second: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub waves: Vec<Vec<String>>,
    pub conflicts: Vec<Conflict>,
}

/// Pairs of files that changed together in at least `threshold` commits,
/// read from `git log --name-only --format=%x1e` output.
pub fn co_changes(git_log: &str, threshold: usize) -> BTreeMap<(String, String), usize> {
    let mut counts = BTreeMap::new();
    for commit in git_log.split('\u{1e}') {
        let files: BTreeSet<&str> = commit
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        if files.len() > 30 {
            continue;
        }
        let files: Vec<&str> = files.into_iter().collect();
        for (index, first) in files.iter().enumerate() {
            for second in &files[index + 1..] {
                *counts
                    .entry((first.to_string(), second.to_string()))
                    .or_insert(0) += 1;
            }
        }
    }
    counts.retain(|_, count| *count >= threshold);
    counts
}

fn overlaps(first: &str, second: &str) -> bool {
    let covers = |folder: &str, path: &str| folder.ends_with('/') && path.starts_with(folder);
    first == second || covers(first, second) || covers(second, first)
}

fn conflict_between(
    first: &Task,
    second: &Task,
    co_changes: &BTreeMap<(String, String), usize>,
) -> Option<String> {
    for left in &first.paths {
        for right in &second.paths {
            if overlaps(left, right) {
                return Some(format!("both touch {}", if left.len() <= right.len() { left } else { right }));
            }
        }
    }
    for ((a, b), count) in co_changes {
        let touches = |task: &Task, file: &str| task.paths.iter().any(|path| overlaps(path, file));
        if (touches(first, a) && touches(second, b)) || (touches(first, b) && touches(second, a)) {
            return Some(format!("{a} and {b} changed together in {count} commits"));
        }
    }
    None
}

/// Orders tasks into waves with no predicted conflicts inside a wave,
/// keeping the given order as the priority.
pub fn plan(tasks: &[Task], co_changes: &BTreeMap<(String, String), usize>) -> Plan {
    let mut conflicts = Vec::new();
    let mut conflicting: BTreeSet<(usize, usize)> = BTreeSet::new();
    for (i, first) in tasks.iter().enumerate() {
        for (j, second) in tasks.iter().enumerate().skip(i + 1) {
            if let Some(reason) = conflict_between(first, second, co_changes) {
                conflicting.insert((i, j));
                conflicts.push(Conflict {
                    first: first.name.clone(),
                    second: second.name.clone(),
                    reason,
                });
            }
        }
    }
    let mut waves: Vec<Vec<usize>> = Vec::new();
    for task in 0..tasks.len() {
        let wave = waves.iter_mut().find(|wave| {
            wave.iter().all(|other| {
                !conflicting.contains(&((*other).min(task), (*other).max(task)))
            })
        });
        match wave {
            Some(wave) => wave.push(task),
            None => waves.push(vec![task]),
        }
    }
    Plan {
        waves: waves
            .into_iter()
            .map(|wave| wave.into_iter().filter_map(|index| Some(tasks.get(index)?.name.clone())).collect())
            .collect(),
        conflicts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(name: &str, paths: &[&str]) -> Task {
        Task {
            name: name.into(),
            paths: paths.iter().map(|path| path.to_string()).collect(),
        }
    }

    #[test]
    fn separates_overlapping_work() {
        let tasks = [
            task("api", &["server/routes/"]),
            task("docs", &["docs/api.md"]),
            task("auth", &["server/routes/auth.rs"]),
            task("ui", &["web/login.tsx"]),
        ];
        let history = "server/models/user.rs\nweb/login.tsx\n\u{1e}server/models/user.rs\nweb/login.tsx\n\u{1e}README.md\n";
        let co = co_changes(history, 2);
        let mut tasks = tasks.to_vec();
        tasks.push(task("models", &["server/models/user.rs"]));
        let plan = plan(&tasks, &co);
        assert_eq!(plan.waves, vec![vec!["api", "docs", "ui"], vec!["auth", "models"]]);
        assert_eq!(plan.conflicts.len(), 2);
        assert!(plan.conflicts[1].reason.contains("changed together in 2 commits"));
    }
}
