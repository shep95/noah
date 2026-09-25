//! A compiler for agent instructions. AGENTS.md, CLAUDE.md, .rules, cursor
//! rules and similar files are read together and checked the way code is:
//! rules that contradict each other, rules repeated across files, paths that
//! no longer exist, and files too long for an agent to follow reliably.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Files agents read for instructions, relative to a project root.
pub const INSTRUCTION_FILES: &[&str] = &[
    "AGENTS.md",
    "CLAUDE.md",
    ".rules",
    ".cursorrules",
    ".windsurfrules",
    ".clinerules",
    "GEMINI.md",
    ".github/copilot-instructions.md",
    ".noah/memory.md",
    ".noah/preferences.md",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub file: String,
    pub line: usize,
    pub text: String,
    pub polarity: Polarity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Polarity {
    Do,
    Dont,
    Neutral,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Finding {
    Contradiction { first: Rule, second: Rule },
    Duplicate { first: Rule, second: Rule },
    MissingPath { rule: Rule, path: String },
    TooLong { file: String, lines: usize },
}

const DONT_WORDS: &[&str] = &["never", "don't", "do not", "avoid", "must not", "mustn't", "no longer", "stop", "without"];
const DO_WORDS: &[&str] = &["always", "must", "use", "prefer", "should", "ensure", "do "];
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "to", "of", "in", "on", "for", "and", "or", "is", "are", "be", "it", "this",
    "that", "with", "when", "you", "your", "we", "our", "any", "all", "as", "by", "at", "from",
    "never", "don't", "do", "not", "avoid", "must", "mustn't", "always", "use", "prefer", "should",
    "ensure", "stop", "no", "longer", "without", "instead", "please",
];

/// Pulls rules (bullets and imperative sentences) out of an instruction file.
pub fn extract_rules(file: &str, text: &str) -> Vec<Rule> {
    let mut in_code = false;
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                in_code = !in_code;
                return None;
            }
            if in_code || trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let body = trimmed
                .trim_start_matches(['-', '*', '+'])
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')')
                .trim();
            if body.split_whitespace().count() < 3 {
                return None;
            }
            let lowercase = body.to_lowercase();
            let polarity = if DONT_WORDS.iter().any(|word| contains_word(&lowercase, word)) {
                Polarity::Dont
            } else if DO_WORDS.iter().any(|word| contains_word(&lowercase, word.trim())) {
                Polarity::Do
            } else {
                Polarity::Neutral
            };
            Some(Rule {
                file: file.to_string(),
                line: index + 1,
                text: body.to_string(),
                polarity,
            })
        })
        .collect()
}

fn contains_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(start, _)| {
        let before = text[..start].chars().last();
        let after = text[start + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

fn content_words(text: &str) -> BTreeSet<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '\'' || c == '.'))
        .map(|word| word.trim_matches('.').trim_matches('\''))
        .filter(|word| word.len() > 1 && !STOP_WORDS.contains(word))
        .map(str::to_string)
        .collect()
}

fn similarity(first: &str, second: &str) -> f32 {
    let first = content_words(first);
    let second = content_words(second);
    if first.is_empty() || second.is_empty() {
        return 0.0;
    }
    let shared = first.intersection(&second).count() as f32;
    shared / first.union(&second).count() as f32
}

fn backticked_paths(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .filter(|span| {
            let has_extension = span.rsplit_once('.').is_some_and(|(stem, extension)| {
                !stem.is_empty()
                    && (1..=5).contains(&extension.len())
                    && extension.chars().all(|c| c.is_ascii_alphanumeric())
            });
            (span.contains('/') || has_extension)
                && !span.contains(' ')
                && !span.contains('(')
                && !span.starts_with('-')
                && !span.contains("://")
                && span.chars().any(|c| c.is_alphabetic())
        })
        .map(str::to_string)
        .collect()
}

/// Checks instruction files together. `path_exists` resolves paths the rules
/// mention against the project.
pub fn check(files: &[(String, String)], path_exists: impl Fn(&str) -> bool) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut rules = Vec::new();
    for (file, text) in files {
        let lines = text.lines().count();
        if lines > 400 {
            findings.push(Finding::TooLong {
                file: file.clone(),
                lines,
            });
        }
        rules.extend(extract_rules(file, text));
    }
    for (index, first) in rules.iter().enumerate() {
        for second in &rules[index + 1..] {
            let score = similarity(&first.text, &second.text);
            let opposite = matches!(
                (first.polarity, second.polarity),
                (Polarity::Do, Polarity::Dont) | (Polarity::Dont, Polarity::Do)
            );
            if opposite && score >= 0.5 {
                findings.push(Finding::Contradiction {
                    first: first.clone(),
                    second: second.clone(),
                });
            } else if !opposite && score >= 0.85 && first.file != second.file {
                findings.push(Finding::Duplicate {
                    first: first.clone(),
                    second: second.clone(),
                });
            }
        }
        for path in backticked_paths(&first.text) {
            if path.contains('*') || path.contains('<') || path.contains('{') {
                continue;
            }
            if !path_exists(&path) {
                findings.push(Finding::MissingPath {
                    rule: first.clone(),
                    path,
                });
            }
        }
    }
    findings
}

pub fn report(findings: &[Finding], files_checked: usize, rules_found: usize) -> String {
    let mut out = format!(
        "# agent instructions check\n\n{files_checked} file(s), {rules_found} rule(s), {} finding(s)\n\n",
        findings.len()
    );
    for finding in findings {
        match finding {
            Finding::Contradiction { first, second } => out.push_str(&format!(
                "- contradiction: {}:{} \"{}\" vs {}:{} \"{}\"\n",
                first.file, first.line, first.text, second.file, second.line, second.text
            )),
            Finding::Duplicate { first, second } => out.push_str(&format!(
                "- duplicate: {}:{} repeats {}:{} \"{}\"\n",
                second.file, second.line, first.file, first.line, first.text
            )),
            Finding::MissingPath { rule, path } => out.push_str(&format!(
                "- stale path: {}:{} mentions `{path}`, which doesn't exist\n",
                rule.file, rule.line
            )),
            Finding::TooLong { file, lines } => out.push_str(&format!(
                "- too long: {file} has {lines} lines; agents follow short, specific rules more reliably\n"
            )),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_contradictions_duplicates_and_stale_paths() {
        let files = vec![
            (
                "AGENTS.md".to_string(),
                "# rules\n- Always use tabs for indentation in Rust files.\n- Run tests with `script/test` before committing.\n```\nnever run this\n```\n".to_string(),
            ),
            (
                ".rules".to_string(),
                "- Never use tabs for indentation in Rust files.\n- Run tests with `script/test` before committing.\n".to_string(),
            ),
        ];
        let findings = check(&files, |path| path != "script/test");
        let kinds: Vec<&str> = findings
            .iter()
            .map(|finding| match finding {
                Finding::Contradiction { .. } => "contradiction",
                Finding::Duplicate { .. } => "duplicate",
                Finding::MissingPath { .. } => "missing",
                Finding::TooLong { .. } => "long",
            })
            .collect();
        assert_eq!(kinds, ["contradiction", "duplicate", "missing", "missing"]);
    }

    #[test]
    fn unrelated_rules_are_fine() {
        let files = vec![(
            "AGENTS.md".to_string(),
            "- Never commit secrets to the repository.\n- Always write tests for new parsers.\n".to_string(),
        )];
        assert!(check(&files, |_| true).is_empty());
    }
}
