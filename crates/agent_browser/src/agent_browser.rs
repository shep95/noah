//! A real web browser for shepherd and the person using noah, driven through
//! agent-browser (github.com/vercel-labs/agent-browser, Apache-2.0).
//!
//! shepherd's `browser` tool and the browser room run their commands in one
//! shared agent-browser session, so everything shepherd does in a page shows
//! up live in the room, and the person can take over at any moment.

mod browser_panel;

use anyhow::{Context as _, Result};
use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Global, SharedString, Task};
use std::path::PathBuf;

pub use browser_panel::{BrowserPanel, ToggleFocus};

/// The agent-browser session shared by shepherd and the browser room.
pub const SESSION: &str = "noah";

const BINARY_NAME: &str = if cfg!(windows) {
    "agent-browser.exe"
} else {
    "agent-browser"
};

pub fn init(cx: &mut App) {
    let browser = cx.new(|_| AgentBrowser::default());
    cx.set_global(GlobalAgentBrowser(browser));
    browser_panel::init(cx);
}

struct GlobalAgentBrowser(Entity<AgentBrowser>);

impl Global for GlobalAgentBrowser {}

pub enum BrowserEvent {
    /// A command finished, so the page may have changed or the browser may
    /// have just started.
    CommandFinished,
}

#[derive(Default)]
pub struct AgentBrowser {
    running_commands: usize,
}

impl EventEmitter<BrowserEvent> for AgentBrowser {}

impl AgentBrowser {
    pub fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalAgentBrowser>()
            .map(|global| global.0.clone())
    }

    pub fn is_busy(&self) -> bool {
        self.running_commands > 0
    }

    /// Runs one agent-browser command (for example `["open", "example.com"]`)
    /// in noah's shared session and returns what it printed.
    pub fn run(&mut self, arguments: Vec<String>, cx: &mut Context<Self>) -> Task<Result<String>> {
        self.running_commands += 1;
        cx.notify();
        let command = cx.background_spawn(run_command(arguments));
        cx.spawn(async move |this, cx| {
            let result = command.await;
            this.update(cx, |this, cx| {
                this.running_commands = this.running_commands.saturating_sub(1);
                cx.emit(BrowserEvent::CommandFinished);
                cx.notify();
            })?;
            result
        })
    }
}

async fn run_command(arguments: Vec<String>) -> Result<String> {
    let binary = find_binary().context(
        "agent-browser was not found. It ships inside noah's installers; \
         reinstall noah, or put agent-browser on your PATH.",
    )?;
    let mut command = util::command::new_command(&binary);
    command.arg("--session").arg(SESSION).args(&arguments);
    if std::env::var_os("AGENT_BROWSER_EXECUTABLE_PATH").is_none()
        && let Some(browser) = find_browser()
    {
        command.env("AGENT_BROWSER_EXECUTABLE_PATH", browser);
    }
    let output = command
        .output()
        .await
        .with_context(|| format!("could not start {}", binary.display()))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() {
        return Ok(stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let message = if stderr.is_empty() { stdout } else { stderr };
    anyhow::bail!(explain_failure(&message))
}

/// Rewrites the one failure people hit on a fresh machine into something they
/// can act on; everything else is passed through as agent-browser said it.
fn explain_failure(message: &str) -> String {
    let lowercase = message.to_lowercase();
    if lowercase.contains("no browser") || lowercase.contains("chrome not found") {
        format!(
            "{message}\nNo Chrome, Edge or Chromium was found. Install one, or use \
             \"download a browser\" in noah's browser room."
        )
    } else {
        message.to_string()
    }
}

/// Where agent-browser lives: next to noah (Windows), in `libexec` (Linux),
/// in `Resources` (macOS), or on the PATH.
pub fn find_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("NOAH_AGENT_BROWSER") {
        return Some(PathBuf::from(path));
    }
    if let Some(directory) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|parent| parent.to_path_buf()))
    {
        let candidates = [
            directory.join(BINARY_NAME),
            directory.join("../libexec").join(BINARY_NAME),
            directory.join("../Resources").join(BINARY_NAME),
        ];
        if let Some(found) = candidates.into_iter().find(|candidate| candidate.is_file()) {
            return Some(found);
        }
    }
    which::which(BINARY_NAME).ok()
}

/// agent-browser finds Chrome, Brave and Playwright's Chromium on its own.
/// These are the Chromium browsers it doesn't look for, most importantly the
/// Edge every Windows machine has, so shepherd can browse without a download.
fn find_browser() -> Option<PathBuf> {
    if cfg!(windows) {
        let program_files = ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"];
        return program_files
            .into_iter()
            .filter_map(std::env::var_os)
            .map(|root| {
                PathBuf::from(root)
                    .join("Microsoft")
                    .join("Edge")
                    .join("Application")
                    .join("msedge.exe")
            })
            .find(|path| path.is_file());
    }
    if cfg!(target_os = "macos") {
        return [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file());
    }
    [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "microsoft-edge",
        "microsoft-edge-stable",
    ]
    .into_iter()
    .find_map(|name| which::which(name).ok())
}

/// Turns what someone typed in the address bar into a URL: full URLs pass
/// through, things that look like hosts get a scheme, anything else becomes a
/// DuckDuckGo search.
pub fn address_to_url(address: &str) -> SharedString {
    let address = address.trim();
    if address.contains("://") || address.starts_with("about:") {
        return address.to_string().into();
    }
    let host = address.split(['/', '?', '#']).next().unwrap_or_default();
    let is_local = host.starts_with("localhost") || host.starts_with("127.0.0.1");
    if is_local {
        return format!("http://{address}").into();
    }
    if !address.contains(' ') && host.contains('.') {
        return format!("https://{address}").into();
    }
    let query: String = address
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            b' ' => "+".to_string(),
            other => format!("%{other:02X}"),
        })
        .collect();
    format!("https://duckduckgo.com/?q={query}").into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_become_urls() {
        assert_eq!(address_to_url("https://a.dev/x"), "https://a.dev/x");
        assert_eq!(
            address_to_url("example.com/docs"),
            "https://example.com/docs"
        );
        assert_eq!(address_to_url("localhost:3000"), "http://localhost:3000");
        assert_eq!(
            address_to_url("rust async book"),
            "https://duckduckgo.com/?q=rust+async+book"
        );
    }

    #[test]
    fn missing_browser_is_explained() {
        assert!(explain_failure("Error: no browser found").contains("download a browser"));
        assert_eq!(explain_failure("Unknown ref: e9"), "Unknown ref: e9");
    }
}
