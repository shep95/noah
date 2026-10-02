//! The native messaging host behind noah shield. The browser extension cannot
//! turn a backlight or run a program, so it asks noah, which the browser
//! starts on demand (`noah` with the extension's origin as its argument) and
//! talks to over stdin and stdout in the native messaging framing: a
//! little-endian length, then a JSON message.
//!
//! What it does for the shield:
//! * the screen's real brightness (WMI on Windows, brightnessctl or sysfs on
//!   Linux, the `brightness` tool on macOS);
//! * Tor for the shield's tunnel: finds an installed Tor, fetches the Tor
//!   Project's expert bundle when there is none, starts it on a port of its
//!   own with an exit country when one is asked for, reports how far it has
//!   bootstrapped, and stops it.

use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

pub const HOST_NAME: &str = "com.asherin.noah_shield";
/// The id the shield's manifest key resolves to in Chromium browsers.
pub const CHROME_EXTENSION_ID: &str = "mabjdhafmgbhjaeiaojlfnjfcjeombaj";
pub const FIREFOX_EXTENSION_ID: &str = "shield@noah.asherin.com";
/// Not Tor's usual 9050 or Tor Browser's 9150, so both can run alongside.
pub const SOCKS_PORT: u16 = 9350;
const TOR_DIST: &str = "https://dist.torproject.org/torbrowser/";
const MAX_MESSAGE: usize = 1024 * 1024;

/// True when the browser started this process as the shield's host.
pub fn invoked_by_browser(args: &[String]) -> bool {
    args.iter().skip(1).any(|argument| {
        argument.starts_with("chrome-extension://")
            || argument.starts_with("moz-extension://")
            || argument == FIREFOX_EXTENSION_ID
    })
}

pub fn serve() -> Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    loop {
        let mut length = [0u8; 4];
        if let Err(error) = input.read_exact(&mut length) {
            if error.kind() == std::io::ErrorKind::UnexpectedEof {
                return Ok(());
            }
            return Err(error.into());
        }
        let length = u32::from_le_bytes(length) as usize;
        if length > MAX_MESSAGE {
            bail!("message of {length} bytes is too large");
        }
        let mut body = vec![0u8; length];
        input.read_exact(&mut body)?;
        let reply = match serde_json::from_slice::<Value>(&body) {
            Ok(message) => {
                handle(&message).unwrap_or_else(|error| json!({ "error": format!("{error:#}") }))
            }
            Err(error) => json!({ "error": format!("not a message: {error}") }),
        };
        let bytes = serde_json::to_vec(&reply)?;
        output.write_all(&(bytes.len() as u32).to_le_bytes())?;
        output.write_all(&bytes)?;
        output.flush()?;
    }
}

fn handle(message: &Value) -> Result<Value> {
    let kind = message.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "hello" => Ok(json!({
            "ok": true,
            "host": env!("CARGO_PKG_VERSION"),
            "platform": std::env::consts::OS,
            "brightness": brightness::get().is_ok(),
            "tor": tor::status()?,
        })),
        "brightness.get" => Ok(json!({ "level": brightness::get()? })),
        "brightness.set" => {
            let level = message
                .get("level")
                .and_then(Value::as_u64)
                .context("level is missing")?;
            let level = level.min(100) as u8;
            brightness::set(level)?;
            Ok(json!({ "ok": true, "level": level }))
        }
        "tor.status" => tor::status(),
        "tor.install" => tor::install(),
        "tor.start" => {
            let country = message.get("country").and_then(Value::as_str).unwrap_or("");
            tor::start(country)
        }
        "tor.stop" => tor::stop(),
        "tor.log" => Ok(json!({ "log": tor::log_tail(60) })),
        // The shield's footprint page hands shepherd a removal plan. The
        // prompt only lands in the composer; the person still sends it, and
        // every browser action shepherd takes afterwards asks as usual.
        // The shield's "tell noah about this site": one JSON file per site
        // under ~/.noah/shield/sites, which shepherd's fetch and browser
        // tools read before trusting the site.
        "page.report" => {
            let report = message.get("report").context("report is missing")?;
            let site = report.get("site").and_then(Value::as_str).unwrap_or("");
            let file_name = noah_trust::shield_reports::file_name_for(site)
                .context("the report names no site")?;
            let text = serde_json::to_vec_pretty(report)?;
            anyhow::ensure!(text.len() <= MAX_REPORT_BYTES, "the report is too large");
            let directory = paths::shield_sites_directory();
            std::fs::create_dir_all(&directory)
                .with_context(|| format!("couldn't create {}", directory.display()))?;
            let target = directory.join(file_name);
            let partial = directory.join(format!(".{}.partial", std::process::id()));
            std::fs::write(&partial, &text)
                .with_context(|| format!("couldn't write {}", partial.display()))?;
            std::fs::rename(&partial, &target)
                .with_context(|| format!("couldn't place {}", target.display()))?;
            Ok(json!({ "ok": true, "path": target.display().to_string() }))
        }
        // The one-way clipboard: names and salted hashes of the person's
        // stored secrets, so the shield can put `$NAME` where a secret was
        // pasted. The values never leave noah.
        "secrets.fingerprints" => {
            let path = paths::shield_secret_fingerprints_file();
            let text = match std::fs::metadata(&path) {
                Ok(metadata) if metadata.len() <= MAX_REPORT_BYTES as u64 => {
                    std::fs::read_to_string(&path)
                        .with_context(|| format!("couldn't read {}", path.display()))?
                }
                Ok(_) => anyhow::bail!("the fingerprint file is too large"),
                Err(_) => return Ok(json!({ "salt": "", "secrets": [] })),
            };
            let fingerprints: Value =
                serde_json::from_str(&text).context("the fingerprint file is not json")?;
            Ok(fingerprints)
        }
        "agent.prompt" => {
            let prompt = message
                .get("prompt")
                .and_then(Value::as_str)
                .context("prompt is missing")?;
            anyhow::ensure!(
                prompt.len() <= MAX_AGENT_PROMPT_BYTES,
                "the prompt is too long"
            );
            open_agent_prompt(prompt)?;
            Ok(json!({ "ok": true }))
        }
        other => bail!("unknown request {other:?}"),
    }
}

/// A removal plan lists a few dozen forms with the person's details; far
/// more than this is not one.
const MAX_AGENT_PROMPT_BYTES: usize = 24 * 1024;
/// The encoded prompt travels as one argument; Windows takes about 32 K
/// characters on a command line.
const MAX_ENCODED_PROMPT_CHARS: usize = 30_000;
/// A site report from the shield is a few KB.
const MAX_REPORT_BYTES: usize = 64 * 1024;

/// Opens noah (or reaches the running copy) with the prompt waiting in
/// shepherd's composer, the same way a `zed://agent?prompt=` link does.
fn open_agent_prompt(prompt: &str) -> Result<()> {
    let url = format!("zed://agent?prompt={}", urlencoding::encode(prompt));
    anyhow::ensure!(
        url.len() <= MAX_ENCODED_PROMPT_CHARS,
        "the prompt is too long to hand over; copy it instead"
    );
    let cli = std::env::current_exe().context("could not find noah's command-line program")?;
    Command::new(cli)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("could not start noah")?;
    Ok(())
}

// ---- registration ------------------------------------------------------------------------------

/// Writes the host manifests and tells every browser on this machine where
/// they are. Returns the places it registered in.
pub fn register(host_binary: &Path) -> Result<Vec<String>> {
    let host_binary =
        std::fs::canonicalize(host_binary).unwrap_or_else(|_| host_binary.to_path_buf());
    let directory = paths::data_dir().join("shield-host");
    std::fs::create_dir_all(&directory)?;
    let path_text = host_binary.to_string_lossy().into_owned();
    // Windows paths come back from canonicalize with the \\?\ prefix, which
    // Chrome will not launch.
    let path_text = path_text.trim_start_matches(r"\\?\").to_string();
    let chrome_manifest = directory.join(format!("{HOST_NAME}.json"));
    let firefox_manifest = directory.join(format!("{HOST_NAME}.firefox.json"));
    std::fs::write(
        &chrome_manifest,
        serde_json::to_vec_pretty(&json!({
            "name": HOST_NAME,
            "description": "noah, for noah shield: screen brightness and Tor",
            "path": path_text,
            "type": "stdio",
            "allowed_origins": [format!("chrome-extension://{CHROME_EXTENSION_ID}/")],
        }))?,
    )?;
    std::fs::write(
        &firefox_manifest,
        serde_json::to_vec_pretty(&json!({
            "name": HOST_NAME,
            "description": "noah, for noah shield: screen brightness and Tor",
            "path": path_text,
            "type": "stdio",
            "allowed_extensions": [FIREFOX_EXTENSION_ID],
        }))?,
    )?;
    let mut registered = Vec::new();
    #[cfg(target_os = "windows")]
    {
        for vendor in [
            r"Google\Chrome",
            "Chromium",
            r"Microsoft\Edge",
            r"BraveSoftware\Brave-Browser",
            "Vivaldi",
        ] {
            let key = format!(r"HKCU\Software\{vendor}\NativeMessagingHosts\{HOST_NAME}");
            registry_set(&key, &chrome_manifest)?;
            registered.push(key);
        }
        let key = format!(r"HKCU\Software\Mozilla\NativeMessagingHosts\{HOST_NAME}");
        registry_set(&key, &firefox_manifest)?;
        registered.push(key);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = paths::home_dir();
        let (chromium_roots, firefox_root): (Vec<PathBuf>, PathBuf) = if cfg!(target_os = "macos") {
            let support = home.join("Library/Application Support");
            (
                [
                    "Google/Chrome",
                    "Chromium",
                    "Microsoft Edge",
                    "BraveSoftware/Brave-Browser",
                    "Vivaldi",
                ]
                .iter()
                .map(|vendor| support.join(vendor).join("NativeMessagingHosts"))
                .collect(),
                support.join("Mozilla/NativeMessagingHosts"),
            )
        } else {
            let config = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"));
            (
                [
                    "google-chrome",
                    "google-chrome-beta",
                    "chromium",
                    "microsoft-edge",
                    "BraveSoftware/Brave-Browser",
                    "vivaldi",
                ]
                .iter()
                .map(|vendor| config.join(vendor).join("NativeMessagingHosts"))
                .collect(),
                home.join(".mozilla/native-messaging-hosts"),
            )
        };
        for root in chromium_roots {
            std::fs::create_dir_all(&root)?;
            let target = root.join(format!("{HOST_NAME}.json"));
            std::fs::copy(&chrome_manifest, &target)?;
            registered.push(target.to_string_lossy().into_owned());
        }
        std::fs::create_dir_all(&firefox_root)?;
        let target = firefox_root.join(format!("{HOST_NAME}.json"));
        std::fs::copy(&firefox_manifest, &target)?;
        registered.push(target.to_string_lossy().into_owned());
    }
    Ok(registered)
}

#[cfg(target_os = "windows")]
fn registry_set(key: &str, manifest: &Path) -> Result<()> {
    let output = hidden(Command::new("reg"))
        .args(["add", key, "/ve", "/t", "REG_SZ", "/d"])
        .arg(manifest)
        .arg("/f")
        .output()
        .context("reg.exe could not run")?;
    if !output.status.success() {
        bail!(
            "reg add {key} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// A command that opens no console window of its own.
fn hidden(mut command: Command) -> Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.stdin(Stdio::null());
    command
}

// ---- brightness --------------------------------------------------------------------------------

pub mod brightness {
    use super::*;

    #[cfg(target_os = "windows")]
    fn powershell(script: &str) -> Result<String> {
        let output = hidden(Command::new("powershell"))
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                script,
            ])
            .output()
            .context("powershell could not run")?;
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            let error = error.lines().next().unwrap_or("").trim().to_string();
            bail!("this screen does not take brightness commands ({error})");
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    #[cfg(target_os = "windows")]
    pub fn get() -> Result<u8> {
        let text = powershell(
            "(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness -ErrorAction Stop | Select-Object -First 1).CurrentBrightness",
        )?;
        text.parse::<u8>()
            .map_err(|_| anyhow::anyhow!("this screen does not report its brightness"))
    }

    #[cfg(target_os = "windows")]
    pub fn set(level: u8) -> Result<()> {
        powershell(&format!(
            "$m = Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods -ErrorAction Stop | Select-Object -First 1; Invoke-CimMethod -InputObject $m -MethodName WmiSetBrightness -Arguments @{{Timeout=[uint32]1; Brightness=[byte]{level}}} -ErrorAction Stop | Out-Null"
        ))?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    pub fn get() -> Result<u8> {
        let output = hidden(Command::new("brightness"))
            .arg("-l")
            .output()
            .context("the `brightness` tool is not installed (brew install brightness)")?;
        let text = String::from_utf8_lossy(&output.stdout);
        let value = text
            .lines()
            .find_map(|line| {
                line.rsplit("brightness ")
                    .next()
                    .and_then(|tail| tail.trim().parse::<f64>().ok())
            })
            .context("this screen does not report its brightness")?;
        Ok((value * 100.0).round().clamp(0.0, 100.0) as u8)
    }

    #[cfg(target_os = "macos")]
    pub fn set(level: u8) -> Result<()> {
        let status = hidden(Command::new("brightness"))
            .arg(format!("{:.2}", level as f64 / 100.0))
            .status()
            .context("the `brightness` tool is not installed (brew install brightness)")?;
        if !status.success() {
            bail!("this screen does not take brightness commands");
        }
        Ok(())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    fn backlight() -> Option<PathBuf> {
        let entries = std::fs::read_dir("/sys/class/backlight").ok()?;
        entries
            .flatten()
            .map(|entry| entry.path())
            .find(|path| path.join("brightness").is_file())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    pub fn get() -> Result<u8> {
        if let Ok(output) = hidden(Command::new("brightnessctl")).args(["-m"]).output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(percent) = text
                .split(',')
                .nth(3)
                .and_then(|field| field.trim_end_matches('%').parse::<u8>().ok())
            {
                return Ok(percent);
            }
        }
        let device = backlight().context("no backlight this computer lets programs turn")?;
        let current: f64 = std::fs::read_to_string(device.join("brightness"))?
            .trim()
            .parse()?;
        let max: f64 = std::fs::read_to_string(device.join("max_brightness"))?
            .trim()
            .parse()?;
        Ok((current / max.max(1.0) * 100.0).round().clamp(0.0, 100.0) as u8)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    pub fn set(level: u8) -> Result<()> {
        if let Ok(status) = hidden(Command::new("brightnessctl"))
            .args(["-q", "set", &format!("{level}%")])
            .status()
            && status.success()
        {
            return Ok(());
        }
        let device = backlight().context("no backlight this computer lets programs turn")?;
        let max: f64 = std::fs::read_to_string(device.join("max_brightness"))?
            .trim()
            .parse()?;
        let value = (max * level as f64 / 100.0)
            .round()
            .max(if level == 0 { 0.0 } else { 1.0 });
        std::fs::write(device.join("brightness"), format!("{}", value as u64))
            .context("the backlight is not writable for this user (install brightnessctl, or add yourself to the video group)")?;
        Ok(())
    }
}

// ---- tor ---------------------------------------------------------------------------------------

pub mod tor {
    use super::*;
    use sha2::Digest as _;

    fn tor_dir() -> PathBuf {
        paths::data_dir().join("tor")
    }

    fn executable(name: &str) -> String {
        if cfg!(target_os = "windows") {
            format!("{name}.exe")
        } else {
            name.to_string()
        }
    }

    /// Where Tor Browser keeps its own tor, when it is installed.
    fn browser_candidates() -> Vec<PathBuf> {
        let home = paths::home_dir();
        let mut candidates = Vec::new();
        if cfg!(target_os = "windows") {
            let roots = [
                home.join("Desktop"),
                home.join("Downloads"),
                std::env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join("AppData/Local")),
                PathBuf::from(r"C:\Program Files"),
            ];
            for root in roots {
                candidates.push(root.join(r"Tor Browser\Browser\TorBrowser\Tor\tor.exe"));
            }
        } else if cfg!(target_os = "macos") {
            candidates.push(PathBuf::from(
                "/Applications/Tor Browser.app/Contents/MacOS/Tor/tor",
            ));
            candidates.push(home.join("Applications/Tor Browser.app/Contents/MacOS/Tor/tor"));
        } else {
            for root in [
                home.join("tor-browser"),
                home.join("Downloads/tor-browser"),
                home.join(".local/share/torbrowser/tbb/x86_64/tor-browser"),
            ] {
                candidates.push(root.join("Browser/TorBrowser/Tor/tor"));
            }
        }
        candidates
    }

    fn in_path(name: &str) -> Option<PathBuf> {
        let name = executable(name);
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|directory| directory.join(&name))
            .find(|candidate| candidate.is_file())
    }

    pub fn binary() -> Option<PathBuf> {
        if let Some(path) = std::env::var_os("NOAH_SHIELD_TOR").map(PathBuf::from)
            && path.is_file()
        {
            return Some(path);
        }
        let bundled = tor_dir().join("bin").join("tor").join(executable("tor"));
        if bundled.is_file() {
            return Some(bundled);
        }
        browser_candidates()
            .into_iter()
            .find(|candidate| candidate.is_file())
            .or_else(|| in_path("tor"))
    }

    /// The geoip tables next to a tor, needed to pick an exit country.
    fn geoip_for(binary: &Path) -> Option<(PathBuf, PathBuf)> {
        let mut candidates = Vec::new();
        if let Some(tor_folder) = binary.parent() {
            candidates.push(tor_folder.join("../data"));
            candidates.push(tor_folder.join("../Data/Tor"));
            candidates.push(tor_folder.join("../../Data/Tor"));
        }
        candidates.push(PathBuf::from("/usr/share/tor"));
        candidates.push(PathBuf::from("/usr/local/share/tor"));
        candidates.push(PathBuf::from("/opt/homebrew/share/tor"));
        candidates
            .into_iter()
            .map(|folder| (folder.join("geoip"), folder.join("geoip6")))
            .find(|(v4, v6)| v4.is_file() && v6.is_file())
    }

    fn pid_file() -> PathBuf {
        tor_dir().join("tor.pid")
    }

    fn country_file() -> PathBuf {
        tor_dir().join("tor.country")
    }

    fn log_file() -> PathBuf {
        tor_dir().join("tor.log")
    }

    fn saved_pid() -> Option<u32> {
        std::fs::read_to_string(pid_file())
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    fn listening() -> bool {
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], SOCKS_PORT));
        std::net::TcpStream::connect_timeout(&address, Duration::from_millis(400)).is_ok()
    }

    #[cfg(unix)]
    fn alive(pid: u32) -> bool {
        // Signal 0 checks that the process exists without touching it.
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    #[cfg(not(unix))]
    fn alive(pid: u32) -> bool {
        let output = hidden(Command::new("tasklist"))
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output();
        match output {
            Ok(output) => String::from_utf8_lossy(&output.stdout).contains(&format!("\"{pid}\"")),
            Err(_) => false,
        }
    }

    fn running() -> bool {
        saved_pid().map(alive).unwrap_or(false) || listening()
    }

    #[cfg(target_os = "linux")]
    fn is_tor_process(pid: u32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/comm"))
            .map(|name| name.trim() == "tor")
            .unwrap_or(false)
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    fn is_tor_process(pid: u32) -> bool {
        let output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output();
        match output {
            Ok(output) => String::from_utf8_lossy(&output.stdout)
                .trim()
                .rsplit('/')
                .next()
                .is_some_and(|name| name == "tor"),
            Err(_) => false,
        }
    }

    #[cfg(not(unix))]
    fn is_tor_process(pid: u32) -> bool {
        let output = hidden(Command::new("tasklist"))
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output();
        match output {
            Ok(output) => String::from_utf8_lossy(&output.stdout)
                .to_ascii_lowercase()
                .starts_with("\"tor.exe\""),
            Err(_) => false,
        }
    }

    pub fn log_tail(lines: usize) -> Vec<String> {
        let Ok(text) = std::fs::read_to_string(log_file()) else {
            return Vec::new();
        };
        let all: Vec<&str> = text.lines().collect();
        let start = all.len().saturating_sub(lines);
        all[start..].iter().map(|line| line.to_string()).collect()
    }

    fn bootstrapped() -> (u8, Option<String>) {
        let mut percent = 0u8;
        let mut problem = None;
        for line in log_tail(400) {
            if let Some(index) = line.find("Bootstrapped ") {
                let rest = &line[index + "Bootstrapped ".len()..];
                let digits: String = rest
                    .chars()
                    .take_while(|character| character.is_ascii_digit())
                    .collect();
                if let Ok(value) = digits.parse::<u8>() {
                    percent = value;
                    problem = None;
                }
            } else if line.contains("[warn]") || line.contains("[err]") {
                let text = line.splitn(2, "] ").nth(1).unwrap_or(&line).to_string();
                problem = Some(text);
            }
        }
        (percent, problem)
    }

    pub fn status() -> Result<Value> {
        let binary = binary();
        let running = running();
        let (percent, problem) = if running { bootstrapped() } else { (0, None) };
        let country = std::fs::read_to_string(country_file())
            .unwrap_or_default()
            .trim()
            .to_string();
        Ok(json!({
            "installed": binary.is_some(),
            "binary": binary.as_ref().map(|path| path.to_string_lossy().into_owned()),
            "countries": binary.as_ref().and_then(|path| geoip_for(path)).is_some(),
            "running": running,
            "bootstrapped": percent,
            "ready": running && percent >= 100,
            "problem": problem,
            "country": country,
            "port": SOCKS_PORT,
        }))
    }

    fn platform_bundle() -> Result<(&'static str, &'static str)> {
        let os = match std::env::consts::OS {
            "windows" => "windows",
            "macos" => "macos",
            "linux" => "linux",
            other => bail!("no Tor bundle for {other}"),
        };
        let arch = match std::env::consts::ARCH {
            "x86_64" => "x86_64",
            "aarch64" => "aarch64",
            "x86" => "i686",
            other => bail!("no Tor bundle for {other}"),
        };
        Ok((os, arch))
    }

    /// The newest stable Tor Browser release folder, which carries the expert bundle.
    fn latest_version(index: &str) -> Option<String> {
        let mut best: Option<(Vec<u32>, String)> = None;
        for piece in index.split("href=\"").skip(1) {
            let Some(end) = piece.find('"') else { continue };
            let name = piece[..end].trim_end_matches('/');
            if name.is_empty()
                || !name
                    .chars()
                    .all(|character| character.is_ascii_digit() || character == '.')
            {
                continue;
            }
            let parts: Option<Vec<u32>> = name.split('.').map(|part| part.parse().ok()).collect();
            let Some(parts) = parts else { continue };
            if best.as_ref().is_none_or(|(current, _)| parts > *current) {
                best = Some((parts, name.to_string()));
            }
        }
        best.map(|(_, name)| name)
    }

    /// Downloads with the system's curl (Windows 10 and later, macOS and
    /// nearly every Linux ship one), which keeps a TLS stack out of noah's
    /// command-line program. Only https, no redirects off it.
    fn fetch(url: &str) -> Result<Vec<u8>> {
        let target =
            std::env::temp_dir().join(format!("noah-shield-{}.download", std::process::id()));
        let output = hidden(Command::new("curl"))
            .args([
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--max-time",
                "900",
                "--user-agent",
                "noah-shield-host",
                "--output",
            ])
            .arg(&target)
            .arg(url)
            .output()
            .context("curl is not on this computer, and noah needs it to fetch Tor")?;
        if !output.status.success() {
            std::fs::remove_file(&target).ok();
            bail!(
                "could not fetch {url}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let bytes = std::fs::read(&target)?;
        std::fs::remove_file(&target).ok();
        Ok(bytes)
    }

    pub fn install() -> Result<Value> {
        let (os, arch) = platform_bundle()?;
        let index = String::from_utf8_lossy(&fetch(TOR_DIST)?).into_owned();
        let version = latest_version(&index)
            .context("the Tor Project's download list had no release in it")?;
        let name = format!("tor-expert-bundle-{os}-{arch}-{version}.tar.gz");
        let url = format!("{TOR_DIST}{version}/{name}");
        let bytes = fetch(&url)?;
        let sums = String::from_utf8_lossy(&fetch(&format!("{url}.sha256sum"))?).into_owned();
        let expected = sums
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let actual = format!("{:x}", sha2::Sha256::digest(&bytes));
        if expected.len() != 64 || expected != actual {
            bail!(
                "the downloaded Tor did not match the Tor Project's checksum; nothing was installed"
            );
        }
        let target = tor_dir().join("bin");
        if target.exists() {
            std::fs::remove_dir_all(&target)?;
        }
        std::fs::create_dir_all(&target)?;
        let decoder = flate2::read::GzDecoder::new(&bytes[..]);
        let mut archive = tar::Archive::new(decoder);
        archive.unpack(&target)?;
        let binary = target.join("tor").join(executable("tor"));
        if !binary.is_file() {
            bail!("the Tor bundle had no tor program where expected");
        }
        Ok(json!({ "ok": true, "version": version, "binary": binary.to_string_lossy() }))
    }

    pub fn start(country: &str) -> Result<Value> {
        let country: String = country
            .chars()
            .filter(|character| character.is_ascii_alphabetic())
            .take(2)
            .collect::<String>()
            .to_ascii_lowercase();
        let binary = binary().context("Tor is not installed yet")?;
        let previous = std::fs::read_to_string(country_file())
            .unwrap_or_default()
            .trim()
            .to_string();
        if running() && previous == country {
            return status();
        }
        stop()?;
        let directory = tor_dir();
        std::fs::create_dir_all(directory.join("data"))?;
        let geoip = geoip_for(&binary);
        if !country.is_empty() && geoip.is_none() {
            bail!(
                "this Tor has no country tables, so a location cannot be chosen; the fastest route still works"
            );
        }
        let mut torrc = format!(
            "SocksPort 127.0.0.1:{SOCKS_PORT}\nDataDirectory {}\nLog notice file {}\nClientOnly 1\nAvoidDiskWrites 1\n",
            directory.join("data").display(),
            log_file().display(),
        );
        if let Some((v4, v6)) = &geoip {
            torrc.push_str(&format!(
                "GeoIPFile {}\nGeoIPv6File {}\n",
                v4.display(),
                v6.display()
            ));
        }
        if !country.is_empty() {
            torrc.push_str(&format!("ExitNodes {{{country}}}\nStrictNodes 1\n"));
        }
        let torrc_path = directory.join("torrc");
        std::fs::write(&torrc_path, torrc)?;
        std::fs::write(log_file(), "")?;
        let mut command = Command::new(&binary);
        command
            .arg("-f")
            .arg(&torrc_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(folder) = binary.parent() {
            command.current_dir(folder);
        }
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt as _;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            command.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt as _;
            // Its own session, so it outlives this host, which the browser
            // ends as soon as it has its answer.
            command.process_group(0);
        }
        let child = command
            .spawn()
            .with_context(|| format!("{} could not start", binary.display()))?;
        std::fs::write(pid_file(), child.id().to_string())?;
        std::fs::write(country_file(), &country)?;
        // Give it a moment so the first status already says something.
        std::thread::sleep(Duration::from_millis(600));
        status()
    }

    pub fn stop() -> Result<Value> {
        if let Some(pid) = saved_pid() {
            // The pid file can be stale and the number reused by something
            // else of the person's; only a tor process is stopped.
            if !is_tor_process(pid) {
                std::fs::remove_file(pid_file()).ok();
                std::fs::remove_file(country_file()).ok();
                return Ok(json!({ "ok": true, "running": running() }));
            }
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
            #[cfg(not(unix))]
            {
                hidden(Command::new("taskkill"))
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .output()
                    .ok();
            }
            for _ in 0..20 {
                if !alive(pid) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            std::fs::remove_file(pid_file()).ok();
        }
        std::fs::remove_file(country_file()).ok();
        Ok(json!({ "ok": true, "running": running() }))
    }

    #[cfg(test)]
    mod tests {
        use super::latest_version;

        #[test]
        fn picks_the_newest_stable_folder() {
            let index = r#"<a href="13.5.9/">13.5.9/</a> <a href="14.5.4/">14.5.4/</a> <a href="15.0a3/">15.0a3/</a> <a href="14.5.10/">14.5.10/</a> <a href="../">../</a>"#;
            assert_eq!(latest_version(index).as_deref(), Some("14.5.10"));
        }
    }
}
