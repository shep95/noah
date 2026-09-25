//! A tamper-evident record of what shepherd changed: every edit, file write,
//! delete and command, with the model, the conversation and the prompt that
//! led to it. Each entry carries the hash of the one before it, so editing or
//! removing a past entry breaks the chain, and [`verify`] says where.

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::io::Write as _;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// RFC 3339.
    pub time: String,
    /// "shepherd" or "person".
    pub actor: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// 1-based inclusive line range the change covers in the new file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<(u32, u32)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    /// SHA-256 of the person's prompt that started the turn, so lineage can
    /// be checked without storing the prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub previous: String,
    #[serde(default)]
    pub hash: String,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn entry_hash(entry: &Entry) -> Result<String> {
    let mut unhashed = entry.clone();
    unhashed.hash = String::new();
    Ok(sha256_hex(&serde_json::to_vec(&unhashed)?))
}

/// Appends an entry, chaining it to the last one in the log.
pub fn append(log: &Path, mut entry: Entry) -> Result<Entry> {
    if let Some(directory) = log.parent() {
        std::fs::create_dir_all(directory)?;
    }
    entry.previous = last_hash(log)?.unwrap_or_default();
    entry.hash = entry_hash(&entry)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .with_context(|| format!("couldn't open {}", log.display()))?;
    writeln!(file, "{}", serde_json::to_string(&entry)?)?;
    Ok(entry)
}

fn last_hash(log: &Path) -> Result<Option<String>> {
    let Ok(text) = std::fs::read_to_string(log) else {
        return Ok(None);
    };
    let Some(line) = text.lines().rev().find(|line| !line.trim().is_empty()) else {
        return Ok(None);
    };
    let entry: Entry = serde_json::from_str(line).context("the provenance log's last entry is unreadable")?;
    Ok(Some(entry.hash))
}

pub fn read(log: &Path) -> Result<Vec<Entry>> {
    let text = match std::fs::read_to_string(log) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .map(|(index, line)| {
            serde_json::from_str(line)
                .with_context(|| format!("provenance entry {} is unreadable", index + 1))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Integrity {
    Intact { entries: usize },
    /// The first entry (1-based) whose hash or link doesn't match.
    Broken { entry: usize, reason: String },
}

pub fn verify(log: &Path) -> Result<Integrity> {
    let entries = read(log)?;
    let mut previous = String::new();
    for (index, entry) in entries.iter().enumerate() {
        if entry.previous != previous {
            return Ok(Integrity::Broken {
                entry: index + 1,
                reason: "it doesn't link to the entry before it, so an entry was removed or reordered".into(),
            });
        }
        if entry_hash(entry)? != entry.hash {
            return Ok(Integrity::Broken {
                entry: index + 1,
                reason: "its contents changed after it was written".into(),
            });
        }
        previous = entry.hash.clone();
    }
    Ok(Integrity::Intact {
        entries: entries.len(),
    })
}

/// The entries that touched `line` of `path`, newest first: who or what
/// wrote it. Paths are compared as written in the log.
pub fn history_of_line<'a>(entries: &'a [Entry], path: &str, line: u32) -> Vec<&'a Entry> {
    entries
        .iter()
        .rev()
        .filter(|entry| entry.path.as_deref() == Some(path))
        .filter(|entry| {
            entry
                .lines
                .is_none_or(|(start, end)| start <= line && line <= end)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(path: &str, lines: (u32, u32)) -> Entry {
        Entry {
            time: "2026-09-25T10:00:00Z".into(),
            actor: "shepherd".into(),
            action: "edit".into(),
            path: Some(path.into()),
            lines: Some(lines),
            model: Some("venice/qwen3-coder".into()),
            ..Default::default()
        }
    }

    #[test]
    fn chains_and_verifies() {
        let directory = tempfile::tempdir().expect("tempdir");
        let log = directory.path().join(".noah/provenance.jsonl");
        let first = append(&log, edit("src/a.rs", (1, 5))).expect("append");
        let second = append(&log, edit("src/a.rs", (3, 3))).expect("append");
        assert_eq!(second.previous, first.hash);
        assert_eq!(verify(&log).expect("verify"), Integrity::Intact { entries: 2 });

        let entries = read(&log).expect("read");
        assert_eq!(history_of_line(&entries, "src/a.rs", 3).len(), 2);
        assert_eq!(history_of_line(&entries, "src/a.rs", 5).len(), 1);
    }

    #[test]
    fn detects_tampering() {
        let directory = tempfile::tempdir().expect("tempdir");
        let log = directory.path().join("provenance.jsonl");
        append(&log, edit("src/a.rs", (1, 5))).expect("append");
        append(&log, edit("src/b.rs", (1, 1))).expect("append");
        append(&log, edit("src/c.rs", (1, 1))).expect("append");

        let text = std::fs::read_to_string(&log).expect("read");
        std::fs::write(&log, text.replacen("src/b.rs", "src/x.rs", 1)).expect("write");
        assert!(matches!(verify(&log).expect("verify"), Integrity::Broken { entry: 2, .. }));

        let lines: Vec<&str> = text.lines().collect();
        std::fs::write(&log, format!("{}\n{}\n", lines[0], lines[2])).expect("write");
        assert!(matches!(verify(&log).expect("verify"), Integrity::Broken { entry: 2, .. }));
    }
}
