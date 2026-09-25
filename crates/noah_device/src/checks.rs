use anyhow::Result;
use std::collections::{BTreeSet, HashMap};
use std::fmt::Display;
use std::net::IpAddr;
use std::thread;

/// Ordered worst-first, so sorting checks by status puts problems at the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Bad,
    Warning,
    Unknown,
    Good,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Protection,
    Network,
    Access,
    Updates,
    Encryption,
    Hardware,
    Privacy,
    Health,
}

#[derive(Clone, Debug)]
pub struct Check {
    pub id: &'static str,
    pub title: String,
    pub category: Category,
    pub status: Status,
    pub detail: String,
    pub recommendation: Option<String>,
}

impl Check {
    pub(crate) fn new(
        id: &'static str,
        title: impl Into<String>,
        category: Category,
        status: Status,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            category,
            status,
            detail: detail.into(),
            recommendation: None,
        }
    }

    pub(crate) fn unknown(
        id: &'static str,
        title: impl Into<String>,
        category: Category,
        reason: impl Display,
    ) -> Self {
        Self::new(
            id,
            title,
            category,
            Status::Unknown,
            format!("noah couldn't check this: {reason}"),
        )
    }

    pub(crate) fn recommend(mut self, recommendation: impl Into<String>) -> Self {
        self.recommendation = Some(recommendation.into());
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ListeningPort {
    pub protocol: String,
    pub address: String,
    pub port: u16,
    pub process: Option<String>,
}

impl ListeningPort {
    fn describe(&self) -> String {
        let address = if self.address.contains(':') {
            format!("[{}]", self.address)
        } else {
            self.address.clone()
        };
        match &self.process {
            Some(process) => format!("{} {address}:{} ({process})", self.protocol, self.port),
            None => format!("{} {address}:{}", self.protocol, self.port),
        }
    }
}

pub(crate) fn sort_worst_first(checks: &mut [Check]) {
    checks.sort_by_key(|check| check.status);
}

/// Blocking. Runs this OS's checks (each command with a ~10s timeout; a failing/missing command
/// yields `Status::Unknown` with the reason, never a panic or an `Err`). Sorted worst-first.
pub fn run_security_checks() -> Vec<Check> {
    let mut jobs = platform::check_jobs();
    jobs.push(|| vec![exposed_ports_check(try_listening_ports())]);

    // The checks are dominated by process start-up time (PowerShell in particular), so they run
    // concurrently rather than one after another.
    let mut checks: Vec<Check> = thread::scope(|scope| {
        let handles: Vec<_> = jobs.into_iter().map(|job| scope.spawn(job)).collect();
        handles
            .into_iter()
            .filter_map(|handle| match handle.join() {
                Ok(checks) => Some(checks),
                Err(_) => {
                    log::error!("a security check panicked");
                    None
                }
            })
            .flatten()
            .collect()
    });
    sort_worst_first(&mut checks);
    checks
}

/// Ports listening on all interfaces or a non-loopback address, i.e. reachable from the network.
pub fn listening_ports() -> Vec<ListeningPort> {
    match try_listening_ports() {
        Ok(ports) => ports,
        Err(error) => {
            log::warn!("failed to list listening ports: {error:#}");
            Vec::new()
        }
    }
}

fn try_listening_ports() -> Result<Vec<ListeningPort>> {
    Ok(exposed_only(platform::all_listening_ports()?))
}

fn exposed_only(ports: Vec<ListeningPort>) -> Vec<ListeningPort> {
    let unique: BTreeSet<ListeningPort> = ports
        .into_iter()
        .filter(|port| !is_loopback_address(&port.address))
        .collect();
    unique.into_iter().collect()
}

fn is_loopback_address(address: &str) -> bool {
    if address.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match address.parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => address.is_loopback(),
        Ok(IpAddr::V6(address)) => {
            address.is_loopback()
                || address
                    .to_ipv4_mapped()
                    .is_some_and(|mapped| mapped.is_loopback())
        }
        Err(_) => false,
    }
}

/// Splits `0.0.0.0:22`, `[::]:22`, `*:22`, `127.0.0.53%lo:53` or `[fe80::1%eth0]:546` into the
/// address (without brackets or interface suffix) and port.
fn parse_socket_address(text: &str) -> Option<(String, u16)> {
    let (address, port) = text.rsplit_once(':')?;
    let port = port.parse::<u16>().ok()?;
    let address = address.trim_start_matches('[').trim_end_matches(']');
    let address = address.split('%').next().unwrap_or(address);
    if address.is_empty() {
        return None;
    }
    Some((address.to_string(), port))
}

fn exposed_ports_check(ports: Result<Vec<ListeningPort>>) -> Check {
    const ID: &str = "open_ports";
    const TITLE: &str = "Ports open to the network";
    match ports {
        Err(error) => Check::unknown(ID, TITLE, Category::Network, format!("{error:#}")),
        Ok(ports) if ports.is_empty() => Check::new(
            ID,
            TITLE,
            Category::Network,
            Status::Good,
            "No programs are accepting connections from the network.",
        ),
        Ok(ports) => {
            let list = ports
                .iter()
                .map(ListeningPort::describe)
                .collect::<Vec<_>>()
                .join(", ");
            let count = ports.len();
            let noun = if count == 1 { "port is" } else { "ports are" };
            Check::new(
                ID,
                TITLE,
                Category::Network,
                Status::Warning,
                format!("{count} {noun} open to the network: {list}"),
            )
            .recommend(
                "Make sure you recognize each program. Close or uninstall anything you don't \
                 need, and keep the firewall on so only trusted networks can reach them.",
            )
        }
    }
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn secure_boot_check(enabled: Option<bool>, unknown_reason: &str) -> Check {
    const ID: &str = "secure_boot";
    const TITLE: &str = "Secure Boot";
    match enabled {
        Some(true) => Check::new(
            ID,
            TITLE,
            Category::Hardware,
            Status::Good,
            "Secure Boot is on, so only trusted software can load while the computer starts.",
        ),
        Some(false) => Check::new(
            ID,
            TITLE,
            Category::Hardware,
            Status::Warning,
            "Secure Boot is off, so malware could load before the operating system starts.",
        )
        .recommend("Turn on Secure Boot in your computer's UEFI/BIOS settings."),
        None => Check::unknown(ID, TITLE, Category::Hardware, unknown_reason),
    }
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn pending_restart_check(pending: Option<bool>, detail: &str) -> Check {
    const ID: &str = "pending_restart";
    const TITLE: &str = "Pending restart";
    match pending {
        Some(true) => Check::new(ID, TITLE, Category::Updates, Status::Warning, detail)
            .recommend("Restart your computer to finish installing updates."),
        Some(false) => Check::new(
            ID,
            TITLE,
            Category::Updates,
            Status::Good,
            "No restart is needed to finish updates.",
        ),
        None => Check::unknown(ID, TITLE, Category::Updates, detail),
    }
}

/// Reads PowerShell-style booleans, which arrive as JSON booleans, 0/1 or `"True"`/`"False"`.
#[cfg_attr(not(windows), allow(dead_code))]
fn json_bool(value: &serde_json::Value) -> Option<bool> {
    match value {
        serde_json::Value::Bool(value) => Some(*value),
        serde_json::Value::Number(number) => number.as_i64().map(|number| number != 0),
        serde_json::Value::String(text) => parse_true_false(text),
        _ => None,
    }
}

fn parse_true_false(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

/// PowerShell's `ConvertTo-Json` emits a bare object for one result and an array for several.
#[cfg_attr(not(windows), allow(dead_code))]
fn json_objects(text: &str) -> Result<Vec<serde_json::Value>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    Ok(match serde_json::from_str::<serde_json::Value>(text)? {
        serde_json::Value::Array(items) => items,
        other => vec![other],
    })
}

#[cfg_attr(not(windows), allow(dead_code))]
fn defender_checks(json: &str) -> Vec<Check> {
    const TITLE: &str = "Antivirus (Microsoft Defender)";
    let status = match json_objects(json) {
        Ok(objects) => match objects.into_iter().next() {
            Some(status) => status,
            None => {
                return vec![Check::unknown(
                    "antivirus",
                    TITLE,
                    Category::Protection,
                    "Microsoft Defender returned no status",
                )];
            }
        },
        Err(error) => {
            return vec![Check::unknown(
                "antivirus",
                TITLE,
                Category::Protection,
                format!("unexpected Microsoft Defender status: {error}"),
            )];
        }
    };

    let antivirus_enabled = status.get("AntivirusEnabled").and_then(json_bool);
    let real_time = status.get("RealTimeProtectionEnabled").and_then(json_bool);
    let mut checks = Vec::new();

    checks.push(match (antivirus_enabled, real_time) {
        (Some(true), Some(true)) => Check::new(
            "antivirus",
            TITLE,
            Category::Protection,
            Status::Good,
            "Microsoft Defender is on and scanning files in real time.",
        ),
        (Some(false), _) => Check::new(
            "antivirus",
            TITLE,
            Category::Protection,
            Status::Bad,
            "Microsoft Defender Antivirus is turned off.",
        )
        .recommend(
            "Turn on Microsoft Defender in Windows Security, or make sure another antivirus is \
             installed and active.",
        ),
        (_, Some(false)) => Check::new(
            "antivirus",
            TITLE,
            Category::Protection,
            Status::Bad,
            "Real-time protection is off, so new files aren't scanned as they arrive.",
        )
        .recommend(
            "Open Windows Security > Virus & threat protection > Manage settings and turn on \
             Real-time protection.",
        ),
        _ => Check::unknown(
            "antivirus",
            TITLE,
            Category::Protection,
            "Microsoft Defender didn't report whether it is on",
        ),
    });

    if let Some(age) = status
        .get("AntivirusSignatureAge")
        .and_then(serde_json::Value::as_u64)
    {
        const SIGNATURES_TITLE: &str = "Antivirus definitions";
        let days = if age == 1 { "day" } else { "days" };
        checks.push(match age {
            0..=3 => Check::new(
                "antivirus_definitions",
                SIGNATURES_TITLE,
                Category::Updates,
                Status::Good,
                format!("Virus definitions were updated {age} {days} ago."),
            ),
            4..=14 => Check::new(
                "antivirus_definitions",
                SIGNATURES_TITLE,
                Category::Updates,
                Status::Warning,
                format!("Virus definitions are {age} {days} old."),
            )
            .recommend("Open Windows Security and check for protection updates."),
            _ => Check::new(
                "antivirus_definitions",
                SIGNATURES_TITLE,
                Category::Updates,
                Status::Bad,
                format!("Virus definitions are {age} {days} old, so new threats may be missed."),
            )
            .recommend("Open Windows Security and check for protection updates."),
        });
    }

    if let Some(tamper_protected) = status.get("IsTamperProtected").and_then(json_bool) {
        const TAMPER_TITLE: &str = "Tamper protection";
        checks.push(if tamper_protected {
            Check::new(
                "tamper_protection",
                TAMPER_TITLE,
                Category::Protection,
                Status::Good,
                "Tamper protection stops other apps from turning off Microsoft Defender.",
            )
        } else {
            Check::new(
                "tamper_protection",
                TAMPER_TITLE,
                Category::Protection,
                Status::Warning,
                "Tamper protection is off, so malware could switch off Microsoft Defender.",
            )
            .recommend(
                "Open Windows Security > Virus & threat protection > Manage settings and turn on \
                 Tamper Protection.",
            )
        });
    }

    checks
}

#[cfg_attr(not(windows), allow(dead_code))]
fn windows_firewall_check(json: &str) -> Check {
    const ID: &str = "firewall";
    const TITLE: &str = "Firewall";
    let profiles = match json_objects(json) {
        Ok(profiles) if !profiles.is_empty() => profiles,
        Ok(_) => {
            return Check::unknown(
                ID,
                TITLE,
                Category::Protection,
                "no firewall profiles found",
            );
        }
        Err(error) => {
            return Check::unknown(
                ID,
                TITLE,
                Category::Protection,
                format!("unexpected firewall status: {error}"),
            );
        }
    };

    let disabled: Vec<String> = profiles
        .iter()
        .filter(|profile| profile.get("Enabled").and_then(json_bool) == Some(false))
        .map(|profile| {
            profile
                .get("Name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Unnamed")
                .to_string()
        })
        .collect();

    if disabled.is_empty() {
        Check::new(
            ID,
            TITLE,
            Category::Protection,
            Status::Good,
            "Windows Firewall is on for every network type.",
        )
    } else {
        Check::new(
            ID,
            TITLE,
            Category::Protection,
            Status::Bad,
            format!(
                "Windows Firewall is off for these networks: {}.",
                disabled.join(", ")
            ),
        )
        .recommend("Open Windows Security > Firewall & network protection and turn it on.")
    }
}

/// Reads `manage-bde -status <drive>` output.
#[cfg_attr(not(windows), allow(dead_code))]
fn bitlocker_check(output: &str, drive: &str) -> Check {
    const ID: &str = "disk_encryption";
    const TITLE: &str = "Drive encryption (BitLocker)";
    let field = |name: &str| {
        output.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim().eq_ignore_ascii_case(name)).then(|| value.trim().to_string())
        })
    };
    let Some(protection) = field("Protection Status") else {
        return Check::unknown(
            ID,
            TITLE,
            Category::Encryption,
            "run noah as administrator to check",
        );
    };
    let conversion = field("Conversion Status").unwrap_or_default();
    let percentage = field("Percentage Encrypted").unwrap_or_default();

    if protection.to_ascii_lowercase().contains("protection on") {
        return Check::new(
            ID,
            TITLE,
            Category::Encryption,
            Status::Good,
            format!("BitLocker protects drive {drive}."),
        );
    }

    let conversion_lower = conversion.to_ascii_lowercase();
    if conversion_lower.contains("fully decrypted") {
        Check::new(
            ID,
            TITLE,
            Category::Encryption,
            Status::Warning,
            format!("Drive {drive} isn't encrypted. Anyone with the device could read your files."),
        )
        .recommend("Turn on BitLocker or Device encryption in Settings > Privacy & security.")
    } else if conversion_lower.contains("in progress") {
        Check::new(
            ID,
            TITLE,
            Category::Encryption,
            Status::Warning,
            format!("{conversion} on drive {drive} ({percentage})."),
        )
        .recommend("Keep the computer on and plugged in until encryption finishes.")
    } else {
        Check::new(
            ID,
            TITLE,
            Category::Encryption,
            Status::Warning,
            format!("BitLocker protection is suspended on drive {drive}."),
        )
        .recommend("Resume BitLocker protection in Control Panel > BitLocker Drive Encryption.")
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn uac_check(enable_lua: Option<u32>, consent_prompt_admin: Option<u32>) -> Check {
    const ID: &str = "uac";
    const TITLE: &str = "User Account Control";
    match (enable_lua, consent_prompt_admin) {
        (Some(0), _) => Check::new(
            ID,
            TITLE,
            Category::Access,
            Status::Bad,
            "User Account Control is off, so apps can make system changes without asking.",
        )
        .recommend("Turn User Account Control back on in Control Panel > User Accounts."),
        (Some(_), Some(0)) => Check::new(
            ID,
            TITLE,
            Category::Access,
            Status::Warning,
            "User Account Control never asks before apps make system changes.",
        )
        .recommend("Raise the User Account Control setting to \"Notify me\"."),
        (Some(_), _) => Check::new(
            ID,
            TITLE,
            Category::Access,
            Status::Good,
            "Windows asks before apps make system changes.",
        ),
        (None, _) => Check::unknown(
            ID,
            TITLE,
            Category::Access,
            "the User Account Control setting couldn't be read",
        ),
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn remote_desktop_check(deny_connections: Option<u32>) -> Check {
    const ID: &str = "remote_desktop";
    const TITLE: &str = "Remote Desktop";
    match deny_connections {
        Some(0) => Check::new(
            ID,
            TITLE,
            Category::Access,
            Status::Warning,
            "Remote Desktop is on, so this computer accepts remote sign-ins.",
        )
        .recommend(
            "Turn off Remote Desktop in Settings > System > Remote Desktop if you don't use it.",
        ),
        Some(_) => Check::new(
            ID,
            TITLE,
            Category::Access,
            Status::Good,
            "Remote Desktop is off.",
        ),
        None => Check::unknown(
            ID,
            TITLE,
            Category::Access,
            "the Remote Desktop setting couldn't be read",
        ),
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn smb1_check(output: &str) -> Check {
    const ID: &str = "smb1";
    const TITLE: &str = "SMBv1 file sharing";
    match parse_true_false(output) {
        Some(true) => Check::new(
            ID,
            TITLE,
            Category::Network,
            Status::Bad,
            "The outdated SMBv1 protocol is on. It was used by the WannaCry ransomware to spread.",
        )
        .recommend(
            "Turn off \"SMB 1.0/CIFS File Sharing Support\" in Windows Features, or run \
             Set-SmbServerConfiguration -EnableSMB1Protocol $false as administrator.",
        ),
        Some(false) => Check::new(
            ID,
            TITLE,
            Category::Network,
            Status::Good,
            "The outdated SMBv1 protocol is off.",
        ),
        None => Check::unknown(
            ID,
            TITLE,
            Category::Network,
            "run noah as administrator to check",
        ),
    }
}

/// Reads the newest hotfix date printed as `yyyy-MM-dd`.
#[cfg_attr(not(windows), allow(dead_code))]
fn last_update_check(output: &str, today: chrono::NaiveDate) -> Check {
    const ID: &str = "os_updates";
    const TITLE: &str = "Windows updates";
    let installed = output
        .lines()
        .find_map(|line| chrono::NaiveDate::parse_from_str(line.trim(), "%Y-%m-%d").ok());
    let Some(installed) = installed else {
        return Check::unknown(
            ID,
            TITLE,
            Category::Updates,
            "no installed updates were reported",
        );
    };
    let days = (today - installed).num_days().max(0);
    let when = match days {
        0 => "today".to_string(),
        1 => "yesterday".to_string(),
        days => format!("{days} days ago"),
    };
    if days > 60 {
        Check::new(
            ID,
            TITLE,
            Category::Updates,
            Status::Warning,
            format!("The last Windows update was installed {when} ({installed})."),
        )
        .recommend("Open Settings > Windows Update and install the latest updates.")
    } else {
        Check::new(
            ID,
            TITLE,
            Category::Updates,
            Status::Good,
            format!("The last Windows update was installed {when}."),
        )
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn guest_account_check(json: &str) -> Check {
    const ID: &str = "guest_account";
    const TITLE: &str = "Guest account";
    let accounts = match json_objects(json) {
        Ok(accounts) => accounts,
        Err(error) => {
            return Check::unknown(
                ID,
                TITLE,
                Category::Access,
                format!("unexpected account list: {error}"),
            );
        }
    };
    let enabled = accounts
        .iter()
        .any(|account| account.get("Enabled").and_then(json_bool) == Some(true));
    if enabled {
        Check::new(
            ID,
            TITLE,
            Category::Access,
            Status::Warning,
            "The built-in Guest account is on, so anyone can sign in without a password.",
        )
        .recommend("Turn it off by running `net user guest /active:no` as administrator.")
    } else {
        Check::new(
            ID,
            TITLE,
            Category::Access,
            Status::Good,
            "The built-in Guest account is off.",
        )
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn auto_sign_in_check(auto_admin_logon: Option<&str>) -> Check {
    const ID: &str = "auto_sign_in";
    const TITLE: &str = "Automatic sign-in";
    if auto_admin_logon.map(str::trim) == Some("1") {
        Check::new(
            ID,
            TITLE,
            Category::Access,
            Status::Warning,
            "Windows signs in automatically without a password, and the password is stored in \
             the registry.",
        )
        .recommend("Turn off automatic sign-in with `netplwiz` so a password is required.")
    } else {
        Check::new(
            ID,
            TITLE,
            Category::Access,
            Status::Good,
            "A password is needed to sign in.",
        )
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn parse_netstat(output: &str, process_names: &HashMap<u32, String>) -> Vec<ListeningPort> {
    output
        .lines()
        .filter_map(|line| {
            let columns: Vec<&str> = line.split_whitespace().collect();
            let [protocol, local, foreign, state, pid] = columns.as_slice() else {
                return None;
            };
            if !protocol.eq_ignore_ascii_case("TCP") {
                return None;
            }
            // The state column is translated on non-English Windows, but a listening socket is
            // the only kind with no remote endpoint.
            let is_listening = state.eq_ignore_ascii_case("LISTENING")
                || matches!(*foreign, "0.0.0.0:0" | "[::]:0");
            if !is_listening {
                return None;
            }
            let (address, port) = parse_socket_address(local)?;
            let pid = pid.parse::<u32>().ok();
            Some(ListeningPort {
                protocol: "TCP".to_string(),
                address,
                port,
                process: pid.and_then(|pid| process_names.get(&pid).cloned()),
            })
        })
        .collect()
}

/// Reads `tasklist /fo csv /nh` output into a pid -> image name map.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_tasklist_csv(output: &str) -> HashMap<u32, String> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim().strip_prefix('"')?;
            let mut fields = line.split("\",\"");
            let name = fields.next()?;
            let pid = fields.next()?.parse::<u32>().ok()?;
            Some((pid, name.to_string()))
        })
        .collect()
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_ufw_status(output: &str) -> Option<bool> {
    output.lines().find_map(|line| {
        let value = line.trim().strip_prefix("Status:")?.trim();
        match value {
            "active" => Some(true),
            "inactive" => Some(false),
            _ => None,
        }
    })
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_ufw_config(text: &str) -> Option<bool> {
    text.lines().find_map(|line| {
        let value = line.trim().strip_prefix("ENABLED=")?;
        parse_true_false(value.trim_matches(['"', '\'']))
    })
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_firewalld_state(output: &str) -> Option<bool> {
    let output = output.trim();
    if output.contains("not running") {
        Some(false)
    } else if output.lines().any(|line| line.trim() == "running") {
        Some(true)
    } else {
        None
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn linux_firewall_check(
    ufw: Option<bool>,
    firewalld: Option<bool>,
    nftables: Option<bool>,
    any_tool_found: bool,
) -> Check {
    const ID: &str = "firewall";
    const TITLE: &str = "Firewall";
    let active: Vec<&str> = [
        (ufw, "ufw"),
        (firewalld, "firewalld"),
        (nftables, "nftables"),
    ]
    .into_iter()
    .filter(|(state, _)| *state == Some(true))
    .map(|(_, name)| name)
    .collect();
    if !active.is_empty() {
        return Check::new(
            ID,
            TITLE,
            Category::Protection,
            Status::Good,
            format!("The firewall is on ({}).", active.join(", ")),
        );
    }
    let recommendation = "Turn on a firewall, for example with `sudo ufw enable`.";
    if ufw == Some(false) || firewalld == Some(false) || nftables == Some(false) {
        Check::new(
            ID,
            TITLE,
            Category::Protection,
            Status::Warning,
            "The firewall is off, so other devices on your network can reach any open port.",
        )
        .recommend(recommendation)
    } else if !any_tool_found {
        Check::new(
            ID,
            TITLE,
            Category::Protection,
            Status::Warning,
            "No firewall (ufw, firewalld or nftables) was found.",
        )
        .recommend("Install and enable a firewall such as ufw.")
    } else {
        Check::unknown(
            ID,
            TITLE,
            Category::Protection,
            "the firewall status can only be read as administrator",
        )
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, Default, PartialEq, Eq)]
struct SshdSettings {
    permit_root_login: Option<String>,
    password_authentication: Option<String>,
}

/// Reads the effective global `sshd_config` settings. Like sshd, the first value for a keyword
/// wins, `Include` is followed in place, and `Match` blocks are ignored because they only
/// apply to some connections.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_sshd_config(text: &str, include: &mut dyn FnMut(&str) -> Vec<String>) -> SshdSettings {
    let mut settings = SshdSettings::default();
    parse_sshd_config_into(text, include, &mut settings, 0);
    settings
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_sshd_config_into(
    text: &str,
    include: &mut dyn FnMut(&str) -> Vec<String>,
    settings: &mut SshdSettings,
    depth: usize,
) {
    const MAX_INCLUDE_DEPTH: usize = 8;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (keyword, value) =
            match line.find(|character: char| character.is_whitespace() || character == '=') {
                Some(index) => {
                    let (keyword, rest) = line.split_at(index);
                    (
                        keyword,
                        rest.trim_start_matches(|character: char| {
                            character.is_whitespace() || character == '='
                        })
                        .trim(),
                    )
                }
                None => (line, ""),
            };
        let value = value.trim_matches('"');
        match keyword.to_ascii_lowercase().as_str() {
            "match" => return,
            "include" if depth < MAX_INCLUDE_DEPTH => {
                for pattern in value.split_whitespace() {
                    for included in include(pattern) {
                        parse_sshd_config_into(&included, include, settings, depth + 1);
                    }
                }
            }
            "permitrootlogin" if settings.permit_root_login.is_none() => {
                settings.permit_root_login = Some(value.to_ascii_lowercase());
            }
            "passwordauthentication" if settings.password_authentication.is_none() => {
                settings.password_authentication = Some(value.to_ascii_lowercase());
            }
            _ => {}
        }
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn ssh_server_check(settings: &SshdSettings) -> Check {
    const ID: &str = "ssh_server";
    const TITLE: &str = "SSH server";
    // These are OpenSSH's defaults when the keyword isn't set.
    let root_login = settings
        .permit_root_login
        .as_deref()
        .unwrap_or("prohibit-password");
    let password_authentication = settings.password_authentication.as_deref().unwrap_or("yes");

    if root_login == "yes" {
        Check::new(
            ID,
            TITLE,
            Category::Access,
            Status::Bad,
            "The SSH server lets anyone sign in as root with a password.",
        )
        .recommend("Set `PermitRootLogin no` in /etc/ssh/sshd_config and restart sshd.")
    } else if password_authentication == "yes" {
        Check::new(
            ID,
            TITLE,
            Category::Access,
            Status::Warning,
            "The SSH server accepts passwords, which can be guessed by automated attacks.",
        )
        .recommend(
            "Use SSH keys and set `PasswordAuthentication no` in /etc/ssh/sshd_config, or \
             uninstall the SSH server if you don't need it.",
        )
    } else {
        Check::new(
            ID,
            TITLE,
            Category::Access,
            Status::Good,
            "The SSH server only accepts keys and doesn't allow password sign-in as root.",
        )
    }
}

/// Reads `/etc/apt/apt.conf.d/20auto-upgrades`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_apt_auto_upgrades(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("APT::Periodic::Unattended-Upgrade") else {
            return false;
        };
        let value = rest.trim().trim_end_matches(';').trim().trim_matches('"');
        !value.is_empty() && value != "0"
    })
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn automatic_updates_check(tool: &str, installed: bool, enabled: bool) -> Check {
    const ID: &str = "automatic_updates";
    const TITLE: &str = "Automatic security updates";
    if installed && enabled {
        Check::new(
            ID,
            TITLE,
            Category::Updates,
            Status::Good,
            format!("Security updates are installed automatically by {tool}."),
        )
    } else if installed {
        Check::new(
            ID,
            TITLE,
            Category::Updates,
            Status::Warning,
            format!("{tool} is installed but not turned on."),
        )
        .recommend(format!(
            "Enable {tool} so security fixes are installed automatically."
        ))
    } else {
        Check::new(
            ID,
            TITLE,
            Category::Updates,
            Status::Warning,
            "Security updates aren't installed automatically.",
        )
        .recommend(format!("Install and enable {tool}."))
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn linux_encryption_check(lsblk_output: &str) -> Check {
    const ID: &str = "disk_encryption";
    const TITLE: &str = "Disk encryption";
    let encrypted = lsblk_output
        .lines()
        .any(|line| line.split_whitespace().any(|column| column == "crypt"));
    if encrypted {
        Check::new(
            ID,
            TITLE,
            Category::Encryption,
            Status::Good,
            "An encrypted (LUKS) volume is in use.",
        )
    } else {
        Check::new(
            ID,
            TITLE,
            Category::Encryption,
            Status::Warning,
            "No encrypted disks were found. Anyone with the device could read your files.",
        )
        .recommend("Use full-disk encryption (LUKS) the next time you install Linux.")
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_mokutil_state(output: &str) -> Option<bool> {
    let output = output.to_ascii_lowercase();
    if output.contains("secureboot enabled") {
        Some(true)
    } else if output.contains("secureboot disabled") {
        Some(false)
    } else {
        None
    }
}

/// The `SecureBoot` EFI variable is four attribute bytes followed by a one-byte flag.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_secure_boot_efivar(bytes: &[u8]) -> Option<bool> {
    match bytes.get(4) {
        Some(0) => Some(false),
        Some(1) => Some(true),
        _ => None,
    }
}

/// Reads `ss -tulpnH` output.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_ss(output: &str) -> Vec<ListeningPort> {
    output
        .lines()
        .filter_map(|line| {
            let columns: Vec<&str> = line.split_whitespace().collect();
            let protocol = columns.first()?.to_ascii_uppercase();
            if protocol != "TCP" && protocol != "UDP" {
                return None;
            }
            let (address, port) = parse_socket_address(columns.get(4)?)?;
            let process = line.split_once("((\"").and_then(|(_, rest)| {
                let (name, _) = rest.split_once('"')?;
                Some(name.to_string())
            });
            Some(ListeningPort {
                protocol,
                address,
                port,
                process,
            })
        })
        .collect()
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn filevault_check(output: &str) -> Check {
    const ID: &str = "disk_encryption";
    const TITLE: &str = "FileVault disk encryption";
    if output.contains("FileVault is On") {
        let detail = if output.contains("in progress") {
            "FileVault is on and still encrypting the disk."
        } else {
            "FileVault encrypts your disk."
        };
        Check::new(ID, TITLE, Category::Encryption, Status::Good, detail)
    } else if output.contains("FileVault is Off") {
        Check::new(
            ID,
            TITLE,
            Category::Encryption,
            Status::Warning,
            "FileVault is off. Anyone with this Mac could read your files.",
        )
        .recommend("Turn on FileVault in System Settings > Privacy & Security.")
    } else {
        Check::unknown(
            ID,
            TITLE,
            Category::Encryption,
            "fdesetup gave an unexpected answer",
        )
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn gatekeeper_check(output: &str) -> Check {
    const ID: &str = "gatekeeper";
    const TITLE: &str = "Gatekeeper";
    if output.contains("assessments enabled") {
        Check::new(
            ID,
            TITLE,
            Category::Protection,
            Status::Good,
            "Gatekeeper only lets trusted apps open.",
        )
    } else if output.contains("assessments disabled") {
        Check::new(
            ID,
            TITLE,
            Category::Protection,
            Status::Bad,
            "Gatekeeper is off, so any downloaded app can run without being checked.",
        )
        .recommend("Turn it back on by running `sudo spctl --master-enable`.")
    } else {
        Check::unknown(
            ID,
            TITLE,
            Category::Protection,
            "spctl gave an unexpected answer",
        )
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn sip_check(output: &str) -> Check {
    const ID: &str = "system_integrity_protection";
    const TITLE: &str = "System Integrity Protection";
    let lower = output.to_ascii_lowercase();
    if lower.contains("status: enabled") {
        Check::new(
            ID,
            TITLE,
            Category::Protection,
            Status::Good,
            "System Integrity Protection guards macOS system files.",
        )
    } else if lower.contains("status: disabled") || lower.contains("custom configuration") {
        Check::new(
            ID,
            TITLE,
            Category::Protection,
            Status::Bad,
            "System Integrity Protection is off or weakened, so malware can change system files.",
        )
        .recommend("Restart into Recovery and run `csrutil enable`.")
    } else {
        Check::unknown(
            ID,
            TITLE,
            Category::Protection,
            "csrutil gave an unexpected answer",
        )
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn macos_firewall_check(output: &str) -> Check {
    const ID: &str = "firewall";
    const TITLE: &str = "Firewall";
    let state = output.split_once("State = ").and_then(|(_, rest)| {
        rest.chars()
            .next()
            .and_then(|character| character.to_digit(10))
    });
    let enabled = match state {
        Some(state) => Some(state > 0),
        None if output.contains("is enabled") || output.contains("is blocking") => Some(true),
        None if output.contains("is disabled") => Some(false),
        None => None,
    };
    match enabled {
        Some(true) => Check::new(
            ID,
            TITLE,
            Category::Protection,
            Status::Good,
            "The macOS firewall is on.",
        ),
        Some(false) => Check::new(
            ID,
            TITLE,
            Category::Protection,
            Status::Warning,
            "The macOS firewall is off, so other devices can reach apps that accept connections.",
        )
        .recommend("Turn on the firewall in System Settings > Network > Firewall."),
        None => Check::unknown(
            ID,
            TITLE,
            Category::Protection,
            "socketfilterfw gave an unexpected answer",
        ),
    }
}

/// `value` is the output of `defaults read ... AutomaticCheckEnabled`, or `None` when the key
/// isn't set (which means macOS uses its default of checking automatically).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn macos_automatic_updates_check(value: Option<&str>) -> Check {
    const ID: &str = "automatic_updates";
    const TITLE: &str = "Automatic updates";
    match value.map(parse_true_false) {
        None | Some(Some(true)) => Check::new(
            ID,
            TITLE,
            Category::Updates,
            Status::Good,
            "macOS checks for updates automatically.",
        ),
        Some(Some(false)) => Check::new(
            ID,
            TITLE,
            Category::Updates,
            Status::Warning,
            "macOS doesn't check for updates automatically.",
        )
        .recommend("Turn on automatic updates in System Settings > General > Software Update."),
        Some(None) => Check::unknown(
            ID,
            TITLE,
            Category::Updates,
            "the update setting had an unexpected value",
        ),
    }
}

/// Reads `lsof -nP -iTCP -sTCP:LISTEN` output.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_lsof(output: &str) -> Vec<ListeningPort> {
    output
        .lines()
        .filter_map(|line| {
            let columns: Vec<&str> = line.split_whitespace().collect();
            let [command, .., protocol, name, state] = columns.as_slice() else {
                return None;
            };
            if *state != "(LISTEN)" {
                return None;
            }
            let (address, port) = parse_socket_address(name)?;
            Some(ListeningPort {
                protocol: protocol.to_ascii_uppercase(),
                address,
                port,
                process: Some(command.replace("\\x20", " ")),
            })
        })
        .collect()
}

type CheckJob = fn() -> Vec<Check>;

#[cfg(windows)]
use windows_checks as platform;

#[cfg(windows)]
mod windows_checks {
    use super::*;
    use crate::{COMMAND_TIMEOUT, run_command, run_powershell};
    use anyhow::Context as _;

    pub(super) fn check_jobs() -> Vec<CheckJob> {
        vec![
            defender as CheckJob,
            || vec![firewall()],
            || vec![bitlocker()],
            || vec![uac()],
            || vec![remote_desktop()],
            || vec![smb1()],
            || vec![last_update()],
            || vec![secure_boot()],
            || vec![guest_account()],
            || vec![auto_sign_in()],
            || vec![pending_restart()],
        ]
    }

    fn powershell_output(script: &str) -> Result<String> {
        let output = run_powershell(script, COMMAND_TIMEOUT)?;
        if output.success() {
            Ok(output.stdout)
        } else {
            anyhow::bail!("{}", output.error_summary())
        }
    }

    fn registry_u32(path: &str, name: &str) -> Option<u32> {
        windows_registry::LOCAL_MACHINE
            .open(path)
            .ok()?
            .get_u32(name)
            .ok()
    }

    fn registry_string(path: &str, name: &str) -> Option<String> {
        windows_registry::LOCAL_MACHINE
            .open(path)
            .ok()?
            .get_string(name)
            .ok()
    }

    fn defender() -> Vec<Check> {
        match powershell_output(
            "Get-MpComputerStatus | Select-Object RealTimeProtectionEnabled,AntivirusEnabled,\
             AntivirusSignatureAge,IsTamperProtected | ConvertTo-Json -Compress",
        ) {
            Ok(json) => defender_checks(&json),
            Err(error) => vec![Check::unknown(
                "antivirus",
                "Antivirus (Microsoft Defender)",
                Category::Protection,
                format!("{error:#}"),
            )],
        }
    }

    fn firewall() -> Check {
        match powershell_output(
            "Get-NetFirewallProfile | Select-Object Name,@{Name='Enabled';Expression={$_.Enabled.ToString()}} \
             | ConvertTo-Json -Compress",
        ) {
            Ok(json) => windows_firewall_check(&json),
            Err(error) => Check::unknown(
                "firewall",
                "Firewall",
                Category::Protection,
                format!("{error:#}"),
            ),
        }
    }

    fn bitlocker() -> Check {
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string());
        match run_command("manage-bde", &["-status", &drive], COMMAND_TIMEOUT) {
            Ok(output) => bitlocker_check(&output.stdout, &drive),
            Err(error) => Check::unknown(
                "disk_encryption",
                "Drive encryption (BitLocker)",
                Category::Encryption,
                format!("{error:#}"),
            ),
        }
    }

    fn uac() -> Check {
        const PATH: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System";
        uac_check(
            registry_u32(PATH, "EnableLUA"),
            registry_u32(PATH, "ConsentPromptBehaviorAdmin"),
        )
    }

    fn remote_desktop() -> Check {
        remote_desktop_check(registry_u32(
            r"SYSTEM\CurrentControlSet\Control\Terminal Server",
            "fDenyTSConnections",
        ))
    }

    fn smb1() -> Check {
        match powershell_output("(Get-SmbServerConfiguration).EnableSMB1Protocol") {
            Ok(output) => smb1_check(&output),
            Err(error) => {
                log::debug!("SMBv1 check failed: {error:#}");
                smb1_check("")
            }
        }
    }

    fn last_update() -> Check {
        match powershell_output(
            "Get-HotFix | Where-Object { $_.InstalledOn } | Sort-Object InstalledOn -Descending \
             | Select-Object -First 1 | ForEach-Object { $_.InstalledOn.ToString('yyyy-MM-dd') }",
        ) {
            Ok(output) => last_update_check(&output, chrono::Local::now().date_naive()),
            Err(error) => Check::unknown(
                "os_updates",
                "Windows updates",
                Category::Updates,
                format!("{error:#}"),
            ),
        }
    }

    fn secure_boot() -> Check {
        // The registry value is readable without administrator rights, unlike
        // Confirm-SecureBootUEFI, so it is tried first.
        if let Some(enabled) = registry_u32(
            r"SYSTEM\CurrentControlSet\Control\SecureBoot\State",
            "UEFISecureBootEnabled",
        ) {
            return secure_boot_check(Some(enabled != 0), "");
        }
        match powershell_output("Confirm-SecureBootUEFI") {
            Ok(output) => secure_boot_check(
                parse_true_false(&output),
                "Confirm-SecureBootUEFI gave an unexpected answer",
            ),
            Err(error) => secure_boot_check(None, &format!("{error:#}")),
        }
    }

    fn guest_account() -> Check {
        match powershell_output(
            "Get-LocalUser | Where-Object { $_.SID.Value -like 'S-1-5-*-501' } \
             | Select-Object Name,Enabled | ConvertTo-Json -Compress",
        ) {
            Ok(json) => guest_account_check(&json),
            Err(error) => Check::unknown(
                "guest_account",
                "Guest account",
                Category::Access,
                format!("{error:#}"),
            ),
        }
    }

    fn auto_sign_in() -> Check {
        let value = registry_string(
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon",
            "AutoAdminLogon",
        );
        auto_sign_in_check(value.as_deref())
    }

    fn pending_restart() -> Check {
        const KEYS: [&str; 2] = [
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired",
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending",
        ];
        let pending = KEYS
            .iter()
            .any(|key| windows_registry::LOCAL_MACHINE.open(key).is_ok());
        pending_restart_check(
            Some(pending),
            "Windows needs to restart to finish installing updates.",
        )
    }

    pub(super) fn all_listening_ports() -> Result<Vec<ListeningPort>> {
        let netstat = run_command("netstat", &["-ano"], COMMAND_TIMEOUT)?;
        if !netstat.success() {
            anyhow::bail!("netstat {}", netstat.error_summary());
        }
        let process_names = match run_command("tasklist", &["/fo", "csv", "/nh"], COMMAND_TIMEOUT)
            .context("could not list processes")
        {
            Ok(output) => parse_tasklist_csv(&output.stdout),
            Err(error) => {
                log::warn!("{error:#}");
                HashMap::new()
            }
        };
        Ok(parse_netstat(&netstat.stdout, &process_names))
    }
}

#[cfg(target_os = "linux")]
use linux_checks as platform;

#[cfg(target_os = "linux")]
mod linux_checks {
    use super::*;
    use crate::{COMMAND_TIMEOUT, run_command};
    use std::path::{Path, PathBuf};

    pub(super) fn check_jobs() -> Vec<CheckJob> {
        let mut jobs: Vec<CheckJob> = vec![
            || vec![firewall()],
            || vec![automatic_updates()],
            || vec![disk_encryption()],
            || vec![secure_boot()],
            || vec![pending_restart()],
        ];
        if Path::new(SSHD_CONFIG).exists() {
            jobs.push(|| vec![ssh_server()]);
        }
        jobs
    }

    const SSHD_CONFIG: &str = "/etc/ssh/sshd_config";

    fn has_program(name: &str) -> bool {
        which::which(name).is_ok()
    }

    fn firewall() -> Check {
        let mut any_tool_found = false;

        let mut ufw = None;
        if has_program("ufw") {
            any_tool_found = true;
            ufw = run_command("ufw", &["status"], COMMAND_TIMEOUT)
                .ok()
                .and_then(|output| parse_ufw_status(&output.stdout));
        }
        if ufw.is_none() {
            // `ufw status` needs root; its config file is readable by everyone.
            if let Ok(config) = std::fs::read_to_string("/etc/ufw/ufw.conf") {
                any_tool_found = true;
                ufw = parse_ufw_config(&config);
            }
        }

        let mut firewalld = None;
        if has_program("firewall-cmd") {
            any_tool_found = true;
            firewalld = run_command("firewall-cmd", &["--state"], COMMAND_TIMEOUT)
                .ok()
                .and_then(|output| parse_firewalld_state(&output.combined()));
        }

        let mut nftables = None;
        if has_program("nft") {
            any_tool_found = true;
            nftables = run_command("nft", &["list", "ruleset"], COMMAND_TIMEOUT)
                .ok()
                .filter(|output| output.success())
                .map(|output| !output.stdout.trim().is_empty());
        }

        linux_firewall_check(ufw, firewalld, nftables, any_tool_found)
    }

    fn resolve_sshd_include(pattern: &str) -> Vec<String> {
        let path = if Path::new(pattern).is_absolute() {
            PathBuf::from(pattern)
        } else {
            Path::new("/etc/ssh").join(pattern)
        };
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let paths = match (file_name.split_once('*'), path.parent()) {
            (Some((prefix, suffix)), Some(directory)) => {
                let mut matches: Vec<PathBuf> = std::fs::read_dir(directory)
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| entry.ok())
                    .map(|entry| entry.path())
                    .filter(|candidate| {
                        candidate.file_name().is_some_and(|name| {
                            let name = name.to_string_lossy();
                            name.len() >= prefix.len() + suffix.len()
                                && name.starts_with(prefix)
                                && name.ends_with(suffix)
                        })
                    })
                    .collect();
                matches.sort();
                matches
            }
            _ => vec![path],
        };
        paths
            .into_iter()
            .filter_map(|path| match std::fs::read_to_string(&path) {
                Ok(text) => Some(text),
                Err(error) => {
                    log::debug!("could not read {}: {error}", path.display());
                    None
                }
            })
            .collect()
    }

    fn ssh_server() -> Check {
        match std::fs::read_to_string(SSHD_CONFIG) {
            Ok(text) => ssh_server_check(&parse_sshd_config(&text, &mut resolve_sshd_include)),
            Err(error) => Check::unknown(
                "ssh_server",
                "SSH server",
                Category::Access,
                format!("could not read {SSHD_CONFIG}: {error}"),
            ),
        }
    }

    fn automatic_updates() -> Check {
        if has_program("apt-get") {
            let installed = has_program("unattended-upgrade")
                || Path::new("/usr/bin/unattended-upgrade").exists();
            let enabled = std::fs::read_to_string("/etc/apt/apt.conf.d/20auto-upgrades")
                .map(|text| parse_apt_auto_upgrades(&text))
                .unwrap_or(false);
            automatic_updates_check("unattended-upgrades", installed, enabled)
        } else if has_program("dnf") || has_program("dnf5") {
            const TIMERS: [&str; 3] = [
                "dnf-automatic.timer",
                "dnf-automatic-install.timer",
                "dnf5-automatic.timer",
            ];
            let installed = has_program("dnf-automatic")
                || TIMERS
                    .iter()
                    .any(|timer| Path::new("/usr/lib/systemd/system").join(timer).exists());
            let mut arguments = vec!["is-enabled"];
            arguments.extend(TIMERS);
            let enabled = run_command("systemctl", &arguments, COMMAND_TIMEOUT)
                .map(|output| output.stdout.lines().any(|line| line.trim() == "enabled"))
                .unwrap_or(false);
            automatic_updates_check("dnf-automatic", installed, enabled)
        } else {
            Check::unknown(
                "automatic_updates",
                "Automatic security updates",
                Category::Updates,
                "no supported update tool (unattended-upgrades or dnf-automatic) was found",
            )
        }
    }

    fn disk_encryption() -> Check {
        match run_command("lsblk", &["-n", "-o", "TYPE"], COMMAND_TIMEOUT) {
            Ok(output) if output.success() => linux_encryption_check(&output.stdout),
            Ok(output) => Check::unknown(
                "disk_encryption",
                "Disk encryption",
                Category::Encryption,
                format!("lsblk {}", output.error_summary()),
            ),
            Err(error) => Check::unknown(
                "disk_encryption",
                "Disk encryption",
                Category::Encryption,
                format!("{error:#}"),
            ),
        }
    }

    fn secure_boot() -> Check {
        if let Ok(output) = run_command("mokutil", &["--sb-state"], COMMAND_TIMEOUT)
            && let Some(enabled) = parse_mokutil_state(&output.combined())
        {
            return secure_boot_check(Some(enabled), "");
        }
        if !Path::new("/sys/firmware/efi").exists() {
            return secure_boot_check(
                None,
                "this computer starts in legacy BIOS mode, which has no Secure Boot",
            );
        }
        let enabled = std::fs::read(
            "/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c",
        )
        .ok()
        .and_then(|bytes| parse_secure_boot_efivar(&bytes));
        secure_boot_check(enabled, "the Secure Boot state couldn't be read")
    }

    fn pending_restart() -> Check {
        let marker = Path::new("/var/run/reboot-required");
        if marker.exists() {
            let packages = std::fs::read_to_string("/var/run/reboot-required.pkgs")
                .map(|text| {
                    let packages: BTreeSet<&str> = text
                        .lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty())
                        .collect();
                    packages.into_iter().collect::<Vec<_>>().join(", ")
                })
                .unwrap_or_default();
            let detail = if packages.is_empty() {
                "A restart is needed to finish installing updates.".to_string()
            } else {
                format!("A restart is needed to finish installing updates to {packages}.")
            };
            return pending_restart_check(Some(true), &detail);
        }
        if has_program("apt-get") {
            return pending_restart_check(Some(false), "");
        }
        if has_program("needs-restarting") {
            return match run_command("needs-restarting", &["-r"], COMMAND_TIMEOUT) {
                Ok(output) => match output.exit_code {
                    Some(0) => pending_restart_check(Some(false), ""),
                    Some(1) => pending_restart_check(
                        Some(true),
                        "A restart is needed to finish installing updates.",
                    ),
                    _ => pending_restart_check(
                        None,
                        &format!("needs-restarting {}", output.error_summary()),
                    ),
                },
                Err(error) => pending_restart_check(None, &format!("{error:#}")),
            };
        }
        pending_restart_check(
            None,
            "this Linux distribution doesn't report pending restarts",
        )
    }

    pub(super) fn all_listening_ports() -> Result<Vec<ListeningPort>> {
        let output = run_command("ss", &["-tulpnH"], COMMAND_TIMEOUT)?;
        if !output.success() {
            anyhow::bail!("ss {}", output.error_summary());
        }
        Ok(parse_ss(&output.stdout))
    }
}

#[cfg(target_os = "macos")]
use macos_checks as platform;

#[cfg(target_os = "macos")]
mod macos_checks {
    use super::*;
    use crate::{COMMAND_TIMEOUT, run_command};

    pub(super) fn check_jobs() -> Vec<CheckJob> {
        vec![
            || vec![filevault()],
            || vec![gatekeeper()],
            || vec![sip()],
            || vec![firewall()],
            || vec![automatic_updates()],
        ]
    }

    fn checked(
        program: &str,
        args: &[&str],
        parse: fn(&str) -> Check,
        id: &'static str,
        title: &str,
        category: Category,
    ) -> Check {
        match run_command(program, args, COMMAND_TIMEOUT) {
            Ok(output) => parse(&output.combined()),
            Err(error) => Check::unknown(id, title, category, format!("{error:#}")),
        }
    }

    fn filevault() -> Check {
        checked(
            "fdesetup",
            &["status"],
            filevault_check,
            "disk_encryption",
            "FileVault disk encryption",
            Category::Encryption,
        )
    }

    fn gatekeeper() -> Check {
        checked(
            "spctl",
            &["--status"],
            gatekeeper_check,
            "gatekeeper",
            "Gatekeeper",
            Category::Protection,
        )
    }

    fn sip() -> Check {
        checked(
            "csrutil",
            &["status"],
            sip_check,
            "system_integrity_protection",
            "System Integrity Protection",
            Category::Protection,
        )
    }

    fn firewall() -> Check {
        checked(
            "/usr/libexec/ApplicationFirewall/socketfilterfw",
            &["--getglobalstate"],
            macos_firewall_check,
            "firewall",
            "Firewall",
            Category::Protection,
        )
    }

    fn automatic_updates() -> Check {
        match run_command(
            "defaults",
            &[
                "read",
                "/Library/Preferences/com.apple.SoftwareUpdate",
                "AutomaticCheckEnabled",
            ],
            COMMAND_TIMEOUT,
        ) {
            Ok(output) if output.success() => macos_automatic_updates_check(Some(&output.stdout)),
            Ok(output) if output.combined().contains("does not exist") => {
                macos_automatic_updates_check(None)
            }
            Ok(output) => Check::unknown(
                "automatic_updates",
                "Automatic updates",
                Category::Updates,
                output.error_summary(),
            ),
            Err(error) => Check::unknown(
                "automatic_updates",
                "Automatic updates",
                Category::Updates,
                format!("{error:#}"),
            ),
        }
    }

    pub(super) fn all_listening_ports() -> Result<Vec<ListeningPort>> {
        let output = run_command("lsof", &["-nP", "-iTCP", "-sTCP:LISTEN"], COMMAND_TIMEOUT)?;
        // lsof exits with 1 when nothing matched, which just means no listening ports.
        if !output.success() && !output.stdout.trim().is_empty() {
            anyhow::bail!("lsof {}", output.error_summary());
        }
        Ok(parse_lsof(&output.stdout))
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod platform {
    use super::*;

    pub(super) fn check_jobs() -> Vec<CheckJob> {
        Vec::new()
    }

    pub(super) fn all_listening_ports() -> Result<Vec<ListeningPort>> {
        anyhow::bail!("listing ports isn't supported on this operating system")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(protocol: &str, address: &str, port: u16, process: Option<&str>) -> ListeningPort {
        ListeningPort {
            protocol: protocol.to_string(),
            address: address.to_string(),
            port,
            process: process.map(str::to_string),
        }
    }

    #[test]
    fn status_orders_worst_first() {
        let mut checks = vec![
            Check::new("a", "A", Category::Health, Status::Good, ""),
            Check::new("b", "B", Category::Health, Status::Unknown, ""),
            Check::new("c", "C", Category::Health, Status::Bad, ""),
            Check::new("d", "D", Category::Health, Status::Warning, ""),
        ];
        sort_worst_first(&mut checks);
        let order: Vec<Status> = checks.iter().map(|check| check.status).collect();
        assert_eq!(
            order,
            [Status::Bad, Status::Warning, Status::Unknown, Status::Good]
        );
    }

    #[test]
    fn parses_socket_addresses() {
        assert_eq!(
            parse_socket_address("0.0.0.0:22"),
            Some(("0.0.0.0".into(), 22))
        );
        assert_eq!(parse_socket_address("[::]:445"), Some(("::".into(), 445)));
        assert_eq!(parse_socket_address("*:8080"), Some(("*".into(), 8080)));
        assert_eq!(
            parse_socket_address("127.0.0.53%lo:53"),
            Some(("127.0.0.53".into(), 53))
        );
        assert_eq!(
            parse_socket_address("[fe80::1%eth0]:546"),
            Some(("fe80::1".into(), 546))
        );
        assert_eq!(parse_socket_address("0.0.0.0:*"), None);
        assert_eq!(parse_socket_address("nonsense"), None);
    }

    #[test]
    fn loopback_detection() {
        assert!(is_loopback_address("127.0.0.1"));
        assert!(is_loopback_address("127.0.0.53"));
        assert!(is_loopback_address("::1"));
        assert!(is_loopback_address("::ffff:127.0.0.1"));
        assert!(is_loopback_address("localhost"));
        assert!(!is_loopback_address("0.0.0.0"));
        assert!(!is_loopback_address("::"));
        assert!(!is_loopback_address("*"));
        assert!(!is_loopback_address("192.168.1.20"));
    }

    #[test]
    fn exposed_only_drops_loopback_and_duplicates() {
        let ports = exposed_only(vec![
            port("TCP", "127.0.0.1", 631, Some("cupsd")),
            port("TCP", "0.0.0.0", 22, Some("sshd")),
            port("TCP", "0.0.0.0", 22, Some("sshd")),
            port("TCP", "::1", 631, None),
        ]);
        assert_eq!(ports, vec![port("TCP", "0.0.0.0", 22, Some("sshd"))]);
    }

    #[test]
    fn exposed_ports_check_lists_ports() {
        let check = exposed_ports_check(Ok(vec![
            port("TCP", "0.0.0.0", 22, Some("sshd")),
            port("TCP", "::", 445, None),
        ]));
        assert_eq!(check.status, Status::Warning);
        assert_eq!(check.category, Category::Network);
        assert!(check.detail.contains("TCP 0.0.0.0:22 (sshd)"));
        assert!(check.detail.contains("TCP [::]:445"));
        assert!(check.recommendation.is_some());

        assert_eq!(exposed_ports_check(Ok(Vec::new())).status, Status::Good);
        assert_eq!(
            exposed_ports_check(Err(anyhow::anyhow!("ss missing"))).status,
            Status::Unknown
        );
    }

    #[test]
    fn parses_netstat_output() {
        let output = "
Active Connections

  Proto  Local Address          Foreign Address        State           PID
  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1140
  TCP    127.0.0.1:6463         0.0.0.0:0              LISTENING       9876
  TCP    192.168.1.20:52000     140.82.112.25:443      ESTABLISHED     4321
  TCP    0.0.0.0:3389           0.0.0.0:0              ABHÖREN         1300
  TCP    [::]:445               [::]:0                 LISTENING       4
  UDP    0.0.0.0:5353           *:*                                    2345
";
        let names = parse_tasklist_csv(
            "\"System\",\"4\",\"Services\",\"0\",\"144 K\"\r\n\
             \"svchost.exe\",\"1140\",\"Services\",\"0\",\"12,345 K\"\r\n",
        );
        let ports = parse_netstat(output, &names);
        assert_eq!(
            ports,
            vec![
                port("TCP", "0.0.0.0", 135, Some("svchost.exe")),
                port("TCP", "127.0.0.1", 6463, None),
                port("TCP", "0.0.0.0", 3389, None),
                port("TCP", "::", 445, Some("System")),
            ]
        );
        assert_eq!(exposed_only(ports).len(), 3);
    }

    #[test]
    fn parses_tasklist_csv() {
        let names = parse_tasklist_csv(
            "\"System Idle Process\",\"0\",\"Services\",\"0\",\"8 K\"\n\
             \"chrome.exe\",\"4242\",\"Console\",\"1\",\"250,000 K\"\n\
             garbage line\n",
        );
        assert_eq!(
            names.get(&0).map(String::as_str),
            Some("System Idle Process")
        );
        assert_eq!(names.get(&4242).map(String::as_str), Some("chrome.exe"));
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn parses_ss_output() {
        let output = "\
udp   UNCONN 0      0          127.0.0.53%lo:53          0.0.0.0:*    users:((\"systemd-resolve\",pid=612,fd=13))
udp   UNCONN 0      0                0.0.0.0:5353        0.0.0.0:*
tcp   LISTEN 0      128              0.0.0.0:22          0.0.0.0:*    users:((\"sshd\",pid=900,fd=3))
tcp   LISTEN 0      4096                [::]:22             [::]:*
tcp   LISTEN 0      511                    *:8080              *:*    users:((\"node\",pid=1234,fd=20))
tcp   LISTEN 0      5              [::1]:631              [::]:*
";
        let ports = parse_ss(output);
        assert_eq!(
            ports,
            vec![
                port("UDP", "127.0.0.53", 53, Some("systemd-resolve")),
                port("UDP", "0.0.0.0", 5353, None),
                port("TCP", "0.0.0.0", 22, Some("sshd")),
                port("TCP", "::", 22, None),
                port("TCP", "*", 8080, Some("node")),
                port("TCP", "::1", 631, None),
            ]
        );
        assert_eq!(exposed_only(ports).len(), 4);
        assert!(
            parse_ss("Netid State Recv-Q Send-Q Local Address:Port Peer Address:Port").is_empty()
        );
    }

    #[test]
    fn parses_lsof_output() {
        let output = "\
COMMAND     PID   USER   FD   TYPE             DEVICE SIZE/OFF NODE NAME
rapportd    512   me    8u  IPv4 0x1234567890abcdef      0t0  TCP *:49152 (LISTEN)
rapportd    512   me    9u  IPv6 0x1234567890abcdef      0t0  TCP *:49152 (LISTEN)
ControlCe   530   me   10u  IPv4 0x1234567890abcdef      0t0  TCP 127.0.0.1:7000 (LISTEN)
Code\\x20H  777   me   40u  IPv6 0x1234567890abcdef      0t0  TCP [::1]:9229 (LISTEN)
postgres    800   me    7u  IPv4 0x1234567890abcdef      0t0  TCP 192.168.1.5:5432 (LISTEN)
";
        let ports = parse_lsof(output);
        assert_eq!(
            ports,
            vec![
                port("TCP", "*", 49152, Some("rapportd")),
                port("TCP", "*", 49152, Some("rapportd")),
                port("TCP", "127.0.0.1", 7000, Some("ControlCe")),
                port("TCP", "::1", 9229, Some("Code H")),
                port("TCP", "192.168.1.5", 5432, Some("postgres")),
            ]
        );
        assert_eq!(exposed_only(ports).len(), 2);
    }

    #[test]
    fn defender_all_good() {
        let checks = defender_checks(
            r#"{"RealTimeProtectionEnabled":true,"AntivirusEnabled":true,"AntivirusSignatureAge":0,"IsTamperProtected":true}"#,
        );
        assert_eq!(checks.len(), 3);
        assert!(checks.iter().all(|check| check.status == Status::Good));
    }

    #[test]
    fn defender_problems() {
        let checks = defender_checks(
            r#"{"RealTimeProtectionEnabled":false,"AntivirusEnabled":true,"AntivirusSignatureAge":40,"IsTamperProtected":false}"#,
        );
        let status = |id: &str| {
            checks
                .iter()
                .find(|check| check.id == id)
                .map(|check| check.status)
        };
        assert_eq!(status("antivirus"), Some(Status::Bad));
        assert_eq!(status("antivirus_definitions"), Some(Status::Bad));
        assert_eq!(status("tamper_protection"), Some(Status::Warning));

        let checks = defender_checks(
            r#"{"RealTimeProtectionEnabled":true,"AntivirusEnabled":true,"AntivirusSignatureAge":9,"IsTamperProtected":null}"#,
        );
        assert_eq!(checks.len(), 2);
        assert_eq!(checks[1].status, Status::Warning);
    }

    #[test]
    fn defender_bad_output_is_unknown() {
        let checks = defender_checks("Get-MpComputerStatus : The term is not recognized");
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].status, Status::Unknown);
        assert_eq!(defender_checks("")[0].status, Status::Unknown);
    }

    #[test]
    fn windows_firewall_profiles() {
        let all_on = r#"[{"Name":"Domain","Enabled":"True"},{"Name":"Private","Enabled":"True"},{"Name":"Public","Enabled":"True"}]"#;
        assert_eq!(windows_firewall_check(all_on).status, Status::Good);

        let public_off = r#"[{"Name":"Domain","Enabled":1},{"Name":"Private","Enabled":true},{"Name":"Public","Enabled":"False"}]"#;
        let check = windows_firewall_check(public_off);
        assert_eq!(check.status, Status::Bad);
        assert!(check.detail.contains("Public"));
        assert!(!check.detail.contains("Private"));

        let single = r#"{"Name":"Domain","Enabled":"True"}"#;
        assert_eq!(windows_firewall_check(single).status, Status::Good);
        assert_eq!(windows_firewall_check("").status, Status::Unknown);
    }

    #[test]
    fn bitlocker_states() {
        let on = "BitLocker Drive Encryption: Configuration Tool version 10.0.22621
Copyright (C) 2013 Microsoft Corporation. All rights reserved.

Volume C: [OS]
[OS Volume]

    Size:                 475.83 GB
    BitLocker Version:    2.0
    Conversion Status:    Used Space Only Encrypted
    Percentage Encrypted: 100.0%
    Encryption Method:    XTS-AES 128
    Protection Status:    Protection On
    Lock Status:          Unlocked
    Identification Field: Unknown
    Key Protectors:
        TPM
        Numerical Password
";
        assert_eq!(bitlocker_check(on, "C:").status, Status::Good);

        let off = "Volume C: [OS]
    Conversion Status:    Fully Decrypted
    Percentage Encrypted: 0.0%
    Protection Status:    Protection Off
";
        let check = bitlocker_check(off, "C:");
        assert_eq!(check.status, Status::Warning);
        assert!(check.detail.contains("isn't encrypted"));

        let suspended = "    Conversion Status:    Fully Encrypted
    Protection Status:    Protection Off
";
        assert!(
            bitlocker_check(suspended, "C:")
                .detail
                .contains("suspended")
        );

        let denied = "BitLocker Drive Encryption: Configuration Tool version 10.0.22621
ERROR: An attempt to access a required resource was denied.
Check that you have administrative rights on the computer.
";
        let check = bitlocker_check(denied, "C:");
        assert_eq!(check.status, Status::Unknown);
        assert!(check.detail.contains("administrator"));
    }

    #[test]
    fn registry_based_windows_checks() {
        assert_eq!(uac_check(Some(1), Some(5)).status, Status::Good);
        assert_eq!(uac_check(Some(1), Some(0)).status, Status::Warning);
        assert_eq!(uac_check(Some(0), Some(5)).status, Status::Bad);
        assert_eq!(uac_check(None, None).status, Status::Unknown);

        assert_eq!(remote_desktop_check(Some(1)).status, Status::Good);
        assert_eq!(remote_desktop_check(Some(0)).status, Status::Warning);
        assert_eq!(remote_desktop_check(None).status, Status::Unknown);

        assert_eq!(auto_sign_in_check(Some("1")).status, Status::Warning);
        assert_eq!(auto_sign_in_check(Some("0")).status, Status::Good);
        assert_eq!(auto_sign_in_check(None).status, Status::Good);
    }

    #[test]
    fn smb1_states() {
        assert_eq!(smb1_check("True\r\n").status, Status::Bad);
        assert_eq!(smb1_check("False\r\n").status, Status::Good);
        assert_eq!(smb1_check("").status, Status::Unknown);
    }

    #[test]
    fn last_update_age() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 25).unwrap();
        let recent = last_update_check("2026-09-10\r\n", today);
        assert_eq!(recent.status, Status::Good);
        assert!(recent.detail.contains("15 days ago"));

        let old = last_update_check("2026-05-01\r\n", today);
        assert_eq!(old.status, Status::Warning);
        assert!(old.recommendation.is_some());

        assert_eq!(last_update_check("", today).status, Status::Unknown);
        assert!(
            last_update_check("2026-09-25", today)
                .detail
                .contains("today")
        );
    }

    #[test]
    fn guest_account_states() {
        assert_eq!(
            guest_account_check(r#"{"Name":"Guest","Enabled":false}"#).status,
            Status::Good
        );
        assert_eq!(
            guest_account_check(r#"{"Name":"Gast","Enabled":true}"#).status,
            Status::Warning
        );
        assert_eq!(guest_account_check("").status, Status::Good);
        assert_eq!(guest_account_check("{not json").status, Status::Unknown);
    }

    #[test]
    fn secure_boot_and_restart_states() {
        assert_eq!(secure_boot_check(Some(true), "").status, Status::Good);
        assert_eq!(secure_boot_check(Some(false), "").status, Status::Warning);
        let unknown = secure_boot_check(None, "Cmdlet not supported on this platform");
        assert_eq!(unknown.status, Status::Unknown);
        assert!(unknown.detail.contains("Cmdlet not supported"));

        assert_eq!(
            pending_restart_check(Some(true), "x").status,
            Status::Warning
        );
        assert_eq!(pending_restart_check(Some(false), "").status, Status::Good);
        assert_eq!(pending_restart_check(None, "x").status, Status::Unknown);
    }

    #[test]
    fn parses_linux_firewall_outputs() {
        assert_eq!(
            parse_ufw_status(
                "Status: active\n\nTo                         Action      From\n--                         ------      ----\n22/tcp                     ALLOW       Anywhere\n"
            ),
            Some(true)
        );
        assert_eq!(parse_ufw_status("Status: inactive\n"), Some(false));
        assert_eq!(
            parse_ufw_status("ERROR: You need to be root to run this script\n"),
            None
        );
        assert_eq!(
            parse_ufw_config(
                "# /etc/ufw/ufw.conf\n#\n\n# Set to yes to start on boot.\nENABLED=yes\nLOGLEVEL=low\n"
            ),
            Some(true)
        );
        assert_eq!(parse_ufw_config("ENABLED=no\n"), Some(false));
        assert_eq!(parse_firewalld_state("running\n"), Some(true));
        assert_eq!(parse_firewalld_state("\nnot running\n"), Some(false));
        assert_eq!(parse_firewalld_state(""), None);
    }

    #[test]
    fn linux_firewall_combinations() {
        assert_eq!(
            linux_firewall_check(Some(true), None, None, true).status,
            Status::Good
        );
        assert_eq!(
            linux_firewall_check(Some(false), None, Some(true), true).status,
            Status::Good
        );
        assert_eq!(
            linux_firewall_check(Some(false), None, None, true).status,
            Status::Warning
        );
        assert_eq!(
            linux_firewall_check(None, None, None, false).status,
            Status::Warning
        );
        assert_eq!(
            linux_firewall_check(None, None, None, true).status,
            Status::Unknown
        );
    }

    #[test]
    fn sshd_config_first_value_wins_and_follows_includes() {
        let main = "\
# This is the sshd server system-wide configuration file.
Include /etc/ssh/sshd_config.d/*.conf

#PermitRootLogin prohibit-password
PermitRootLogin yes
PasswordAuthentication yes
KbdInteractiveAuthentication no

Match User anoncvs
	PasswordAuthentication no
";
        let mut requested = Vec::new();
        let settings = parse_sshd_config(main, &mut |pattern| {
            requested.push(pattern.to_string());
            vec!["PasswordAuthentication no\n".to_string()]
        });
        assert_eq!(requested, vec!["/etc/ssh/sshd_config.d/*.conf"]);
        assert_eq!(
            settings,
            SshdSettings {
                permit_root_login: Some("yes".into()),
                password_authentication: Some("no".into()),
            }
        );
        assert_eq!(ssh_server_check(&settings).status, Status::Bad);
    }

    #[test]
    fn sshd_config_defaults_and_match_blocks() {
        let settings = parse_sshd_config(
            "Port 22\nMatch Address 10.0.0.0/8\n  PermitRootLogin yes\n",
            &mut |_| Vec::new(),
        );
        assert_eq!(settings, SshdSettings::default());
        // OpenSSH allows passwords unless told otherwise.
        assert_eq!(ssh_server_check(&settings).status, Status::Warning);

        let settings = parse_sshd_config(
            "permitrootlogin=no\n\tPasswordAuthentication   \"no\"\n",
            &mut |_| Vec::new(),
        );
        assert_eq!(settings.permit_root_login.as_deref(), Some("no"));
        assert_eq!(settings.password_authentication.as_deref(), Some("no"));
        assert_eq!(ssh_server_check(&settings).status, Status::Good);
    }

    #[test]
    fn sshd_config_include_recursion_is_bounded() {
        let settings = parse_sshd_config("Include self\n", &mut |_| {
            vec!["Include self\n".to_string()]
        });
        assert_eq!(settings, SshdSettings::default());
    }

    #[test]
    fn apt_auto_upgrades() {
        assert!(parse_apt_auto_upgrades(
            "APT::Periodic::Update-Package-Lists \"1\";\nAPT::Periodic::Unattended-Upgrade \"1\";\n"
        ));
        assert!(!parse_apt_auto_upgrades(
            "APT::Periodic::Update-Package-Lists \"1\";\nAPT::Periodic::Unattended-Upgrade \"0\";\n"
        ));
        assert!(!parse_apt_auto_upgrades(""));

        assert_eq!(
            automatic_updates_check("unattended-upgrades", true, true).status,
            Status::Good
        );
        assert_eq!(
            automatic_updates_check("unattended-upgrades", true, false).status,
            Status::Warning
        );
        assert_eq!(
            automatic_updates_check("dnf-automatic", false, false).status,
            Status::Warning
        );
    }

    #[test]
    fn lsblk_encryption() {
        let encrypted = "disk\npart\npart\ncrypt\nlvm\nlvm\n";
        assert_eq!(linux_encryption_check(encrypted).status, Status::Good);
        let plain = "disk\npart\npart\nrom\nloop\n";
        assert_eq!(linux_encryption_check(plain).status, Status::Warning);
    }

    #[test]
    fn linux_secure_boot_parsers() {
        assert_eq!(parse_mokutil_state("SecureBoot enabled\n"), Some(true));
        assert_eq!(
            parse_mokutil_state("SecureBoot disabled\nPlatform is in Setup Mode\n"),
            Some(false)
        );
        assert_eq!(
            parse_mokutil_state("EFI variables are not supported on this system\n"),
            None
        );
        assert_eq!(parse_secure_boot_efivar(&[6, 0, 0, 0, 1]), Some(true));
        assert_eq!(parse_secure_boot_efivar(&[6, 0, 0, 0, 0]), Some(false));
        assert_eq!(parse_secure_boot_efivar(&[6, 0]), None);
    }

    #[test]
    fn macos_parsers() {
        assert_eq!(filevault_check("FileVault is On.\n").status, Status::Good);
        assert_eq!(
            filevault_check("FileVault is On.\nEncryption in progress: Percent completed = 42\n")
                .status,
            Status::Good
        );
        assert_eq!(
            filevault_check("FileVault is Off.\n").status,
            Status::Warning
        );
        assert_eq!(filevault_check("").status, Status::Unknown);

        assert_eq!(
            gatekeeper_check("assessments enabled\n").status,
            Status::Good
        );
        assert_eq!(
            gatekeeper_check("assessments disabled\n").status,
            Status::Bad
        );

        assert_eq!(
            sip_check("System Integrity Protection status: enabled.\n").status,
            Status::Good
        );
        assert_eq!(
            sip_check("System Integrity Protection status: disabled.\n").status,
            Status::Bad
        );
        assert_eq!(
            sip_check("System Integrity Protection status: unknown (Custom Configuration).\n")
                .status,
            Status::Bad
        );

        assert_eq!(
            macos_firewall_check("Firewall is enabled. (State = 1)\n").status,
            Status::Good
        );
        assert_eq!(
            macos_firewall_check(
                "Firewall is blocking all non-essential incoming connections. (State = 2)\n"
            )
            .status,
            Status::Good
        );
        assert_eq!(
            macos_firewall_check("Firewall is disabled. (State = 0)\n").status,
            Status::Warning
        );
        assert_eq!(macos_firewall_check("").status, Status::Unknown);

        assert_eq!(
            macos_automatic_updates_check(Some("1\n")).status,
            Status::Good
        );
        assert_eq!(
            macos_automatic_updates_check(Some("0\n")).status,
            Status::Warning
        );
        assert_eq!(macos_automatic_updates_check(None).status, Status::Good);
    }

    #[test]
    fn json_bool_accepts_powershell_shapes() {
        assert_eq!(json_bool(&serde_json::json!(true)), Some(true));
        assert_eq!(json_bool(&serde_json::json!(0)), Some(false));
        assert_eq!(json_bool(&serde_json::json!("True")), Some(true));
        assert_eq!(json_bool(&serde_json::json!("NotConfigured")), None);
        assert_eq!(json_bool(&serde_json::Value::Null), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn security_checks_never_panic() {
        let checks = run_security_checks();
        assert!(checks.iter().any(|check| check.id == "open_ports"));
        assert!(
            checks
                .windows(2)
                .all(|pair| pair[0].status <= pair[1].status)
        );
    }
}
