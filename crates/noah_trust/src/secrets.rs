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

const CREDENTIAL: &str = "credential";

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
            CREDENTIAL,
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

/// The kinds of secret on the lines a unified diff adds, so noah can warn
/// before a key is committed and pushed. In source code, unquoted
/// `name = value` assignments are skipped, since there they are usually
/// `key = read_key()`; in config and env files they are the usual way a
/// secret leaks, so they count.
pub fn find_in_added_lines(diff: &str) -> Vec<&'static str> {
    struct AddedText {
        in_config_file: bool,
        text: String,
    }
    let mut files: Vec<AddedText> = Vec::new();
    for line in diff.lines() {
        if let Some(header) = line.strip_prefix("+++") {
            let path = header.trim_start().split('\t').next().unwrap_or_default();
            let path = path.strip_prefix("b/").unwrap_or(path);
            files.push(AddedText {
                in_config_file: is_config_file(path),
                text: String::new(),
            });
        } else if let Some(added) = line.strip_prefix('+') {
            if files.is_empty() {
                files.push(AddedText {
                    in_config_file: false,
                    text: String::new(),
                });
            }
            if let Some(file) = files.last_mut() {
                file.text.push_str(added);
                file.text.push('\n');
            }
        }
    }
    let mut kinds = Vec::new();
    for pattern in PATTERNS.iter() {
        let found = files.iter().any(|file| {
            pattern.regex.captures_iter(&file.text).any(|captures| {
                let Some(secret) = captures.get(pattern.group) else {
                    return false;
                };
                if pattern.label == CREDENTIAL && !file.in_config_file {
                    let quoted = file.text[..secret.start()].ends_with(['"', '\'']);
                    if !quoted {
                        return false;
                    }
                }
                !is_placeholder(secret.as_str())
            })
        });
        if found && !kinds.contains(&pattern.label) {
            kinds.push(pattern.label);
        }
    }
    kinds
}

fn is_config_file(path: &str) -> bool {
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_lowercase();
    name == ".env"
        || name.starts_with(".env.")
        || name.starts_with("docker-compose")
        || [
            ".env",
            ".ini",
            ".properties",
            ".yml",
            ".yaml",
            ".toml",
            ".cfg",
            ".conf",
        ]
        .iter()
        .any(|extension| name.ends_with(extension))
}

fn is_placeholder(value: &str) -> bool {
    let lowercase = value.to_lowercase();
    value.starts_with(['$', '<', '{'])
        || value.starts_with("[redacted")
        || [
            "your",
            "example",
            "placeholder",
            "changeme",
            "xxxx",
            "dummy",
            "test",
        ]
        .iter()
        .any(|word| lowercase.contains(word))
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
        let output =
            "export GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123456789AB\nAKIAABCDEFGHIJKLMNOP";
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
        assert_eq!(
            url.text,
            "postgres://app:[redacted password in a URL]@db.local/app"
        );
    }

    #[test]
    fn brokered_secrets_become_their_name() {
        let known = vec![("STRIPE_TEST".to_string(), "abc123value".to_string())];
        let redacted = redact("using abc123value now", &known);
        assert_eq!(redacted.text, "using $STRIPE_TEST now");
    }

    #[test]
    fn private_keys_are_hidden_whole() {
        let key =
            "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\ndef\n-----END OPENSSH PRIVATE KEY-----";
        assert_eq!(redact(key, &[]).text, "[redacted private key]");
    }

    #[test]
    fn finds_secrets_a_commit_adds() {
        let diff = "diff --git a/.env b/.env\n+++ b/.env\n\
+GITHUB_TOKEN=ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\
+const API_KEY = \"q8Zr2mTx9LwP4vKs\";\n\
-OLD_TOKEN=ghp_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n";
        assert_eq!(
            find_in_added_lines(diff),
            vec!["GitHub token", "credential"]
        );
    }

    #[test]
    fn ordinary_code_is_not_a_secret() {
        let diff = "+++ b/src/main.rs\n\
+let api_key = std::env::var(\"VENICE_API_KEY\")?;\n\
+token = read_token_from_keychain()\n\
+API_KEY = \"your-api-key-here\"\n";
        assert!(find_in_added_lines(diff).is_empty());
    }

    #[test]
    fn passwords_in_urls_are_found() {
        let diff = "+++ b/src/db.rs\n\
+let url = \"postgres://app:hunter22@db.local/app\";\n\
+let other = postgres_url(\"postgres://app:${DB_PASSWORD}@db.local/app\");\n";
        assert_eq!(find_in_added_lines(diff), vec!["password in a URL"]);
    }

    #[test]
    fn unquoted_values_in_config_files_are_secrets() {
        for (path, line) in [
            (".env", "DATABASE_PASSWORD=hunter22hunter"),
            (".env.local", "API_KEY=q8Zr2mTx9LwP4vKs"),
            ("deploy/prod.env", "CLIENT_SECRET=q8Zr2mTx9LwP4vKs"),
            ("config/settings.yml", "  api_key: q8Zr2mTx9LwP4vKs"),
            ("app.properties", "db.password=hunter22hunter"),
            (
                "docker-compose.override.yml",
                "      POSTGRES_PASSWORD: hunter22hunter",
            ),
        ] {
            let diff = format!("diff --git a/{path} b/{path}\n+++ b/{path}\n+{line}\n");
            assert_eq!(find_in_added_lines(&diff), vec!["credential"], "{path}");
        }
    }

    #[test]
    fn placeholders_in_config_files_are_not_secrets() {
        let diff = "+++ b/.env.example\n\
+API_KEY=your-api-key-here\n\
+DATABASE_PASSWORD=${DATABASE_PASSWORD}\n\
+SECRET_TOKEN=changeme123\n";
        assert!(find_in_added_lines(diff).is_empty());
    }

    #[test]
    fn config_rules_apply_only_to_config_files() {
        let diff = "+++ b/.env\n\
+PORT=3000\n\
+++ b/src/main.rs\n\
+token = read_token_from_keychain()\n";
        assert!(find_in_added_lines(diff).is_empty());
    }

    #[test]
    fn secret_names() {
        assert!(is_valid_secret_name("STRIPE_KEY"));
        assert!(!is_valid_secret_name("stripe"));
        assert!(!is_valid_secret_name("1KEY"));
    }
}
