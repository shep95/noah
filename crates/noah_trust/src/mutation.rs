//! Mutation testing for the lines shepherd changed: small deliberate bugs
//! (a flipped comparison, a swapped boolean, a dropped call, a replaced
//! return value) are put in one at a time and the tests are run. A mutant
//! the tests don't catch marks behavior no test checks, which line coverage
//! can't show.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mutant {
    /// 1-based line.
    pub line: usize,
    pub original: String,
    pub mutated: String,
    pub description: String,
}

pub const DEFAULT_LIMIT: usize = 8;
pub const MAX_LIMIT: usize = 30;

const SWAPS: &[(&str, &str, &str)] = &[
    (" == ", " != ", "equality flipped"),
    (" != ", " == ", "inequality flipped"),
    (" <= ", " < ", "boundary moved"),
    (" >= ", " > ", "boundary moved"),
    (" < ", " <= ", "boundary moved"),
    (" > ", " >= ", "boundary moved"),
    (" && ", " || ", "and became or"),
    (" || ", " && ", "or became and"),
    (" and ", " or ", "and became or"),
    (" or ", " and ", "or became and"),
    ("true", "false", "true became false"),
    ("false", "true", "false became true"),
    ("True", "False", "True became False"),
    ("False", "True", "False became True"),
    // Before the general arithmetic swaps, so `x + 1` is reported as an
    // off-by-one rather than as plus becoming minus.
    ("+ 1", "- 1", "off by one"),
    ("- 1", "+ 1", "off by one"),
    (" + ", " - ", "plus became minus"),
    (" - ", " + ", "minus became plus"),
    ("+ 1", "+ 2", "off by one"),
    ("- 1", "- 2", "off by one"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language {
    Rust,
    JavaScript,
    Python,
    Ruby,
    C,
    Go,
    Other,
}

impl Language {
    fn for_path(path: &str) -> Self {
        let extension = path
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_lowercase())
            .unwrap_or_default();
        match extension.as_str() {
            "rs" => Self::Rust,
            "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" => Self::JavaScript,
            "py" | "pyi" => Self::Python,
            "rb" => Self::Ruby,
            "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" => Self::C,
            "go" => Self::Go,
            _ => Self::Other,
        }
    }
}

fn is_comment_or_blank(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.is_empty()
        || trimmed.starts_with("//")
        || trimmed.starts_with('#')
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
        || trimmed.starts_with("--")
        || trimmed.starts_with("import ")
        || trimmed.starts_with("use ")
}

/// Mutants for lines `first..=last` (1-based) of `source`, at most `limit`,
/// spread across the range so a big change isn't all one line's mutants.
/// Only operators that don't depend on the language are used; see
/// [`mutants_for_path`].
pub fn mutants(source: &str, first: usize, last: usize, limit: usize) -> Vec<Mutant> {
    collect_mutants(Language::Other, source, first, last, limit)
}

/// Like [`mutants`], adding the operators that depend on the language, which
/// is read from `path`'s extension.
pub fn mutants_for_path(
    path: &str,
    source: &str,
    first: usize,
    last: usize,
    limit: usize,
) -> Vec<Mutant> {
    collect_mutants(Language::for_path(path), source, first, last, limit)
}

fn collect_mutants(
    language: Language,
    source: &str,
    first: usize,
    last: usize,
    limit: usize,
) -> Vec<Mutant> {
    let mut per_line: Vec<Vec<Mutant>> = Vec::new();
    let mut previous_code_line: Option<&str> = None;
    for (index, line) in source.lines().enumerate() {
        let number = index + 1;
        let previous = previous_code_line;
        if !is_comment_or_blank(line) {
            previous_code_line = Some(line);
        }
        if number < first || number > last || is_comment_or_blank(line) {
            continue;
        }
        let mut candidates: Vec<(String, &'static str)> = Vec::new();
        for (from, to, description) in SWAPS {
            if let Some(mutated) = swap(line, from, to) {
                candidates.push((mutated, *description));
            }
        }
        if let Some(mutated) = replace_return_value(line, language) {
            candidates.push((mutated, "return value replaced"));
        }
        if let Some(mutated) = skip_error_branch(line, language) {
            candidates.push((mutated, "error branch skipped"));
        }
        if let Some(mutated) = swap_arguments(line) {
            candidates.push((mutated, "arguments swapped"));
        }
        if let Some(mutated) = delete_call(line, previous) {
            candidates.push((mutated, "call deleted"));
        }
        let mut line_mutants: Vec<Mutant> = Vec::new();
        for (mutated, description) in candidates {
            if mutated == line || line_mutants.iter().any(|mutant| mutant.mutated == mutated) {
                continue;
            }
            line_mutants.push(Mutant {
                line: number,
                original: line.to_string(),
                mutated,
                description: description.to_string(),
            });
        }
        if !line_mutants.is_empty() {
            per_line.push(line_mutants);
        }
    }
    let mut chosen = Vec::new();
    let mut round = 0;
    while chosen.len() < limit {
        let mut added = false;
        for line_mutants in &per_line {
            if let Some(mutant) = line_mutants.get(round) {
                chosen.push(mutant.clone());
                added = true;
                if chosen.len() == limit {
                    break;
                }
            }
        }
        if !added {
            break;
        }
        round += 1;
    }
    chosen.sort_by_key(|mutant| mutant.line);
    chosen
}

fn is_identifier_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

fn swap(line: &str, from: &str, to: &str) -> Option<String> {
    let position = find_outside_strings(line, from)?;
    // `true` must not match inside `untrue_flag`, and `+ 1` must not match
    // the start of `+ 10`.
    if from.starts_with(is_identifier_character)
        && line[..position].chars().last().is_some_and(is_identifier_character)
    {
        return None;
    }
    if from.ends_with(is_identifier_character)
        && line[position + from.len()..]
            .chars()
            .next()
            .is_some_and(is_identifier_character)
    {
        return None;
    }
    let mut mutated = line.to_string();
    mutated.replace_range(position..position + from.len(), to);
    Some(mutated)
}

fn indentation(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// Whether brackets in `text` open and close within it, ignoring strings,
/// so the text isn't one piece of an expression spread over several lines.
fn is_balanced(text: &str) -> bool {
    let mut depth: i64 = 0;
    let mut in_string: Option<char> = None;
    let mut escaped = false;
    for character in text.chars() {
        if let Some(quote) = in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote {
                in_string = None;
            }
            continue;
        }
        match character {
            '"' | '\'' | '`' => in_string = Some(character),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0 && in_string.is_none()
}

/// `return x;` becomes a return of the type's empty value, for languages
/// where that value can be written without knowing the type. Only single-line
/// returns are touched.
fn replace_return_value(line: &str, language: Language) -> Option<String> {
    let trimmed = line.trim();
    let uses_semicolons = matches!(language, Language::Rust | Language::JavaScript | Language::C);
    let expression = if uses_semicolons {
        trimmed.strip_prefix("return ")?.strip_suffix(';')?.trim()
    } else {
        trimmed.strip_prefix("return ")?.trim()
    };
    if expression.is_empty() || !is_balanced(expression) || expression.contains(['{', '}']) {
        return None;
    }
    if !uses_semicolons
        && (expression.ends_with(['\\', ',', '(', '[', '+', '-', '*', '/', '.'])
            || expression.ends_with(" and")
            || expression.ends_with(" or"))
    {
        return None;
    }
    let starts_with_word = |words: &[&str]| {
        words.iter().any(|word| {
            expression.strip_prefix(*word).is_some_and(|rest| {
                !rest.starts_with(is_identifier_character)
            })
        })
    };
    let replacement = match language {
        Language::Rust => {
            if expression.starts_with("Some(") && expression.ends_with(')') {
                "None"
            } else if expression.starts_with(['&', '*', '|', '('])
                || starts_with_word(&["Ok", "Err", "None", "Self", "self", "Default", "true", "false"])
            {
                // A borrowed value, a closure, a tuple, or a `Result` has no
                // `Default` in general, so the mutant would only fail to
                // compile.
                return None;
            } else {
                "Default::default()"
            }
        }
        Language::JavaScript => {
            if starts_with_word(&["null", "undefined", "true", "false"]) {
                return None;
            }
            "null"
        }
        Language::Python => {
            if starts_with_word(&["None", "True", "False", "yield"]) {
                return None;
            }
            "None"
        }
        Language::Ruby => {
            if starts_with_word(&["nil", "true", "false"]) || expression.contains(" if ")
                || expression.contains(" unless ")
            {
                return None;
            }
            "nil"
        }
        Language::C => {
            if starts_with_word(&["0", "NULL", "nullptr", "true", "false"]) {
                return None;
            }
            "0"
        }
        Language::Go | Language::Other => return None,
    };
    let terminator = if uses_semicolons { ";" } else { "" };
    Some(format!("{}return {replacement}{terminator}", indentation(line)))
}

static GO_ERROR_CHECK: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^if\s+([A-Za-z_]\w*)\s*!=\s*nil\s*\{$").ok());

/// `if err != nil {` becomes a branch that never runs.
fn skip_error_branch(line: &str, language: Language) -> Option<String> {
    if language != Language::Go {
        return None;
    }
    let Some(regex) = &*GO_ERROR_CHECK else {
        return None;
    };
    let captures = regex.captures(line.trim())?;
    let name = captures.get(1)?.as_str();
    if !name.to_lowercase().contains("err") {
        return None;
    }
    // `if false {` alone would leave `err` unused whenever the branch
    // doesn't mention it, and Go refuses to compile unused variables, so the
    // mutant would count as caught without any test running.
    Some(format!("{}if {name} != nil && false {{", indentation(line)))
}

static TWO_ARGUMENT_CALL: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"([A-Za-z_][\w.]*)\(\s*([A-Za-z_]\w*)\s*,\s*([A-Za-z_]\w*)\s*\)").ok()
});

const DEFINITION_KEYWORDS: &[&str] = &["fn", "function", "def", "func", "fun", "class", "struct", "enum"];

/// `f(a, b)` becomes `f(b, a)` for a call with exactly two plain names.
fn swap_arguments(line: &str) -> Option<String> {
    let Some(regex) = &*TWO_ARGUMENT_CALL else {
        return None;
    };
    let first_word = line
        .trim_start()
        .split(|character: char| !is_identifier_character(character))
        .find(|word| {
            !matches!(
                *word,
                "" | "pub" | "export" | "default" | "async" | "static" | "private" | "public" | "protected"
            )
        });
    if first_word.is_some_and(|word| DEFINITION_KEYWORDS.contains(&word)) {
        return None;
    }
    for captures in regex.captures_iter(line) {
        let (Some(whole), Some(name), Some(first), Some(second)) =
            (captures.get(0), captures.get(1), captures.get(2), captures.get(3))
        else {
            continue;
        };
        let preceded_by_identifier = line[..whole.start()]
            .chars()
            .last()
            .is_some_and(is_identifier_character);
        let after = line[whole.end()..].trim_start();
        // A line starting with `name(a, b) {` or `name(a, b): T` defines a
        // method rather than calling one.
        let is_definition = whole.start() == indentation(line).len()
            && (after.starts_with(['{', ':']) || after.starts_with("=>"));
        if preceded_by_identifier
            || is_definition
            || first.as_str() == second.as_str()
            || find_outside_strings(line, whole.as_str()) != Some(whole.start())
            || matches!(name.as_str(), "if" | "while" | "for" | "switch" | "match" | "return" | "catch")
        {
            continue;
        }
        let mut mutated = line.to_string();
        mutated.replace_range(
            first.start()..second.end(),
            &format!("{}, {}", second.as_str(), first.as_str()),
        );
        return Some(mutated);
    }
    None
}

const STATEMENT_KEYWORDS: &[&str] = &[
    "return", "let", "const", "var", "if", "else", "while", "for", "match", "switch", "case",
    "throw", "yield", "break", "continue", "new", "delete", "do", "try", "catch", "fn", "func",
    "def", "goto", "static", "pub", "mut", "impl", "type",
];

/// A statement line that is only a call, such as `log(x);` or
/// `self.items.clear();`, becomes an empty line. Anything that declares,
/// assigns or returns is left alone, as is a line that continues the one
/// before it.
fn delete_call(line: &str, previous_code_line: Option<&str>) -> Option<String> {
    let trimmed = line.trim();
    let body = trimmed.strip_suffix(';')?;
    let body = body.strip_prefix("await ").unwrap_or(body);
    if let Some(previous) = previous_code_line {
        let previous = previous.trim_end();
        if !(previous.ends_with(';') || previous.ends_with('{') || previous.ends_with('}')) {
            return None;
        }
    }
    if !is_plain_call(body) {
        return None;
    }
    Some(indentation(line).to_string())
}

/// `path(args)` optionally followed by `?`, `.await` and more
/// `.name(args)` links, and nothing else.
fn is_plain_call(text: &str) -> bool {
    let mut rest = text;
    let mut first_segment = true;
    loop {
        let name_length = rest
            .char_indices()
            .find(|(_, character)| {
                !(is_identifier_character(*character) || matches!(*character, '.' | ':' | '!'))
            })
            .map_or(rest.len(), |(index, _)| index);
        let name = &rest[..name_length];
        if name.is_empty() || !name.starts_with(|character: char| character.is_alphabetic() || character == '_') {
            return false;
        }
        if first_segment {
            let first_word = name
                .split(|character: char| !is_identifier_character(character))
                .next()
                .unwrap_or_default();
            // `this(...)` and `super(...)` delegate to another constructor,
            // which can't be removed.
            if STATEMENT_KEYWORDS.contains(&first_word) || matches!(name, "this" | "super") {
                return false;
            }
            first_segment = false;
        }
        rest = &rest[name_length..];
        let Some(arguments) = rest.strip_prefix('(') else {
            return false;
        };
        let Some(close) = matching_parenthesis(arguments) else {
            return false;
        };
        rest = &arguments[close + 1..];
        rest = rest.strip_prefix('?').unwrap_or(rest);
        if rest.is_empty() {
            return true;
        }
        if rest == ".await" || rest == ".await?" {
            return true;
        }
        let Some(next) = rest.strip_prefix('.') else {
            return false;
        };
        rest = next;
    }
}

/// The index in `text` of the `)` closing a `(` that came just before it.
fn matching_parenthesis(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string: Option<char> = None;
    let mut escaped = false;
    for (index, character) in text.char_indices() {
        if let Some(quote) = in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote {
                in_string = None;
            }
            continue;
        }
        match character {
            '"' | '\'' | '`' => in_string = Some(character),
            '(' | '[' | '{' => depth += 1,
            ')' if depth == 0 => return Some(index),
            ')' | ']' | '}' => depth = depth.checked_sub(1)?,
            _ => {}
        }
    }
    None
}

/// The one-line report for a mutant the tests didn't catch.
pub fn survivor_line(mutant: &Mutant) -> String {
    let mutated = mutant.mutated.trim();
    format!(
        "line {}: {} → {} survived — no test checks this change",
        mutant.line,
        mutant.original.trim(),
        if mutated.is_empty() { "(deleted)" } else { mutated }
    )
}

fn find_outside_strings(line: &str, needle: &str) -> Option<usize> {
    let mut in_string: Option<char> = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if let Some(quote) = in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote {
                in_string = None;
            }
            continue;
        }
        if character == '"' || character == '\'' || character == '`' {
            in_string = Some(character);
            continue;
        }
        if line[index..].starts_with(needle) {
            return Some(index);
        }
    }
    None
}

/// Puts one mutant into the source, returning the mutated text.
pub fn apply(source: &str, mutant: &Mutant) -> Option<String> {
    let mut lines: Vec<&str> = source.split('\n').collect();
    let line = lines.get_mut(mutant.line.checked_sub(1)?)?;
    let trailing_return = line.ends_with('\r');
    if line.trim_end_matches('\r') != mutant.original {
        return None;
    }
    let replacement = if trailing_return {
        format!("{}\r", mutant.mutated)
    } else {
        mutant.mutated.clone()
    };
    let mut output = String::with_capacity(source.len() + 8);
    for (index, current) in lines.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        if index + 1 == mutant.line {
            output.push_str(&replacement);
        } else {
            output.push_str(current);
        }
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "fn check(age: u32) -> bool {\n    // adults only\n    let label = \"a == b\";\n    age >= 18 && enabled == true\n}\n";

    #[test]
    fn makes_mutants_outside_strings_and_comments() {
        let found = mutants(SOURCE, 1, 5, 10);
        assert!(found.iter().all(|mutant| mutant.line == 4));
        let descriptions: Vec<&str> = found.iter().map(|mutant| mutant.description.as_str()).collect();
        assert_eq!(
            descriptions,
            ["equality flipped", "boundary moved", "and became or", "true became false"]
        );
    }

    fn descriptions_for(path: &str, line: &str) -> Vec<(String, String)> {
        mutants_for_path(path, line, 1, 1, MAX_LIMIT)
            .into_iter()
            .map(|mutant| (mutant.description, mutant.mutated))
            .collect()
    }

    fn mutated_by(path: &str, line: &str, description: &str) -> Vec<String> {
        descriptions_for(path, line)
            .into_iter()
            .filter(|(found, _)| found == description)
            .map(|(_, mutated)| mutated)
            .collect()
    }

    #[test]
    fn replaces_return_values_by_language() {
        assert_eq!(
            mutated_by("src/a.rs", "    return count + offset;", "return value replaced"),
            ["    return Default::default();"]
        );
        assert_eq!(
            mutated_by("src/a.rs", "    return Some(item);", "return value replaced"),
            ["    return None;"]
        );
        assert_eq!(mutated_by("a.ts", "  return user.name;", "return value replaced"), ["  return null;"]);
        assert_eq!(mutated_by("a.py", "    return total", "return value replaced"), ["    return None"]);
        assert_eq!(mutated_by("a.rb", "  return total", "return value replaced"), ["  return nil"]);
        assert_eq!(mutated_by("a.c", "  return size * 2;", "return value replaced"), ["  return 0;"]);

        for (path, line) in [
            ("a.rs", "    return Ok(value);"),
            ("a.rs", "    return &self.name;"),
            ("a.rs", "    return;"),
            ("a.rs", "    return compute("),
            ("a.rs", "    return Foo { a };"),
            ("a.py", "    return None"),
            ("a.py", "    return (first,"),
            ("a.js", "    return null;"),
            ("a.js", "    return value"),
            ("a.rb", "  return value if ready"),
            ("a.go", "    return value, nil"),
            ("a.txt", "    return value;"),
        ] {
            assert!(mutated_by(path, line, "return value replaced").is_empty(), "{path}: {line}");
        }
    }

    #[test]
    fn moves_boundaries_by_one() {
        assert_eq!(mutated_by("a.rs", "if a < b {", "boundary moved"), ["if a <= b {"]);
        assert_eq!(mutated_by("a.rs", "if a <= b {", "boundary moved"), ["if a < b {"]);
        assert_eq!(mutated_by("a.rs", "if a > b {", "boundary moved"), ["if a >= b {"]);
        assert_eq!(mutated_by("a.rs", "if a >= b {", "boundary moved"), ["if a > b {"]);
        assert_eq!(
            mutated_by("a.rs", "let end = start + 1;", "off by one"),
            ["let end = start - 1;", "let end = start + 2;"]
        );
        assert_eq!(
            mutated_by("a.rs", "let end = start - 1;", "off by one"),
            ["let end = start + 1;", "let end = start - 2;"]
        );
        assert!(mutated_by("a.rs", "let end = start + 10;", "off by one").is_empty());
    }

    #[test]
    fn deletes_plain_calls_only() {
        let source = "fn run() {\n    self.items.clear();\n    log::info!(\"done {}\", count);\n    save(path).await?;\n    let x = make();\n    total = add(a, b);\n    return finish();\n    foo(a,\n        b);\n    helper(a)(b);\n    super(props);\n}\n";
        let deleted: Vec<usize> = mutants_for_path("a.rs", source, 1, 12, MAX_LIMIT)
            .into_iter()
            .filter(|mutant| mutant.description == "call deleted")
            .map(|mutant| mutant.line)
            .collect();
        assert_eq!(deleted, [2, 3, 4]);
        let found = mutants_for_path("a.rs", source, 2, 2, MAX_LIMIT);
        assert_eq!(found[0].mutated, "    ");
        let applied = apply(source, &found[0]).expect("applies");
        assert!(!applied.contains("clear"));
        assert_eq!(applied.lines().count(), source.lines().count());
        assert!(mutated_by("a.py", "    print(total)", "call deleted").is_empty());
        assert_eq!(mutated_by("a.js", "    this.save();", "call deleted"), ["    "]);
    }

    #[test]
    fn swaps_two_plain_arguments() {
        assert_eq!(
            mutated_by("a.rs", "    let size = max(width, height);", "arguments swapped"),
            ["    let size = max(height, width);"]
        );
        assert_eq!(
            mutated_by("a.js", "  if (contains(list, item)) {", "arguments swapped"),
            ["  if (contains(item, list)) {"]
        );
        for line in [
            "  function max(a, b) {",
            "  def max(a, b):",
            "  max(a, b) {",
            "  let x = max(a, a);",
            "  let x = max(a, b + 1);",
            "  let x = max(a, b, c);",
            "  let x = \"max(a, b)\";",
        ] {
            assert!(mutated_by("a.js", line, "arguments swapped").is_empty(), "{line}");
        }
    }

    #[test]
    fn skips_go_error_branches() {
        assert_eq!(
            mutated_by("main.go", "\tif err != nil {", "error branch skipped"),
            ["\tif err != nil && false {"]
        );
        assert_eq!(
            mutated_by("main.go", "\tif writeErr != nil {", "error branch skipped"),
            ["\tif writeErr != nil && false {"]
        );
        assert!(mutated_by("main.go", "\tif user != nil {", "error branch skipped").is_empty());
        assert!(mutated_by("main.rs", "\tif err != nil {", "error branch skipped").is_empty());
    }

    #[test]
    fn reports_survivors_in_one_line() {
        let mutant = Mutant {
            line: 42,
            original: "    if (a >= b) {".into(),
            mutated: "    if (a > b) {".into(),
            description: "boundary moved".into(),
        };
        assert_eq!(
            survivor_line(&mutant),
            "line 42: if (a >= b) { → if (a > b) { survived — no test checks this change"
        );
        let deleted = Mutant {
            line: 7,
            original: "    save();".into(),
            mutated: "    ".into(),
            description: "call deleted".into(),
        };
        assert_eq!(
            survivor_line(&deleted),
            "line 7: save(); → (deleted) survived — no test checks this change"
        );
    }

    #[test]
    fn limits_spread_across_lines() {
        let source = "a == b && c < d;\ne == f || g > h;\n";
        let found = mutants_for_path("a.rs", source, 1, 2, 4);
        assert_eq!(found.len(), 4);
        assert_eq!(found.iter().filter(|mutant| mutant.line == 1).count(), 2);
        assert_eq!(mutants_for_path("a.rs", source, 1, 2, MAX_LIMIT).len(), 6);
    }

    #[test]
    fn applies_exactly_one_mutant() {
        let found = mutants(SOURCE, 4, 4, 1);
        let mutated = apply(SOURCE, &found[0]).expect("applies");
        assert!(mutated.contains("enabled != true"));
        assert_eq!(mutated.lines().count(), SOURCE.lines().count());
        assert!(apply("different", &found[0]).is_none());
    }
}
