//! Where noah keeps what it learns about a project. Everything lives in the
//! project's `.noah` folder, as plain files the person can read, edit, commit
//! or delete; nothing about the project leaves the machine.

use std::path::{Path, PathBuf};

pub const FOLDER: &str = ".noah";

/// Facts and conventions shepherd should remember between conversations.
pub const MEMORY: &str = "memory.md";
/// What the project is for; changes are checked against it for drift.
pub const INTENT: &str = "intent.md";
/// Acceptance criteria, invariants and non-goals.
pub const SPEC: &str = "spec.md";
/// Why the code is the way it is: decisions, incidents and their lessons.
pub const WHY: &str = "why.md";
/// Style and review preferences learned from what the person keeps and
/// rejects. Visible and editable, never hidden.
pub const PREFERENCES: &str = "preferences.md";
/// The structural map of the codebase.
pub const MAP: &str = "map.md";
pub const TOUR: &str = "tour.md";
pub const EVIDENCE: &str = "evidence";
pub const PROVENANCE: &str = "provenance.jsonl";
pub const CALIBRATION: &str = "calibration.jsonl";
pub const QUARANTINE: &str = "quarantine.jsonl";

pub fn path(root: &Path, name: &str) -> PathBuf {
    root.join(FOLDER).join(name)
}

/// The project knowledge files shepherd reads at the start of every
/// conversation, in the order they're shown to it.
pub const CONTEXT_FILES: &[(&str, &str)] = &[
    (INTENT, "intent"),
    (SPEC, "spec"),
    (MEMORY, "memory"),
    (PREFERENCES, "preferences"),
    (WHY, "why log"),
];

/// Longest a context file may be before it's cut, so project knowledge
/// can't crowd out the conversation.
pub const CONTEXT_FILE_LIMIT: usize = 12_000;

pub fn clip(text: &str) -> String {
    if text.len() <= CONTEXT_FILE_LIMIT {
        return text.to_string();
    }
    let mut end = CONTEXT_FILE_LIMIT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n… (cut at {} characters)", &text[..end], CONTEXT_FILE_LIMIT)
}
