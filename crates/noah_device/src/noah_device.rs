pub mod adblock;
pub mod checks;
pub mod duplicates;
pub mod health;
pub mod startup;

use anyhow::{Context as _, Result, bail};
use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub(crate) struct CommandOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn success(&self) -> bool {
        self.exit_code == Some(0)
    }

    /// Stdout and stderr together, for tools that print their answer to either stream.
    #[cfg_attr(windows, allow(dead_code))]
    pub fn combined(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }

    pub fn error_summary(&self) -> String {
        let stderr = self.stderr.trim();
        let message = if stderr.is_empty() {
            self.stdout.trim()
        } else {
            stderr
        };
        let first_line = message.lines().next().unwrap_or_default().trim();
        match (self.exit_code, first_line.is_empty()) {
            (Some(code), true) => format!("exited with code {code}"),
            (None, true) => "was terminated".to_string(),
            (_, false) => first_line.to_string(),
        }
    }
}

// This crate's API is deliberately blocking and is called from background threads, so the
// std process API (disallowed workspace-wide in favor of async commands) is what we want here.
#[allow(clippy::disallowed_methods)]
pub(crate) fn run_command(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<CommandOutput> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console_window(&mut command);

    let mut child = command
        .spawn()
        .with_context(|| format!("could not run {program}"))?;

    // Pipes are drained on separate threads so a chatty command can't fill a pipe buffer and
    // stall before we get to read it.
    let stdout_reader = child
        .stdout
        .take()
        .map(|stdout| thread::spawn(|| read_lossy(stdout)));
    let stderr_reader = child
        .stderr
        .take()
        .map(|stderr| thread::spawn(|| read_lossy(stderr)));

    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("could not wait for {program}"))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            if let Err(error) = child.kill() {
                log::warn!("failed to kill timed out {program}: {error}");
            }
            if let Err(error) = child.wait() {
                log::warn!("failed to reap timed out {program}: {error}");
            }
            bail!(
                "{program} did not finish within {} seconds",
                timeout.as_secs()
            );
        }
        thread::sleep(Duration::from_millis(25));
    };

    Ok(CommandOutput {
        exit_code: status.code(),
        stdout: join_reader(stdout_reader),
        stderr: join_reader(stderr_reader),
    })
}

fn read_lossy(mut reader: impl Read) -> String {
    let mut bytes = Vec::new();
    if let Err(error) = reader.read_to_end(&mut bytes) {
        log::debug!("failed to read command output: {error}");
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn join_reader(reader: Option<thread::JoinHandle<String>>) -> String {
    reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default()
}

#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt as _;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut Command) {}

/// Runs a PowerShell script. The script is passed with `-EncodedCommand` so that no quoting of
/// paths or strings inside it can be mangled by the Windows command line.
#[cfg(windows)]
pub(crate) fn run_powershell(script: &str, timeout: Duration) -> Result<CommandOutput> {
    let script = format!("[Console]::OutputEncoding = [System.Text.Encoding]::UTF8\n{script}");
    let encoded = encode_powershell_command(&script);
    run_command(
        "powershell",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encoded,
        ],
        timeout,
    )
}

/// Encodes a script for PowerShell's `-EncodedCommand`: base64 of its UTF-16LE bytes.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn encode_powershell_command(script: &str) -> String {
    let bytes: Vec<u8> = script
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    base64_encode(&bytes)
}

#[cfg_attr(not(windows), allow(dead_code))]
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk.first().copied().unwrap_or(0);
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        let combined = (u32::from(first) << 16) | (u32::from(second) << 8) | u32::from(third);
        let sextets = [
            (combined >> 18) & 0x3f,
            (combined >> 12) & 0x3f,
            (combined >> 6) & 0x3f,
            combined & 0x3f,
        ];
        for (index, sextet) in sextets.iter().enumerate() {
            if index <= chunk.len() {
                encoded.push(char::from(ALPHABET[*sextet as usize]));
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}

/// Quotes a string as a PowerShell single-quoted literal.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Formats a byte count for people, e.g. `1.5 GB`.
pub(crate) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit_index = 0;
    while value >= 1024.0 && unit_index + 1 < UNITS.len() {
        value /= 1024.0;
        unit_index += 1;
    }
    let unit = UNITS.get(unit_index).copied().unwrap_or("bytes");
    if unit_index == 0 {
        format!("{bytes} {unit}")
    } else {
        format!("{value:.1} {unit}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn powershell_encoding_uses_utf16_le() {
        // `dir` in UTF-16LE is 64 00 69 00 72 00.
        assert_eq!(encode_powershell_command("dir"), "ZABpAHIA");
    }

    #[test]
    fn powershell_quote_escapes_single_quotes() {
        assert_eq!(powershell_quote(r"C:\it's here"), r"'C:\it''s here'");
    }

    #[test]
    fn format_bytes_picks_units() {
        assert_eq!(format_bytes(0), "0 bytes");
        assert_eq!(format_bytes(1023), "1023 bytes");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024 * 1024), "5.0 GB");
    }

    #[test]
    fn run_command_reports_missing_program() {
        assert!(run_command("noah-definitely-not-a-command", &[], COMMAND_TIMEOUT).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn run_command_captures_output_and_times_out() {
        let output = run_command(
            "sh",
            &["-c", "echo hello; echo oops >&2; exit 3"],
            COMMAND_TIMEOUT,
        )
        .unwrap();
        assert_eq!(output.stdout.trim(), "hello");
        assert_eq!(output.stderr.trim(), "oops");
        assert_eq!(output.exit_code, Some(3));
        assert!(!output.success());
        assert_eq!(output.error_summary(), "oops");

        let started = Instant::now();
        let result = run_command("sleep", &["5"], Duration::from_millis(200));
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(4));
    }
}
