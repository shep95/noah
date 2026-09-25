//! Tells reversible commands from ones that can't be taken back. Reversible
//! work stays frictionless; for irreversible commands noah always asks, even
//! when the person has told it to allow the terminal, and shows what would be
//! affected and how to check first.

use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Irreversible {
    /// What kind of action this is, in plain words.
    pub kind: &'static str,
    /// What could be lost.
    pub effect: &'static str,
    /// A command that shows what would happen without doing it, when there is one.
    pub dry_run: Option<String>,
}

struct Rule {
    regex: Regex,
    kind: &'static str,
    effect: &'static str,
    dry_run: fn(&str) -> Option<String>,
}

fn none(_: &str) -> Option<String> {
    None
}

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    let rules: [(&str, &str, &str, fn(&str) -> Option<String>); 16] = [
        (
            r"(?i)(^|[;&|]\s*|\bsudo\s+)rm\s+(-[a-z]*r[a-z]*|-[a-z]*\s+-r|--recursive)\b",
            "recursive delete",
            "files and folders are deleted permanently, without going to the trash",
            |command| {
                let targets: Vec<&str> = command
                    .split_whitespace()
                    .skip_while(|word| *word != "rm")
                    .skip(1)
                    .filter(|word| !word.starts_with('-'))
                    .collect();
                (!targets.is_empty()).then(|| format!("find {} | head -50", targets.join(" ")))
            },
        ),
        (
            r"(?i)\b(Remove-Item\b.*-Recurse|rmdir\s+/s|rd\s+/s|del\s+/[sq])",
            "recursive delete",
            "files and folders are deleted permanently",
            none,
        ),
        (
            r"\bgit\s+push\b.*(\s--force\b|\s-f\b|\s--force-with-lease\b|\s\+\S)",
            "force push",
            "commits on the remote branch that aren't in yours are discarded for everyone",
            |_| Some("git fetch && git log --oneline HEAD..@{upstream}".to_string()),
        ),
        (
            r"\bgit\s+push\b.*\s(--delete|-d)\b",
            "remote branch deletion",
            "the branch is removed from the remote for everyone",
            none,
        ),
        (
            r"\bgit\s+reset\s+--hard\b",
            "hard reset",
            "uncommitted changes in tracked files are thrown away",
            |_| Some("git status --short && git diff --stat".to_string()),
        ),
        (
            r"\bgit\s+clean\b.*\s-[a-z]*f",
            "git clean",
            "untracked files are deleted permanently",
            |command| Some(command.replacen("clean", "clean -n", 1)),
        ),
        (
            r"\bgit\s+(checkout|restore)\s+(--\s+)?\.(\s|$)|\bgit\s+branch\s+-D\b|\bgit\s+stash\s+(drop|clear)\b",
            "discarding git work",
            "local changes, a branch or stashed work are thrown away",
            |_| Some("git status --short && git stash list".to_string()),
        ),
        (
            r"(?i)\b(drop\s+(table|database|schema|index|view|column)|truncate\s+(table\s+)?\w+|alter\s+table\s+\w+\s+drop)\b",
            "destructive database change",
            "tables, columns or rows are removed from the database",
            none,
        ),
        (
            r"(?i)\bdelete\s+from\s+\w+\s*(;|$)",
            "delete without a where clause",
            "every row in the table is removed",
            none,
        ),
        (
            r"(?i)\b(migrate|migration\s+run|db:migrate|db\s+push|alembic\s+upgrade|prisma\s+migrate\s+deploy|flyway\s+migrate|liquibase\s+update)\b",
            "database migration",
            "the database schema changes, and rolling back may lose data",
            none,
        ),
        (
            r"(?i)\b(terraform|tofu)\s+(apply|destroy)\b|\bpulumi\s+(up|destroy)\b",
            "infrastructure change",
            "cloud resources are created, changed or destroyed",
            |command| {
                Some(if command.contains("pulumi") {
                    "pulumi preview".to_string()
                } else {
                    "terraform plan".to_string()
                })
            },
        ),
        (
            r"(?i)\bkubectl\s+(delete|apply|replace|drain|scale)\b|\bhelm\s+(uninstall|upgrade|rollback)\b",
            "cluster change",
            "running services in the cluster are changed or removed",
            |command| {
                command
                    .contains("kubectl")
                    .then(|| format!("{command} --dry-run=server"))
            },
        ),
        (
            r"(?i)\b(aws|gcloud|az|doctl|flyctl|heroku)\b.*\b(delete|destroy|rm|remove|terminate)\b",
            "cloud resource deletion",
            "cloud resources and their data are deleted",
            none,
        ),
        (
            r"(?i)\b(npm|pnpm|yarn|cargo|twine|gem|poetry)\s+publish\b|\bdocker\s+push\b",
            "publishing a package",
            "a version is published for everyone and usually can't be taken back",
            |command| Some(format!("{command} --dry-run")),
        ),
        (
            r"(?i)\b(dd\s+if=|mkfs(\.\w+)?\s|diskpart|format\s+[a-z]:|shred\s)",
            "disk overwrite",
            "a disk or partition is overwritten",
            none,
        ),
        (
            r"(?i)\b(chmod|chown)\s+-R\b|\b(shutdown|reboot|halt)\b",
            "system change",
            "permissions across many files or the machine's state change",
            none,
        ),
    ];
    rules
        .into_iter()
        .filter_map(|(pattern, kind, effect, dry_run)| {
            Some(Rule {
                regex: Regex::new(pattern).ok()?,
                kind,
                effect,
                dry_run,
            })
        })
        .collect()
});

pub fn classify(command: &str) -> Option<Irreversible> {
    RULES.iter().find(|rule| rule.regex.is_match(command)).map(|rule| Irreversible {
        kind: rule.kind,
        effect: rule.effect,
        dry_run: (rule.dry_run)(command),
    })
}

/// For a recursive delete, counts what the named paths contain, so the
/// confirmation can say "412 files, 38 MB" instead of guessing. Paths are
/// resolved against `working_directory`; globs and variables are left out.
pub fn deletion_preview(command: &str, working_directory: &Path) -> Option<String> {
    let targets: Vec<PathBuf> = command
        .split_whitespace()
        .skip_while(|word| *word != "rm")
        .skip(1)
        .filter(|word| !word.starts_with('-') && !word.contains(['*', '?', '$', '~', '`']))
        .map(|word| working_directory.join(word.trim_matches(['"', '\''])))
        .collect();
    if targets.is_empty() {
        return None;
    }
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut stack = targets.clone();
    while let Some(path) = stack.pop() {
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                stack.extend(entries.flatten().map(|entry| entry.path()));
            }
        } else {
            files += 1;
            bytes += metadata.len();
        }
        if files > 1_000_000 {
            break;
        }
    }
    let size = if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    };
    Some(format!(
        "{files} file{} ({size}) under {}",
        if files == 1 { "" } else { "s" },
        targets
            .iter()
            .map(|target| target.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everyday_commands_are_reversible() {
        for command in [
            "cargo test",
            "git status",
            "git push origin main",
            "rm notes.txt",
            "npm install",
            "ls -la",
            "git commit -m 'drop support for ie11'",
        ] {
            assert_eq!(classify(command), None, "{command}");
        }
    }

    #[test]
    fn irreversible_commands_are_caught() {
        for (command, kind) in [
            ("rm -rf target", "recursive delete"),
            ("sudo rm -r /var/app", "recursive delete"),
            ("git push --force origin main", "force push"),
            ("git push -f", "force push"),
            ("git reset --hard HEAD~3", "hard reset"),
            ("git clean -fdx", "git clean"),
            ("psql -c 'DROP TABLE users'", "destructive database change"),
            ("sqlite3 app.db 'delete from sessions;'", "delete without a where clause"),
            ("npx prisma migrate deploy", "database migration"),
            ("terraform apply -auto-approve", "infrastructure change"),
            ("kubectl delete pod web-1", "cluster change"),
            ("cargo publish", "publishing a package"),
        ] {
            assert_eq!(classify(command).map(|found| found.kind), Some(kind), "{command}");
        }
    }

    #[test]
    fn suggests_a_dry_run() {
        let found = classify("git clean -fd").expect("irreversible");
        assert_eq!(found.dry_run.as_deref(), Some("git clean -n -fd"));
        let found = classify("kubectl apply -f deploy.yaml").expect("irreversible");
        assert_eq!(
            found.dry_run.as_deref(),
            Some("kubectl apply -f deploy.yaml --dry-run=server")
        );
    }

    #[test]
    fn previews_deletions() {
        let directory = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(directory.path().join("build/nested")).expect("mkdir");
        std::fs::write(directory.path().join("build/a.o"), vec![0u8; 1024]).expect("write");
        std::fs::write(directory.path().join("build/nested/b.o"), b"x").expect("write");
        let preview = deletion_preview("rm -rf build", directory.path()).expect("preview");
        assert!(preview.starts_with("2 files"), "{preview}");
    }
}
