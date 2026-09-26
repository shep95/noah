//! Custom chat agents: named instructions the person writes, such as a tax
//! helper or an editor who critiques their writing, picked per
//! conversation. Each lives in `~/.noah/chat/agents/asherin.<name>.md` as a
//! small frontmatter block followed by the instructions:
//!
//! ```text
//! ---
//! name: tax helper
//! description: answers questions about my taxes, carefully
//! model: anthropic/claude-sonnet-4-5
//! tools: search_web, fetch
//! ---
//! you help me with my taxes. ...
//! ```
//!
//! An agent's instructions go after shepherd's own rules, and its tools can
//! only narrow what the chat room already allows, never widen it.

use std::fmt;

/// The folder under the chat folder that holds agent files.
pub const AGENTS_FOLDER: &str = "agents";

const FILE_PREFIX: &str = "asherin.";
const FILE_SUFFIX: &str = ".md";
const MAX_NAME_LENGTH: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatAgent {
    /// The sanitized name, which is also the file's `asherin.<id>.md`.
    pub id: String,
    /// The name as written in the file.
    pub name: String,
    pub description: String,
    /// `provider/model`, used when the agent is picked for a conversation.
    pub model: Option<String>,
    /// The tools the agent asks for. `None` means every tool the chat room
    /// allows. See [`restrict_tools`] for how this is enforced.
    pub tools: Option<Vec<String>>,
    pub instructions: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentFileError {
    MissingFrontmatter,
    UnclosedFrontmatter,
    MalformedLine { line: usize, text: String },
    UnknownField { line: usize, field: String },
    DuplicateField { field: String },
    MissingName,
    InvalidName(String),
    EmptyInstructions,
    InvalidModel(String),
    FileNameMismatch { file_id: String, name_id: String },
}

impl fmt::Display for AgentFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentFileError::MissingFrontmatter => write!(
                formatter,
                "the file must start with a `---` line, then `name: ...`, then another `---`"
            ),
            AgentFileError::UnclosedFrontmatter => {
                write!(formatter, "the frontmatter has no closing `---` line")
            }
            AgentFileError::MalformedLine { line, text } => write!(
                formatter,
                "line {line} should look like `field: value`, but is `{text}`"
            ),
            AgentFileError::UnknownField { line, field } => write!(
                formatter,
                "line {line} has an unknown field `{field}`; use name, description, model or tools"
            ),
            AgentFileError::DuplicateField { field } => {
                write!(formatter, "`{field}` is set more than once")
            }
            AgentFileError::MissingName => write!(formatter, "the frontmatter needs a `name:`"),
            AgentFileError::InvalidName(name) => write!(
                formatter,
                "`{name}` can't be an agent name; use letters, numbers, spaces or dashes"
            ),
            AgentFileError::EmptyInstructions => {
                write!(formatter, "there are no instructions after the frontmatter")
            }
            AgentFileError::InvalidModel(model) => write!(
                formatter,
                "model `{model}` should be written as `provider/model`"
            ),
            AgentFileError::FileNameMismatch { file_id, name_id } => write!(
                formatter,
                "the name makes `asherin.{name_id}.md`, but the file is `asherin.{file_id}.md`; rename one to match"
            ),
        }
    }
}

impl std::error::Error for AgentFileError {}

/// Turns what a person calls an agent into its id: lowercase ASCII letters,
/// digits and single dashes, at most 40 characters. `None` when nothing
/// usable is left.
pub fn sanitize_agent_name(name: &str) -> Option<String> {
    let name = name.trim();
    let name = name.strip_prefix(FILE_PREFIX).unwrap_or(name);
    let mut id = String::new();
    let mut pending_dash = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            let needs_dash = pending_dash && !id.is_empty();
            if id.len() + usize::from(needs_dash) + 1 > MAX_NAME_LENGTH {
                break;
            }
            if needs_dash {
                id.push('-');
            }
            pending_dash = false;
            id.push(character.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    let id = id.trim_end_matches('-').to_string();
    (!id.is_empty()).then_some(id)
}

/// `asherin.<id>.md`.
pub fn agent_file_name(id: &str) -> String {
    format!("{FILE_PREFIX}{id}{FILE_SUFFIX}")
}

/// The id in an agent file's name, or `None` for files that aren't agents.
pub fn agent_id_from_file_name(file_name: &str) -> Option<&str> {
    let id = file_name
        .strip_prefix(FILE_PREFIX)?
        .strip_suffix(FILE_SUFFIX)?;
    (sanitize_agent_name(id).as_deref() == Some(id)).then_some(id)
}

/// Reads an agent file. `file_id` is the id from the file's name, which the
/// `name:` field must agree with.
pub fn parse_agent_file(file_id: &str, text: &str) -> Result<ChatAgent, AgentFileError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines().enumerate();
    match lines.next() {
        Some((_, first)) if first.trim() == "---" => {}
        _ => return Err(AgentFileError::MissingFrontmatter),
    }

    let mut name = None;
    let mut description = None;
    let mut model = None;
    let mut tools = None;
    let mut body_start = None;
    for (index, line) in lines.by_ref() {
        let trimmed = line.trim();
        if trimmed == "---" {
            body_start = Some(index + 1);
            break;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let line_number = index + 1;
        let Some((field, value)) = trimmed.split_once(':') else {
            return Err(AgentFileError::MalformedLine {
                line: line_number,
                text: trimmed.to_string(),
            });
        };
        let field = field.trim().to_lowercase();
        let value = unquote(value.trim()).to_string();
        let slot = match field.as_str() {
            "name" => &mut name,
            "description" => &mut description,
            "model" => &mut model,
            "tools" => &mut tools,
            _ => {
                return Err(AgentFileError::UnknownField {
                    line: line_number,
                    field,
                });
            }
        };
        if slot.replace(value).is_some() {
            return Err(AgentFileError::DuplicateField { field });
        }
    }
    let Some(body_start) = body_start else {
        return Err(AgentFileError::UnclosedFrontmatter);
    };

    let name = name
        .filter(|name| !name.is_empty())
        .ok_or(AgentFileError::MissingName)?;
    let name_id =
        sanitize_agent_name(&name).ok_or_else(|| AgentFileError::InvalidName(name.clone()))?;
    if name_id != file_id {
        return Err(AgentFileError::FileNameMismatch {
            file_id: file_id.to_string(),
            name_id,
        });
    }

    let model = model.filter(|model| !model.is_empty());
    if let Some(model) = &model
        && !model.split_once('/').is_some_and(|(provider, model)| {
            !provider.trim().is_empty() && !model.trim().is_empty()
        })
    {
        return Err(AgentFileError::InvalidModel(model.clone()));
    }

    let tools = tools.map(|tools| {
        tools
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split(',')
            .map(|tool| unquote(tool.trim()).to_string())
            .filter(|tool| !tool.is_empty())
            .collect::<Vec<_>>()
    });

    let instructions = text
        .lines()
        .skip(body_start)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if instructions.is_empty() {
        return Err(AgentFileError::EmptyInstructions);
    }

    Ok(ChatAgent {
        id: name_id,
        name,
        description: description.unwrap_or_default(),
        model,
        tools,
        instructions,
    })
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
}

/// The tools a conversation with `agent_tools` may use, given the tools the
/// chat room allows. This is the only way an agent's tool list is applied,
/// so an agent file can remove tools but never add one the room doesn't
/// allow, whatever it asks for.
pub fn restrict_tools<'a>(
    chat_room_tools: impl IntoIterator<Item = &'a str>,
    agent_tools: Option<&[String]>,
) -> Vec<&'a str> {
    chat_room_tools
        .into_iter()
        .filter(|tool| {
            agent_tools.is_none_or(|agent_tools| agent_tools.iter().any(|wanted| wanted == tool))
        })
        .collect()
}

/// What goes after shepherd's own rules in a conversation that uses `agent`.
pub fn system_prompt_section(agent: &ChatAgent) -> String {
    format!(
        "## asherin.{id}\n\nin this conversation you are asherin.{id}, a custom agent the person wrote. \
         follow their instructions below as long as they don't conflict with the rules above.\n\n{instructions}",
        id = agent.id,
        instructions = agent.instructions
    )
}

/// A starting file for a new agent.
pub fn agent_file_template(id: &str, description: &str, instructions: &str) -> String {
    let description = description.lines().next().unwrap_or_default().trim();
    let instructions = instructions.trim();
    let instructions = if instructions.is_empty() {
        "describe who this agent is and how it should answer. for example: \"you are a patient \
         editor. point out the weakest sentence in what i paste and suggest one rewrite.\""
    } else {
        instructions
    };
    format!(
        "---\nname: {id}\ndescription: {description}\n# model: anthropic/claude-sonnet-4-5\n# tools: search_web, fetch, read_file\n---\n\n{instructions}\n"
    )
}

/// What the model is asked, for `/agent new <description>`, to draft an
/// agent's instructions. It answers with the instructions only.
pub fn draft_instructions_prompt(description: &str) -> String {
    format!(
        "write the instructions for a custom chat assistant, addressed to the assistant as \"you\". \
         the person described it as: \"{}\". cover who it is, how it should answer, what it should \
         ask when something is unclear, and what it must not do. keep it under 200 words, lowercase \
         and plain, with no heading, no preamble and no frontmatter.",
        description.trim()
    )
}

/// What `/agent new <description>` names the agent: the first few words of
/// the description.
pub fn agent_name_from_description(description: &str) -> Option<String> {
    let words = description
        .split_whitespace()
        .filter(|word| !matches!(*word, "a" | "an" | "the" | "for" | "who" | "that"))
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    sanitize_agent_name(&words)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAX_HELPER: &str = "---\nname: Tax Helper\ndescription: answers questions about my taxes\nmodel: anthropic/claude-sonnet-4-5\ntools: [search_web, \"fetch\", terminal, edit_file]\n---\n\nyou help me with my taxes.\ncite the rule you rely on.\n";

    #[test]
    fn parses_frontmatter() {
        let agent = parse_agent_file("tax-helper", TAX_HELPER).expect("valid agent");
        assert_eq!(agent.id, "tax-helper");
        assert_eq!(agent.name, "Tax Helper");
        assert_eq!(agent.description, "answers questions about my taxes");
        assert_eq!(agent.model.as_deref(), Some("anthropic/claude-sonnet-4-5"));
        assert_eq!(
            agent.tools,
            Some(vec![
                "search_web".to_string(),
                "fetch".to_string(),
                "terminal".to_string(),
                "edit_file".to_string(),
            ])
        );
        assert_eq!(
            agent.instructions,
            "you help me with my taxes.\ncite the rule you rely on."
        );

        let minimal = parse_agent_file(
            "critic",
            "---\n# a comment\nname: 'critic'\n\n---\nbe blunt about my writing.",
        )
        .expect("minimal agent");
        assert_eq!(minimal.description, "");
        assert_eq!(minimal.model, None);
        assert_eq!(minimal.tools, None);

        let template = agent_file_template("new-agent", "what it does", "");
        let parsed = parse_agent_file("new-agent", &template).expect("template parses");
        assert_eq!(parsed.model, None);
        assert_eq!(parsed.tools, None);
    }

    #[test]
    fn rejects_malformed_files() {
        let cases: [(&str, AgentFileError); 9] = [
            ("you are helpful", AgentFileError::MissingFrontmatter),
            (
                "---\nname: critic\nbe blunt",
                AgentFileError::MalformedLine {
                    line: 3,
                    text: "be blunt".to_string(),
                },
            ),
            ("---\nname: critic\n", AgentFileError::UnclosedFrontmatter),
            (
                "---\nname: critic\ncolour: red\n---\nbe blunt",
                AgentFileError::UnknownField {
                    line: 3,
                    field: "colour".to_string(),
                },
            ),
            (
                "---\nname: critic\nname: other\n---\nbe blunt",
                AgentFileError::DuplicateField {
                    field: "name".to_string(),
                },
            ),
            (
                "---\ndescription: x\n---\nbe blunt",
                AgentFileError::MissingName,
            ),
            (
                "---\nname: !!!\n---\nbe blunt",
                AgentFileError::InvalidName("!!!".to_string()),
            ),
            (
                "---\nname: critic\n---\n\n  \n",
                AgentFileError::EmptyInstructions,
            ),
            (
                "---\nname: critic\nmodel: sonnet\n---\nbe blunt",
                AgentFileError::InvalidModel("sonnet".to_string()),
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(parse_agent_file("critic", text), Err(expected), "{text:?}");
        }
        let mismatch = parse_agent_file("critic", "---\nname: editor\n---\nbe blunt");
        assert_eq!(
            mismatch,
            Err(AgentFileError::FileNameMismatch {
                file_id: "critic".to_string(),
                name_id: "editor".to_string(),
            })
        );
        let message = mismatch
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(message.contains("asherin.editor.md"), "{message}");
        assert!(
            AgentFileError::MissingFrontmatter
                .to_string()
                .contains("must start with a `---` line")
        );
    }

    #[test]
    fn sanitizes_names() {
        assert_eq!(
            sanitize_agent_name("Tax Helper!").as_deref(),
            Some("tax-helper")
        );
        assert_eq!(
            sanitize_agent_name("  editor who critiques my writing ").as_deref(),
            Some("editor-who-critiques-my-writing")
        );
        assert_eq!(
            sanitize_agent_name("asherin.critic").as_deref(),
            Some("critic")
        );
        assert_eq!(
            sanitize_agent_name("../../etc/passwd").as_deref(),
            Some("etc-passwd")
        );
        assert_eq!(sanitize_agent_name("--a--b--").as_deref(), Some("a-b"));
        assert_eq!(sanitize_agent_name("日本語"), None);
        assert_eq!(sanitize_agent_name("  "), None);
        let long = sanitize_agent_name(&"word ".repeat(30)).expect("long name");
        assert!(long.len() <= MAX_NAME_LENGTH, "{long}");
        assert!(!long.ends_with('-'));

        assert_eq!(agent_file_name("critic"), "asherin.critic.md");
        assert_eq!(
            agent_id_from_file_name("asherin.tax-helper.md"),
            Some("tax-helper")
        );
        assert_eq!(agent_id_from_file_name("asherin.Tax.md"), None);
        assert_eq!(agent_id_from_file_name("notes.md"), None);
        assert_eq!(
            agent_name_from_description("an editor who critiques my writing").as_deref(),
            Some("editor-critiques-my")
        );
    }

    #[test]
    fn agents_never_gain_tools_the_chat_room_lacks() {
        let chat_room = ["search_web", "fetch", "read_file", "grep"];
        let agent = parse_agent_file("tax-helper", TAX_HELPER).expect("valid agent");
        let allowed = restrict_tools(chat_room, agent.tools.as_deref());
        assert_eq!(allowed, ["search_web", "fetch"]);
        assert!(!allowed.contains(&"terminal"));
        assert!(!allowed.contains(&"edit_file"));

        assert_eq!(restrict_tools(chat_room, None), chat_room);
        assert!(restrict_tools(chat_room, Some(&[])).is_empty());
        assert!(
            restrict_tools(
                chat_room,
                Some(&["edit_file".to_string(), "terminal".to_string()])
            )
            .is_empty()
        );
    }

    #[test]
    fn places_instructions_after_the_base_rules() {
        let agent = parse_agent_file("tax-helper", TAX_HELPER).expect("valid agent");
        let section = system_prompt_section(&agent);
        assert!(section.starts_with("## asherin.tax-helper\n"));
        assert!(section.contains("don't conflict with the rules above"));
        assert!(section.ends_with("cite the rule you rely on."));
    }
}
