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
            |command| match deletion_targets(command) {
                Some((DeleteSyntax::Unix, targets)) if !targets.is_empty() => {
                    Some(format!("find {} | head -50", targets.join(" ")))
                }
                _ => None,
            },
        ),
        (
            // PowerShell accepts any unambiguous prefix of `-Recurse`, and cmd's
            // `/s` may come after other switches such as `/q` or `/f`.
            r#"(?i)(^\s*|[;&|(]\s*|\b(?:cmd(?:\.exe)?\s+/[ck]|(?:powershell|pwsh)(?:\.exe)?(?:\s+-\w+)*)\s+["']?)(remove-item|ri|rmdir|rd|del|erase)\b[^|;&]*(\s-r(ec(u(r(s(e)?)?)?)?)?\b|\s/s\b)"#,
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
    RULES
        .iter()
        .find(|rule| rule.regex.is_match(command))
        .map(|rule| Irreversible {
            kind: rule.kind,
            effect: rule.effect,
            dry_run: (rule.dry_run)(command),
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeleteSyntax {
    Unix,
    Windows,
}

const COMMAND_WRAPPERS: [&str; 9] = [
    "sudo",
    "cmd",
    "cmd.exe",
    "/c",
    "/k",
    "powershell",
    "powershell.exe",
    "pwsh",
    "pwsh.exe",
];

const WINDOWS_DELETE_COMMANDS: [&str; 6] = ["remove-item", "ri", "rmdir", "rd", "del", "erase"];

/// The words naming what the first delete command in `command` removes, up to
/// the next shell separator or redirection, with flags left out.
fn deletion_targets(command: &str) -> Option<(DeleteSyntax, Vec<&str>)> {
    command.split([';', '&', '|', '\n']).find_map(|segment| {
        let mut words = segment.split_whitespace().skip_while(|word| {
            word.starts_with('-')
                || COMMAND_WRAPPERS
                    .iter()
                    .any(|wrapper| word.eq_ignore_ascii_case(wrapper))
        });
        let command_word = words.next()?.trim_start_matches(['"', '\'']);
        let syntax = if command_word == "rm" {
            DeleteSyntax::Unix
        } else if WINDOWS_DELETE_COMMANDS
            .iter()
            .any(|delete| command_word.eq_ignore_ascii_case(delete))
        {
            DeleteSyntax::Windows
        } else {
            return None;
        };
        let targets = words
            .take_while(|word| !word.contains(['>', '<']))
            .filter(|word| match syntax {
                DeleteSyntax::Unix => !word.starts_with('-'),
                DeleteSyntax::Windows => !is_windows_switch(word),
            })
            .collect();
        Some((syntax, targets))
    })
}

/// Whether a word is a PowerShell parameter or a cmd switch such as `/s` or
/// `/a:h`. Longer words starting with `/` are absolute paths.
fn is_windows_switch(word: &str) -> bool {
    if word.starts_with('-') {
        return true;
    }
    let Some(rest) = word.strip_prefix('/') else {
        return false;
    };
    let mut characters = rest.chars();
    characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic())
        && matches!(characters.next(), None | Some(':'))
}

/// For a recursive delete, counts what the named paths contain, so the
/// confirmation can say "412 files, 38 MB" instead of guessing. Paths are
/// resolved against `working_directory`; globs and variables are left out.
pub fn deletion_preview(command: &str, working_directory: &Path) -> Option<String> {
    let (_, words) = deletion_targets(command)?;
    let targets: Vec<PathBuf> = words
        .into_iter()
        .map(|word| word.trim_matches(['"', '\'']))
        .filter(|word| !word.is_empty() && !word.contains(['*', '?', '$', '~', '`']))
        .map(|word| working_directory.join(word))
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
            (
                "sqlite3 app.db 'delete from sessions;'",
                "delete without a where clause",
            ),
            ("npx prisma migrate deploy", "database migration"),
            ("terraform apply -auto-approve", "infrastructure change"),
            ("kubectl delete pod web-1", "cluster change"),
            ("cargo publish", "publishing a package"),
        ] {
            assert_eq!(
                classify(command).map(|found| found.kind),
                Some(kind),
                "{command}"
            );
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

    #[test]
    fn windows_recursive_deletes_are_caught() {
        for command in [
            "Remove-Item -Recurse foo",
            "Remove-Item foo -Recurse -Force",
            "remove-item -recurse foo",
            "rmdir /s /q build",
            "RMDIR /S build",
            "rd /s /q build",
            "del /s /q *.tmp",
            "rmdir /q /s build",
            "rd /Q /S x",
            "del /f /s /q x",
            "erase /s x",
            "Remove-Item x -r",
            "Remove-Item x -Rec",
            "ri -r x",
            "rmdir x -Recurse",
            "del x -Recurse",
            "cd app && rmdir /s /q build",
            "cmd /c rd /s /q build",
            "powershell -NoProfile -Command \"Remove-Item -Recurse -Force dist\"",
        ] {
            let found = classify(command).expect(command);
            assert_eq!(found.kind, "recursive delete", "{command}");
            assert_eq!(found.dry_run, None, "{command}");
        }
    }

    #[test]
    fn windows_single_deletes_are_reversible() {
        for command in [
            "Remove-Item foo.txt",
            "Remove-Item foo.txt -Force",
            "rmdir build",
            "del notes.txt",
            "del /q file.txt",
            "del /f /q file.txt",
            "ri notes.txt",
            "erase old.log",
        ] {
            assert_eq!(classify(command), None, "{command}");
        }
    }

    #[test]
    fn every_form_of_force_push_is_caught() {
        for command in [
            "git push --force-with-lease",
            "git push --force-with-lease origin main",
            "git push origin main --force",
            "git push origin +main",
        ] {
            let found = classify(command).expect(command);
            assert_eq!(found.kind, "force push", "{command}");
            assert_eq!(
                found.dry_run.as_deref(),
                Some("git fetch && git log --oneline HEAD..@{upstream}"),
                "{command}"
            );
        }
        assert_eq!(
            classify("git push origin --delete feature").map(|found| found.kind),
            Some("remote branch deletion")
        );
    }

    #[test]
    fn recursive_delete_suggests_listing_the_targets() {
        let found = classify("rm -rf build dist").expect("irreversible");
        assert_eq!(found.dry_run.as_deref(), Some("find build dist | head -50"));
        let found = classify("rm -r --verbose").expect("irreversible");
        assert_eq!(found.dry_run, None, "nothing named, nothing to list");
    }

    #[test]
    fn recursive_delete_listing_stops_at_shell_separators() {
        for (command, dry_run) in [
            ("rm -rf dist; npm run build", "find dist | head -50"),
            (
                "rm -rf node_modules && npm run build",
                "find node_modules | head -50",
            ),
            ("rm -rf build || true", "find build | head -50"),
            ("rm -rf build | tee log", "find build | head -50"),
            ("rm -rf build & wait", "find build | head -50"),
            ("rm -rf build 2>/dev/null", "find build | head -50"),
            ("cd app && rm -rf build", "find build | head -50"),
        ] {
            let found = classify(command).expect(command);
            assert_eq!(found.dry_run.as_deref(), Some(dry_run), "{command}");
        }
    }

    #[test]
    fn deletion_preview_stops_at_shell_separators() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path();
        std::fs::create_dir_all(root.join("dist")).expect("mkdir");
        std::fs::write(root.join("dist/app.js"), b"x").expect("write");
        std::fs::create_dir_all(root.join("npm")).expect("mkdir");
        std::fs::write(root.join("npm/other.js"), b"x").expect("write");

        let expected = format!("1 file (0.0 KB) under {}", root.join("dist").display());
        for command in [
            "rm -rf dist; npm run build",
            "rm -rf dist && npm run build",
            "rm -rf 'dist'&&npm run build",
            "rm -rf dist || npm run build",
            "rm -rf dist | npm run build",
            "rm -rf dist & npm run build",
        ] {
            assert_eq!(
                deletion_preview(command, root),
                Some(expected.clone()),
                "{command}"
            );
        }
    }

    #[test]
    fn deletion_preview_understands_windows_deletes() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path();
        std::fs::create_dir_all(root.join("build/nested")).expect("mkdir");
        std::fs::write(root.join("build/a.o"), b"x").expect("write");
        std::fs::write(root.join("build/nested/b.o"), b"x").expect("write");

        let expected = format!("2 files (0.0 KB) under {}", root.join("build").display());
        for command in [
            "rmdir /s /q build",
            "RD /S build",
            "Remove-Item build -Recurse",
            "Remove-Item -Recurse -Force build",
            "Remove-Item -Recurse -Force \"build\"; echo done",
            "del /s build",
            "cmd /c rd /s /q build",
        ] {
            assert_eq!(
                deletion_preview(command, root),
                Some(expected.clone()),
                "{command}"
            );
        }
    }

    #[test]
    fn deletion_preview_needs_concrete_targets() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path();
        assert_eq!(deletion_preview("rm -rf", root), None);
        assert_eq!(deletion_preview("rm -rf *.tmp", root), None);
        assert_eq!(deletion_preview("rm -rf build/* src/?.o", root), None);
        assert_eq!(deletion_preview("rm -rf $HOME ~/cache `pwd`", root), None);
        assert_eq!(deletion_preview("ls -la build", root), None, "no rm");
    }

    #[test]
    fn deletion_preview_counts_a_single_file() {
        let directory = tempfile::tempdir().expect("tempdir");
        let root = directory.path();
        std::fs::write(root.join("big.bin"), vec![0u8; 2 * 1_048_576]).expect("write");
        std::fs::write(root.join("small.txt"), vec![0u8; 1024]).expect("write");

        let preview = deletion_preview("rm -rf 'small.txt' *.log", root).expect("preview");
        assert_eq!(
            preview,
            format!("1 file (1.0 KB) under {}", root.join("small.txt").display())
        );

        let preview = deletion_preview("sudo rm -r \"big.bin\"", root).expect("preview");
        assert!(preview.starts_with("1 file (2.0 MB) under "), "{preview}");
    }

    #[test]
    fn deletion_preview_of_missing_paths_is_empty() {
        let directory = tempfile::tempdir().expect("tempdir");
        let preview = deletion_preview("rm -rf does-not-exist", directory.path()).expect("preview");
        assert!(preview.starts_with("0 files (0.0 KB) under "), "{preview}");
    }
}
