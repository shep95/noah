//! The leash: "block this program's network until i say". On Windows the
//! system firewall takes one outbound rule per program; noah's rules share
//! one group so they can be listed and lifted. macOS and Linux have no
//! built-in way to block one program's traffic, so there the leash says so
//! instead of pretending.

use std::path::{Path, PathBuf};

use anyhow::Result;

/// The firewall group every leash rule belongs to.
#[cfg_attr(not(windows), allow(dead_code))]
const GROUP: &str = "noah leash";

pub fn supported() -> bool {
    cfg!(windows)
}

/// Why the leash isn't available here, for the room to show.
pub fn unsupported_reason() -> &'static str {
    "the leash works on Windows, whose firewall can block one program; macOS and Linux have no built-in way to cut off a single program's network"
}

/// A rule name that says what it is in the firewall's own list.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn rule_name(exe: &Path) -> String {
    let name = exe
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| exe.display().to_string());
    format!("noah leash: {name}")
}

/// Blocks every outbound connection `exe` makes, after an administrator
/// prompt.
#[cfg(windows)]
pub fn leash(exe: &Path) -> Result<()> {
    use crate::powershell_quote;
    anyhow::ensure!(exe.is_absolute() && exe.is_file(), "{} is not a program", exe.display());
    let inner = format!(
        "$ErrorActionPreference = 'Stop'\n\
         Remove-NetFirewallRule -DisplayName {name} -ErrorAction SilentlyContinue\n\
         New-NetFirewallRule -DisplayName {name} -Group {group} -Direction Outbound \
           -Program {program} -Action Block -Profile Any | Out-Null",
        name = powershell_quote(&rule_name(exe)),
        group = powershell_quote(GROUP),
        program = powershell_quote(&exe.display().to_string()),
    );
    crate::adblock::run_elevated_powershell(&inner, &format!("block {}", exe.display()))
}

/// Lets `exe` talk to the network again, after an administrator prompt.
#[cfg(windows)]
pub fn unleash(exe: &Path) -> Result<()> {
    use crate::powershell_quote;
    let inner = format!(
        "$ErrorActionPreference = 'Stop'\n\
         Get-NetFirewallRule -Group {group} | Get-NetFirewallApplicationFilter | \
           Where-Object {{ $_.Program -eq {program} }} | Get-NetFirewallRule | Remove-NetFirewallRule",
        group = powershell_quote(GROUP),
        program = powershell_quote(&exe.display().to_string()),
    );
    crate::adblock::run_elevated_powershell(&inner, &format!("unblock {}", exe.display()))
}

/// The programs on a leash now. Reading the firewall needs no prompt.
#[cfg(windows)]
pub fn leashed() -> Vec<PathBuf> {
    use crate::powershell_quote;
    let script = format!(
        "Get-NetFirewallRule -Group {} -ErrorAction SilentlyContinue | \
           Get-NetFirewallApplicationFilter | ForEach-Object {{ $_.Program }}",
        powershell_quote(GROUP)
    );
    match crate::run_powershell(&script, crate::COMMAND_TIMEOUT) {
        Ok(output) if output.success() => parse_programs(&output.stdout),
        Ok(output) => {
            log::warn!("couldn't list leashed programs: {}", output.error_summary());
            Vec::new()
        }
        Err(error) => {
            log::warn!("couldn't list leashed programs: {error:#}");
            Vec::new()
        }
    }
}

#[cfg(not(windows))]
pub fn leash(_exe: &Path) -> Result<()> {
    anyhow::bail!(unsupported_reason())
}

#[cfg(not(windows))]
pub fn unleash(_exe: &Path) -> Result<()> {
    anyhow::bail!(unsupported_reason())
}

#[cfg(not(windows))]
pub fn leashed() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub(crate) fn parse_programs(output: &str) -> Vec<PathBuf> {
    let mut programs: Vec<PathBuf> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.eq_ignore_ascii_case("any"))
        .map(PathBuf::from)
        .collect();
    programs.sort();
    programs.dedup();
    programs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_names_and_listing() {
        assert_eq!(
            rule_name(Path::new("/opt/app/tracker.exe")),
            "noah leash: tracker.exe"
        );
        let programs = parse_programs("C:\\\\a\\\\b.exe\r\nAny\r\n\r\nC:\\\\a\\\\b.exe\r\nC:\\\\c.exe\r\n");
        assert_eq!(programs.len(), 2);
    }

    #[cfg(not(windows))]
    #[test]
    fn elsewhere_the_leash_says_why_not() {
        assert!(!supported());
        let error = leash(Path::new("/bin/true")).expect_err("unsupported");
        assert!(error.to_string().contains("Windows"));
    }
}
