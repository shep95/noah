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
    /// For patterns that only say where a secret could be: the least Shannon
    /// entropy, in bits per character, a value needs to count as one.
    min_entropy: Option<f64>,
}

const CREDENTIAL: &str = "credential";
const HIGH_ENTROPY: &str = "high-entropy secret";

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
        (r"\bhf_[A-Za-z0-9]{30,}\b", "Hugging Face token", 0),
        (
            r"\bSG\.[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}",
            "SendGrid key",
            0,
        ),
        (r"\bSK[0-9a-fA-F]{32}\b", "Twilio key", 0),
        (
            r"\b[MN][A-Za-z\d]{23,}\.[\w-]{6}\.[\w-]{27,}",
            "Discord token",
            0,
        ),
        (
            r"AccountKey=([A-Za-z0-9+/=]{40,})",
            "Azure storage key",
            1,
        ),
        (
            r#""private_key_id"\s*:\s*"([0-9a-f]{40})""#,
            "GCP service account key",
            1,
        ),
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
    .map(|(pattern, label, group)| (pattern, label, group, None))
    .chain([(
        r#"(?i)\b[A-Z0-9_]*(?:KEY|SECRET|TOKEN)["']?\s*[=:]\s*["']?([A-Za-z0-9+/=_.\-]{32,})"#,
        HIGH_ENTROPY,
        1,
        Some(MIN_SECRET_ENTROPY),
    )])
    .filter_map(|(pattern, label, group, min_entropy)| {
        Some(Pattern {
            regex: Regex::new(pattern).ok()?,
            label,
            group,
            min_entropy,
        })
    })
    .collect()
});

/// Random keys sit well above this; words, repeated characters and
/// placeholders sit below it.
const MIN_SECRET_ENTROPY: f64 = 3.5;

/// Shannon entropy in bits per character.
fn entropy(value: &str) -> f64 {
    let mut counts: Vec<(char, usize)> = Vec::new();
    let mut total = 0usize;
    for character in value.chars() {
        total += 1;
        match counts.iter_mut().find(|(seen, _)| *seen == character) {
            Some((_, count)) => *count += 1,
            None => counts.push((character, 1)),
        }
    }
    if total == 0 {
        return 0.0;
    }
    counts
        .iter()
        .map(|(_, count)| {
            let probability = *count as f64 / total as f64;
            -probability * probability.log2()
        })
        .sum()
}

impl Pattern {
    /// Whether a match of this pattern is a secret rather than a value that
    /// only sits where one could. Generated keys mix letters and digits;
    /// long identifiers such as `derive_key_from_password` don't, and must
    /// stay readable in code shepherd reads.
    fn is_secret(&self, value: &str) -> bool {
        match self.min_entropy {
            Some(min_entropy) => {
                entropy(value) >= min_entropy
                    && value.chars().any(|character| character.is_ascii_digit())
                    && value.chars().any(|character| character.is_ascii_alphabetic())
                    && !is_placeholder(value)
            }
            None => true,
        }
    }
}

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
            if secret.as_str().starts_with('$')
                || secret.as_str().starts_with("[redacted")
                || !pattern.is_secret(secret.as_str())
            {
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
                if (pattern.label == CREDENTIAL || pattern.label == HIGH_ENTROPY)
                    && !file.in_config_file
                {
                    let quoted = file.text[..secret.start()].ends_with(['"', '\'']);
                    if !quoted {
                        return false;
                    }
                }
                !is_placeholder(secret.as_str()) && pattern.is_secret(secret.as_str())
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
    fn more_provider_tokens_are_hidden() {
        // Split so GitHub push protection doesn't mistake the fixtures for real tokens.
        for (text, label) in [
            (
                concat!("HF=hf", "_abcdefghijklmnopqrstuvwxyzABCDEFGH"),
                "Hugging Face token",
            ),
            (
                concat!("SG.aBcDeFgHiJkLmNoPqRsT", ".uVwXyZ0123456789_-abcdEFGHijkl"),
                "SendGrid key",
            ),
            (
                concat!("sid SK", "0123456789abcdef0123456789ABCDEF"),
                "Twilio key",
            ),
            (
                concat!(
                    "bot MTk4NjIyNDgzNDcxOTI1MjQ4",
                    ".Cl2FMQ.ZnCjm1XVW7vRze4b7Cq4se7kKWs"
                ),
                "Discord token",
            ),
        ] {
            let redacted = redact(text, &[]);
            assert!(
                redacted.text.contains(&format!("[redacted {label}]")),
                "{text} -> {}",
                redacted.text
            );
            let diff = format!("+++ b/src/config.rs\n+let value = \"{text}\";\n");
            assert!(find_in_added_lines(&diff).contains(&label), "{text}");
        }
    }

    #[test]
    fn azure_account_keys_are_hidden_but_not_the_connection_string() {
        let key = "Zm9vYmFyYmF6cXV4MTIzNDU2Nzg5MGFiY2RlZmdoaWprbG1ub3BxcnN0dXZ3eHl6QUJDREVGR0g=";
        let text = format!(
            "DefaultEndpointsProtocol=https;AccountName=store;AccountKey={key};EndpointSuffix=core.windows.net"
        );
        assert_eq!(
            redact(&text, &[]).text,
            "DefaultEndpointsProtocol=https;AccountName=store;AccountKey=[redacted Azure storage key];EndpointSuffix=core.windows.net"
        );
        let diff = format!("+++ b/src/storage.ts\n+const connection = \"{text}\";\n");
        assert_eq!(find_in_added_lines(&diff), vec!["Azure storage key"]);
    }

    #[test]
    fn gcp_service_account_files_are_hidden() {
        let json = r#"{
  "type": "service_account",
  "private_key_id": "0123456789abcdef0123456789abcdef01234567",
  "private_key": "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASC\n-----END PRIVATE KEY-----\n",
  "client_email": "bot@project.iam.gserviceaccount.com"
}"#;
        let redacted = redact(json, &[]);
        assert!(redacted
            .text
            .contains(r#""private_key_id": "[redacted GCP service account key]""#));
        assert!(redacted.text.contains(r#""private_key": "[redacted private key]\n""#));
        assert!(!redacted.text.contains("MIIEvQ"));
        assert!(redacted.text.contains("bot@project.iam.gserviceaccount.com"));

        let diff = format!(
            "+++ b/service-account.json\n{}\n",
            json.lines().map(|line| format!("+{line}")).collect::<Vec<_>>().join("\n")
        );
        let kinds = find_in_added_lines(&diff);
        assert!(kinds.contains(&"private key"));
        assert!(kinds.contains(&"GCP service account key"));
    }

    #[test]
    fn high_entropy_values_under_key_names_are_secrets() {
        let value = "q8Zr2mTx9LwP4vKs7NbY3cHd6FjG1eUa";
        assert!(entropy(value) >= MIN_SECRET_ENTROPY);
        for line in [
            format!("SIGNING_KEY={value}"),
            format!("stripe_key: {value}"),
            format!("WebhookSecretKey = \"{value}\""),
        ] {
            let redacted = redact(&line, &[]);
            assert!(!redacted.text.contains(value), "{line}");
            assert_eq!(redacted.count, 1, "{line}");
        }
        let diff = format!("+++ b/.env\n+SIGNING_KEY={value}\n");
        assert_eq!(find_in_added_lines(&diff), vec![HIGH_ENTROPY]);
        let diff = format!("+++ b/src/sign.py\n+SIGNING_KEY = \"{value}\"\n");
        assert_eq!(find_in_added_lines(&diff), vec![HIGH_ENTROPY]);
    }

    #[test]
    fn low_entropy_placeholders_and_ordinary_values_are_not_secrets() {
        let image = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
        for line in [
            "CACHE_KEY=aaaaaaaabbbbbbbbccccccccdddddddd".to_string(),
            "SIGNING_KEY=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx".to_string(),
            "SIGNING_KEY=your_signing_key_goes_here_Q8zR2mTx9LwP".to_string(),
            "SIGNING_KEY=example7Zr2mTx9LwP4vKs7NbY3cHd6FjG1eUa".to_string(),
            "REQUEST_ID=550e8400-e29b-41d4-a716-446655440000".to_string(),
            format!("IMAGE_DATA={image}"),
            "let cache_key = compute_cache_key(&request);".to_string(),
            "let session_key = derive_session_key_from_master_password_and_salt(salt);".to_string(),
        ] {
            let redacted = redact(&line, &[]);
            assert_eq!(redacted.text, line);
            let diff = format!("+++ b/.env\n+{line}\n");
            assert!(find_in_added_lines(&diff).is_empty(), "{line}");
        }
        let diff = "+++ b/src/main.rs\n\
+let token = read_token();\n\
+let session_key = derive_session_key_from_master_secret_and_salt(salt);\n\
+SIGNING_KEY = q8Zr2mTx9LwP4vKs7NbY3cHd6FjG1eUa\n";
        assert!(find_in_added_lines(diff).is_empty());
    }

    #[test]
    fn secret_names() {
        assert!(is_valid_secret_name("STRIPE_KEY"));
        assert!(!is_valid_secret_name("stripe"));
        assert!(!is_valid_secret_name("1KEY"));
    }
}
