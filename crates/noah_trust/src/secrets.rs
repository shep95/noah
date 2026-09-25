//! Keeps secrets out of what shepherd sees. Command output and file contents
//! pass through [`redact`] before they reach the model, and secrets the
//! person keeps in noah's secret store are handed to commands as environment
//! variables at run time, then scrubbed back to their `$NAME` if a command
//! prints them.

use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Redacted {
    pub text: String,
    pub count: usize,
}

struct Pattern {
    regex: Regex,
    label: &'static str,
    /// The capture group holding the secret; the rest of the match (such as
    /// `password=`) stays so the output still makes sense.
    group: usize,
}

static PATTERNS: LazyLock<Vec<Pattern>> = LazyLock::new(|| {
    [
        (
            r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z0-9 ]*PRIVATE KEY-----",
            "private key",
            0,
        ),
        (r"\bAKIA[0-9A-Z]{16}\b", "AWS access key", 0),
        (r"\bgh[pousr]_[A-Za-z0-9]{36,}\b", "GitHub token", 0),
        (r"\bgithub_pat_[A-Za-z0-9_]{22,}\b", "GitHub token", 0),
        (r"\bglpat-[A-Za-z0-9_-]{20,}\b", "GitLab token", 0),
        (r"\bsk-ant-[A-Za-z0-9_-]{20,}\b", "Anthropic key", 0),
        (r"\bsk-(proj-)?[A-Za-z0-9_-]{20,}\b", "API key", 0),
        (r"\bxox[abprs]-[A-Za-z0-9-]{10,}\b", "Slack token", 0),
        (r"\bAIza[0-9A-Za-z_-]{35}\b", "Google API key", 0),
        (r"\b[rs]k_live_[0-9A-Za-z]{20,}\b", "Stripe key", 0),
        (r"\bnpm_[A-Za-z0-9]{36}\b", "npm token", 0),
        (
            r"\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b",
            "JSON web token",
            0,
        ),
        (
            r"(?i)\b[a-z]+://[^\s:/@]+:([^\s@/]{3,})@",
            "password in a URL",
            1,
        ),
        (
            r#"(?i)\b[A-Z0-9_]*(?:password|passwd|pwd|secret|api[_-]?key|access[_-]?key|auth[_-]?token|token|private[_-]?key|client[_-]?secret)[A-Z0-9_]*\s*[=:]\s*["']?([^\s"',;]{8,})"#,
            "credential",
            1,
        ),
    ]
    .into_iter()
    .filter_map(|(pattern, label, group)| {
        Some(Pattern {
            regex: Regex::new(pattern).ok()?,
            label,
            group,
        })
    })
    .collect()
});

/// Replaces secrets with `[redacted <kind>]`. `known` secrets (name, value)
/// from noah's secret store are replaced with `$NAME` first, so commands that
/// echo them stay readable.
pub fn redact(text: &str, known: &[(String, String)]) -> Redacted {
    let mut output = text.to_string();
    let mut count = 0;
    for (name, value) in known {
        if value.len() >= 4 && output.contains(value.as_str()) {
            count += output.matches(value.as_str()).count();
            output = output.replace(value.as_str(), &format!("${name}"));
        }
    }
    for pattern in PATTERNS.iter() {
        let mut replaced = String::with_capacity(output.len());
        let mut last = 0;
        for captures in pattern.regex.captures_iter(&output) {
            let Some(secret) = captures.get(pattern.group) else {
                continue;
            };
            if secret.as_str().starts_with('$') || secret.as_str().starts_with("[redacted") {
                continue;
            }
            replaced.push_str(&output[last..secret.start()]);
            replaced.push_str(&format!("[redacted {}]", pattern.label));
            last = secret.end();
            count += 1;
        }
        if last > 0 {
            replaced.push_str(&output[last..]);
            output = replaced;
        }
    }
    Redacted {
        text: output,
        count,
    }
}

/// Whether a name is usable as an environment variable for a brokered secret.
pub fn is_valid_secret_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_uppercase() || first == '_')
        && characters.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_output_is_untouched() {
        let output = "Compiling noah v0.1.0\nFinished in 3.2s\ntoken count: 42";
        let redacted = redact(output, &[]);
        assert_eq!(redacted.text, output);
        assert_eq!(redacted.count, 0);
    }

    #[test]
    fn well_known_secrets_are_hidden() {
        let output = "export GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123456789AB\nAKIAABCDEFGHIJKLMNOP";
        let redacted = redact(output, &[]);
        assert!(!redacted.text.contains("ghp_abc"));
        assert!(!redacted.text.contains("AKIAABCDEFGHIJKLMNOP"));
        assert_eq!(redacted.count, 2);
    }

    #[test]
    fn assignments_keep_their_name() {
        let redacted = redact("DATABASE_PASSWORD=hunter22hunter\nport=5432", &[]);
        assert_eq!(
            redacted.text,
            "DATABASE_PASSWORD=[redacted credential]\nport=5432"
        );
        let url = redact("postgres://app:s3cretpass@db.local/app", &[]);
        assert_eq!(url.text, "postgres://app:[redacted password in a URL]@db.local/app");
    }

    #[test]
    fn brokered_secrets_become_their_name() {
        let known = vec![("STRIPE_TEST".to_string(), "abc123value".to_string())];
        let redacted = redact("using abc123value now", &known);
        assert_eq!(redacted.text, "using $STRIPE_TEST now");
    }

    #[test]
    fn private_keys_are_hidden_whole() {
        let key = "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\ndef\n-----END OPENSSH PRIVATE KEY-----";
        assert_eq!(redact(key, &[]).text, "[redacted private key]");
    }

    #[test]
    fn secret_names() {
        assert!(is_valid_secret_name("STRIPE_KEY"));
        assert!(!is_valid_secret_name("stripe"));
        assert!(!is_valid_secret_name("1KEY"));
    }
}
