use anyhow::{Context as _, Result, anyhow, bail};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const BLOCKLIST_URL: &str = "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts";
pub const START_MARKER: &str = "# >>> noah ad blocker >>>";
pub const END_MARKER: &str = "# <<< noah ad blocker <<<";

const CANCELLED_MESSAGE: &str = "the administrator prompt was cancelled";

/// Host names that blocklists map to themselves for the system's own use, not for blocking.
const RESERVED_HOST_NAMES: [&str; 6] = [
    "localhost",
    "localhost.localdomain",
    "local",
    "broadcasthost",
    "0.0.0.0",
    "ip6-localhost",
];

pub fn hosts_path() -> PathBuf {
    if cfg!(windows) {
        let system_root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        system_root
            .join("System32")
            .join("drivers")
            .join("etc")
            .join("hosts")
    } else {
        PathBuf::from("/etc/hosts")
    }
}

/// Domains from `0.0.0.0 domain` / `127.0.0.1 domain` lines, lowercased and deduplicated in
/// order of first appearance.
pub fn parse_blocklist(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut domains = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or_default();
        let mut fields = line.split_whitespace();
        let Some(address) = fields.next() else {
            continue;
        };
        if address != "0.0.0.0" && address != "127.0.0.1" {
            continue;
        }
        for domain in fields {
            let domain = domain.trim_end_matches('.').to_ascii_lowercase();
            if !is_blockable_domain(&domain) {
                continue;
            }
            if seen.insert(domain.clone()) {
                domains.push(domain);
            }
        }
    }
    domains
}

fn is_blockable_domain(domain: &str) -> bool {
    !domain.is_empty()
        && !RESERVED_HOST_NAMES.contains(&domain)
        && !domain.starts_with("ip6-")
        && domain.parse::<std::net::IpAddr>().is_err()
        && domain.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
        })
}

fn line_ending(hosts: &str) -> &'static str {
    if hosts.contains("\r\n") { "\r\n" } else { "\n" }
}

fn is_marker(line: &str, marker: &str) -> bool {
    line.trim() == marker
}

/// Returns `hosts` with its noah section (if any) replaced by one blocking `domains`. Everything
/// outside the section is preserved byte-for-byte and the file's line endings are kept, so
/// `without_blocklist` restores the original exactly.
pub fn with_blocklist(hosts: &str, domains: &[String]) -> String {
    let base = without_blocklist(hosts);
    let eol = line_ending(hosts);
    let mut result = String::with_capacity(base.len() + domains.len() * 32 + 64);
    result.push_str(&base);

    let base_ends_with_newline = base.is_empty() || base.ends_with('\n');
    if !base_ends_with_newline {
        // The original had no final newline. The section then ends without one too, which is how
        // `without_blocklist` knows to take this added separator away again.
        result.push_str(eol);
    }
    result.push_str(START_MARKER);
    result.push_str(eol);
    for domain in domains {
        result.push_str("0.0.0.0 ");
        result.push_str(domain);
        result.push_str(eol);
    }
    result.push_str(END_MARKER);
    if base_ends_with_newline {
        result.push_str(eol);
    }
    result
}

/// Returns `hosts` with every noah section removed.
pub fn without_blocklist(hosts: &str) -> String {
    let mut text = hosts.to_string();
    while let Some(range) = section_range(&text) {
        text.replace_range(range, "");
    }
    text
}

/// The byte range of the first noah section, including the line ending after the end marker,
/// or the separator before the start marker when the section ends the file without one.
fn section_range(text: &str) -> Option<std::ops::Range<usize>> {
    let mut offset = 0;
    let mut start = None;
    for line in text.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        match start {
            None if is_marker(line, START_MARKER) => start = Some(line_start),
            Some(start) if is_marker(line, END_MARKER) => {
                return Some(expand_to_separator(
                    text,
                    start,
                    offset,
                    line.ends_with('\n'),
                ));
            }
            _ => {}
        }
    }
    // A start marker without an end marker means an earlier write was cut short; everything
    // after it is ours.
    start.map(|start| expand_to_separator(text, start, text.len(), text.ends_with('\n')))
}

fn expand_to_separator(
    text: &str,
    start: usize,
    end: usize,
    ends_with_newline: bool,
) -> std::ops::Range<usize> {
    if ends_with_newline || end != text.len() {
        return start..end;
    }
    let before = text.get(..start).unwrap_or_default();
    if before.ends_with("\r\n") {
        start - 2..end
    } else if before.ends_with('\n') {
        start - 1..end
    } else {
        start..end
    }
}

/// The number of domains in the noah section, or 0 when there is none.
pub fn blocked_domain_count(hosts: &str) -> usize {
    let Some(range) = section_range(hosts) else {
        return 0;
    };
    hosts
        .get(range)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .count()
}

/// Blocking. Writes `contents` to the hosts file through the operating system's administrator
/// prompt, then re-reads the file to confirm the change.
pub fn write_hosts_elevated(contents: &str) -> Result<()> {
    let hosts = hosts_path();
    if std::fs::read_to_string(&hosts).is_ok_and(|current| current == contents) {
        return Ok(());
    }

    let temporary = std::env::temp_dir().join(format!(
        "noah-hosts-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::write(&temporary, contents)
        .with_context(|| format!("could not write {}", temporary.display()))?;

    let copy_result = copy_elevated(&temporary, &hosts);
    if let Err(error) = std::fs::remove_file(&temporary) {
        log::warn!("could not remove {}: {error}", temporary.display());
    }

    let written = std::fs::read_to_string(&hosts)
        .with_context(|| format!("could not read {} after writing it", hosts.display()))?;
    if written == contents {
        return Ok(());
    }
    match copy_result {
        Err(error) => Err(error),
        Ok(()) => bail!(
            "{} was not changed. Security software may be protecting it.",
            hosts.display()
        ),
    }
}

/// How long to wait for the person to answer the administrator prompt.
#[cfg_attr(
    not(any(windows, target_os = "linux", target_os = "macos")),
    allow(dead_code)
)]
const ELEVATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10 * 60);

#[cfg(windows)]
fn copy_elevated(source: &Path, destination: &Path) -> Result<()> {
    use crate::{encode_powershell_command, powershell_quote, run_powershell};

    let inner = format!(
        "$ErrorActionPreference = 'Stop'\n\
         Copy-Item -LiteralPath {} -Destination {} -Force\n\
         ipconfig /flushdns | Out-Null",
        powershell_quote(&source.display().to_string()),
        powershell_quote(&destination.display().to_string()),
    );
    // Declining the UAC prompt surfaces as ERROR_CANCELLED (1223), either directly or wrapped
    // depending on the PowerShell version.
    // The elevated PowerShell gets its script via -EncodedCommand because Start-Process joins
    // -ArgumentList without quoting, which would split paths containing spaces.
    let outer = format!(
        "try {{\n\
           $process = Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -PassThru \
             -WindowStyle Hidden -ArgumentList '-NoProfile','-NonInteractive','-EncodedCommand',{}\n\
           exit $process.ExitCode\n\
         }} catch {{\n\
           $cancelled = $_.Exception.NativeErrorCode -eq 1223 -or \
             $_.Exception.InnerException.NativeErrorCode -eq 1223 -or \
             $_.Exception.HResult -eq -2147023673\n\
           if ($cancelled) {{ Write-Output 'noah-cancelled'; exit 1223 }}\n\
           Write-Output $_.Exception.Message\n\
           exit 1\n\
         }}",
        powershell_quote(&encode_powershell_command(&inner)),
    );
    let output = run_powershell(&outer, ELEVATION_TIMEOUT)?;
    if output.success() {
        return Ok(());
    }
    if output.exit_code == Some(1223) || output.stdout.contains("noah-cancelled") {
        return Err(anyhow!(CANCELLED_MESSAGE));
    }
    bail!(
        "could not update {}: {}",
        destination.display(),
        output.error_summary()
    )
}

#[cfg(target_os = "linux")]
fn copy_elevated(source: &Path, destination: &Path) -> Result<()> {
    use crate::run_command;

    if which::which("pkexec").is_err() {
        bail!(
            "pkexec (polkit) is needed to change {}. Install polkit, or copy the new file there \
             yourself with sudo.",
            destination.display()
        );
    }
    let source = source.to_string_lossy();
    let destination_text = destination.to_string_lossy();
    let output = run_command(
        "pkexec",
        &["cp", &*source, &*destination_text],
        ELEVATION_TIMEOUT,
    )?;
    match output.exit_code {
        Some(0) => Ok(()),
        // pkexec exits with 126 when the dialog is dismissed and 127 when authentication fails.
        Some(126) | Some(127) if !output.stderr.contains("No authentication agent") => {
            Err(anyhow!(CANCELLED_MESSAGE))
        }
        _ => bail!(
            "could not update {}: {}",
            destination.display(),
            output.error_summary()
        ),
    }
}

#[cfg(target_os = "macos")]
fn copy_elevated(source: &Path, destination: &Path) -> Result<()> {
    use crate::run_command;

    let shell_command = format!(
        "cp {} {} && dscacheutil -flushcache; killall -HUP mDNSResponder",
        shell_quote(&source.to_string_lossy()),
        shell_quote(&destination.to_string_lossy()),
    );
    let script = format!(
        "do shell script {} with administrator privileges",
        applescript_string(&shell_command)
    );
    let output = run_command("osascript", &["-e", &script], ELEVATION_TIMEOUT)?;
    if output.success() {
        return Ok(());
    }
    if output.stderr.contains("-128") {
        return Err(anyhow!(CANCELLED_MESSAGE));
    }
    bail!(
        "could not update {}: {}",
        destination.display(),
        output.error_summary()
    )
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn copy_elevated(_source: &Path, destination: &Path) -> Result<()> {
    bail!(
        "changing {} isn't supported on this operating system",
        destination.display()
    )
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn applescript_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', r"\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_BLOCKLIST: &str = "\
# Title: StevenBlack/hosts
#
# This hosts file is a merged collection of hosts from reputable sources,
# ===============================================================

127.0.0.1 localhost
127.0.0.1 localhost.localdomain
127.0.0.1 local
255.255.255.255 broadcasthost
::1 localhost
::1 ip6-localhost
::1 ip6-loopback
fe80::1%lo0 localhost
ff00::0 ip6-localnet
ff02::1 ip6-allnodes
0.0.0.0 0.0.0.0

# Custom host records are listed here.

# End of custom host records.
# Start StevenBlack

#=====================================
# Title: Hosts contributed by Steven Black
# http://stevenblack.com

0.0.0.0 ck.getcookiestxt.com
0.0.0.0 eu1.clevertap-prod.com
0.0.0.0 wizhumpgyros.com  # trailing comment
0.0.0.0 Ads.Example.COM
0.0.0.0 ck.getcookiestxt.com
127.0.0.1 tracker.example.net
0.0.0.0 ip6-allrouters
192.168.1.1 router.example.com
0.0.0.0 bad<domain>.com
";

    #[test]
    fn parses_blocklist() {
        assert_eq!(
            parse_blocklist(SAMPLE_BLOCKLIST),
            vec![
                "ck.getcookiestxt.com",
                "eu1.clevertap-prod.com",
                "wizhumpgyros.com",
                "ads.example.com",
                "tracker.example.net",
            ]
        );
        assert!(parse_blocklist("").is_empty());
        assert_eq!(
            parse_blocklist("0.0.0.0 a.example b.example\r\n"),
            vec!["a.example", "b.example"]
        );
    }

    fn domains() -> Vec<String> {
        vec!["ads.example.com".into(), "tracker.example.net".into()]
    }

    const LINUX_HOSTS: &str = "127.0.0.1\tlocalhost\n127.0.1.1\tmy-laptop\n\n# The following lines are desirable for IPv6 capable hosts\n::1     ip6-localhost ip6-loopback\n";

    #[test]
    fn applies_blocklist_at_the_end() {
        let updated = with_blocklist(LINUX_HOSTS, &domains());
        assert_eq!(
            updated,
            format!(
                "{LINUX_HOSTS}{START_MARKER}\n0.0.0.0 ads.example.com\n0.0.0.0 tracker.example.net\n{END_MARKER}\n"
            )
        );
        assert_eq!(blocked_domain_count(&updated), 2);
        assert_eq!(blocked_domain_count(LINUX_HOSTS), 0);
    }

    #[test]
    fn applying_twice_is_idempotent_and_replaces_the_section() {
        let once = with_blocklist(LINUX_HOSTS, &domains());
        assert_eq!(with_blocklist(&once, &domains()), once);

        let replaced = with_blocklist(&once, &["only.example".to_string()]);
        assert_eq!(blocked_domain_count(&replaced), 1);
        assert_eq!(replaced.matches(START_MARKER).count(), 1);
        assert!(!replaced.contains("ads.example.com"));
    }

    #[test]
    fn removal_restores_the_original_exactly() {
        for original in [
            LINUX_HOSTS,
            "",
            "127.0.0.1 localhost",
            "127.0.0.1 localhost\r\n::1 localhost",
            "\n\n",
            "# only a comment\r\n",
        ] {
            let applied = with_blocklist(original, &domains());
            assert_eq!(without_blocklist(&applied), original, "{original:?}");
            let applied_empty = with_blocklist(original, &[]);
            assert_eq!(without_blocklist(&applied_empty), original, "{original:?}");
            assert_eq!(without_blocklist(original), original);
        }
    }

    #[test]
    fn preserves_crlf_line_endings() {
        let windows_hosts = "# Copyright (c) 1993-2009 Microsoft Corp.\r\n#\r\n# 127.0.0.1       localhost\r\n# ::1             localhost\r\n";
        let applied = with_blocklist(windows_hosts, &domains());
        assert!(applied.starts_with(windows_hosts));
        assert_eq!(
            applied.matches('\n').count(),
            applied.matches("\r\n").count()
        );
        assert!(applied.ends_with(&format!("{END_MARKER}\r\n")));
        assert_eq!(without_blocklist(&applied), windows_hosts);
    }

    #[test]
    fn user_lines_after_the_section_survive_removal() {
        let applied = with_blocklist("127.0.0.1 localhost\n", &domains());
        let edited = format!("{applied}10.0.0.5 nas.home\n");
        assert_eq!(
            without_blocklist(&edited),
            "127.0.0.1 localhost\n10.0.0.5 nas.home\n"
        );
        let reapplied = with_blocklist(&edited, &domains());
        assert!(reapplied.starts_with("127.0.0.1 localhost\n10.0.0.5 nas.home\n"));
        assert_eq!(blocked_domain_count(&reapplied), 2);
    }

    #[test]
    fn section_in_the_middle_is_removed_cleanly() {
        let hosts = format!(
            "127.0.0.1 localhost\n{START_MARKER}\n0.0.0.0 a.example\n{END_MARKER}\n10.0.0.5 nas.home\n"
        );
        assert_eq!(
            without_blocklist(&hosts),
            "127.0.0.1 localhost\n10.0.0.5 nas.home\n"
        );
        assert_eq!(blocked_domain_count(&hosts), 1);
    }

    #[test]
    fn unterminated_section_is_removed() {
        let hosts = format!("127.0.0.1 localhost\n{START_MARKER}\n0.0.0.0 a.example\n0.0.0.0 b.ex");
        assert_eq!(without_blocklist(&hosts), "127.0.0.1 localhost");
        assert_eq!(blocked_domain_count(&hosts), 2);
    }

    #[test]
    fn multiple_sections_are_all_removed() {
        let section = format!("{START_MARKER}\n0.0.0.0 a.example\n{END_MARKER}\n");
        let hosts = format!("one\n{section}two\n{section}");
        assert_eq!(without_blocklist(&hosts), "one\ntwo\n");
    }

    #[test]
    fn hosts_path_points_at_the_system_file() {
        let path = hosts_path();
        assert!(path.ends_with("hosts"));
        if cfg!(windows) {
            assert!(path.ends_with(r"System32\drivers\etc\hosts"));
        } else {
            assert_eq!(path, PathBuf::from("/etc/hosts"));
        }
    }

    #[test]
    fn quoting_helpers() {
        assert_eq!(shell_quote("/tmp/it's"), r"'/tmp/it'\''s'");
        assert_eq!(applescript_string(r#"cp "a" \b"#), r#""cp \"a\" \\b""#);
    }
}
