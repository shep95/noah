//! Mutation testing for the lines shepherd changed: small deliberate bugs
//! (a flipped comparison, a swapped boolean) are put in one at a time and the
//! tests are run. A mutant the tests don't catch marks behavior no test
//! checks, which line coverage can't show.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mutant {
    /// 1-based line.
    pub line: usize,
    pub original: String,
    pub mutated: String,
    pub description: String,
}

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
    (" + ", " - ", "plus became minus"),
    (" - ", " + ", "minus became plus"),
    ("+ 1", "+ 2", "off by one"),
    ("- 1", "- 2", "off by one"),
];

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
pub fn mutants(source: &str, first: usize, last: usize, limit: usize) -> Vec<Mutant> {
    let mut per_line: Vec<Vec<Mutant>> = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let number = index + 1;
        if number < first || number > last || is_comment_or_blank(line) {
            continue;
        }
        let mut line_mutants = Vec::new();
        for (from, to, description) in SWAPS {
            let Some(position) = find_outside_strings(line, from) else {
                continue;
            };
            let word_like = from.chars().all(char::is_alphanumeric);
            if word_like {
                let before = line[..position].chars().last();
                let after = line[position + from.len()..].chars().next();
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_')
                    || after.is_some_and(|c| c.is_alphanumeric() || c == '_')
                {
                    continue;
                }
            }
            let mut mutated = line.to_string();
            mutated.replace_range(position..position + from.len(), to);
            if line_mutants.iter().any(|mutant: &Mutant| mutant.mutated == mutated) {
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

    #[test]
    fn applies_exactly_one_mutant() {
        let found = mutants(SOURCE, 4, 4, 1);
        let mutated = apply(SOURCE, &found[0]).expect("applies");
        assert!(mutated.contains("enabled != true"));
        assert_eq!(mutated.lines().count(), SOURCE.lines().count());
        assert!(apply("different", &found[0]).is_none());
    }
}
