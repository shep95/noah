use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupItem {
    pub name: String,
    pub command: String,
    pub location: String,
}

/// Blocking. Lists programs that start automatically when the user signs in.
///
/// Windows: the HKCU/HKLM `...\CurrentVersion\Run` keys and both Startup folders. Linux:
/// `~/.config/autostart/*.desktop` and enabled `systemctl --user` units. macOS: the plists in
/// `~/Library/LaunchAgents`, `/Library/LaunchAgents` and `/Library/LaunchDaemons`.
pub fn startup_items() -> Vec<StartupItem> {
    let mut items = platform_items();
    items.sort_by(|left, right| {
        left.location
            .cmp(&right.location)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    items
}

/// Lists the files in a startup folder, using each file's name (without extension) as the item
/// name and its full path as the command.
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn folder_items(
    directory: &Path,
    location: &str,
    include: impl Fn(&Path) -> bool,
) -> Vec<StartupItem> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::debug!("could not list {}: {error}", directory.display());
            }
            return Vec::new();
        }
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && include(path))
        .map(|path| StartupItem {
            name: path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default(),
            command: path.display().to_string(),
            location: location.to_string(),
        })
        .collect()
}

/// Reads the `Name` and `Exec` of an XDG autostart `.desktop` file. Returns `None` when the entry
/// is disabled (`Hidden=true` or `X-GNOME-Autostart-enabled=false`) or has no command.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_desktop_entry(text: &str) -> Option<(Option<String>, String)> {
    let mut in_desktop_entry = false;
    let mut name = None;
    let mut exec = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "Name" if name.is_none() => name = Some(value.to_string()),
            "Exec" if exec.is_none() => exec = Some(value.to_string()),
            "Hidden" if value.eq_ignore_ascii_case("true") => return None,
            "X-GNOME-Autostart-enabled" if value.eq_ignore_ascii_case("false") => return None,
            _ => {}
        }
    }
    let exec = exec.filter(|exec| !exec.is_empty())?;
    Some((name.filter(|name| !name.is_empty()), exec))
}

/// Reads unit names from `systemctl --user list-unit-files --state=enabled --no-legend`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_enabled_units(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let mut columns = line.split_whitespace();
            let unit = columns.next()?;
            let state = columns.next()?;
            (unit.contains('.') && state == "enabled").then(|| unit.to_string())
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn platform_items() -> Vec<StartupItem> {
    use crate::{COMMAND_TIMEOUT, run_command};

    let mut items = Vec::new();
    if let Some(autostart) = dirs::config_dir().map(|config| config.join("autostart")) {
        let location = autostart.display().to_string();
        match std::fs::read_dir(&autostart) {
            Ok(entries) => {
                for path in entries
                    .filter_map(|entry| entry.ok())
                    .map(|entry| entry.path())
                {
                    if path.extension().and_then(|extension| extension.to_str()) != Some("desktop")
                    {
                        continue;
                    }
                    let text = match std::fs::read_to_string(&path) {
                        Ok(text) => text,
                        Err(error) => {
                            log::debug!("could not read {}: {error}", path.display());
                            continue;
                        }
                    };
                    if let Some((name, command)) = parse_desktop_entry(&text) {
                        let name = name.unwrap_or_else(|| {
                            path.file_stem()
                                .map(|stem| stem.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        });
                        items.push(StartupItem {
                            name,
                            command,
                            location: location.clone(),
                        });
                    }
                }
            }
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    log::debug!("could not list {location}: {error}");
                }
            }
        }
    }

    match run_command(
        "systemctl",
        &[
            "--user",
            "list-unit-files",
            "--state=enabled",
            "--no-legend",
            "--no-pager",
        ],
        COMMAND_TIMEOUT,
    ) {
        Ok(output) if output.success() => {
            items.extend(
                parse_enabled_units(&output.stdout)
                    .into_iter()
                    .map(|unit| StartupItem {
                        name: unit.clone(),
                        command: unit,
                        location: "systemd user units".to_string(),
                    }),
            );
        }
        Ok(output) => log::debug!("systemctl --user {}", output.error_summary()),
        Err(error) => log::debug!("could not list systemd user units: {error:#}"),
    }
    items
}

#[cfg(windows)]
fn platform_items() -> Vec<StartupItem> {
    use std::path::PathBuf;
    use windows_registry::{CURRENT_USER, Key, LOCAL_MACHINE};

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const RUN_KEY_32_BIT: &str = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";

    fn registry_items(root: &Key, root_name: &str, path: &str) -> Vec<StartupItem> {
        let key = match root.open(path) {
            Ok(key) => key,
            Err(error) => {
                log::debug!("could not open {root_name}\\{path}: {error}");
                return Vec::new();
            }
        };
        let values = match key.values() {
            Ok(values) => values,
            Err(error) => {
                log::debug!("could not read {root_name}\\{path}: {error}");
                return Vec::new();
            }
        };
        values
            .filter_map(|(name, value)| {
                let command = String::try_from(value).ok()?;
                Some(StartupItem {
                    name,
                    command,
                    location: format!("{root_name}\\{path}"),
                })
            })
            .collect()
    }

    let is_shortcut_or_program = |path: &Path| {
        !path
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("desktop.ini"))
    };

    let mut items = Vec::new();
    items.extend(registry_items(CURRENT_USER, "HKCU", RUN_KEY));
    items.extend(registry_items(LOCAL_MACHINE, "HKLM", RUN_KEY));
    items.extend(registry_items(LOCAL_MACHINE, "HKLM", RUN_KEY_32_BIT));

    let startup_folder = PathBuf::from(r"Microsoft\Windows\Start Menu\Programs\Startup");
    if let Some(roaming) = dirs::data_dir() {
        let folder = roaming.join(&startup_folder);
        items.extend(folder_items(
            &folder,
            &folder.display().to_string(),
            is_shortcut_or_program,
        ));
    }
    let program_data = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    let folder = program_data.join(&startup_folder);
    items.extend(folder_items(
        &folder,
        &folder.display().to_string(),
        is_shortcut_or_program,
    ));
    items
}

#[cfg(target_os = "macos")]
fn platform_items() -> Vec<StartupItem> {
    use std::path::PathBuf;

    let mut directories = Vec::new();
    if let Some(home) = dirs::home_dir() {
        directories.push(home.join("Library/LaunchAgents"));
    }
    directories.push(PathBuf::from("/Library/LaunchAgents"));
    directories.push(PathBuf::from("/Library/LaunchDaemons"));

    directories
        .iter()
        .flat_map(|directory| {
            folder_items(directory, &directory.display().to_string(), |path| {
                path.extension().and_then(|extension| extension.to_str()) == Some("plist")
            })
        })
        .collect()
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn platform_items() -> Vec<StartupItem> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_desktop_entries() {
        let entry = "\
[Desktop Entry]
Type=Application
Name=Dropbox
Name[de]=Dropbox Deutsch
Comment=Sync your files
Exec=dropbox start -i
Icon=dropbox
X-GNOME-Autostart-enabled=true

[Desktop Action New]
Name=Something else
Exec=other
";
        assert_eq!(
            parse_desktop_entry(entry),
            Some((Some("Dropbox".into()), "dropbox start -i".into()))
        );
    }

    #[test]
    fn disabled_desktop_entries_are_skipped() {
        assert_eq!(
            parse_desktop_entry("[Desktop Entry]\nName=A\nExec=a\nHidden=true\n"),
            None
        );
        assert_eq!(
            parse_desktop_entry(
                "[Desktop Entry]\nName=A\nExec=a\nX-GNOME-Autostart-enabled=false\n"
            ),
            None
        );
        assert_eq!(
            parse_desktop_entry("[Desktop Entry]\nName=No command\n"),
            None
        );
        assert_eq!(
            parse_desktop_entry("[Desktop Entry]\nExec=/usr/bin/thing --flag\n"),
            Some((None, "/usr/bin/thing --flag".into()))
        );
    }

    #[test]
    fn parses_enabled_units() {
        let output = "\
pipewire-pulse.socket      enabled enabled
pipewire.socket            enabled enabled
xdg-user-dirs.service      enabled enabled
syncthing.service          enabled

3 unit files listed.
";
        assert_eq!(
            parse_enabled_units(output),
            vec![
                "pipewire-pulse.socket",
                "pipewire.socket",
                "xdg-user-dirs.service",
                "syncthing.service",
            ]
        );
        assert!(parse_enabled_units("").is_empty());
    }

    #[test]
    fn lists_folder_items() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("com.example.agent.plist"), "").unwrap();
        std::fs::write(directory.path().join("notes.txt"), "").unwrap();
        std::fs::create_dir(directory.path().join("sub.plist")).unwrap();
        let items = folder_items(directory.path(), "Launch Agents", |path| {
            path.extension().and_then(|extension| extension.to_str()) == Some("plist")
        });
        assert_eq!(
            items,
            vec![StartupItem {
                name: "com.example.agent".into(),
                command: directory
                    .path()
                    .join("com.example.agent.plist")
                    .display()
                    .to_string(),
                location: "Launch Agents".into(),
            }]
        );
        assert!(folder_items(&directory.path().join("missing"), "x", |_| true).is_empty());
    }

    #[test]
    fn startup_items_does_not_panic() {
        let items = startup_items();
        assert!(
            items
                .windows(2)
                .all(|pair| pair[0].location <= pair[1].location)
        );
    }
}
