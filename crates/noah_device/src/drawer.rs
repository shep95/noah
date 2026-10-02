//! The drawer: what is on this machine that the person did not put here.
//! A baseline of programs, startup items, listening ports and browser
//! extensions is kept at `~/.noah/device/drawer.json`; each look compares
//! the machine to it and lists what arrived and what went. "That's all
//! mine" makes the present the new baseline.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::checks::ListeningPort;
use crate::intel::AppInventory;
use crate::startup::StartupItem;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// RFC 3339, when the baseline was accepted.
    #[serde(default)]
    pub taken: String,
    #[serde(default)]
    pub apps: Vec<String>,
    #[serde(default)]
    pub startup: Vec<String>,
    #[serde(default)]
    pub ports: Vec<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extension {
    pub browser: String,
    pub name: String,
    pub version: String,
    pub id: String,
}

/// What changed against the baseline. Each name is one line as the room
/// shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Drawer {
    pub since: Option<String>,
    pub new_apps: Vec<String>,
    pub gone_apps: Vec<String>,
    pub new_startup: Vec<String>,
    pub gone_startup: Vec<String>,
    pub new_ports: Vec<String>,
    pub new_extensions: Vec<String>,
    pub gone_extensions: Vec<String>,
}

impl Drawer {
    pub fn is_empty(&self) -> bool {
        self.new_apps.is_empty()
            && self.gone_apps.is_empty()
            && self.new_startup.is_empty()
            && self.gone_startup.is_empty()
            && self.new_ports.is_empty()
            && self.new_extensions.is_empty()
            && self.gone_extensions.is_empty()
    }

    pub fn count(&self) -> usize {
        self.new_apps.len()
            + self.gone_apps.len()
            + self.new_startup.len()
            + self.gone_startup.len()
            + self.new_ports.len()
            + self.new_extensions.len()
            + self.gone_extensions.len()
    }
}

pub fn path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".noah")
        .join("device")
        .join("drawer.json")
}

pub fn load(path: &Path) -> Option<Snapshot> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save(path: &Path, snapshot: &Snapshot) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(snapshot)?;
    let partial = path.with_extension(format!("partial{}", std::process::id()));
    std::fs::write(&partial, text)?;
    std::fs::rename(&partial, path)?;
    Ok(())
}

fn app_line(app: &crate::intel::InstalledApp) -> String {
    match &app.publisher {
        Some(publisher) if !publisher.is_empty() => {
            format!("{} ({}, {})", app.name, publisher, app.source)
        }
        _ => format!("{} ({})", app.name, app.source),
    }
}

fn startup_line(item: &StartupItem) -> String {
    format!("{} · {}", item.name, item.location)
}

fn port_line(port: &ListeningPort) -> String {
    match &port.process {
        Some(process) if !process.is_empty() => {
            format!("{} {} · {process}", port.protocol, port.port)
        }
        _ => format!("{} {}", port.protocol, port.port),
    }
}

fn extension_line(extension: &Extension) -> String {
    format!("{} · {} ({})", extension.name, extension.browser, extension.id)
}

/// The machine as it is now, as one snapshot.
pub fn snapshot(
    apps: &AppInventory,
    startup: &[StartupItem],
    ports: &[ListeningPort],
    extensions: &[Extension],
) -> Snapshot {
    let mut snapshot = Snapshot {
        taken: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        apps: apps.apps.iter().map(app_line).collect(),
        startup: startup.iter().map(startup_line).collect(),
        ports: ports.iter().map(port_line).collect(),
        extensions: extensions.iter().map(extension_line).collect(),
    };
    for list in [
        &mut snapshot.apps,
        &mut snapshot.startup,
        &mut snapshot.ports,
        &mut snapshot.extensions,
    ] {
        list.sort();
        list.dedup();
    }
    snapshot
}

fn only_in(left: &[String], right: &[String]) -> Vec<String> {
    left.iter()
        .filter(|item| !right.contains(item))
        .cloned()
        .collect()
}

/// What `now` has that the baseline did not, and the reverse. Ports only
/// count when new: a port that closed is nothing to worry about.
pub fn compare(baseline: Option<&Snapshot>, now: &Snapshot) -> Drawer {
    let Some(baseline) = baseline else {
        return Drawer::default();
    };
    Drawer {
        since: Some(baseline.taken.clone()),
        new_apps: only_in(&now.apps, &baseline.apps),
        gone_apps: only_in(&baseline.apps, &now.apps),
        new_startup: only_in(&now.startup, &baseline.startup),
        gone_startup: only_in(&baseline.startup, &now.startup),
        new_ports: only_in(&now.ports, &baseline.ports),
        new_extensions: only_in(&now.extensions, &baseline.extensions),
        gone_extensions: only_in(&baseline.extensions, &now.extensions),
    }
}

/// Extensions installed in the browsers on this machine, read from their
/// profile folders: Chromium browsers keep one folder per extension with a
/// manifest, Firefox keeps `extensions.json`. Nothing runs; files are read.
pub fn browser_extensions() -> Vec<Extension> {
    let mut extensions = Vec::new();
    for (browser, root) in chromium_profile_roots() {
        if let Ok(profiles) = std::fs::read_dir(&root) {
            for profile in profiles.flatten() {
                let name = profile.file_name().to_string_lossy().to_string();
                if name != "Default" && !name.starts_with("Profile ") {
                    continue;
                }
                read_chromium_extensions(&browser, &profile.path().join("Extensions"), &mut extensions);
            }
        }
    }
    for root in firefox_profile_roots() {
        if let Ok(profiles) = std::fs::read_dir(&root) {
            for profile in profiles.flatten() {
                read_firefox_extensions(&profile.path().join("extensions.json"), &mut extensions);
            }
        }
    }
    extensions.sort_by(|left, right| {
        left.browser
            .cmp(&right.browser)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    extensions.dedup_by(|left, right| left.browser == right.browser && left.id == right.id);
    extensions
}

fn chromium_profile_roots() -> Vec<(String, PathBuf)> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let candidates: Vec<(&str, PathBuf)> = if cfg!(target_os = "windows") {
        let local = dirs::data_local_dir().unwrap_or_else(|| home.join("AppData").join("Local"));
        vec![
            ("Chrome", local.join("Google").join("Chrome").join("User Data")),
            ("Edge", local.join("Microsoft").join("Edge").join("User Data")),
            ("Brave", local.join("BraveSoftware").join("Brave-Browser").join("User Data")),
            ("Chromium", local.join("Chromium").join("User Data")),
        ]
    } else if cfg!(target_os = "macos") {
        let support = home.join("Library").join("Application Support");
        vec![
            ("Chrome", support.join("Google").join("Chrome")),
            ("Edge", support.join("Microsoft Edge")),
            ("Brave", support.join("BraveSoftware").join("Brave-Browser")),
            ("Chromium", support.join("Chromium")),
        ]
    } else {
        let config = dirs::config_dir().unwrap_or_else(|| home.join(".config"));
        vec![
            ("Chrome", config.join("google-chrome")),
            ("Edge", config.join("microsoft-edge")),
            ("Brave", config.join("BraveSoftware").join("Brave-Browser")),
            ("Chromium", config.join("chromium")),
        ]
    };
    candidates
        .into_iter()
        .filter(|(_, path)| path.is_dir())
        .map(|(browser, path)| (browser.to_string(), path))
        .collect()
}

fn firefox_profile_roots() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let candidates: Vec<PathBuf> = if cfg!(target_os = "windows") {
        let roaming = dirs::config_dir().unwrap_or_else(|| home.join("AppData").join("Roaming"));
        vec![roaming.join("Mozilla").join("Firefox").join("Profiles")]
    } else if cfg!(target_os = "macos") {
        vec![
            home.join("Library")
                .join("Application Support")
                .join("Firefox")
                .join("Profiles"),
        ]
    } else {
        vec![
            home.join(".mozilla").join("firefox"),
            home.join("snap").join("firefox").join("common").join(".mozilla").join("firefox"),
        ]
    };
    candidates.into_iter().filter(|path| path.is_dir()).collect()
}

/// A manifest's name may be a locale key (`__MSG_appName__`); the folder's
/// `_locales/en/messages.json` resolves it when it can.
fn chromium_extension_name(version_directory: &Path, manifest: &serde_json::Value) -> String {
    let raw = manifest
        .get("name")
        .and_then(|name| name.as_str())
        .unwrap_or("")
        .to_string();
    let Some(key) = raw.strip_prefix("__MSG_").and_then(|rest| rest.strip_suffix("__")) else {
        return raw;
    };
    let locale = manifest
        .get("default_locale")
        .and_then(|locale| locale.as_str())
        .unwrap_or("en");
    for candidate in [locale, "en", "en_US"] {
        let messages = version_directory
            .join("_locales")
            .join(candidate)
            .join("messages.json");
        if let Ok(text) = std::fs::read_to_string(&messages)
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
            && let Some(message) = value
                .get(key)
                .or_else(|| value.get(key.to_lowercase()))
                .and_then(|entry| entry.get("message"))
                .and_then(|message| message.as_str())
        {
            return message.to_string();
        }
    }
    raw
}

pub(crate) fn read_chromium_extensions(
    browser: &str,
    extensions_directory: &Path,
    out: &mut Vec<Extension>,
) {
    let Ok(ids) = std::fs::read_dir(extensions_directory) else {
        return;
    };
    for id in ids.flatten() {
        let Ok(versions) = std::fs::read_dir(id.path()) else {
            continue;
        };
        let mut newest: Option<(String, PathBuf)> = None;
        for version in versions.flatten() {
            let name = version.file_name().to_string_lossy().to_string();
            if version.path().join("manifest.json").is_file()
                && newest.as_ref().is_none_or(|(current, _)| name > *current)
            {
                newest = Some((name, version.path()));
            }
        }
        let Some((_, version_directory)) = newest else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(version_directory.join("manifest.json")) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let name = chromium_extension_name(&version_directory, &manifest);
        if name.is_empty() {
            continue;
        }
        out.push(Extension {
            browser: browser.to_string(),
            name,
            version: manifest
                .get("version")
                .and_then(|version| version.as_str())
                .unwrap_or("")
                .to_string(),
            id: id.file_name().to_string_lossy().to_string(),
        });
    }
}

pub(crate) fn read_firefox_extensions(path: &Path, out: &mut Vec<Extension>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return;
    };
    let Some(addons) = value.get("addons").and_then(|addons| addons.as_array()) else {
        return;
    };
    for addon in addons {
        let kind = addon.get("type").and_then(|kind| kind.as_str()).unwrap_or("");
        if kind != "extension" {
            continue;
        }
        let location = addon
            .get("location")
            .and_then(|location| location.as_str())
            .unwrap_or("");
        // Built-in system add-ons are Firefox itself, not something installed.
        if location.starts_with("app-system") || location == "app-builtin" {
            continue;
        }
        let name = addon
            .get("defaultLocale")
            .and_then(|locale| locale.get("name"))
            .and_then(|name| name.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        out.push(Extension {
            browser: "Firefox".to_string(),
            name,
            version: addon
                .get("version")
                .and_then(|version| version.as_str())
                .unwrap_or("")
                .to_string(),
            id: addon.get("id").and_then(|id| id.as_str()).unwrap_or("").to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_lists_arrivals_and_departures_but_not_closed_ports() {
        let baseline = Snapshot {
            taken: "2026-09-01T00:00:00Z".into(),
            apps: vec!["a".into(), "b".into()],
            startup: vec!["s1".into()],
            ports: vec!["tcp 22".into(), "tcp 80".into()],
            extensions: vec!["x".into()],
        };
        let now = Snapshot {
            taken: String::new(),
            apps: vec!["a".into(), "c".into()],
            startup: vec!["s1".into(), "s2".into()],
            ports: vec!["tcp 22".into(), "tcp 4444".into()],
            extensions: vec![],
        };
        let drawer = compare(Some(&baseline), &now);
        assert_eq!(drawer.new_apps, vec!["c"]);
        assert_eq!(drawer.gone_apps, vec!["b"]);
        assert_eq!(drawer.new_startup, vec!["s2"]);
        assert_eq!(drawer.new_ports, vec!["tcp 4444"]);
        assert_eq!(drawer.gone_extensions, vec!["x"]);
        assert_eq!(drawer.count(), 5);
        assert!(compare(None, &now).is_empty());
    }

    #[test]
    fn chromium_and_firefox_profiles_are_read_from_files() {
        let root = tempfile::tempdir().expect("tempdir");
        let version = root.path().join("Extensions").join("abcdefgh").join("2.1_0");
        std::fs::create_dir_all(version.join("_locales").join("en")).expect("mkdir");
        std::fs::write(
            version.join("manifest.json"),
            r#"{"name":"__MSG_appName__","version":"2.1","default_locale":"en"}"#,
        )
        .expect("write");
        std::fs::write(
            version.join("_locales").join("en").join("messages.json"),
            r#"{"appName":{"message":"Tab Saver"}}"#,
        )
        .expect("write");
        let mut out = Vec::new();
        read_chromium_extensions("Chrome", &root.path().join("Extensions"), &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "Tab Saver");
        assert_eq!(out[0].id, "abcdefgh");

        let firefox = root.path().join("extensions.json");
        std::fs::write(
            &firefox,
            r#"{"addons":[{"id":"u@x","type":"extension","location":"app-profile","version":"1","defaultLocale":{"name":"uBlock"}},{"id":"sys@mozilla","type":"extension","location":"app-system-defaults","defaultLocale":{"name":"Built in"}}]}"#,
        )
        .expect("write");
        read_firefox_extensions(&firefox, &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].name, "uBlock");
    }

    #[test]
    fn snapshots_round_trip_through_the_file() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("device").join("drawer.json");
        let snapshot = Snapshot {
            taken: "2026-10-02T00:00:00Z".into(),
            apps: vec!["a".into()],
            ..Default::default()
        };
        save(&path, &snapshot).expect("save");
        assert_eq!(load(&path), Some(snapshot));
        assert_eq!(load(&root.path().join("missing.json")), None);
    }
}
