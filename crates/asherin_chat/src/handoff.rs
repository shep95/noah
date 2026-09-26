//! "take this to code": a chat's decisions become a short summary and
//! proposed acceptance criteria in the project's `.noah/spec.md`, and a
//! shepherd-room thread starts from that summary instead of the whole chat.

use noah_trust::spec::{ClauseKind, Spec};

/// What the summary model is asked after the chat's messages.
pub const HANDOFF_PROMPT: &str = "the conversation above is being handed to shepherd, a coding \
agent that will work on it in the person's project. reply in exactly this format and nothing \
else:\n\nsummary:\n- <one decision per line, with its reason. include constraints and open \
questions. leave out options that were considered and dropped.>\n\nclauses:\n- <one testable \
acceptance criterion per line: behavior a reviewer can observe, not how it's built. at most \
8.>\n\nwrite in lowercase.";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Handoff {
    pub summary: String,
    pub clauses: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Preamble,
    Summary,
    Clauses,
}

/// Reads the summary model's reply to [`HANDOFF_PROMPT`]. A reply that
/// ignored the format becomes the summary as is, with no clauses.
pub fn parse_handoff(reply: &str) -> Option<Handoff> {
    let mut section = Section::Preamble;
    let mut preamble = Vec::new();
    let mut summary = Vec::new();
    let mut clauses = Vec::new();
    for line in reply.lines() {
        let heading = line
            .trim()
            .trim_start_matches('#')
            .trim_matches('*')
            .trim()
            .trim_end_matches(':')
            .trim_matches('*')
            .trim()
            .to_lowercase();
        match heading.as_str() {
            "summary" | "decisions" => {
                section = Section::Summary;
                continue;
            }
            "clauses" | "acceptance criteria" | "spec clauses" => {
                section = Section::Clauses;
                continue;
            }
            _ => {}
        }
        match section {
            Section::Preamble => preamble.push(line),
            Section::Summary => summary.push(line),
            Section::Clauses => {
                if let Some(clause) = clause_text(line) {
                    clauses.push(clause);
                }
            }
        }
    }
    let summary = if section == Section::Preamble {
        preamble.join("\n")
    } else {
        summary.join("\n")
    };
    let summary = summary.trim().to_string();
    (!summary.is_empty() || !clauses.is_empty()).then_some(Handoff { summary, clauses })
}

fn clause_text(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let bullet = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| {
            let (number, rest) = trimmed.split_once(". ")?;
            number.chars().all(|c| c.is_ascii_digit()).then_some(rest)
        })?;
    let text = match bullet.split_once(':') {
        Some((id, rest))
            if id
                .trim()
                .to_uppercase()
                .strip_prefix("AC-")
                .is_some_and(|number| number.chars().all(|c| c.is_ascii_digit())) =>
        {
            rest
        }
        _ => bullet,
    };
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// Adds `clauses` to a spec's markdown as new acceptance criteria, numbered
/// after the highest `AC-n` already there, under a heading that says where
/// they came from and that they await review. The rest of the file is left
/// exactly as it was. Returns the new text and the ids given.
pub fn append_clauses(existing: &str, clauses: &[String], source: &str) -> (String, Vec<String>) {
    if clauses.is_empty() {
        return (existing.to_string(), Vec::new());
    }
    let highest = Spec::parse(existing)
        .clauses
        .iter()
        .filter(|clause| clause.kind == ClauseKind::Acceptance)
        .filter_map(|clause| clause.id.rsplit('-').next()?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);

    let mut text = existing.trim_end().to_string();
    if text.is_empty() {
        text.push_str("# spec");
    }
    let source = source.split_whitespace().collect::<Vec<_>>().join(" ");
    text.push_str(&format!(
        "\n\n## acceptance criteria proposed in asherin.chat (\"{source}\"), review before building\n\n"
    ));
    let mut ids = Vec::new();
    for (offset, clause) in clauses.iter().enumerate() {
        let id = format!("AC-{}", highest + 1 + offset as u32);
        let clause = clause.split_whitespace().collect::<Vec<_>>().join(" ");
        text.push_str(&format!("- {id}: {clause}\n"));
        ids.push(id);
    }
    (text, ids)
}

/// The first message of the shepherd-room thread a chat is handed to.
pub fn handoff_seed(
    chat_title: &str,
    handoff: &Handoff,
    clause_ids: &[String],
    note: &str,
) -> String {
    let mut seed = format!(
        "picking up from asherin.chat: \"{}\".\n\n",
        chat_title.trim()
    );
    if !handoff.summary.is_empty() {
        seed.push_str("what we decided:\n");
        seed.push_str(&handoff.summary);
        seed.push_str("\n\n");
    }
    match clause_ids {
        [] => {}
        [only] => seed.push_str(&format!(
            "i drafted {only} in .noah/spec.md for review.\n\n"
        )),
        [first, .., last] => seed.push_str(&format!(
            "i drafted {first} to {last} in .noah/spec.md for review.\n\n"
        )),
    }
    let note = note.trim();
    if !note.is_empty() {
        seed.push_str(note);
        seed.push_str("\n\n");
    }
    seed.push_str(
        "read the spec clauses and the code they touch, then propose a plan before changing anything.",
    );
    seed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_handoff_reply() {
        let reply = "sure, here it is.\n\n**summary:**\n- exponential backoff with jitter, capped at 60s\n- keep retries idempotent\n\n## clauses\n- AC-1: a failed webhook is retried after 2s, then 4s, up to 60s\n* retries stop after 24 hours\n3. duplicate deliveries are ignored\n- \n";
        let handoff = parse_handoff(reply).expect("handoff");
        assert_eq!(
            handoff.summary,
            "- exponential backoff with jitter, capped at 60s\n- keep retries idempotent"
        );
        assert_eq!(
            handoff.clauses,
            [
                "a failed webhook is retried after 2s, then 4s, up to 60s",
                "retries stop after 24 hours",
                "duplicate deliveries are ignored",
            ]
        );

        let unformatted = parse_handoff("we chose redis streams.").expect("handoff");
        assert_eq!(unformatted.summary, "we chose redis streams.");
        assert!(unformatted.clauses.is_empty());
        assert_eq!(parse_handoff("  \n"), None);
    }

    #[test]
    fn appends_clauses_after_the_highest_id() {
        let existing = "# checkout\n\n## acceptance criteria\n\n- AC-1: a saved card can be charged\n- AC-7: receipts are emailed\n- declined cards show the reason\n\n## non-goals\n\n- refunds\n";
        let (text, ids) = append_clauses(
            existing,
            &[
                "retries back off".to_string(),
                "  duplicates\nare ignored ".to_string(),
            ],
            "webhook retries",
        );
        assert_eq!(ids, ["AC-8", "AC-9"]);
        assert!(text.starts_with(existing.trim_end()));
        assert!(text.ends_with(
            "## acceptance criteria proposed in asherin.chat (\"webhook retries\"), review before building\n\n- AC-8: retries back off\n- AC-9: duplicates are ignored\n"
        ));
        let reparsed = Spec::parse(&text);
        assert_eq!(
            reparsed.clause("AC-9").map(|clause| clause.text.as_str()),
            Some("duplicates are ignored")
        );
        assert_eq!(
            reparsed.clause("NG-1").map(|clause| clause.text.as_str()),
            Some("refunds")
        );

        let (fresh, ids) = append_clauses("", &["it works".to_string()], "idea");
        assert_eq!(ids, ["AC-1"]);
        assert!(fresh.starts_with("# spec\n\n## acceptance criteria proposed"));

        let (unchanged, ids) = append_clauses(existing, &[], "idea");
        assert_eq!(unchanged, existing);
        assert!(ids.is_empty());
    }

    #[test]
    fn seeds_the_shepherd_thread() {
        let handoff = Handoff {
            summary: "- use redis streams".to_string(),
            clauses: vec!["a".to_string(), "b".to_string()],
        };
        let seed = handoff_seed(
            "webhook retries",
            &handoff,
            &["AC-3".to_string(), "AC-4".to_string()],
            "start with the worker",
        );
        assert!(seed.starts_with("picking up from asherin.chat: \"webhook retries\".\n\nwhat we decided:\n- use redis streams\n\n"));
        assert!(seed.contains("i drafted AC-3 to AC-4 in .noah/spec.md for review."));
        assert!(seed.contains("start with the worker"));
        assert!(!handoff_seed("x", &handoff, &[], "").contains("drafted"));
    }
}
