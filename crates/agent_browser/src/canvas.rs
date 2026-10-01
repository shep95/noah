//! The canvas: the interface shepherd just wrote, shown in the browser room
//! without being asked. A folder with an `index.html` is served from a port
//! of noah's own; a project with a `dev` or `start` script gets that script
//! started and its address read off its output. Servers live as long as noah
//! does and are reused for the same folder.

use anyhow::{Context as _, Result, bail};
use std::collections::HashMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

struct Served {
    url: String,
    /// The dev server's process, so a second canvas for the same folder
    /// reuses it and noah's exit takes it down.
    child: Option<smol::process::Child>,
}

fn served() -> &'static Mutex<HashMap<PathBuf, Arc<Mutex<Served>>>> {
    static SERVED: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<Served>>>>> = OnceLock::new();
    SERVED.get_or_init(Default::default)
}

/// What the canvas would show for this folder, if anything: the address of
/// a running server for it, or nothing when it holds no interface.
pub fn kind_of(root: &Path) -> Option<CanvasKind> {
    let package = root.join("package.json");
    if package.is_file()
        && let Ok(text) = std::fs::read_to_string(&package)
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(scripts) = json.get("scripts").and_then(|scripts| scripts.as_object())
    {
        for script in ["dev", "start", "serve", "preview"] {
            if scripts.contains_key(script) {
                return Some(CanvasKind::DevScript(script.to_string()));
            }
        }
    }
    for candidate in [
        "index.html",
        "public/index.html",
        "src/index.html",
        "dist/index.html",
    ] {
        if root.join(candidate).is_file() {
            return Some(CanvasKind::Static(candidate.to_string()));
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq)]
pub enum CanvasKind {
    /// A page to serve straight from the folder.
    Static(String),
    /// A `package.json` script that starts a dev server.
    DevScript(String),
}

/// Serves or starts the folder's interface and returns its address.
pub async fn open(root: PathBuf) -> Result<String> {
    let kind = kind_of(&root).context("this folder has no interface to show")?;
    if let Some(entry) = served().lock().ok().and_then(|map| map.get(&root).cloned())
        && let Ok(mut entry) = entry.lock()
    {
        let alive = match entry.child.as_mut() {
            Some(child) => matches!(child.try_status(), Ok(None)),
            None => true,
        };
        if alive && reachable(&entry.url).await {
            return Ok(entry.url.clone());
        }
    }
    let entry = match kind {
        CanvasKind::Static(page) => {
            let port = serve_static(root.clone())?;
            Served {
                url: format!("http://127.0.0.1:{port}/{page}"),
                child: None,
            }
        }
        CanvasKind::DevScript(script) => start_dev_script(&root, &script).await?,
    };
    let url = entry.url.clone();
    if let Ok(mut map) = served().lock() {
        map.insert(root, Arc::new(Mutex::new(entry)));
    }
    Ok(url)
}

async fn reachable(url: &str) -> bool {
    let Some(address) = url
        .strip_prefix("http://")
        .and_then(|rest| rest.split('/').next())
        .map(|host| host.to_string())
    else {
        return false;
    };
    smol::net::TcpStream::connect(address).await.is_ok()
}

/// A small file server on a free loopback port, for pages that need no
/// build. Paths stay inside the folder; nothing else is reachable.
fn serve_static(root: PathBuf) -> Result<u16> {
    let server = tiny_http::Server::http("127.0.0.1:0")
        .map_err(|error| anyhow::anyhow!("could not open a port for the canvas: {error}"))?;
    let port = server.server_addr().to_ip().context("no port")?.port();
    std::thread::Builder::new()
        .name("noah-canvas".into())
        .spawn(move || {
            let canonical_root = root.canonicalize().unwrap_or_else(|_| root.clone());
            for request in server.incoming_requests() {
                // Only a browser on this machine, addressing the server by
                // its own name: a web page elsewhere that rebinds a host name
                // to 127.0.0.1 carries that name in the Host header and gets
                // nothing.
                let host_ok = request
                    .headers()
                    .iter()
                    .find(|header| header.field.equiv("Host"))
                    .map(|header| host_is_local(header.value.as_str()))
                    .unwrap_or(false);
                let raw = request
                    .url()
                    .split(['?', '#'])
                    .next()
                    .unwrap_or("/")
                    .to_string();
                let decoded = percent_decode(&raw);
                let mut path = root.clone();
                let mut escaped = false;
                for part in decoded.split('/') {
                    match part {
                        "" | "." => {}
                        // Backslashes and drive letters would let Windows'
                        // path joining leave the folder; hidden files (.env,
                        // .git) are never served.
                        part if part == ".."
                            || part.contains(['\\', ':'])
                            || part.starts_with('.') =>
                        {
                            escaped = true
                        }
                        part => path.push(part),
                    }
                }
                if path.is_dir() {
                    path = path.join("index.html");
                }
                // A symlink inside the folder may point outside it.
                let inside = path
                    .canonicalize()
                    .map(|real| real.starts_with(&canonical_root))
                    .unwrap_or(false);
                let response = if !host_ok {
                    tiny_http::Response::from_string("not from here").with_status_code(403)
                } else if escaped || !inside || !path.is_file() {
                    tiny_http::Response::from_string("not here").with_status_code(404)
                } else {
                    match std::fs::File::open(&path) {
                        Ok(file) => {
                            let mut bytes = Vec::new();
                            let mut limited = file.take(MAX_SERVED_BYTES + 1);
                            if limited.read_to_end(&mut bytes).is_err() {
                                tiny_http::Response::from_string("unreadable").with_status_code(500)
                            } else if bytes.len() as u64 > MAX_SERVED_BYTES {
                                tiny_http::Response::from_string("too large").with_status_code(413)
                            } else {
                                let content_type = content_type_for(&path);
                                tiny_http::Response::from_data(bytes).with_header(
                                    tiny_http::Header::from_bytes("Content-Type", content_type)
                                        .expect("static header"),
                                )
                            }
                        }
                        Err(_) => {
                            tiny_http::Response::from_string("unreadable").with_status_code(500)
                        }
                    }
                };
                let response = response.with_header(
                    tiny_http::Header::from_bytes("Cache-Control", "no-store")
                        .expect("static header"),
                );
                if let Err(error) = request.respond(response) {
                    log::debug!("canvas: {error}");
                }
            }
        })
        .context("could not start the canvas thread")?;
    Ok(port)
}

/// A page's own assets are small; anything past this is not one.
const MAX_SERVED_BYTES: u64 = 64 * 1024 * 1024;

fn host_is_local(host: &str) -> bool {
    let name = host.rsplit_once(':').map(|(name, _)| name).unwrap_or(host);
    matches!(name, "127.0.0.1" | "localhost" | "[::1]")
}

fn percent_decode(text: &str) -> String {
    // Works on bytes throughout: slicing the string could split a multi-byte
    // character and panic on a crafted request.
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Some(high) = (bytes[index + 1] as char).to_digit(16)
            && let Some(low) = (bytes[index + 2] as char).to_digit(16)
        {
            out.push((high * 16 + low) as u8);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn content_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        "txt" | "md" => "text/plain; charset=utf-8",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

/// Runs the project's dev script through its package manager and waits for
/// the address it prints. The process stays up for the room.
async fn start_dev_script(root: &Path, script: &str) -> Result<Served> {
    let manager = if root.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if root.join("yarn.lock").is_file() {
        "yarn"
    } else if root.join("bun.lockb").is_file() || root.join("bun.lock").is_file() {
        "bun"
    } else {
        "npm"
    };
    if !root.join("node_modules").is_dir() {
        let status = util::command::new_command(manager)
            .arg("install")
            .current_dir(root)
            .status()
            .await
            .with_context(|| {
                format!("{manager} is not installed, so the dev server cannot start")
            })?;
        if !status.success() {
            bail!("`{manager} install` failed in {}", root.display());
        }
    }
    let mut command = util::command::new_command(manager);
    if manager == "npm" {
        command.arg("run");
    }
    command
        .arg(script)
        .current_dir(root)
        .env("BROWSER", "none")
        .env("CI", "true")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command
        .spawn()
        .with_context(|| format!("{manager} is not installed, so the dev server cannot start"))?;
    let stdout = child
        .stdout
        .take()
        .context("no output from the dev server")?;
    let stderr = child
        .stderr
        .take()
        .context("no output from the dev server")?;
    let (sender, receiver) = smol::channel::unbounded::<String>();
    for stream in [
        Box::new(stdout) as Box<dyn futures::AsyncRead + Unpin + Send>,
        Box::new(stderr),
    ] {
        let sender = sender.clone();
        smol::spawn(async move {
            use futures::AsyncBufReadExt as _;
            let mut lines = futures::io::BufReader::new(stream).lines();
            use futures::StreamExt as _;
            while let Some(Ok(line)) = lines.next().await {
                if sender.send(line).await.is_err() {
                    break;
                }
            }
        })
        .detach();
    }
    drop(sender);
    let deadline = std::time::Instant::now() + Duration::from_secs(90);
    let mut transcript = String::new();
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let line = futures::future::select(
            Box::pin(receiver.recv()),
            Box::pin(smol::Timer::after(remaining)),
        )
        .await;
        let line = match line {
            futures::future::Either::Left((Ok(line), _)) => line,
            _ => break,
        };
        transcript.push_str(&line);
        transcript.push('\n');
        if let Some(url) = local_url_in(&line) {
            return Ok(Served {
                url,
                child: Some(child),
            });
        }
        if let Ok(Some(status)) = child.try_status() {
            bail!(
                "the dev server stopped ({status}):\n{}",
                tail(&transcript, 1200)
            );
        }
    }
    child.kill().ok();
    bail!(
        "the dev server printed no address in 90 seconds:\n{}",
        tail(&transcript, 1200)
    )
}

fn tail(text: &str, keep: usize) -> &str {
    let start = text.len().saturating_sub(keep);
    let start = text
        .char_indices()
        .map(|(index, _)| index)
        .find(|index| *index >= start)
        .unwrap_or(0);
    &text[start..]
}

/// The first local address in a line of dev-server output, with terminal
/// coloring stripped.
pub fn local_url_in(line: &str) -> Option<String> {
    let plain = strip_ansi(line);
    for start in plain
        .match_indices("http://")
        .chain(plain.match_indices("https://"))
    {
        let candidate: String = plain[start.0..]
            .chars()
            .take_while(|character| {
                !character.is_whitespace() && !matches!(character, '"' | '\'' | ')' | ']' | ',')
            })
            .collect();
        let host = candidate
            .split("//")
            .nth(1)
            .unwrap_or("")
            .split('/')
            .next()
            .unwrap_or("");
        let name = host.split(':').next().unwrap_or("");
        if matches!(
            name,
            "localhost" | "127.0.0.1" | "0.0.0.0" | "[::1]" | "[::]"
        ) {
            return Some(
                candidate
                    .replace("0.0.0.0", "127.0.0.1")
                    .replace("[::]", "127.0.0.1"),
            );
        }
    }
    None
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' {
            if characters.peek() == Some(&'[') {
                characters.next();
                for inner in characters.by_ref() {
                    if inner.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(character);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::local_url_in;

    #[test]
    fn finds_the_local_address_in_dev_server_output() {
        assert_eq!(
            local_url_in("  ➜  Local:   \u{1b}[36mhttp://localhost:5173/\u{1b}[39m").as_deref(),
            Some("http://localhost:5173/")
        );
        assert_eq!(
            local_url_in("- Local:        http://0.0.0.0:3000").as_deref(),
            Some("http://127.0.0.1:3000")
        );
        assert_eq!(local_url_in("Compiled successfully"), None);
        assert_eq!(local_url_in("see https://example.com/docs"), None);
    }
}
