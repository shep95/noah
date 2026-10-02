//! Backup honesty: whether this machine has a backup, how old the newest one
//! is, and, where the backup can be read directly (Time Machine), whether a
//! sample file actually comes back from it. A sync folder is not a backup and
//! is not counted as one.

#[cfg(not(target_os = "windows"))]
use std::path::Path;

use chrono::{DateTime, Utc};

use crate::checks::{Category, Check, Status};
use crate::{COMMAND_TIMEOUT, run_command};

const ID: &str = "backup";
const TITLE: &str = "backup that restores";
const STALE_DAYS: i64 = 7;

/// What was found, before it is judged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finding {
    pub tool: String,
    pub last: Option<DateTime<Utc>>,
    /// `Some(true)` when a sample file was read back from the backup,
    /// `Some(false)` when it was looked for and not found, `None` when the
    /// backup can't be read from here.
    pub restored: Option<bool>,
    pub sample: Option<String>,
}

pub(crate) fn judge(finding: Option<&Finding>, now: DateTime<Utc>) -> Check {
    let Some(finding) = finding else {
        return Check::new(
            ID,
            TITLE,
            Category::Protection,
            Status::Bad,
            "no backup tool is set up on this machine. a disk failure, a theft or ransomware would take everything with it.",
        )
        .recommend(if cfg!(target_os = "macos") {
            "turn on Time Machine with an external disk: System Settings, General, Time Machine"
        } else if cfg!(target_os = "windows") {
            "turn on File History with an external disk: Settings, System, Storage, Advanced storage settings, Backup options"
        } else {
            "set up Déjà Dup (Backups) or Timeshift with an external disk"
        });
    };
    let age_days = finding.last.map(|last| (now - last).num_days());
    let age_text = match (finding.last, age_days) {
        (Some(last), Some(days)) => format!(
            "the newest {} backup is from {} ({} day{} ago)",
            finding.tool,
            last.format("%Y-%m-%d"),
            days,
            if days == 1 { "" } else { "s" }
        ),
        _ => format!("{} is set up, but noah couldn't tell when it last ran", finding.tool),
    };
    let restore_text = match (finding.restored, &finding.sample) {
        (Some(true), Some(sample)) => format!("; {sample} was read back from it, so it restores"),
        (Some(false), Some(sample)) => {
            format!("; {sample} was not in it, so files you expect may be missing")
        }
        _ => "; noah can't read this kind of backup directly, so restoring was not tested".to_string(),
    };
    let status = match (age_days, finding.restored) {
        (_, Some(false)) => Status::Warning,
        (Some(days), _) if days <= STALE_DAYS => {
            if finding.restored == Some(true) {
                Status::Good
            } else {
                Status::Warning
            }
        }
        (Some(_), _) => Status::Warning,
        (None, _) => Status::Unknown,
    };
    let check = Check::new(
        ID,
        TITLE,
        Category::Protection,
        status,
        format!("{age_text}{restore_text}."),
    );
    match status {
        Status::Good => check,
        _ if age_days.is_some_and(|days| days > STALE_DAYS) => {
            check.recommend("plug in the backup disk and let a backup finish")
        }
        _ => check.recommend("restore one file from the backup by hand once, to know it works"),
    }
}

/// Blocking. Looks for this OS's backup tools.
pub fn backup_check() -> Check {
    judge(platform_finding().as_ref(), Utc::now())
}

#[cfg(target_os = "macos")]
fn platform_finding() -> Option<Finding> {
    let output = run_command("tmutil", &["latestbackup"], COMMAND_TIMEOUT).ok()?;
    let latest = output.stdout.trim().to_string();
    if !output.success() || latest.is_empty() {
        let destinations = run_command("tmutil", &["destinationinfo"], COMMAND_TIMEOUT).ok()?;
        if destinations.combined().contains("Name") {
            return Some(Finding {
                tool: "Time Machine".into(),
                last: None,
                restored: None,
                sample: None,
            });
        }
        return None;
    }
    // Backups are folders named like 2026-10-01-093012(.backup).
    let name = Path::new(&latest)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let stamp: String = name.chars().take(17).collect();
    let last = chrono::NaiveDateTime::parse_from_str(&stamp, "%Y-%m-%d-%H%M%S")
        .ok()
        .map(|time| time.and_utc());
    let (restored, sample) = match sample_file() {
        Some(sample) => {
            let relative = sample.strip_prefix("/").unwrap_or(&sample).to_path_buf();
            let in_backup = find_in_backup(Path::new(&latest), &relative);
            (
                Some(in_backup.is_some_and(|path| std::fs::read(path).is_ok())),
                Some(sample.display().to_string()),
            )
        }
        None => (None, None),
    };
    Some(Finding {
        tool: "Time Machine".into(),
        last,
        restored,
        sample,
    })
}

/// Time Machine keeps the volume name as the first folder; the file is under
/// it at its full path.
#[cfg(target_os = "macos")]
fn find_in_backup(latest: &Path, relative: &Path) -> Option<std::path::PathBuf> {
    let volumes = std::fs::read_dir(latest).ok()?;
    for volume in volumes.flatten() {
        let candidate = volume.path().join(relative);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// A file a backup of this person's home must hold: an older document that
/// has not changed in a day, so it is in any backup newer than that.
#[cfg(target_os = "macos")]
fn sample_file() -> Option<std::path::PathBuf> {
    let home = dirs::home_dir()?;
    let day_ago = std::time::SystemTime::now().checked_sub(std::time::Duration::from_secs(86_400))?;
    for folder in ["Documents", "Desktop", "Pictures"] {
        let Ok(entries) = std::fs::read_dir(home.join(folder)) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let hidden = path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'));
            if metadata.is_file()
                && !hidden
                && metadata.len() > 0
                && metadata.modified().is_ok_and(|modified| modified < day_ago)
            {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn platform_finding() -> Option<Finding> {
    let local = dirs::data_local_dir()?;
    let configuration = local
        .join("Microsoft")
        .join("Windows")
        .join("FileHistory")
        .join("Configuration");
    if configuration.join("Config1.xml").is_file() || configuration.join("Config2.xml").is_file() {
        let last = std::fs::read_dir(&configuration)
            .ok()?
            .flatten()
            .filter_map(|entry| entry.metadata().ok()?.modified().ok())
            .max()
            .map(DateTime::<Utc>::from);
        return Some(Finding {
            tool: "File History".into(),
            last,
            restored: None,
            sample: None,
        });
    }
    let output = run_command(
        "powershell",
        &[
            "-NoProfile",
            "-Command",
            "(Get-WBSummary -ErrorAction SilentlyContinue).LastSuccessfulBackupTime",
        ],
        COMMAND_TIMEOUT,
    )
    .ok()?;
    let text = output.stdout.trim();
    if text.is_empty() {
        return None;
    }
    Some(Finding {
        tool: "Windows Server Backup".into(),
        last: chrono::DateTime::parse_from_rfc2822(text)
            .ok()
            .map(|time| time.with_timezone(&Utc)),
        restored: None,
        sample: None,
    })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_finding() -> Option<Finding> {
    if let Ok(output) = run_command(
        "gsettings",
        &["get", "org.gnome.DejaDup", "last-backup"],
        COMMAND_TIMEOUT,
    ) {
        let stamp = output.stdout.trim().trim_matches('\'');
        if output.success() && !stamp.is_empty() {
            return Some(Finding {
                tool: "Déjà Dup".into(),
                last: DateTime::parse_from_rfc3339(stamp)
                    .ok()
                    .map(|time| time.with_timezone(&Utc)),
                restored: None,
                sample: None,
            });
        }
    }
    let timeshift = Path::new("/etc/timeshift/timeshift.json");
    if timeshift.is_file() {
        let snapshots = Path::new("/timeshift/snapshots");
        let last = std::fs::read_dir(snapshots).ok().and_then(|entries| {
            entries
                .flatten()
                .filter_map(|entry| {
                    chrono::NaiveDateTime::parse_from_str(
                        &entry.file_name().to_string_lossy(),
                        "%Y-%m-%d_%H-%M-%S",
                    )
                    .ok()
                })
                .max()
                .map(|time| time.and_utc())
        });
        return Some(Finding {
            tool: "Timeshift (system files only, not your home folder)".into(),
            last,
            restored: None,
            sample: None,
        });
    }
    for tool in ["restic", "borg", "duplicity", "rsnapshot"] {
        if which::which(tool).is_ok() {
            return Some(Finding {
                tool: tool.to_string(),
                last: None,
                restored: None,
                sample: None,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: u32) -> DateTime<Utc> {
        chrono::NaiveDate::from_ymd_opt(2026, 10, day)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .map(|time| time.and_utc())
            .expect("valid date")
    }

    #[test]
    fn no_backup_is_bad_and_a_tested_fresh_one_is_good() {
        assert_eq!(judge(None, at(2)).status, Status::Bad);
        let fresh = Finding {
            tool: "Time Machine".into(),
            last: Some(at(1)),
            restored: Some(true),
            sample: Some("~/Documents/a.pdf".into()),
        };
        let check = judge(Some(&fresh), at(2));
        assert_eq!(check.status, Status::Good, "{}", check.detail);
        assert!(check.detail.contains("restores"));
    }

    #[test]
    fn untested_stale_or_missing_files_are_warnings() {
        let untested = Finding {
            tool: "Déjà Dup".into(),
            last: Some(at(1)),
            restored: None,
            sample: None,
        };
        assert_eq!(judge(Some(&untested), at(2)).status, Status::Warning);
        let stale = Finding {
            last: Some(at(1)),
            restored: Some(true),
            ..untested.clone()
        };
        assert_eq!(judge(Some(&stale), at(20)).status, Status::Warning);
        let missing = Finding {
            restored: Some(false),
            sample: Some("x".into()),
            ..untested.clone()
        };
        assert_eq!(judge(Some(&missing), at(2)).status, Status::Warning);
        let unknown_age = Finding {
            last: None,
            ..untested
        };
        assert_eq!(judge(Some(&unknown_age), at(2)).status, Status::Unknown);
    }
}
