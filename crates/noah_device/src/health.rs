use crate::checks::{Category, Check, Status, sort_worst_first};
use crate::format_bytes;
use std::collections::HashSet;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use sysinfo::{
    Components, CpuRefreshKind, Disks, MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate,
    RefreshKind, System,
};

const TOP_PROCESS_COUNT: usize = 8;

#[derive(Clone, Debug, Default)]
pub struct HealthSnapshot {
    pub cpu_usage_percent: f32,
    pub cpu_name: String,
    pub cpu_cores: usize,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub swap_total_bytes: u64,
    pub disks: Vec<DiskHealth>,
    pub uptime_seconds: u64,
    pub os_name: String,
    pub os_version: String,
    pub host_name: String,
    pub top_processes: Vec<ProcessUsage>,
    pub temperatures: Vec<(String, f32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DiskHealth {
    pub name: String,
    pub mount_point: PathBuf,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub is_removable: bool,
}

impl DiskHealth {
    fn used_fraction(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        self.total_bytes.saturating_sub(self.available_bytes) as f64 / self.total_bytes as f64
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProcessUsage {
    pub name: String,
    pub pid: u32,
    /// Share of the whole machine's CPU capacity (0-100), not of a single core.
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

/// Blocking: samples CPU twice ~300ms apart (sysinfo needs two samples). Top 8 processes by CPU
/// then memory.
pub fn snapshot() -> HealthSnapshot {
    let mut system = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::everything())
            .with_memory(MemoryRefreshKind::everything()),
    );
    let process_refresh = ProcessRefreshKind::nothing().with_cpu().with_memory();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, process_refresh);

    thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(Duration::from_millis(300)));

    system.refresh_cpu_usage();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, process_refresh);

    let cpu_cores = system.cpus().len().max(1);
    let cpu_name = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_string())
        .unwrap_or_default();

    let processes = system
        .processes()
        .values()
        .filter(|process| process.pid().as_u32() != 0 && process.thread_kind().is_none())
        .map(|process| ProcessUsage {
            name: process.name().to_string_lossy().into_owned(),
            pid: process.pid().as_u32(),
            cpu_percent: process.cpu_usage() / cpu_cores as f32,
            memory_bytes: process.memory(),
        })
        .collect();

    let disks = Disks::new_with_refreshed_list();
    let disks = relevant_disks(disks.list().iter().map(|disk| {
        let file_system = disk.file_system().to_string_lossy().into_owned();
        let name = disk.name().to_string_lossy().into_owned();
        let health = DiskHealth {
            name: if name.is_empty() {
                disk.mount_point().display().to_string()
            } else {
                name
            },
            mount_point: disk.mount_point().to_path_buf(),
            total_bytes: disk.total_space(),
            available_bytes: disk.available_space(),
            is_removable: disk.is_removable(),
        };
        (health, file_system)
    }));

    let components = Components::new_with_refreshed_list();
    let temperatures = components
        .list()
        .iter()
        .filter_map(|component| {
            let temperature = component.temperature()?;
            (temperature.is_finite() && temperature > 0.0)
                .then(|| (component.label().to_string(), temperature))
        })
        .collect();

    HealthSnapshot {
        cpu_usage_percent: system.global_cpu_usage(),
        cpu_name,
        cpu_cores,
        memory_used_bytes: system.used_memory(),
        memory_total_bytes: system.total_memory(),
        swap_used_bytes: system.used_swap(),
        swap_total_bytes: system.total_swap(),
        disks,
        uptime_seconds: System::uptime(),
        os_name: System::name().unwrap_or_else(|| "Unknown".to_string()),
        os_version: System::os_version().unwrap_or_default(),
        host_name: System::host_name().unwrap_or_default(),
        top_processes: top_processes(processes),
        temperatures,
    }
}

fn top_processes(mut processes: Vec<ProcessUsage>) -> Vec<ProcessUsage> {
    processes.sort_by(|left, right| {
        right
            .cpu_percent
            .total_cmp(&left.cpu_percent)
            .then(right.memory_bytes.cmp(&left.memory_bytes))
            .then(left.pid.cmp(&right.pid))
    });
    processes.truncate(TOP_PROCESS_COUNT);
    processes
}

/// Drops pseudo and read-only image file systems (snap packages, tmpfs, overlays) that are
/// always "full" and would otherwise raise false disk-space alarms, and duplicate mounts.
fn relevant_disks(disks: impl Iterator<Item = (DiskHealth, String)>) -> Vec<DiskHealth> {
    const IGNORED_FILE_SYSTEMS: [&str; 8] = [
        "squashfs", "tmpfs", "devtmpfs", "overlay", "ramfs", "efivarfs", "iso9660", "autofs",
    ];
    let mut seen_mount_points = HashSet::new();
    disks
        .filter(|(disk, file_system)| {
            let file_system = file_system.to_ascii_lowercase();
            disk.total_bytes > 0
                && !IGNORED_FILE_SYSTEMS.contains(&file_system.as_str())
                && !disk.mount_point.starts_with("/snap")
                && !disk.mount_point.starts_with("/var/snap")
        })
        .filter_map(|(disk, _)| {
            seen_mount_points
                .insert(disk.mount_point.clone())
                .then_some(disk)
        })
        .collect()
}

/// Health findings derived from a snapshot: disk > 90% full = Bad, > 80% = Warning; memory > 90%
/// = Warning; swap > 80% = Warning; CPU > 90% = Warning; uptime > 14 days = Warning; any
/// temperature >= 90°C = Warning. Sorted worst-first.
pub fn findings(snapshot: &HealthSnapshot) -> Vec<Check> {
    let mut checks = vec![
        disk_space_check(&snapshot.disks),
        memory_check(snapshot),
        cpu_check(snapshot),
        uptime_check(snapshot.uptime_seconds),
    ];
    if snapshot.swap_total_bytes > 0 {
        checks.push(swap_check(snapshot));
    }
    if !snapshot.temperatures.is_empty() {
        checks.push(temperature_check(&snapshot.temperatures));
    }
    sort_worst_first(&mut checks);
    checks
}

fn percent(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        used as f64 * 100.0 / total as f64
    }
}

fn disk_space_check(disks: &[DiskHealth]) -> Check {
    const ID: &str = "disk_space";
    const TITLE: &str = "Disk space";
    // A nearly full USB stick isn't a problem with the computer itself.
    let fixed: Vec<&DiskHealth> = disks.iter().filter(|disk| !disk.is_removable).collect();
    if fixed.is_empty() {
        return Check::unknown(ID, TITLE, Category::Health, "no disks were found");
    }

    let mut worst = Status::Good;
    let mut problems = Vec::new();
    for disk in &fixed {
        let used_percent = disk.used_fraction() * 100.0;
        let status = if used_percent > 90.0 {
            Status::Bad
        } else if used_percent > 80.0 {
            Status::Warning
        } else {
            continue;
        };
        worst = worst.min(status);
        problems.push(format!(
            "{} is {used_percent:.0}% full ({} free)",
            disk.mount_point.display(),
            format_bytes(disk.available_bytes)
        ));
    }

    if problems.is_empty() {
        let free: u64 = fixed.iter().map(|disk| disk.available_bytes).sum();
        Check::new(
            ID,
            TITLE,
            Category::Health,
            Status::Good,
            format!(
                "Every disk has room to spare ({} free in total).",
                format_bytes(free)
            ),
        )
    } else {
        Check::new(
            ID,
            TITLE,
            Category::Health,
            worst,
            format!("{}.", problems.join("; ")),
        )
        .recommend(
            "Free up space by removing duplicate files, emptying the trash or Downloads folder, \
             and uninstalling apps you don't use.",
        )
    }
}

fn memory_check(snapshot: &HealthSnapshot) -> Check {
    const ID: &str = "memory";
    const TITLE: &str = "Memory";
    if snapshot.memory_total_bytes == 0 {
        return Check::unknown(ID, TITLE, Category::Health, "memory size couldn't be read");
    }
    let used_percent = percent(snapshot.memory_used_bytes, snapshot.memory_total_bytes);
    let detail = format!(
        "{used_percent:.0}% of memory is in use ({} of {}).",
        format_bytes(snapshot.memory_used_bytes),
        format_bytes(snapshot.memory_total_bytes)
    );
    if used_percent > 90.0 {
        Check::new(ID, TITLE, Category::Health, Status::Warning, detail)
            .recommend("Close apps or browser tabs you aren't using to speed things up.")
    } else {
        Check::new(ID, TITLE, Category::Health, Status::Good, detail)
    }
}

fn swap_check(snapshot: &HealthSnapshot) -> Check {
    const ID: &str = "swap";
    const TITLE: &str = "Swap usage";
    let used_percent = percent(snapshot.swap_used_bytes, snapshot.swap_total_bytes);
    let detail = format!(
        "{used_percent:.0}% of swap space is in use ({} of {}).",
        format_bytes(snapshot.swap_used_bytes),
        format_bytes(snapshot.swap_total_bytes)
    );
    if used_percent > 80.0 {
        Check::new(ID, TITLE, Category::Health, Status::Warning, detail).recommend(
            "Your computer is short on memory and is using the much slower disk instead. Close \
             unused apps or consider adding memory.",
        )
    } else {
        Check::new(ID, TITLE, Category::Health, Status::Good, detail)
    }
}

fn cpu_check(snapshot: &HealthSnapshot) -> Check {
    const ID: &str = "cpu_load";
    const TITLE: &str = "Processor load";
    let usage = snapshot.cpu_usage_percent;
    if usage > 90.0 {
        let busiest = snapshot
            .top_processes
            .first()
            .map(|process| format!(" The busiest app is {}.", process.name))
            .unwrap_or_default();
        Check::new(
            ID,
            TITLE,
            Category::Health,
            Status::Warning,
            format!("The processor is {usage:.0}% busy.{busiest}"),
        )
        .recommend("If this lasts, close or restart the app using the most CPU.")
    } else {
        Check::new(
            ID,
            TITLE,
            Category::Health,
            Status::Good,
            format!("The processor is {usage:.0}% busy."),
        )
    }
}

fn uptime_check(uptime_seconds: u64) -> Check {
    const ID: &str = "uptime";
    const TITLE: &str = "Time since restart";
    let days = uptime_seconds / (24 * 60 * 60);
    let hours = uptime_seconds / (60 * 60);
    let since = match days {
        0 if hours == 1 => "1 hour".to_string(),
        0 => format!("{hours} hours"),
        1 => "1 day".to_string(),
        days => format!("{days} days"),
    };
    if days > 14 {
        Check::new(
            ID,
            TITLE,
            Category::Health,
            Status::Warning,
            format!("This computer hasn't restarted in {since}."),
        )
        .recommend("Restart to apply pending updates and clear out slowdowns.")
    } else {
        Check::new(
            ID,
            TITLE,
            Category::Health,
            Status::Good,
            format!("Last restarted {since} ago."),
        )
    }
}

fn temperature_check(temperatures: &[(String, f32)]) -> Check {
    const ID: &str = "temperature";
    const TITLE: &str = "Temperature";
    const HOT_CELSIUS: f32 = 90.0;
    let hottest = temperatures
        .iter()
        .max_by(|left, right| left.1.total_cmp(&right.1));
    match hottest {
        Some((label, temperature)) if *temperature >= HOT_CELSIUS => Check::new(
            ID,
            TITLE,
            Category::Hardware,
            Status::Warning,
            format!("{label} is running hot at {temperature:.0}°C."),
        )
        .recommend(
            "Make sure the vents aren't blocked and clean out dust. Heavy apps can also cause \
             this.",
        ),
        Some((label, temperature)) => Check::new(
            ID,
            TITLE,
            Category::Hardware,
            Status::Good,
            format!("The hottest sensor ({label}) reads {temperature:.0}°C."),
        ),
        None => Check::unknown(ID, TITLE, Category::Hardware, "no temperature sensors"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIGABYTE: u64 = 1024 * 1024 * 1024;

    fn disk(mount_point: &str, total: u64, available: u64, is_removable: bool) -> DiskHealth {
        DiskHealth {
            name: mount_point.to_string(),
            mount_point: PathBuf::from(mount_point),
            total_bytes: total,
            available_bytes: available,
            is_removable,
        }
    }

    fn healthy_snapshot() -> HealthSnapshot {
        HealthSnapshot {
            cpu_usage_percent: 12.0,
            cpu_name: "Test CPU".into(),
            cpu_cores: 8,
            memory_used_bytes: 4 * GIGABYTE,
            memory_total_bytes: 16 * GIGABYTE,
            swap_used_bytes: 0,
            swap_total_bytes: 2 * GIGABYTE,
            disks: vec![disk("/", 500 * GIGABYTE, 300 * GIGABYTE, false)],
            uptime_seconds: 3 * 60 * 60,
            os_name: "Linux".into(),
            os_version: "1".into(),
            host_name: "host".into(),
            top_processes: Vec::new(),
            temperatures: vec![("CPU".into(), 55.0)],
        }
    }

    fn status_of(checks: &[Check], id: &str) -> Option<Status> {
        checks
            .iter()
            .find(|check| check.id == id)
            .map(|check| check.status)
    }

    #[test]
    fn healthy_snapshot_is_all_good() {
        let checks = findings(&healthy_snapshot());
        assert_eq!(checks.len(), 6);
        assert!(checks.iter().all(|check| check.status == Status::Good));
        assert!(checks.iter().all(
            |check| check.category == Category::Health || check.category == Category::Hardware
        ));
    }

    #[test]
    fn disk_thresholds() {
        let mut snapshot = healthy_snapshot();
        snapshot.disks = vec![disk("/", 100 * GIGABYTE, 15 * GIGABYTE, false)];
        assert_eq!(
            status_of(&findings(&snapshot), "disk_space"),
            Some(Status::Warning)
        );

        snapshot
            .disks
            .push(disk("/home", 100 * GIGABYTE, 5 * GIGABYTE, false));
        let checks = findings(&snapshot);
        assert_eq!(status_of(&checks, "disk_space"), Some(Status::Bad));
        assert_eq!(checks[0].id, "disk_space");
        let detail = &checks[0].detail;
        assert!(detail.contains("/home is 95% full"), "{detail}");
        assert!(detail.contains("/ is 85% full"), "{detail}");

        snapshot.disks = vec![
            disk("/", 100 * GIGABYTE, 50 * GIGABYTE, false),
            disk("/media/usb", 16 * GIGABYTE, 0, true),
        ];
        assert_eq!(
            status_of(&findings(&snapshot), "disk_space"),
            Some(Status::Good)
        );

        snapshot.disks.clear();
        assert_eq!(
            status_of(&findings(&snapshot), "disk_space"),
            Some(Status::Unknown)
        );
    }

    #[test]
    fn memory_swap_cpu_uptime_and_temperature_thresholds() {
        let mut snapshot = healthy_snapshot();
        snapshot.memory_used_bytes = 15 * GIGABYTE;
        snapshot.swap_used_bytes = 2 * GIGABYTE;
        snapshot.cpu_usage_percent = 97.0;
        snapshot.top_processes = vec![ProcessUsage {
            name: "cargo".into(),
            pid: 42,
            cpu_percent: 80.0,
            memory_bytes: GIGABYTE,
        }];
        snapshot.uptime_seconds = 20 * 24 * 60 * 60;
        snapshot.temperatures.push(("GPU".into(), 95.0));

        let checks = findings(&snapshot);
        for id in ["memory", "swap", "cpu_load", "uptime", "temperature"] {
            assert_eq!(status_of(&checks, id), Some(Status::Warning), "{id}");
        }
        let cpu = checks.iter().find(|check| check.id == "cpu_load").unwrap();
        assert!(cpu.detail.contains("cargo"));
        let temperature = checks
            .iter()
            .find(|check| check.id == "temperature")
            .unwrap();
        assert!(temperature.detail.contains("GPU"));
    }

    #[test]
    fn optional_findings_are_skipped() {
        let mut snapshot = healthy_snapshot();
        snapshot.swap_total_bytes = 0;
        snapshot.temperatures.clear();
        let checks = findings(&snapshot);
        assert_eq!(status_of(&checks, "swap"), None);
        assert_eq!(status_of(&checks, "temperature"), None);
    }

    #[test]
    fn top_processes_sorted_by_cpu_then_memory() {
        let process = |pid: u32, cpu_percent: f32, memory_bytes: u64| ProcessUsage {
            name: format!("process {pid}"),
            pid,
            cpu_percent,
            memory_bytes,
        };
        let processes = (1..=12)
            .map(|pid| process(pid, 0.0, u64::from(pid)))
            .chain([
                process(100, 50.0, 1),
                process(101, 50.0, 10),
                process(102, 5.0, 1),
            ]);
        let top = top_processes(processes.collect());
        let pids: Vec<u32> = top.iter().map(|process| process.pid).collect();
        assert_eq!(pids, vec![101, 100, 102, 12, 11, 10, 9, 8]);
    }

    #[test]
    fn relevant_disks_filters_pseudo_file_systems() {
        let disks = relevant_disks(
            [
                (disk("/", 100, 50, false), "ext4".to_string()),
                (disk("/snap/core/1", 100, 0, false), "squashfs".to_string()),
                (disk("/snap/other", 100, 0, false), "ext4".to_string()),
                (disk("/run", 100, 100, false), "tmpfs".to_string()),
                (disk("/", 100, 50, false), "ext4".to_string()),
                (disk("/empty", 0, 0, false), "ext4".to_string()),
                (disk("C:\\", 100, 10, false), "NTFS".to_string()),
            ]
            .into_iter(),
        );
        let mounts: Vec<PathBuf> = disks.into_iter().map(|disk| disk.mount_point).collect();
        assert_eq!(mounts, vec![PathBuf::from("/"), PathBuf::from("C:\\")]);
    }

    #[test]
    fn snapshot_reads_this_machine() {
        let snapshot = snapshot();
        assert!(snapshot.cpu_cores >= 1);
        assert!(snapshot.memory_total_bytes > 0);
        assert!(snapshot.top_processes.len() <= TOP_PROCESS_COUNT);
        assert!(!findings(&snapshot).is_empty());
    }
}
