//! The project's spec as a first-class object: `.noah/spec.md` holds
//! acceptance criteria, invariants and non-goals, each with a stable id
//! (`AC-1`, `INV-2`, `NG-1`). Evidence bundles cite the clauses a change
//! serves, and review happens against them.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClauseKind {
    Acceptance,
    Invariant,
    NonGoal,
}

impl ClauseKind {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Acceptance => "AC",
            Self::Invariant => "INV",
            Self::NonGoal => "NG",
        }
    }

    pub fn heading(self) -> &'static str {
        match self {
            Self::Acceptance => "acceptance criteria",
            Self::Invariant => "invariants",
            Self::NonGoal => "non-goals",
        }
    }

    fn from_heading(heading: &str) -> Option<Self> {
        let heading = heading.to_lowercase();
        if heading.contains("accept") || heading.contains("criteria") || heading.contains("requirement") {
            Some(Self::Acceptance)
        } else if heading.contains("invariant") || heading.contains("must always") {
            Some(Self::Invariant)
        } else if heading.contains("non-goal") || heading.contains("non goal") || heading.contains("out of scope") {
            Some(Self::NonGoal)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clause {
    pub id: String,
    pub kind: ClauseKind,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Spec {
    pub title: String,
    pub goal: String,
    pub clauses: Vec<Clause>,
}

impl Spec {
    /// Reads a spec written as markdown. Bullets under an "acceptance
    /// criteria", "invariants" or "non-goals" heading become clauses; a bullet
    /// may start with its own id (`- AC-3: ...`), otherwise one is assigned.
    pub fn parse(markdown: &str) -> Self {
        let mut spec = Spec::default();
        let mut section: Option<ClauseKind> = None;
        let mut in_goal = false;
        for line in markdown.lines() {
            let trimmed = line.trim();
            if let Some(heading) = trimmed.strip_prefix('#') {
                let heading = heading.trim_start_matches('#').trim();
                if trimmed.starts_with("# ") && spec.title.is_empty() {
                    spec.title = heading.to_string();
                    continue;
                }
                section = ClauseKind::from_heading(heading);
                in_goal = heading.to_lowercase().contains("goal") && section.is_none();
                continue;
            }
            let bullet = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .or_else(|| {
                    let digits = trimmed.find(". ")?;
                    trimmed[..digits]
                        .chars()
                        .all(|c| c.is_ascii_digit())
                        .then(|| &trimmed[digits + 2..])
                });
            match (section, bullet) {
                (Some(kind), Some(text)) => {
                    let (id, text) = split_id(text, kind).unwrap_or_else(|| {
                        let next = spec.clauses.iter().filter(|clause| clause.kind == kind).count() + 1;
                        (format!("{}-{next}", kind.prefix()), text.trim().to_string())
                    });
                    spec.clauses.push(Clause { id, kind, text });
                }
                (None, _) if in_goal && !trimmed.is_empty() => {
                    if !spec.goal.is_empty() {
                        spec.goal.push(' ');
                    }
                    spec.goal.push_str(trimmed);
                }
                _ => {}
            }
        }
        spec
    }

    pub fn clause(&self, id: &str) -> Option<&Clause> {
        self.clauses
            .iter()
            .find(|clause| clause.id.eq_ignore_ascii_case(id))
    }

    /// Clause ids cited somewhere that the spec doesn't have.
    pub fn unknown_ids<'a>(&self, cited: &'a [String]) -> Vec<&'a str> {
        cited
            .iter()
            .map(String::as_str)
            .filter(|id| self.clause(id).is_none())
            .collect()
    }

    /// Acceptance criteria no evidence bundle has cited yet.
    pub fn uncovered<'a>(&'a self, cited: &[String]) -> Vec<&'a Clause> {
        self.clauses
            .iter()
            .filter(|clause| clause.kind == ClauseKind::Acceptance)
            .filter(|clause| !cited.iter().any(|id| id.eq_ignore_ascii_case(&clause.id)))
            .collect()
    }

    pub fn to_markdown(&self) -> String {
        let mut out = format!(
            "# {}\n\n",
            if self.title.is_empty() { "spec" } else { &self.title }
        );
        if !self.goal.is_empty() {
            out.push_str(&format!("## goal\n\n{}\n\n", self.goal));
        }
        for kind in [ClauseKind::Acceptance, ClauseKind::Invariant, ClauseKind::NonGoal] {
            out.push_str(&format!("## {}\n\n", kind.heading()));
            for clause in self.clauses.iter().filter(|clause| clause.kind == kind) {
                out.push_str(&format!("- {}: {}\n", clause.id, clause.text));
            }
            out.push('\n');
        }
        out
    }

    /// Adds a clause with the next free id and returns the id.
    pub fn add(&mut self, kind: ClauseKind, text: &str) -> String {
        let highest = self
            .clauses
            .iter()
            .filter(|clause| clause.kind == kind)
            .filter_map(|clause| clause.id.rsplit('-').next()?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        let id = format!("{}-{}", kind.prefix(), highest + 1);
        let position = self
            .clauses
            .iter()
            .rposition(|clause| clause.kind == kind)
            .map_or(self.clauses.len(), |last| last + 1);
        self.clauses.insert(
            position,
            Clause {
                id: id.clone(),
                kind,
                text: text.trim().to_string(),
            },
        );
        id
    }
}

fn split_id(text: &str, kind: ClauseKind) -> Option<(String, String)> {
    let (id, rest) = text.split_once(':')?;
    let id = id.trim().trim_matches('*').trim_matches('`');
    let (prefix, number) = id.split_once('-')?;
    (prefix.eq_ignore_ascii_case(kind.prefix()) && number.chars().all(|c| c.is_ascii_digit()))
        .then(|| (id.to_uppercase(), rest.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = "# checkout\n\n## goal\n\nPeople can pay with a saved card.\n\n## acceptance criteria\n\n- AC-1: a saved card can be charged\n- declined cards show the bank's reason\n\n## invariants\n\n1. amounts are integers in cents\n\n## non-goals\n\n* refunds\n";

    #[test]
    fn parses_clauses() {
        let spec = Spec::parse(SPEC);
        assert_eq!(spec.title, "checkout");
        assert_eq!(spec.goal, "People can pay with a saved card.");
        let ids: Vec<&str> = spec.clauses.iter().map(|clause| clause.id.as_str()).collect();
        assert_eq!(ids, ["AC-1", "AC-2", "INV-1", "NG-1"]);
        assert_eq!(spec.clause("inv-1").map(|clause| clause.text.as_str()), Some("amounts are integers in cents"));
    }

    #[test]
    fn tracks_coverage_and_round_trips() {
        let mut spec = Spec::parse(SPEC);
        let cited = vec!["AC-1".to_string(), "AC-7".to_string()];
        assert_eq!(spec.unknown_ids(&cited), ["AC-7"]);
        assert_eq!(spec.uncovered(&cited).len(), 1);
        assert_eq!(spec.add(ClauseKind::Acceptance, "receipts are emailed"), "AC-3");
        let reparsed = Spec::parse(&spec.to_markdown());
        assert_eq!(reparsed, spec);
    }
}
