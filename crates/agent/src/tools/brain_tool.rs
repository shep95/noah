use agent_client_protocol::schema::v1 as acp;
use anyhow::Result;
use gpui::{App, SharedString, Task};
use language_model::LanguageModelToolResultContent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::sync::Arc;

use crate::{AgentTool, ToolCallEventStream, ToolInput};

/// The whole of shepherd's brain is larger than a request carries, so the
/// prompt holds its core and this reads the rest: the full text is split
/// into its numbered sections once, and a paragraph is returned with the
/// section it came from. The person's own brain file, when they have put
/// one in place, replaces the built-in text here as it does in the prompt.
const MAX_SECTION_CHARS: usize = 48 * 1024;
const MAX_SEARCH_CHARS: usize = 24 * 1024;
const MAX_MATCHES: usize = 24;

/// Reads shepherd's own brain beyond the part in the prompt: `index` lists
/// every section, `section` returns one by number or by the start of its
/// title, `search` returns the paragraphs that contain every word given.
/// Consult it before answering inside a domain the index names, and say
/// which section the answer came from.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct BrainToolInput {
    /// `index`, `section` or `search`.
    pub action: BrainAction,
    /// For `section`: a section number (`21`) or the start of its title
    /// (`hacker taxonomy`). For `search`: the words to find, in any order.
    #[serde(default)]
    pub query: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BrainAction {
    Index,
    Section,
    Search,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BrainToolOutput(pub String);

impl From<BrainToolOutput> for LanguageModelToolResultContent {
    fn from(output: BrainToolOutput) -> Self {
        LanguageModelToolResultContent::Text(output.0.into())
    }
}

struct Section {
    number: String,
    title: String,
    body: String,
}

/// The brain's sections, split on its own `section N — title` lines. Text
/// before the first heading is section 0's preamble.
fn sections(text: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut current: Option<Section> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("section ")
            && let Some((number, title)) = rest.split_once(" — ")
            && !number.is_empty()
            && number.chars().all(|c| c.is_ascii_digit() || c == '.')
        {
            if let Some(section) = current.take() {
                sections.push(section);
            }
            current = Some(Section {
                number: number.to_string(),
                title: title.trim().to_string(),
                body: String::new(),
            });
            continue;
        }
        match current.as_mut() {
            Some(section) => {
                section.body.push_str(line);
                section.body.push('\n');
            }
            None => {
                current = Some(Section {
                    number: "0".to_string(),
                    title: "preamble".to_string(),
                    body: format!("{line}\n"),
                });
            }
        }
    }
    if let Some(section) = current.take() {
        sections.push(section);
    }
    sections
}

fn clip(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n\n[clipped: {} of {} characters; search for a narrower phrase to read the rest]",
        &text[..end],
        end,
        text.len()
    )
}

pub fn answer(text: &str, action: BrainAction, query: &str) -> String {
    let sections = sections(text);
    let query = query.trim();
    match action {
        BrainAction::Index => {
            let mut out = format!("{} sections:\n", sections.len());
            for section in &sections {
                let _ = writeln!(
                    out,
                    "section {} — {} ({} chars)",
                    section.number,
                    section.title,
                    section.body.trim().len()
                );
            }
            out
        }
        BrainAction::Section => {
            let wanted = query.to_lowercase();
            let wanted = wanted.trim_start_matches("section ").trim();
            let found = sections.iter().find(|section| {
                section.number == wanted
                    || section.title.to_lowercase().starts_with(wanted)
                    || format!("{} — {}", section.number, section.title.to_lowercase())
                        .starts_with(wanted)
            });
            match found {
                Some(section) => clip(
                    &format!(
                        "section {} — {}\n\n{}",
                        section.number,
                        section.title,
                        section.body.trim()
                    ),
                    MAX_SECTION_CHARS,
                ),
                None => format!("no section matches `{query}`; `brain index` lists them"),
            }
        }
        BrainAction::Search => {
            let words: Vec<String> = query
                .split_whitespace()
                .map(|word| word.to_lowercase())
                .collect();
            if words.is_empty() {
                return "give `search` one or more words".to_string();
            }
            let mut out = String::new();
            let mut matches = 0usize;
            'outer: for section in &sections {
                for paragraph in section.body.split("\n\n") {
                    let paragraph = paragraph.trim();
                    if paragraph.is_empty() {
                        continue;
                    }
                    let lower = paragraph.to_lowercase();
                    if words.iter().all(|word| lower.contains(word.as_str())) {
                        let _ = writeln!(
                            out,
                            "[section {} — {}]\n{}\n",
                            section.number, section.title, paragraph
                        );
                        matches += 1;
                        if matches >= MAX_MATCHES || out.len() >= MAX_SEARCH_CHARS {
                            break 'outer;
                        }
                    }
                }
            }
            if matches == 0 {
                format!("nothing in the brain contains all of: {}", words.join(", "))
            } else {
                clip(&out, MAX_SEARCH_CHARS)
            }
        }
    }
}

pub struct BrainTool;

impl AgentTool for BrainTool {
    type Input = BrainToolInput;
    type Output = BrainToolOutput;

    const NAME: &'static str = "brain";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Read
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(input) => match input.action {
                BrainAction::Index => "brain index".into(),
                BrainAction::Section => format!("brain section {}", input.query).into(),
                BrainAction::Search => format!("brain search {}", input.query).into(),
            },
            Err(_) => "brain".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        _event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.spawn(async move |cx| {
            let input = input
                .recv()
                .await
                .map_err(|error| BrainToolOutput(error.to_string()))?;
            let text = crate::brain_full_text();
            let answer = cx
                .background_executor()
                .spawn(async move { answer(&text, input.action, &input.query) })
                .await;
            Ok(BrainToolOutput(answer))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "shepherd — the brain\n\nsection 0 — identity\n\nwho shepherd is.\n\nsection 21 — hacker taxonomy\n\nscript kiddie — borrowed tools.\n\nnation-state — patience is the weapon.\n\nsection 62 — morality\n\nrules are not morality.\n";

    #[test]
    fn index_lists_every_section() {
        let index = answer(SAMPLE, BrainAction::Index, "");
        assert!(index.starts_with("4 sections:"));
        assert!(index.contains("section 21 — hacker taxonomy"));
    }

    #[test]
    fn section_by_number_or_title() {
        let by_number = answer(SAMPLE, BrainAction::Section, "21");
        assert!(by_number.contains("patience is the weapon"));
        let by_title = answer(SAMPLE, BrainAction::Section, "hacker");
        assert!(by_title.contains("script kiddie"));
        let missing = answer(SAMPLE, BrainAction::Section, "99");
        assert!(missing.starts_with("no section"));
    }

    #[test]
    fn search_needs_every_word_and_names_the_section() {
        let found = answer(SAMPLE, BrainAction::Search, "Patience weapon");
        assert!(found.contains("[section 21 — hacker taxonomy]"));
        assert!(!found.contains("script kiddie"));
        let none = answer(SAMPLE, BrainAction::Search, "patience kiddie");
        assert!(none.starts_with("nothing in the brain"));
    }

    #[test]
    fn long_sections_are_clipped_on_a_char_boundary() {
        let long = format!("section 1 — long\n\n{}", "é".repeat(MAX_SECTION_CHARS));
        let out = answer(&long, BrainAction::Section, "1");
        assert!(out.contains("[clipped:"));
    }
}
