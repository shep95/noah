//! A real web browser for shepherd and the person using noah, driven through
//! agent-browser (github.com/vercel-labs/agent-browser, Apache-2.0).
//!
//! shepherd's `browser` tool and the browser room run their commands in one
//! shared agent-browser session, so everything shepherd does in a page shows
//! up live in the room, and the person can take over at any moment.

mod browser_panel;

use anyhow::{Context as _, Result};
use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Global, SharedString, Task};
use util::ResultExt as _;
use std::path::{Path, PathBuf};

pub use browser_panel::{BrowserPanel, ToggleFocus};

/// The agent-browser session shared by shepherd and the browser room.
pub const SESSION: &str = "noah";

const DO_NOT_DE_ELEVATE: &str = "--do-not-de-elevate";

const BINARY_NAME: &str = if cfg!(windows) {
    "agent-browser.exe"
} else {
    "agent-browser"
};

pub fn init(cx: &mut App) {
    let browser = cx.new(|_| AgentBrowser::default());
    cx.set_global(GlobalAgentBrowser(browser));
    browser_panel::init(cx);
    prewarm(cx);
    // agent-browser keeps its browser running in the background between
    // commands, which would otherwise outlive noah.
    cx.on_app_quit(|_| async {
        if find_binary().is_some() {
            run_command(vec!["close".into()]).await.log_err();
        }
    })
    .detach();
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
    // A profile that outlives the session keeps the browser's cache, cookies
    // and sign-ins, so pages open warm instead of from an empty browser.
    if std::env::var_os("AGENT_BROWSER_PROFILE").is_none() {
        command.env("AGENT_BROWSER_PROFILE", profile_directory());
    }
    if std::env::var_os("AGENT_BROWSER_EXECUTABLE_PATH").is_none()
        && let Some(browser) = find_browser()
    {
        command.env("AGENT_BROWSER_EXECUTABLE_PATH", browser);
    }
    if cfg!(windows) {
        // Chrome and Edge 138+ relaunch themselves de-elevated when started
        // from an elevated process. The first process exits with code 0, and
        // agent-browser reports "Chrome exited early" instead of waiting for
        // the relaunched one (vercel-labs/agent-browser#1406).
        let mut browser_arguments = std::env::var("AGENT_BROWSER_ARGS").unwrap_or_default();
        if !browser_arguments.contains(DO_NOT_DE_ELEVATE) {
            if !browser_arguments.trim().is_empty() {
                browser_arguments.push(',');
            }
            browser_arguments.push_str(DO_NOT_DE_ELEVATE);
        }
        command.env("AGENT_BROWSER_ARGS", browser_arguments);
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

fn profile_directory() -> PathBuf {
    paths::data_dir().join("browser").join("profile")
}

/// Starts the browser in the background shortly after noah opens, for people
/// who have used the browser room before, so it's ready when they open it.
fn prewarm(cx: &mut App) {
    if !profile_directory().is_dir() || find_binary().is_none() {
        return;
    }
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(std::time::Duration::from_secs(4))
            .await;
        run_command(vec!["--json".into(), "stream".into(), "status".into()])
            .await
            .log_err();
    })
    .detach();
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

/// How pages noah draws itself in the browser room look: the start page and
/// DuckDuckGo results take the theme's colors (which follow the wallpaper)
/// and the person's UI font, so the room looks like the rest of noah.
#[derive(Debug, Clone, PartialEq)]
pub struct PageStyle {
    pub dark: bool,
    pub background: String,
    pub surface: String,
    pub border: String,
    pub text: String,
    pub muted: String,
    pub link: String,
    pub font: String,
    pub wallpaper: Option<PathBuf>,
    pub wallpaper_opacity: f32,
    pub placeholder: String,
    /// BCP 47 code of noah's language, for the page's `lang`.
    pub language: String,
    pub right_to_left: bool,
}

impl PageStyle {
    pub fn from_app(cx: &App) -> Self {
        use settings::Settings as _;
        use theme::ActiveTheme as _;

        let theme = cx.theme();
        let colors = theme.colors();
        let dark = !theme.appearance().is_light();
        let base = if dark {
            gpui::black()
        } else {
            gpui::white()
        };
        // Surfaces are translucent over the wallpaper, so flatten them onto
        // the window background the way noah paints them.
        let window = base.blend(colors.background);
        let background = window.blend(colors.editor_background);
        let surface = background.blend(colors.element_background);
        let workspace_settings = workspace::WorkspaceSettings::get_global(cx);
        let wallpaper = match workspace_settings.wallpaper.as_deref() {
            Some(path) => Some(PathBuf::from(path)),
            None => bundled_wallpaper(cx),
        };
        Self {
            dark,
            background: hex(background),
            surface: hex(surface),
            border: hex(background.blend(colors.border)),
            text: hex(background.blend(colors.text)),
            muted: hex(background.blend(colors.text_muted)),
            link: hex(background.blend(colors.text_accent)),
            font: web_font_family(&theme_settings::ThemeSettings::get_global(cx).ui_font.family),
            wallpaper,
            wallpaper_opacity: workspace_settings.wallpaper_opacity.clamp(0.0, 1.0),
            placeholder: noah_i18n::t(cx, "search or enter an address").to_string(),
            language: noah_i18n::current_language(cx).code.to_string(),
            right_to_left: noah_i18n::is_right_to_left(noah_i18n::current_language(cx)),
        }
    }

    pub fn color_scheme(&self) -> &'static str {
        if self.dark { "dark" } else { "light" }
    }

    /// DuckDuckGo's appearance settings, passed in the URL so nothing is
    /// stored with DuckDuckGo: theme, background, text, link, visited link,
    /// URL and header colors, font, and no ads.
    fn search_parameters(&self) -> Vec<(&'static str, String)> {
        let bare = |color: &str| color.trim_start_matches('#').to_string();
        vec![
            ("kae", if self.dark { "d" } else { "-1" }.to_string()),
            ("k7", bare(&self.background)),
            ("kj", bare(&self.background)),
            ("k8", bare(&self.text)),
            ("k9", bare(&self.link)),
            ("kaa", bare(&self.link)),
            ("kx", bare(&self.muted)),
            ("kt", self.font.clone()),
            ("k1", "-1".to_string()),
        ]
    }

    pub fn search_url(&self, query: &str) -> String {
        let mut url = format!("https://duckduckgo.com/?q={}", encode_query(query));
        for (key, value) in self.search_parameters() {
            url.push_str(&format!("&{key}={}", encode_query(&value)));
        }
        url
    }

    /// The page the browser room opens to: a search box in noah's colors, over
    /// the wallpaper the way the editor shows it.
    pub fn start_page_html(&self) -> String {
        let hidden_inputs: String = self
            .search_parameters()
            .into_iter()
            .map(|(key, value)| {
                format!(
                    "<input type=\"hidden\" name=\"{key}\" value=\"{}\">",
                    escape_html(&value)
                )
            })
            .collect();
        let wallpaper = self
            .wallpaper
            .as_ref()
            .and_then(|path| url::Url::from_file_path(path).ok())
            .map(|url| {
                format!(
                    "body::before{{content:\"\";position:fixed;inset:0;background:url(\"{url}\") center/cover no-repeat;opacity:{opacity};z-index:-1}}",
                    opacity = self.wallpaper_opacity
                )
            })
            .unwrap_or_default();
        format!(
            r#"<!doctype html>
<html lang="{language}" dir="{direction}"><head><meta charset="utf-8"><title>noah</title>
<meta name="color-scheme" content="{scheme}">
<style>
html,body{{margin:0;height:100%}}
body{{background:{background};color:{text};font-family:"{font}",system-ui,sans-serif;display:flex;align-items:center;justify-content:center;animation:enter .5s ease-out}}
{wallpaper}
form{{width:min(560px,86vw);display:flex;flex-direction:column;gap:14px;align-items:center}}
.mark{{color:{muted};font-weight:300;letter-spacing:.2em;font-size:13px;text-transform:lowercase}}
input[name=q]{{box-sizing:border-box;width:100%;padding:12px 16px;border-radius:8px;border:1px solid {border};background:{surface};color:{text};font:inherit;font-size:15px;outline:none;transition:border-color .2s ease}}
input[name=q]:focus{{border-color:{link}}}
input[name=q]::placeholder{{color:{muted}}}
@keyframes enter{{from{{opacity:0}}to{{opacity:1}}}}
@media (prefers-reduced-motion:reduce){{body{{animation:none}}}}
</style></head>
<body><form action="https://duckduckgo.com/" method="get" autocomplete="off">
<div class="mark">noah</div>
<input name="q" placeholder="{placeholder}" autofocus>
{hidden_inputs}
</form></body></html>
"#,
            scheme = self.color_scheme(),
            background = self.background,
            text = self.text,
            font = escape_html(&self.font),
            muted = self.muted,
            border = self.border,
            surface = self.surface,
            link = self.link,
            placeholder = escape_html(&self.placeholder),
            language = escape_html(&self.language),
            direction = if self.right_to_left { "rtl" } else { "ltr" },
        )
    }
}

// The color scheme is applied with `set media` on the running browser rather
// than as a launch option: agent-browser relaunches the browser when launch
// options change, which would drop the person's tabs and the live view.
pub(crate) async fn apply_color_scheme(style: &PageStyle) -> Result<String> {
    run_command(vec![
        "set".into(),
        "media".into(),
        style.color_scheme().into(),
    ])
    .await
}

/// Where the start page is written; it is regenerated whenever the look
/// changes.
pub fn start_page_path() -> PathBuf {
    paths::data_dir().join("browser").join("start.html")
}

pub fn start_page_url() -> Option<String> {
    url::Url::from_file_path(start_page_path())
        .ok()
        .map(|url| url.to_string())
}

pub fn write_start_page(style: &PageStyle) -> Result<String> {
    let path = start_page_path();
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    std::fs::write(&path, style.start_page_html())?;
    start_page_url().context("the start page has no file URL")
}

/// The bundled wallpaper only exists inside noah's assets, so it's copied out
/// once for pages in the browser to show.
fn bundled_wallpaper(cx: &App) -> Option<PathBuf> {
    let path = paths::data_dir().join("browser").join("wallpaper.jpg");
    if path.is_file() {
        return Some(path);
    }
    let bytes = cx
        .asset_source()
        .load("images/noah/wallpaper.jpg")
        .log_err()
        .flatten()?;
    std::fs::create_dir_all(path.parent()?).log_err()?;
    std::fs::write(&path, bytes).log_err()?;
    Some(path)
}

/// noah's bundled fonts go by internal names (`.ZedSans`) that web pages
/// can't resolve; pages get the real family names instead.
fn web_font_family(family: &str) -> String {
    match family {
        ".ZedSans" | "Zed Plex Sans" => "IBM Plex Sans".to_string(),
        ".ZedMono" | "Zed Plex Mono" => "Lilex".to_string(),
        other => other.trim_start_matches('.').to_string(),
    }
}

fn hex(color: gpui::Hsla) -> String {
    let rgba = gpui::Rgba::from(color);
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(rgba.r),
        channel(rgba.g),
        channel(rgba.b)
    )
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn encode_query(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            b' ' => "+".to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// Turns what someone typed in the address bar into a URL: full URLs pass
/// through, things that look like hosts get a scheme, anything else becomes a
/// DuckDuckGo search styled like noah.
pub fn address_to_url(address: &str, style: &PageStyle) -> SharedString {
    let address = address.trim();
    if has_scheme(address) || address.to_ascii_lowercase().starts_with("about:") {
        return address.to_string().into();
    }
    let as_path = Path::new(address);
    if (as_path.is_absolute() || address.starts_with("\\\\"))
        && let Ok(url) = url::Url::from_file_path(as_path)
    {
        return url.to_string().into();
    }
    let host = address.split(['/', '?', '#']).next().unwrap_or_default();
    let host_name = if host.starts_with('[') {
        host.split(']').next().map(|name| format!("{name}]")).unwrap_or_default()
    } else {
        host.split(':').next().unwrap_or_default().to_string()
    };
    let is_local = host_name.eq_ignore_ascii_case("localhost")
        || host_name == "127.0.0.1"
        || host_name == "[::1]";
    if is_local {
        return format!("http://{address}").into();
    }
    if !address.contains(' ') && host.contains('.') {
        return format!("https://{address}").into();
    }
    style.search_url(address).into()
}

/// Whether the address starts with a URL scheme such as `https://`, so a
/// search like "what is https://x" is still searched.
fn has_scheme(address: &str) -> bool {
    address.split_once("://").is_some_and(|(scheme, _)| {
        scheme.starts_with(|character: char| character.is_ascii_alphabetic())
            && scheme.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> PageStyle {
        PageStyle {
            dark: true,
            background: "#101412".into(),
            surface: "#1a1f1c".into(),
            border: "#2a302c".into(),
            text: "#e6ece8".into(),
            muted: "#8a948e".into(),
            link: "#9fd4b4".into(),
            font: "IBM Plex Sans".into(),
            wallpaper: None,
            wallpaper_opacity: 0.5,
            placeholder: "search or enter an address".into(),
            language: "en".into(),
            right_to_left: false,
        }
    }

    #[test]
    fn addresses_become_urls() {
        let style = style();
        assert_eq!(address_to_url("https://a.dev/x", &style), "https://a.dev/x");
        assert_eq!(
            address_to_url("example.com/docs", &style),
            "https://example.com/docs"
        );
        assert_eq!(
            address_to_url("localhost:3000", &style),
            "http://localhost:3000"
        );
        let search = address_to_url("rust async book", &style);
        assert!(search.starts_with("https://duckduckgo.com/?q=rust+async+book&kae=d&k7=101412"));
        assert!(search.contains("&kt=IBM+Plex+Sans"));
    }

    #[test]
    fn start_page_uses_the_theme() {
        let html = style().start_page_html();
        assert!(html.contains("background:#101412"));
        assert!(html.contains("font-family:\"IBM Plex Sans\""));
        assert!(html.contains("name=\"k9\" value=\"9fd4b4\""));
        assert!(!html.contains("body::before"), "no wallpaper without a file");
    }

    #[test]
    fn missing_browser_is_explained() {
        assert!(explain_failure("Error: no browser found").contains("download a browser"));
        assert_eq!(explain_failure("Unknown ref: e9"), "Unknown ref: e9");
    }

    #[test]
    fn missing_browser_is_recognized_in_any_case() {
        let explained = explain_failure("Error: Chrome Not Found");
        assert!(explained.starts_with("Error: Chrome Not Found\n"), "{explained}");
        assert!(explained.contains("No Chrome, Edge or Chromium was found"));

        let explained = explain_failure("NO BROWSER available");
        assert!(explained.starts_with("NO BROWSER available\n"), "{explained}");
        assert!(explained.contains("download a browser"));

        assert_eq!(explain_failure(""), "");
    }

    #[test]
    fn local_addresses_use_http() {
        let style = style();
        assert_eq!(address_to_url("localhost", &style), "http://localhost");
        assert_eq!(
            address_to_url("localhost:8080/api?x=1", &style),
            "http://localhost:8080/api?x=1"
        );
        assert_eq!(
            address_to_url("127.0.0.1:5173", &style),
            "http://127.0.0.1:5173"
        );
        assert_eq!(
            address_to_url("127.0.0.1/index.html", &style),
            "http://127.0.0.1/index.html"
        );
    }

    #[test]
    fn bare_domains_use_https() {
        let style = style();
        assert_eq!(address_to_url("example.com", &style), "https://example.com");
        assert_eq!(
            address_to_url("  docs.rs/serde  ", &style),
            "https://docs.rs/serde"
        );
        assert_eq!(
            address_to_url("sub.example.org?q=1#top", &style),
            "https://sub.example.org?q=1#top"
        );
    }

    #[test]
    fn explicit_schemes_pass_through() {
        let style = style();
        for address in [
            "http://example.com",
            "https://example.com/a b",
            "file:///tmp/page.html",
            "chrome://settings",
            "about:blank",
            "about:version",
        ] {
            assert_eq!(address_to_url(address, &style), address, "{address}");
        }
        assert_eq!(
            address_to_url("  about:blank  ", &style),
            "about:blank",
            "surrounding whitespace is trimmed"
        );
    }

    #[test]
    fn lookalikes_are_not_local_or_urls() {
        let style = style();
        assert_eq!(
            address_to_url("localhostfoo.com", &style),
            "https://localhostfoo.com"
        );
        assert_eq!(address_to_url("[::1]:3000", &style), "http://[::1]:3000");
        assert_eq!(
            address_to_url("what is https://x", &style),
            style.search_url("what is https://x")
        );
        assert_eq!(address_to_url("About:blank", &style), "About:blank");
    }

    #[test]
    fn absolute_paths_become_file_urls() {
        let style = style();
        let path = if cfg!(windows) {
            "C:\\site\\index.html"
        } else {
            "/srv/site/index.html"
        };
        let expected = url::Url::from_file_path(path).expect("absolute path").to_string();
        assert_eq!(address_to_url(path, &style), expected.as_str());
    }

    #[test]
    fn text_becomes_a_search() {
        let style = style();
        for text in ["rust", "hello world.com", "what is rust", "a.b c"] {
            assert_eq!(
                address_to_url(text, &style),
                style.search_url(text),
                "{text}"
            );
        }
        assert!(address_to_url("rust", &style).starts_with("https://duckduckgo.com/?q=rust&"));
    }

    #[test]
    fn search_url_carries_the_style() {
        assert_eq!(
            style().search_url("x"),
            "https://duckduckgo.com/?q=x&kae=d&k7=101412&kj=101412&k8=e6ece8\
             &k9=9fd4b4&kaa=9fd4b4&kx=8a948e&kt=IBM+Plex+Sans&k1=-1"
        );
        let light = PageStyle {
            dark: false,
            ..style()
        };
        assert!(light.search_url("x").contains("&kae=-1&"));
    }

    #[test]
    fn search_url_encodes_the_query() {
        let url = style().search_url("c++ & rust?");
        assert!(
            url.starts_with("https://duckduckgo.com/?q=c%2B%2B+%26+rust%3F&"),
            "{url}"
        );
        let url = style().search_url("café #1 a=b/c");
        assert!(
            url.starts_with("https://duckduckgo.com/?q=caf%C3%A9+%231+a%3Db%2Fc&"),
            "{url}"
        );
        let url = style().search_url("keep-these_.~");
        assert!(url.starts_with("https://duckduckgo.com/?q=keep-these_.~&"), "{url}");

        let odd_font = PageStyle {
            font: "A&B=C".into(),
            ..style()
        };
        assert!(odd_font.search_url("x").contains("&kt=A%26B%3DC&"));
    }

    #[test]
    fn start_page_escapes_user_text() {
        let page_style = PageStyle {
            font: "Evil\"</style><script>alert(1)</script>".into(),
            placeholder: "say \"hi\" <b>&</b>".into(),
            language: "en\"><script>".into(),
            ..style()
        };
        let html = page_style.start_page_html();
        assert!(!html.contains("<script>"), "{html}");
        assert!(!html.contains("<b>"), "{html}");
        assert!(html.contains(
            "placeholder=\"say &quot;hi&quot; &lt;b&gt;&amp;&lt;/b&gt;\""
        ));
        assert!(html.contains(
            "font-family:\"Evil&quot;&lt;/style&gt;&lt;script&gt;alert(1)&lt;/script&gt;\""
        ));
        assert!(html.contains("<html lang=\"en&quot;&gt;&lt;script&gt;\""));
        assert_eq!(html.matches("</style>").count(), 1);
    }

    #[test]
    fn start_page_escapes_hidden_inputs() {
        let page_style = PageStyle {
            font: "A&B \"Sans\"".into(),
            ..style()
        };
        let html = page_style.start_page_html();
        assert!(
            html.contains("name=\"kt\" value=\"A&amp;B &quot;Sans&quot;\""),
            "{html}"
        );
    }

    #[test]
    fn start_page_direction_and_scheme() {
        let html = style().start_page_html();
        assert!(html.contains("dir=\"ltr\""));
        assert!(html.contains("content=\"dark\""));
        assert!(html.contains("name=\"kae\" value=\"d\""));

        let right_to_left = PageStyle {
            dark: false,
            language: "ar".into(),
            right_to_left: true,
            ..style()
        };
        let html = right_to_left.start_page_html();
        assert!(html.contains("<html lang=\"ar\" dir=\"rtl\">"));
        assert!(html.contains("content=\"light\""));
        assert!(html.contains("name=\"kae\" value=\"-1\""));
    }

    #[test]
    fn start_page_shows_the_wallpaper() {
        let wallpaper = std::env::temp_dir().join("noah wallpaper.jpg");
        let expected_url = url::Url::from_file_path(&wallpaper)
            .expect("temp_dir is absolute")
            .to_string();
        let page_style = PageStyle {
            wallpaper: Some(wallpaper),
            wallpaper_opacity: 0.25,
            ..style()
        };
        let html = page_style.start_page_html();
        assert!(html.contains("body::before"));
        assert!(html.contains(&format!("url(\"{expected_url}\")")), "{html}");
        assert!(expected_url.contains("noah%20wallpaper.jpg"));
        assert!(html.contains("opacity:0.25;"));
    }

    #[test]
    fn web_font_family_maps_bundled_fonts() {
        assert_eq!(web_font_family(".ZedSans"), "IBM Plex Sans");
        assert_eq!(web_font_family("Zed Plex Sans"), "IBM Plex Sans");
        assert_eq!(web_font_family(".ZedMono"), "Lilex");
        assert_eq!(web_font_family("Zed Plex Mono"), "Lilex");
        assert_eq!(web_font_family(".SystemUIFont"), "SystemUIFont");
        assert_eq!(web_font_family("Inter"), "Inter");
    }

    #[test]
    fn escape_html_escapes_ampersands_first() {
        assert_eq!(escape_html("&lt;"), "&amp;lt;");
        assert_eq!(escape_html("<a href=\"x\">"), "&lt;a href=&quot;x&quot;&gt;");
        assert_eq!(escape_html("plain"), "plain");
    }
}
