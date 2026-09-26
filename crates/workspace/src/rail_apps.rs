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

/// Where a project's own app lives, for pinning it to the rail. `.noah/app.json`
/// (`{"name": "…", "url": "…"}`, the url an address or a path relative to the
/// project) wins; otherwise the first `index.html` in the usual places, named
/// after the project folder. Everything stays on this device.
pub fn app_for_project(root: &std::path::Path) -> Option<RailApp> {
    let project_name = root
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| root.display().to_string());
    if let Ok(bytes) = std::fs::read(root.join(".noah").join("app.json"))
        && let Some(declared) = serde_json::from_slice::<RailApp>(&bytes).log_err()
    {
        let url = if declared.url.contains("://") {
            declared.url
        } else {
            url::Url::from_file_path(root.join(&declared.url))
                .ok()?
                .to_string()
        };
        let name = if declared.name.trim().is_empty() {
            project_name
        } else {
            declared.name
        };
        return Some(RailApp { name, url });
    }
    const ENTRY_POINTS: [&str; 6] = [
        "index.html",
        "dist/index.html",
        "build/index.html",
        "public/index.html",
        "out/index.html",
        "site/index.html",
    ];
    ENTRY_POINTS.iter().find_map(|candidate| {
        let path = root.join(candidate);
        path.is_file().then(|| {
            url::Url::from_file_path(&path).ok().map(|url| RailApp {
                name: project_name.clone(),
                url: url.to_string(),
            })
        })?
    })
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
    fn finds_a_projects_app() {
        let root = tempfile::tempdir().expect("a temp dir");
        assert_eq!(app_for_project(root.path()), None);

        std::fs::create_dir_all(root.path().join("dist")).expect("dist");
        std::fs::write(root.path().join("dist/index.html"), "<p>hi</p>").expect("index");
        let found = app_for_project(root.path()).expect("the dist page");
        assert!(found.url.starts_with("file://") && found.url.ends_with("/dist/index.html"));
        assert_eq!(
            found.name,
            root.path().file_name().unwrap().to_string_lossy()
        );

        std::fs::create_dir_all(root.path().join(".noah")).expect(".noah");
        std::fs::write(
            root.path().join(".noah/app.json"),
            r#"{"name": "my shop", "url": "http://localhost:5173"}"#,
        )
        .expect("app.json");
        let declared = app_for_project(root.path()).expect("the declared app");
        assert_eq!(declared.name, "my shop");
        assert_eq!(declared.url, "http://localhost:5173");
    }

    #[test]
    fn names_read_well() {
        assert_eq!(name_for_url("http://localhost:3000/"), "localhost:3000");
        assert_eq!(name_for_url("https://www.example.com/a"), "example.com");
        assert_eq!(name_for_url("file:///home/me/site/index.html"), "index.html");
    }
}
