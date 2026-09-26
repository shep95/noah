//! asherin.chat's plain logic: slash commands, branch inheritance, bring
//! back, outgoing redaction, custom chat agents, the conversation tree and
//! the hand-off to code. Nothing here touches the UI, the network or the
//! thread database, so each rule can be tested on its own; the agent and
//! agent_ui crates wire it in.

pub mod agents;
pub mod handoff;
pub mod tree;

use std::collections::HashSet;
use std::hash::Hash;

/// The global chat memory, relative to the chat folder. Every chat
/// conversation sees it, whichever branch it's on.
pub const MEMORY_FILE_NAME: &str = "memory.md";

/// How many levels of branches-of-branches a conversation may sit under.
/// Deeper chains would keep growing the inherited context.
pub const MAX_BRANCH_DEPTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommand {
    Research,
    Draft,
    Explain,
    Compare,
    Code,
    Agent,
}

impl SlashCommand {
    pub const ALL: [SlashCommand; 6] = [
        SlashCommand::Research,
        SlashCommand::Draft,
        SlashCommand::Explain,
        SlashCommand::Compare,
        SlashCommand::Code,
        SlashCommand::Agent,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SlashCommand::Research => "research",
            SlashCommand::Draft => "draft",
            SlashCommand::Explain => "explain",
            SlashCommand::Compare => "compare",
            SlashCommand::Code => "code",
            SlashCommand::Agent => "agent",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            SlashCommand::Research => "search the web and answer with sources",
            SlashCommand::Draft => "write a draft you can copy and edit",
            SlashCommand::Explain => "a teaching-style answer that checks understanding",
            SlashCommand::Compare => "options side by side with trade-offs",
            SlashCommand::Code => "take this conversation to the shepherd room",
            SlashCommand::Agent => "new <description>: draft a custom chat agent",
        }
    }

    pub fn input_hint(self) -> &'static str {
        match self {
            SlashCommand::Research => "<question>",
            SlashCommand::Draft => "<what to write>",
            SlashCommand::Explain => "<topic>",
            SlashCommand::Compare => "<options>",
            SlashCommand::Code => "<note for shepherd>",
            SlashCommand::Agent => "new <what the agent should do>",
        }
    }

    /// Commands that are handled by the chat room itself rather than sent
    /// to the model.
    pub fn is_local(self) -> bool {
        matches!(self, SlashCommand::Code | SlashCommand::Agent)
    }

    fn instruction(self) -> Option<&'static str> {
        match self {
            SlashCommand::Research => Some(
                "research this before answering. use search_web with a few phrasings, open the most \
                 relevant pages with fetch and answer from what you read, not from memory. cite every \
                 page you relied on as a markdown link right after the claim it supports, and say \
                 plainly when sources disagree or when you couldn't find something.",
            ),
            SlashCommand::Draft => Some(
                "write a draft of what's asked below. put the draft itself in a single fenced \
                 ```markdown block with nothing else inside it, so it can be copied and edited as a \
                 whole. keep any notes about the draft to a line or two outside the block.",
            ),
            SlashCommand::Explain => Some(
                "explain this like a patient teacher. start from what the person most likely \
                 already knows, build up one idea at a time with a small concrete example, and end \
                 with one short question that checks whether the key point landed.",
            ),
            SlashCommand::Compare => Some(
                "compare the options below side by side. give a markdown table with one column per \
                 option and one row per trade-off that matters here, then say in two or three \
                 sentences which option fits which situation and why.",
            ),
            SlashCommand::Code | SlashCommand::Agent => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedSlashCommand<'a> {
    pub command: SlashCommand,
    pub argument: &'a str,
}

/// Reads a chat slash command at the very start of `text`, such as
/// `/research why is the sky blue`. A command name must be followed by
/// whitespace or the end of the text, so `/researcher` is not `/research`.
pub fn parse_slash_command(text: &str) -> Option<ParsedSlashCommand<'_>> {
    let rest = text.trim_start().strip_prefix('/')?;
    let name_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let (name, argument) = rest.split_at(name_end);
    let command = SlashCommand::ALL
        .into_iter()
        .find(|command| command.name() == name)?;
    Some(ParsedSlashCommand {
        command,
        argument: argument.trim(),
    })
}

/// What the model is sent for a message that starts with a shaping command:
/// the command's instruction, then what the person asked. The stored message
/// keeps the text as typed. Returns `None` for text that isn't shaped.
pub fn shape_request_text(text: &str) -> Option<String> {
    let parsed = parse_slash_command(text)?;
    let instruction = parsed.command.instruction()?;
    let request = if parsed.argument.is_empty() {
        "(the request is the conversation so far.)"
    } else {
        parsed.argument
    };
    Some(format!("{instruction}\n\n{request}"))
}

/// Where a branch splits from its parent conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchPoint<Id> {
    /// After the turn that starts with this user message: the branch keeps
    /// the message and every reply to it.
    AfterTurn(Id),
    /// Just before this user message, which is how editing a sent message
    /// branches: the branch keeps everything before it.
    BeforeMessage(Id),
}

/// How many of the parent's `messages`, from the start, a branch inherits.
/// `user_message_id` gives the id of a user message and `None` for any other
/// kind. Returns `None` when the branch point's message isn't in `messages`.
pub fn inherited_message_count<Message, Id: PartialEq>(
    messages: &[Message],
    point: &BranchPoint<Id>,
    user_message_id: impl Fn(&Message) -> Option<&Id>,
) -> Option<usize> {
    let target = match point {
        BranchPoint::AfterTurn(id) | BranchPoint::BeforeMessage(id) => id,
    };
    let position = messages
        .iter()
        .position(|message| user_message_id(message) == Some(target))?;
    match point {
        BranchPoint::BeforeMessage(_) => Some(position),
        BranchPoint::AfterTurn(_) => {
            let next_turn = messages
                .iter()
                .enumerate()
                .skip(position + 1)
                .find(|(_, message)| user_message_id(message).is_some())
                .map(|(index, _)| index);
            Some(next_turn.unwrap_or(messages.len()))
        }
    }
}

/// The messages a branch starts with: the parent's prefix up to the branch
/// point.
pub fn inherited_prefix<Message: Clone, Id: PartialEq>(
    messages: &[Message],
    point: &BranchPoint<Id>,
    user_message_id: impl Fn(&Message) -> Option<&Id>,
) -> Option<Vec<Message>> {
    let count = inherited_message_count(messages, point, user_message_id)?;
    messages.get(..count).map(<[Message]>::to_vec)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AncestryError {
    /// The chain of parents is longer than [`MAX_BRANCH_DEPTH`].
    TooDeep,
    /// A conversation is its own ancestor, which only a damaged database
    /// could produce.
    Cycle,
}

impl std::fmt::Display for AncestryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AncestryError::TooDeep => write!(
                formatter,
                "branches nest at most {MAX_BRANCH_DEPTH} deep. bring this branch back to its parent or start a new chat"
            ),
            AncestryError::Cycle => write!(formatter, "this conversation's parent links loop"),
        }
    }
}

impl std::error::Error for AncestryError {}

/// The ancestors of `id`, nearest first, walking `parent_of` until a root.
/// Stops with an error rather than walking past [`MAX_BRANCH_DEPTH`] levels.
pub fn ancestors<Id: Clone + Eq + Hash>(
    id: &Id,
    parent_of: impl Fn(&Id) -> Option<Id>,
) -> Result<Vec<Id>, AncestryError> {
    let mut seen = HashSet::from([id.clone()]);
    let mut chain = Vec::new();
    let mut current = id.clone();
    while let Some(parent) = parent_of(&current) {
        if !seen.insert(parent.clone()) {
            return Err(AncestryError::Cycle);
        }
        if chain.len() == MAX_BRANCH_DEPTH {
            return Err(AncestryError::TooDeep);
        }
        chain.push(parent.clone());
        current = parent;
    }
    Ok(chain)
}

/// The depth a new branch of `parent` would have (a root's branch is 1), or
/// why it can't be made.
pub fn new_branch_depth<Id: Clone + Eq + Hash>(
    parent: &Id,
    parent_of: impl Fn(&Id) -> Option<Id>,
) -> Result<usize, AncestryError> {
    let depth = ancestors(parent, parent_of)?.len() + 1;
    if depth > MAX_BRANCH_DEPTH {
        return Err(AncestryError::TooDeep);
    }
    Ok(depth)
}

/// What the summary model is asked after a branch's messages, to bring the
/// branch's result back to its parent.
pub const BRING_BACK_PROMPT: &str = "the conversation above is a branch that split off from a \
larger discussion to explore one question. in one or two sentences, state what this branch \
concluded: the answer or decision and the main reason for it. write it as a plain statement \
with no preamble, no heading and no markdown.";

/// The single message bring back posts into the parent:
/// `⑂ from "<title>": <conclusion>`. Returns `None` when there's no
/// conclusion to post.
pub fn bring_back_message(branch_title: &str, conclusion: &str) -> Option<String> {
    let conclusion = collapse_whitespace(conclusion);
    let conclusion = conclusion
        .strip_prefix("conclusion:")
        .or_else(|| conclusion.strip_prefix("Conclusion:"))
        .unwrap_or(&conclusion)
        .trim();
    if conclusion.is_empty() {
        return None;
    }
    let title = collapse_whitespace(branch_title).replace('"', "'");
    let title = if title.is_empty() {
        "branch".to_string()
    } else {
        title
    };
    Some(format!("⑂ from \"{title}\": {conclusion}"))
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingRedaction {
    /// The message with every secret replaced by `[redacted <kind>]`.
    pub text: String,
    pub count: usize,
}

/// Secrets in a message about to leave the machine, found with noah_trust's
/// patterns. `None` when there are none.
pub fn redact_outgoing(text: &str) -> Option<OutgoingRedaction> {
    let redacted = noah_trust::secrets::redact(text, &[]);
    (redacted.count > 0).then_some(OutgoingRedaction {
        text: redacted.text,
        count: redacted.count,
    })
}

/// The composer's note after redacting: "1 secret redacted before sending".
pub fn redaction_notice(count: usize) -> String {
    if count == 1 {
        "1 secret redacted before sending".to_string()
    } else {
        format!("{count} secrets redacted before sending")
    }
}

/// The composer text a side chat about a shepherd-room task starts with:
/// what the task is and where it stands, then room for the question. Long
/// parts are cut so the side chat doesn't carry the task's whole context.
pub fn side_chat_seed(
    task_title: &str,
    project: &str,
    last_request: Option<&str>,
    latest_answer: Option<&str>,
) -> String {
    let task_title = collapse_whitespace(task_title);
    let task_title = if task_title.is_empty() {
        "untitled task".to_string()
    } else {
        task_title
    };
    let mut seed = format!("about shepherd's task \"{task_title}\" in {project}.\n");
    if let Some(request) = last_request.map(str::trim).filter(|text| !text.is_empty()) {
        seed.push_str(&format!("\nthe latest request: {}\n", clip(request, 600)));
    }
    if let Some(answer) = latest_answer.map(str::trim).filter(|text| !text.is_empty()) {
        seed.push_str(&format!("\nwhere it stands: {}\n", clip(answer, 1200)));
    }
    seed.push_str("\nmy question: ");
    seed
}

fn clip(text: &str, max_characters: usize) -> String {
    let text = collapse_whitespace(text);
    if text.chars().count() <= max_characters {
        return text;
    }
    let mut clipped: String = text.chars().take(max_characters).collect();
    clipped.push('…');
    clipped
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn seeds_side_chats() {
        let seed = side_chat_seed(
            " login\nfix ",
            "noah",
            Some("make the login\n form retry"),
            Some(&"a".repeat(2000)),
        );
        assert!(seed.starts_with("about shepherd's task \"login fix\" in noah.\n"));
        assert!(seed.contains("\nthe latest request: make the login form retry\n"));
        let answer_line = seed
            .lines()
            .find(|line| line.starts_with("where it stands: "))
            .expect("answer line");
        assert_eq!(
            answer_line.chars().count(),
            "where it stands: ".len() + 1200 + 1
        );
        assert!(answer_line.ends_with('…'));
        assert!(seed.ends_with("\nmy question: "));
        assert_eq!(
            side_chat_seed("", "noah", None, Some("  ")),
            "about shepherd's task \"untitled task\" in noah.\n\nmy question: "
        );
    }

    #[test]
    fn parses_slash_commands() {
        let parsed = parse_slash_command("/research  why is the sky blue ");
        assert_eq!(
            parsed,
            Some(ParsedSlashCommand {
                command: SlashCommand::Research,
                argument: "why is the sky blue",
            })
        );
        assert_eq!(
            parse_slash_command("  /code").map(|parsed| (parsed.command, parsed.argument)),
            Some((SlashCommand::Code, ""))
        );
        assert_eq!(
            parse_slash_command("/compare\npostgres vs sqlite").map(|parsed| parsed.argument),
            Some("postgres vs sqlite")
        );
        assert_eq!(parse_slash_command("/researcher x"), None);
        assert_eq!(parse_slash_command("/Research x"), None);
        assert_eq!(parse_slash_command("research x"), None);
        assert_eq!(parse_slash_command("see /research x"), None);
        assert_eq!(parse_slash_command("/"), None);
    }

    #[test]
    fn shapes_requests_but_not_local_commands() {
        let shaped = shape_request_text("/explain borrow checking").expect("shaped");
        assert!(shaped.starts_with("explain this like a patient teacher"));
        assert!(shaped.ends_with("\n\nborrow checking"));
        let shaped = shape_request_text("/research").expect("shaped without argument");
        assert!(shaped.ends_with("(the request is the conversation so far.)"));
        assert_eq!(shape_request_text("/code do it"), None);
        assert_eq!(shape_request_text("/agent new tax helper"), None);
        assert_eq!(shape_request_text("plain question"), None);
        for command in SlashCommand::ALL {
            assert_eq!(command.instruction().is_none(), command.is_local());
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    enum TestMessage {
        User(u32),
        Agent(&'static str),
    }

    fn user_id(message: &TestMessage) -> Option<&u32> {
        match message {
            TestMessage::User(id) => Some(id),
            TestMessage::Agent(_) => None,
        }
    }

    #[test]
    fn builds_branch_prefixes() {
        let messages = vec![
            TestMessage::User(1),
            TestMessage::Agent("a"),
            TestMessage::Agent("b"),
            TestMessage::User(2),
            TestMessage::Agent("c"),
            TestMessage::User(3),
        ];
        assert_eq!(
            inherited_prefix(&messages, &BranchPoint::AfterTurn(1), user_id),
            Some(messages[..3].to_vec())
        );
        assert_eq!(
            inherited_message_count(&messages, &BranchPoint::AfterTurn(2), user_id),
            Some(5)
        );
        assert_eq!(
            inherited_message_count(&messages, &BranchPoint::AfterTurn(3), user_id),
            Some(6)
        );
        assert_eq!(
            inherited_prefix(&messages, &BranchPoint::BeforeMessage(2), user_id),
            Some(messages[..3].to_vec())
        );
        assert_eq!(
            inherited_message_count(&messages, &BranchPoint::BeforeMessage(1), user_id),
            Some(0)
        );
        assert_eq!(
            inherited_message_count(&messages, &BranchPoint::AfterTurn(9), user_id),
            None
        );
    }

    fn chain(length: usize) -> HashMap<usize, usize> {
        (1..=length).map(|id| (id, id - 1)).collect()
    }

    #[test]
    fn caps_branch_depth() {
        let parents = chain(MAX_BRANCH_DEPTH);
        let parent_of = |id: &usize| parents.get(id).copied();
        assert_eq!(ancestors(&0, parent_of), Ok(Vec::new()));
        assert_eq!(ancestors(&3, parent_of), Ok(vec![2, 1, 0]));
        assert_eq!(new_branch_depth(&0, parent_of), Ok(1));
        assert_eq!(
            new_branch_depth(&(MAX_BRANCH_DEPTH - 1), parent_of),
            Ok(MAX_BRANCH_DEPTH)
        );
        assert_eq!(
            new_branch_depth(&MAX_BRANCH_DEPTH, parent_of),
            Err(AncestryError::TooDeep)
        );

        let deeper = chain(MAX_BRANCH_DEPTH + 5);
        assert_eq!(
            ancestors(&(MAX_BRANCH_DEPTH + 5), |id| deeper.get(id).copied()),
            Err(AncestryError::TooDeep)
        );

        let looping = HashMap::from([(1, 2), (2, 1)]);
        assert_eq!(
            ancestors(&1, |id| looping.get(id).copied()),
            Err(AncestryError::Cycle)
        );
    }

    #[test]
    fn formats_bring_back_messages() {
        assert_eq!(
            bring_back_message(
                "redis streams instead",
                "  streams beat a list here because consumer groups\n give us retry tracking for free; keep the 60s ceiling.\n"
            )
            .as_deref(),
            Some(
                "⑂ from \"redis streams instead\": streams beat a list here because consumer groups give us retry tracking for free; keep the 60s ceiling."
            )
        );
        assert_eq!(
            bring_back_message("the \"fast\" path", "Conclusion: use it").as_deref(),
            Some("⑂ from \"the 'fast' path\": use it")
        );
        assert_eq!(
            bring_back_message("  ", "done").as_deref(),
            Some("⑂ from \"branch\": done")
        );
        assert_eq!(bring_back_message("title", " \n "), None);
    }

    #[test]
    fn redacts_outgoing_messages() {
        // Fixtures are split so GitHub push protection doesn't take them for real keys.
        let message = concat!(
            "here's my key sk-ant-",
            "api03-abcdefghijklmnopqrstuvwxyz0123 can you check it"
        );
        let redaction = redact_outgoing(message).expect("a secret");
        assert_eq!(redaction.count, 1);
        assert!(!redaction.text.contains("sk-ant-api03"));
        assert!(redaction.text.contains("[redacted Anthropic key]"));
        assert!(redaction.text.starts_with("here's my key "));

        let env = concat!(
            "DATABASE_URL=postgres://app:hunter2secret@db.internal/app\nSTRIPE_KEY=sk_",
            "live_abcdefghijklmnopqrstuvwx"
        );
        assert_eq!(
            redact_outgoing(env).map(|redaction| redaction.count),
            Some(2)
        );

        assert_eq!(redact_outgoing("how should retries back off?"), None);
        assert_eq!(redaction_notice(1), "1 secret redacted before sending");
        assert_eq!(redaction_notice(3), "3 secrets redacted before sending");
    }
}
