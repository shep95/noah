//! Software people build, pinned to the rail beside the rooms. Each is a page
//! or app address; clicking it opens it in the browser room.

use gpui::{App, AppContext as _, Global, TaskExt as _};
use serde::{Deserialize, Serialize};
use util::ResultExt as _;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RailApp {
    pub name: String,
    pub url: String,
}

#[derive(Default)]
struct RailApps(Vec<RailApp>);

impl Global for RailApps {}

fn file() -> std::path::PathBuf {
    paths::data_dir().join("rail_apps.json")
}

pub(crate) fn init(cx: &mut App) {
    let apps = std::fs::read(file())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<RailApp>>(&bytes).log_err())
        .unwrap_or_default();
    cx.set_global(RailApps(apps));
}

pub fn apps(cx: &App) -> Vec<RailApp> {
    cx.try_global::<RailApps>()
        .map(|apps| apps.0.clone())
        .unwrap_or_default()
}

/// Pins an app, replacing an earlier pin of the same address.
pub fn pin(app: RailApp, cx: &mut App) {
    let mut apps = apps(cx);
    apps.retain(|existing| existing.url != app.url);
    apps.push(app);
    store(apps, cx);
}

pub fn unpin(url: &str, cx: &mut App) {
    let mut apps = apps(cx);
    apps.retain(|existing| existing.url != url);
    store(apps, cx);
}

pub fn is_pinned(url: &str, cx: &App) -> bool {
    apps(cx).iter().any(|app| app.url == url)
}

fn store(apps: Vec<RailApp>, cx: &mut App) {
    let json = serde_json::to_vec_pretty(&apps).log_err();
    cx.set_global(RailApps(apps));
    cx.refresh_windows();
    let Some(json) = json else {
        return;
    };
    cx.background_spawn(async move {
        let file = file();
        if let Some(directory) = file.parent() {
            std::fs::create_dir_all(directory)?;
        }
        std::fs::write(file, json)?;
        anyhow::Ok(())
    })
    .detach_and_log_err(cx);
}

/// A short name for an address: the file name for local pages, the host
/// (with its port, so two dev servers stay apart) for everything else.
pub fn name_for_url(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return url.to_string();
    };
    if parsed.scheme() == "file" {
        return parsed
            .path_segments()
            .and_then(|mut segments| segments.next_back().map(str::to_string))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| url.to_string());
    }
    let host = parsed.host_str().unwrap_or(url).trim_start_matches("www.");
    match parsed.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_well() {
        assert_eq!(name_for_url("http://localhost:3000/"), "localhost:3000");
        assert_eq!(name_for_url("https://www.example.com/a"), "example.com");
        assert_eq!(name_for_url("file:///home/me/site/index.html"), "index.html");
    }
}
