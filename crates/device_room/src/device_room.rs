//! The device room: this computer's security and health, the files it holds
//! twice, what starts with it, and an ad blocker that covers every app.

use std::collections::HashSet;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Context as _;
use fs::{Fs, RemoveOptions};
use futures::AsyncReadExt as _;
use gpui::{
    Action, App, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, Pixels,
    PromptLevel, SharedString, Task, WeakEntity, Window, actions, canvas, fill, outline, point,
    px, size,
};
use http_client::{AsyncBody, HttpClient, Method, Request};
use noah_device::adblock;
use noah_device::checks::{Category, Check, ListeningPort, Status};
use noah_device::duplicates::{DuplicateGroup, ScanProgress};
use noah_device::geo::{self, Country};
use noah_device::health::HealthSnapshot;
use noah_device::intel::{
    self, AppInventory, BluetoothReport, DataFlow, DestinationKind, LocationReport, Neighbour,
    TrafficReport, Watcher, WifiReport,
};
use noah_device::startup::StartupItem;
use ui::{Divider, Tooltip, prelude::*};
use util::ResultExt as _;
use workspace::{
    Workspace,
    dock::{DockPosition, Panel, PanelEvent},
};

actions!(
    device_room,
    [
        /// Opens the device room: this computer's security and health.
        ToggleFocus,
    ]
);

const HEALTH_INTERVAL: Duration = Duration::from_secs(5);
/// How often the live connections are read again while the room is open.
const TRAFFIC_INTERVAL: Duration = Duration::from_secs(20);
/// Two columns need this much window; narrower stacks everything.
const TWO_COLUMN_WIDTH: Pixels = px(1040.);
const FLOWS_SHOWN: usize = 12;
const APPS_SHOWN: usize = 30;
const NEARBY_SHOWN: usize = 10;
const WATCHERS_SHOWN: usize = 10;
/// The only outbound request the room ever makes, and only when asked.
const PUBLIC_ADDRESS_URL: &str = "https://api64.ipify.org";
/// Files smaller than this are skipped when looking for duplicates; tiny
/// copies free almost nothing and there are very many of them.
const DUPLICATE_MIN_SIZE: u64 = 64 * 1024;
const SHOWN_DUPLICATE_GROUPS: usize = 25;

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
            workspace.toggle_panel_focus::<DevicePanel>(window, cx);
        });
    })
    .detach();
}

enum DuplicateScan {
    Idle,
    Scanning {
        progress: ScanProgress,
        cancel: Arc<AtomicBool>,
        _task: Task<()>,
    },
    Done(Vec<DuplicateGroup>),
}

enum AdBlocker {
    Checking,
    Off,
    On { blocked_domains: usize },
    Working(SharedString),
}

/// Everything the intelligence desk shows, gathered together off the
/// foreground thread.
struct Intel {
    traffic: TrafficReport,
    apps: AppInventory,
    wifi: WifiReport,
    neighbours: Vec<Neighbour>,
    bluetooth: BluetoothReport,
    location: LocationReport,
    startup: Vec<StartupItem>,
    ports: Vec<ListeningPort>,
    watchers: Vec<Watcher>,
}

impl Intel {
    fn gather() -> Self {
        // Each source is its own set of commands; together they take a
        // while, so they run side by side.
        let (traffic, apps, wifi, bluetooth, location, startup, ports) =
            std::thread::scope(|scope| {
                let traffic = scope.spawn(intel::traffic_report);
                let apps = scope.spawn(intel::installed_apps);
                let wifi = scope.spawn(intel::wifi_report);
                let bluetooth = scope.spawn(intel::bluetooth_report);
                let location = scope.spawn(intel::location_report);
                let startup = scope.spawn(noah_device::startup::startup_items);
                let ports = scope.spawn(noah_device::checks::listening_ports);
                (
                    traffic.join().unwrap_or_default(),
                    apps.join().unwrap_or_default(),
                    wifi.join().unwrap_or_default(),
                    bluetooth.join().unwrap_or_default(),
                    location.join().unwrap_or_default(),
                    startup.join().unwrap_or_default(),
                    ports.join().unwrap_or_default(),
                )
            });
        let neighbours = intel::neighbours(wifi.gateway);
        let watchers = intel::watchers(&traffic, &startup, &ports, &bluetooth, &wifi);
        Self {
            traffic,
            apps,
            wifi,
            neighbours,
            bluetooth,
            location,
            startup,
            ports,
            watchers,
        }
    }

    fn refresh_traffic(&mut self, traffic: TrafficReport) {
        self.traffic = traffic;
        self.watchers = intel::watchers(
            &self.traffic,
            &self.startup,
            &self.ports,
            &self.bluetooth,
            &self.wifi,
        );
    }
}

enum PublicAddress {
    Unknown,
    Looking,
    Known {
        address: IpAddr,
        country: Option<Country>,
    },
    Failed(SharedString),
}

pub struct DevicePanel {
    focus_handle: FocusHandle,
    fs: Arc<dyn Fs>,
    http_client: Arc<dyn HttpClient>,
    position: DockPosition,
    zoomed: bool,
    active: bool,
    health: Option<HealthSnapshot>,
    health_findings: Vec<Check>,
    checks: Option<Vec<Check>>,
    ports: Vec<ListeningPort>,
    startup: Vec<StartupItem>,
    duplicates: DuplicateScan,
    ad_blocker: AdBlocker,
    message: Option<SharedString>,
    show_all_startup: bool,
    intel: Option<Intel>,
    expanded_flows: HashSet<String>,
    show_all_flows: bool,
    show_all_apps: bool,
    show_all_watchers: bool,
    public_address: PublicAddress,
    _health_task: Option<Task<()>>,
    _checks_task: Option<Task<()>>,
    _intel_task: Option<Task<()>>,
    _traffic_task: Option<Task<()>>,
    _public_address_task: Option<Task<()>>,
}

impl DevicePanel {
    pub async fn load(
        workspace: WeakEntity<Workspace>,
        mut cx: gpui::AsyncWindowContext,
    ) -> anyhow::Result<Entity<Self>> {
        workspace.update_in(&mut cx, |workspace, _window, cx| {
            let fs = workspace.app_state().fs.clone();
            let http_client = cx.http_client();
            cx.new(|cx| Self {
                focus_handle: cx.focus_handle(),
                fs,
                http_client,
                position: DockPosition::Right,
                zoomed: false,
                active: false,
                health: None,
                health_findings: Vec::new(),
                checks: None,
                ports: Vec::new(),
                startup: Vec::new(),
                duplicates: DuplicateScan::Idle,
                ad_blocker: AdBlocker::Checking,
                message: None,
                show_all_startup: false,
                intel: None,
                expanded_flows: HashSet::new(),
                show_all_flows: false,
                show_all_apps: false,
                show_all_watchers: false,
                public_address: PublicAddress::Unknown,
                _health_task: None,
                _checks_task: None,
                _intel_task: None,
                _traffic_task: None,
                _public_address_task: None,
            })
        })
    }

    /// Samples health every few seconds while the room is open, and stops when
    /// it's closed so noah doesn't spend CPU watching the CPU.
    fn start_health_monitor(&mut self, cx: &mut Context<Self>) {
        self._health_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let snapshot = cx
                    .background_spawn(async { noah_device::health::snapshot() })
                    .await;
                let findings = noah_device::health::findings(&snapshot);
                let still_open = this
                    .update(cx, |this, cx| {
                        this.health = Some(snapshot);
                        this.health_findings = findings;
                        cx.notify();
                        this.active
                    })
                    .unwrap_or(false);
                if !still_open {
                    break;
                }
                cx.background_executor().timer(HEALTH_INTERVAL).await;
            }
        }));
    }

    /// Gathers the intelligence desk in the background and, while the room
    /// stays open, reads the live connections again every so often.
    fn gather_intel(&mut self, cx: &mut Context<Self>) {
        self._intel_task = Some(cx.spawn(async move |this, cx| {
            let intel = cx.background_spawn(async { Intel::gather() }).await;
            this.update(cx, |this, cx| {
                this.intel = Some(intel);
                cx.notify();
                this.start_traffic_monitor(cx);
            })
            .log_err();
        }));
    }

    fn start_traffic_monitor(&mut self, cx: &mut Context<Self>) {
        self._traffic_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TRAFFIC_INTERVAL).await;
                let still_open = this
                    .read_with(cx, |this, _| this.active)
                    .unwrap_or(false);
                if !still_open {
                    break;
                }
                let traffic = cx.background_spawn(async { intel::traffic_report() }).await;
                let updated = this
                    .update(cx, |this, cx| {
                        if let Some(intel) = this.intel.as_mut() {
                            intel.refresh_traffic(traffic);
                            cx.notify();
                        }
                    })
                    .is_ok();
                if !updated {
                    break;
                }
            }
        }));
    }

    /// Asks one address service what this device looks like from outside.
    /// This is the room's only request to the internet, and it only happens
    /// on this click.
    fn find_public_address(&mut self, cx: &mut Context<Self>) {
        self.public_address = PublicAddress::Looking;
        cx.notify();
        let http_client = self.http_client.clone();
        self._public_address_task = Some(cx.spawn(async move |this, cx| {
            let result = download_text(http_client.as_ref(), PUBLIC_ADDRESS_URL).await;
            let outcome = match result {
                Ok(text) => match text.trim().parse::<IpAddr>() {
                    Ok(address) => PublicAddress::Known {
                        address,
                        country: geo::country_of(address),
                    },
                    Err(_) => PublicAddress::Failed("the address service gave an odd answer".into()),
                },
                Err(error) => PublicAddress::Failed(format!("{error:#}").into()),
            };
            this.update(cx, |this, cx| {
                this.public_address = outcome;
                cx.notify();
            })
            .log_err();
        }));
    }

    fn run_checks(&mut self, cx: &mut Context<Self>) {
        self.checks = None;
        self.message = None;
        self.intel = None;
        self.gather_intel(cx);
        self.refresh_ad_blocker(cx);
        self._checks_task = Some(cx.spawn(async move |this, cx| {
            let (checks, ports, startup) = cx
                .background_spawn(async {
                    (
                        noah_device::checks::run_security_checks(),
                        noah_device::checks::listening_ports(),
                        noah_device::startup::startup_items(),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                this.checks = Some(checks);
                this.ports = ports;
                this.startup = startup;
                cx.notify();
            })
            .log_err();
        }));
        cx.notify();
    }

    fn refresh_ad_blocker(&mut self, cx: &mut Context<Self>) {
        let fs = self.fs.clone();
        cx.spawn(async move |this, cx| {
            let hosts = fs.load(&adblock::hosts_path()).await.unwrap_or_default();
            let blocked_domains = adblock::blocked_domain_count(&hosts);
            this.update(cx, |this, cx| {
                this.ad_blocker = if blocked_domains == 0 {
                    AdBlocker::Off
                } else {
                    AdBlocker::On { blocked_domains }
                };
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    /// Blocks ad and tracker domains for every app on the device by pointing
    /// them nowhere in the hosts file. Changing that file needs administrator
    /// approval, which the system asks for.
    fn set_ad_blocker(&mut self, enable: bool, cx: &mut Context<Self>) {
        let fs = self.fs.clone();
        let http_client = self.http_client.clone();
        self.ad_blocker = AdBlocker::Working(
            if enable {
                "downloading the block list…"
            } else {
                "turning the ad blocker off…"
            }
            .into(),
        );
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = async {
                let hosts = fs
                    .load(&adblock::hosts_path())
                    .await
                    .context("couldn't read the hosts file")?;
                let updated = if enable {
                    let list = download_text(http_client.as_ref(), adblock::BLOCKLIST_URL).await?;
                    let domains = adblock::parse_blocklist(&list);
                    anyhow::ensure!(!domains.is_empty(), "the downloaded block list was empty");
                    this.update(cx, |this, cx| {
                        this.ad_blocker = AdBlocker::Working(
                            format!(
                                "blocking {} domains; approve the administrator prompt…",
                                domains.len()
                            )
                            .into(),
                        );
                        cx.notify();
                    })?;
                    adblock::with_blocklist(&hosts, &domains)
                } else {
                    adblock::without_blocklist(&hosts)
                };
                cx.background_spawn(async move { adblock::write_hosts_elevated(&updated) })
                    .await
            }
            .await;
            this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.message = Some(format!("ad blocker: {error:#}").into());
                }
                this.refresh_ad_blocker(cx);
            })
            .log_err();
        })
        .detach();
    }

    fn scan_duplicates(&mut self, cx: &mut Context<Self>) {
        let cancel = Arc::new(AtomicBool::new(false));
        let (progress_sender, mut progress_receiver) =
            futures::channel::mpsc::unbounded::<ScanProgress>();
        let scan = cx.background_spawn({
            let cancel = cancel.clone();
            async move {
                let roots = noah_device::duplicates::default_scan_roots();
                let mut report = |progress: ScanProgress| {
                    progress_sender.unbounded_send(progress).ok();
                };
                noah_device::duplicates::find_duplicates(
                    &roots,
                    DUPLICATE_MIN_SIZE,
                    &cancel,
                    &mut report,
                )
            }
        });
        let task = cx.spawn(async move |this, cx| {
            let progress_updates = cx.spawn({
                let this = this.clone();
                async move |cx| {
                    use futures::StreamExt as _;
                    while let Some(progress) = progress_receiver.next().await {
                        let updated = this.update(cx, |this, cx| {
                            if let DuplicateScan::Scanning {
                                progress: current, ..
                            } = &mut this.duplicates
                            {
                                *current = progress;
                                cx.notify();
                            }
                        });
                        if updated.is_err() {
                            break;
                        }
                    }
                }
            });
            let result = scan.await;
            drop(progress_updates);
            this.update(cx, |this, cx| {
                match result {
                    Ok(groups) => this.duplicates = DuplicateScan::Done(groups),
                    Err(error) => {
                        this.duplicates = DuplicateScan::Idle;
                        this.message = Some(format!("duplicate scan: {error:#}").into());
                    }
                }
                cx.notify();
            })
            .log_err();
        });
        self.duplicates = DuplicateScan::Scanning {
            progress: ScanProgress {
                files_seen: 0,
                bytes_hashed: 0,
            },
            cancel,
            _task: task,
        };
        cx.notify();
    }

    fn stop_scan(&mut self, cx: &mut Context<Self>) {
        if let DuplicateScan::Scanning { cancel, .. } = &self.duplicates {
            cancel.store(true, Ordering::Relaxed);
        }
        cx.notify();
    }

    /// Moves the older copies to the system trash after the person confirms,
    /// keeping the newest copy of each file. Nothing is deleted outright, so
    /// a mistake can be restored from the trash.
    fn remove_older_copies(
        &mut self,
        group_indices: Vec<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let DuplicateScan::Done(groups) = &self.duplicates else {
            return;
        };
        let paths: Vec<PathBuf> = group_indices
            .iter()
            .filter_map(|index| groups.get(*index))
            .flat_map(|group| group.older().iter().map(|file| file.path.clone()))
            .collect();
        if paths.is_empty() {
            return;
        }
        let bytes: u64 = group_indices
            .iter()
            .filter_map(|index| groups.get(*index))
            .map(DuplicateGroup::reclaimable_bytes)
            .sum();
        let detail = format!(
            "{} older {} ({}) will be moved to the trash. The newest copy of each file is kept.",
            paths.len(),
            if paths.len() == 1 { "copy" } else { "copies" },
            format_bytes(bytes)
        );
        let answer = window.prompt(
            PromptLevel::Warning,
            "Move older copies to the trash?",
            Some(&detail),
            &["Move to Trash", "Cancel"],
            cx,
        );
        let fs = self.fs.clone();
        cx.spawn(async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            let mut failed = Vec::new();
            for path in &paths {
                if let Err(error) = fs
                    .trash(
                        path,
                        RemoveOptions {
                            recursive: false,
                            ignore_if_not_exists: true,
                        },
                    )
                    .await
                {
                    failed.push(format!("{}: {error:#}", path.display()));
                }
            }
            this.update(cx, |this, cx| {
                if let DuplicateScan::Done(groups) = &mut this.duplicates {
                    let mut sorted = group_indices.clone();
                    sorted.sort_unstable_by(|a, b| b.cmp(a));
                    for index in sorted {
                        if index < groups.len() {
                            groups.remove(index);
                        }
                    }
                }
                this.message = Some(if failed.is_empty() {
                    format!("moved {} files to the trash", paths.len()).into()
                } else {
                    format!(
                        "moved {} files to the trash; {} couldn't be moved: {}",
                        paths.len() - failed.len(),
                        failed.len(),
                        failed.join("; ")
                    )
                    .into()
                });
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    fn render_desk_summary(&self, cx: &Context<Self>) -> impl IntoElement {
        let border = cx.theme().colors().border_variant;
        let Some(intel) = &self.intel else {
            return v_flex()
                .gap_1()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(border)
                .child(Label::new("reading this device…").color(Color::Muted))
                .child(muted(
                    "connections, programs, wifi, bluetooth and what watches. Nothing leaves the device.",
                ));
        };
        let programs = intel.traffic.flows.len();
        let countries = intel.traffic.countries.len();
        let watching: Vec<&Watcher> = intel
            .watchers
            .iter()
            .filter(|watcher| watcher.status <= Status::Warning)
            .collect();
        let headline = if programs == 0 {
            "nothing on this device is talking to the internet right now".to_string()
        } else {
            format!(
                "{programs} {} talking to {} across {countries} {}",
                plural(programs, "program is", "programs are"),
                format!(
                    "{} {}",
                    intel.traffic.total_connections,
                    plural(intel.traffic.total_connections, "place", "places")
                ),
                plural(countries, "country", "countries")
            )
        };
        let (watch_line, watch_color) = if watching.is_empty() {
            ("no tracker, telemetry or remote-control traffic seen".to_string(), Color::Success)
        } else {
            (
                format!(
                    "{} {} worth a look",
                    watching.len(),
                    plural(watching.len(), "thing is", "things are")
                ),
                Color::Warning,
            )
        };
        v_flex()
            .gap_1()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(border)
            .child(Label::new(headline))
            .child(Label::new(watch_line).size(LabelSize::Small).color(watch_color))
            .when_some(intel.traffic.note.clone(), |this, note| this.child(muted(note)))
    }

    fn render_watchers(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut section = section("who is watching").child(muted(
            "programs reporting to trackers or telemetry, remote-control tools, doors open to the \
             network, autostarts from odd places, and trackers within reach.",
        ));
        let Some(intel) = &self.intel else {
            return section.child(muted("looking…"));
        };
        if intel.watchers.is_empty() {
            return section.child(
                Label::new("nothing is watching this device that noah can see")
                    .size(LabelSize::Small)
                    .color(Color::Success),
            );
        }
        let shown = if self.show_all_watchers {
            intel.watchers.len()
        } else {
            WATCHERS_SHOWN
        };
        for watcher in intel.watchers.iter().take(shown) {
            section = section.child(render_finding(watcher.status, &watcher.title, &watcher.detail));
        }
        if intel.watchers.len() > WATCHERS_SHOWN {
            section = section.child(
                Button::new(
                    "device-watchers-toggle",
                    if self.show_all_watchers {
                        "show fewer".to_string()
                    } else {
                        format!("show all {}", intel.watchers.len())
                    },
                )
                .label_size(LabelSize::Small)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.show_all_watchers = !this.show_all_watchers;
                    cx.notify();
                })),
            );
        }
        section
    }

    /// The world, with a dot where this device's data goes and a ring where
    /// the device is, once it has asked.
    fn render_world_map(&self, cx: &Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let status = cx.theme().status();
        let land = colors.text_muted.opacity(0.28);
        let accent = colors.text_accent;
        let warning = status.warning;
        let here_color = colors.text;
        let destinations: Vec<(f32, f32, usize, bool)> = self
            .intel
            .as_ref()
            .map(|intel| {
                intel
                    .traffic
                    .countries
                    .iter()
                    .filter_map(|(country, count)| {
                        let (latitude, longitude) = country.centroid?;
                        let watched = intel.traffic.flows.iter().any(|flow| {
                            flow.destinations.iter().any(|destination| {
                                destination.kind.is_watching()
                                    && destination.country.is_some_and(|c| c.code == country.code)
                            })
                        });
                        Some((latitude, longitude, *count, watched))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let here = match &self.public_address {
            PublicAddress::Known {
                country: Some(country),
                ..
            } => country.centroid,
            _ => None,
        };
        let most = destinations
            .iter()
            .map(|(_, _, count, _)| *count)
            .max()
            .unwrap_or(1)
            .max(1) as f32;
        canvas(
            |_, _, _| (),
            move |bounds: Bounds<Pixels>, _, window, _| {
                // The map keeps a 2:1 shape inside whatever it is given.
                let width = bounds.size.width.min(bounds.size.height * 2.);
                let height = width / 2.;
                let origin = point(
                    bounds.origin.x + (bounds.size.width - width) / 2.,
                    bounds.origin.y + (bounds.size.height - height) / 2.,
                );
                let cell_width = width / geo::WORLD_DOTS_COLUMNS as f32;
                let cell_height = height / geo::WORLD_DOTS_ROWS as f32;
                let dot = cell_width.min(cell_height) * 0.55;
                for (row, line) in geo::WORLD_DOTS.lines().enumerate() {
                    for (column, cell) in line.bytes().enumerate() {
                        if cell != b'#' {
                            continue;
                        }
                        let x = origin.x + cell_width * column as f32 + (cell_width - dot) / 2.;
                        let y = origin.y + cell_height * row as f32 + (cell_height - dot) / 2.;
                        window.paint_quad(
                            fill(Bounds::new(point(x, y), size(dot, dot)), land)
                                .corner_radii(dot / 2.),
                    );
                    }
                }
                let place = |latitude: f32, longitude: f32| {
                    point(
                        origin.x + width * ((longitude + 180.) / 360.),
                        origin.y + height * ((90. - latitude) / 180.),
                    )
                };
                for (latitude, longitude, count, watched) in &destinations {
                    let centre = place(*latitude, *longitude);
                    let radius = px(3.) + px(6.) * ((*count as f32).ln_1p() / most.ln_1p());
                    let color = if *watched { warning } else { accent };
                    window.paint_quad(
                        fill(
                            Bounds::new(
                                point(centre.x - radius, centre.y - radius),
                                size(radius * 2., radius * 2.),
                            ),
                            color.opacity(0.9),
                        )
                        .corner_radii(radius),
                    );
                }
                if let Some((latitude, longitude)) = here {
                    let centre = place(latitude, longitude);
                    let radius = px(7.);
                    window.paint_quad(
                        outline(
                            Bounds::new(
                                point(centre.x - radius, centre.y - radius),
                                size(radius * 2., radius * 2.),
                            ),
                            here_color,
                            gpui::BorderStyle::Solid,
                        )
                        .corner_radii(radius),
                    );
                }
            },
        )
        .w_full()
        .h(px(250.))
    }

    fn render_where_data_goes(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut section = section("where your data goes").child(muted(
            "every live connection, placed in the country its address was given to. Larger dots \
             mean more connections; amber means a tracker, telemetry or remote-control service.",
        ));
        section = section.child(self.render_world_map(cx));
        let Some(intel) = &self.intel else {
            return section.child(muted("reading connections…"));
        };
        if intel.traffic.countries.is_empty() {
            return section.child(muted("no connections outside this network right now"));
        }
        let mut legend = h_flex().flex_wrap().gap_x_3().gap_y_1();
        for (country, count) in intel.traffic.countries.iter().take(12) {
            legend = legend.child(
                h_flex()
                    .gap_1()
                    .child(
                        div()
                            .size(px(6.))
                            .rounded_full()
                            .bg(cx.theme().colors().text_accent),
                    )
                    .child(muted(format!("{} · {count}", country.name))),
            );
        }
        section = section.child(legend);
        section = section.child(self.render_public_address(cx));
        section
    }

    fn render_public_address(&self, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.public_address {
            PublicAddress::Unknown => h_flex()
                .gap_2()
                .items_center()
                .child(
                    Button::new("device-public-address", "show where this device appears to be")
                        .label_size(LabelSize::Small)
                        .on_click(cx.listener(|this, _, _, cx| this.find_public_address(cx))),
                )
                .child(muted("asks one address service; the room's only request out"))
                .into_any_element(),
            PublicAddress::Looking => muted("asking…").into_any_element(),
            PublicAddress::Known { address, country } => v_flex()
                .child(
                    Label::new(format!(
                        "from outside, this device is {address}{}",
                        country
                            .map(|country| format!(" in {}", country.name))
                            .unwrap_or_default()
                    ))
                    .size(LabelSize::Small),
                )
                .child(muted(
                    "that address is what every site and service sees, and it places the device \
                     in a city-sized area. A VPN changes it.",
                ))
                .into_any_element(),
            PublicAddress::Failed(reason) => v_flex()
                .child(muted(format!("couldn't ask: {reason}")))
                .child(
                    Button::new("device-public-address-retry", "try again")
                        .label_size(LabelSize::Small)
                        .on_click(cx.listener(|this, _, _, cx| this.find_public_address(cx))),
                )
                .into_any_element(),
        }
    }

    fn render_flows(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut section = section("programs talking to the internet").child(muted(
            "each program with a live connection, and who is on the other end.",
        ));
        let Some(intel) = &self.intel else {
            return section.child(muted("looking…"));
        };
        if intel.traffic.flows.is_empty() {
            return section.child(muted(
                intel
                    .traffic
                    .note
                    .clone()
                    .unwrap_or_else(|| "no program has a connection open right now".to_string()),
            ));
        }
        let shown = if self.show_all_flows {
            intel.traffic.flows.len()
        } else {
            FLOWS_SHOWN
        };
        for (index, flow) in intel.traffic.flows.iter().take(shown).enumerate() {
            section = section.child(self.render_flow(index, flow, cx));
        }
        if intel.traffic.flows.len() > FLOWS_SHOWN {
            section = section.child(
                Button::new(
                    "device-flows-toggle",
                    if self.show_all_flows {
                        "show fewer".to_string()
                    } else {
                        format!("show all {}", intel.traffic.flows.len())
                    },
                )
                .label_size(LabelSize::Small)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.show_all_flows = !this.show_all_flows;
                    cx.notify();
                })),
            );
        }
        section
    }

    fn render_flow(&self, index: usize, flow: &DataFlow, cx: &mut Context<Self>) -> impl IntoElement {
        let expanded = self.expanded_flows.contains(&flow.process);
        let watching = flow
            .destinations
            .iter()
            .filter(|destination| destination.kind.is_watching())
            .count();
        let countries = flow.countries();
        let country_text = match countries.len() {
            0 => String::new(),
            1..=3 => countries
                .iter()
                .map(|country| country.name)
                .collect::<Vec<_>>()
                .join(", "),
            count => format!("{count} countries"),
        };
        let process = flow.process.clone();
        let mut entry = v_flex().gap_0p5().py_1().child(
            h_flex()
                .id(("device-flow", index))
                .gap_2()
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.expanded_flows.remove(&process) {
                        this.expanded_flows.insert(process.clone());
                    }
                    cx.notify();
                }))
                .child(
                    Icon::new(if expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .size(IconSize::XSmall)
                    .color(Color::Muted),
                )
                .child(Label::new(flow.process.clone()).size(LabelSize::Small))
                .when(watching > 0, |this| {
                    this.child(
                        Label::new(format!(
                            "{watching} {}",
                            plural(watching, "watcher", "watchers")
                        ))
                        .size(LabelSize::XSmall)
                        .color(Color::Warning),
                    )
                })
                .child(div().flex_1())
                .child(muted(format!(
                    "{} {} · {} {}{}",
                    flow.connection_count(),
                    plural(flow.connection_count(), "connection", "connections"),
                    flow.destinations.len(),
                    plural(flow.destinations.len(), "place", "places"),
                    if country_text.is_empty() {
                        String::new()
                    } else {
                        format!(" · {country_text}")
                    }
                ))),
        );
        if expanded {
            if let Some(exe) = &flow.exe {
                entry = entry.child(muted(exe.display().to_string()));
            }
            for destination in &flow.destinations {
                let kind_color = match destination.kind {
                    DestinationKind::Advertising | DestinationKind::RemoteAccess => Color::Warning,
                    DestinationKind::Telemetry => Color::Warning,
                    _ => Color::Muted,
                };
                entry = entry.child(
                    h_flex()
                        .gap_2()
                        .pl_5()
                        .child(Label::new(destination.name()).size(LabelSize::XSmall))
                        .child(
                            Label::new(destination.kind.label())
                                .size(LabelSize::XSmall)
                                .color(kind_color),
                        )
                        .child(div().flex_1())
                        .child(muted(format!(
                            "{}:{}{} · {}",
                            destination.address,
                            destination.port,
                            destination
                                .country
                                .map(|country| format!(" · {}", country.name))
                                .unwrap_or_default(),
                            if destination.connections == 1 {
                                "1 connection".to_string()
                            } else {
                                format!("{} connections", destination.connections)
                            }
                        ))),
                );
            }
        }
        entry
    }

    fn render_wifi(&self) -> impl IntoElement {
        let mut section = section("wifi intelligence");
        let Some(intel) = &self.intel else {
            return section.child(muted("looking…"));
        };
        let wifi = &intel.wifi;
        if let Some(note) = &wifi.note {
            section = section.child(muted(note.clone()));
        }
        if let Some(link) = &wifi.connected {
            let mut details = Vec::new();
            if let Some(security) = &link.security {
                details.push(security.clone());
            }
            if let Some(channel) = &link.channel {
                details.push(format!("channel {channel}"));
            }
            if let Some(frequency) = &link.frequency {
                details.push(frequency.clone());
            }
            if let Some(signal) = &link.signal {
                details.push(format!("signal {signal}"));
            }
            if let Some(rate) = &link.rate {
                details.push(rate.clone());
            }
            section = section
                .child(
                    h_flex()
                        .gap_2()
                        .child(Label::new(format!("on \"{}\"", link.ssid)).size(LabelSize::Small))
                        .child(div().flex_1())
                        .child(muted(details.join(" · "))),
                )
                .when_some(link.bssid.clone(), |this, bssid| {
                    this.child(muted(format!("access point {bssid}")))
                });
        } else if wifi.note.is_none() {
            section = section.child(muted("not on wifi"));
        }
        let mut identity = Vec::new();
        if let Some(address) = wifi.local_address {
            identity.push(format!("this device {address}"));
        }
        if let Some(gateway) = wifi.gateway {
            identity.push(format!("router {gateway}"));
        }
        if let Some(mac) = &wifi.mac_address {
            identity.push(format!(
                "wifi address {mac}{}",
                match wifi.mac_randomized {
                    Some(true) => " (random)",
                    Some(false) => " (real hardware address)",
                    None => "",
                }
            ));
        }
        if !identity.is_empty() {
            section = section.child(muted(identity.join(" · ")));
        }
        if !wifi.dns_servers.is_empty() {
            let servers = wifi
                .dns_servers
                .iter()
                .map(|server| match intel::dns_owner(*server) {
                    Some(owner) => format!("{server} ({owner})"),
                    None => server.to_string(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            section = section.child(muted(format!("DNS {servers}")));
        }
        for finding in &wifi.findings {
            section = section.child(render_finding(finding.status, &finding.title, &finding.detail));
        }
        if !wifi.nearby.is_empty() {
            section = section.child(
                Label::new(format!("{} other networks in range", wifi.nearby.len()))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            );
            for network in wifi.nearby.iter().take(NEARBY_SHOWN) {
                section = section.child(
                    h_flex()
                        .gap_2()
                        .child(Label::new(network.ssid.clone()).size(LabelSize::XSmall))
                        .child(div().flex_1())
                        .child(muted(format!(
                            "{}{}{}",
                            network.security.clone().unwrap_or_default(),
                            network
                                .channel
                                .as_ref()
                                .map(|channel| format!(" · ch {channel}"))
                                .unwrap_or_default(),
                            network
                                .signal
                                .map(|signal| format!(" · {signal}"))
                                .unwrap_or_default()
                        ))),
                );
            }
        }
        section
    }

    fn render_neighbours(&self) -> impl IntoElement {
        let mut section = section("on your network").child(muted(
            "devices this machine has exchanged packets with lately. It shows who has been \
             talking, not everyone who is there.",
        ));
        let Some(intel) = &self.intel else {
            return section.child(muted("looking…"));
        };
        if intel.neighbours.is_empty() {
            return section.child(muted("no other device has spoken to this one recently"));
        }
        for neighbour in intel.neighbours.iter().take(24) {
            section = section.child(
                h_flex()
                    .gap_2()
                    .child(
                        Label::new(neighbour.vendor.unwrap_or("unknown maker").to_string())
                            .size(LabelSize::XSmall),
                    )
                    .when(neighbour.is_gateway, |this| {
                        this.child(
                            Label::new("router")
                                .size(LabelSize::XSmall)
                                .color(Color::Accent),
                        )
                    })
                    .child(div().flex_1())
                    .child(muted(format!("{} · {}", neighbour.address, neighbour.mac))),
            );
        }
        section
    }

    fn render_bluetooth(&self) -> impl IntoElement {
        let mut section = section("bluetooth");
        let Some(intel) = &self.intel else {
            return section.child(muted("looking…"));
        };
        let bluetooth = &intel.bluetooth;
        if let Some(note) = &bluetooth.note {
            return section.child(muted(note.clone()));
        }
        section = section.child(muted(match bluetooth.powered {
            Some(true) => "radio on",
            Some(false) => "radio off",
            None => "radio state unknown",
        }));
        for finding in &bluetooth.findings {
            section = section.child(render_finding(finding.status, &finding.title, &finding.detail));
        }
        if bluetooth.devices.is_empty() {
            section = section.child(muted("no devices known or in range"));
        }
        for device in bluetooth.devices.iter().take(20) {
            let state = match (device.connected, device.paired) {
                (true, _) => "connected",
                (false, true) => "paired",
                (false, false) => "seen",
            };
            section = section.child(
                h_flex()
                    .gap_2()
                    .child(Label::new(device.name.clone()).size(LabelSize::XSmall))
                    .when_some(device.kind.clone(), |this, kind| this.child(muted(kind)))
                    .child(div().flex_1())
                    .child(muted(format!(
                        "{state}{}",
                        device
                            .address
                            .as_ref()
                            .map(|address| format!(" · {address}"))
                            .unwrap_or_default()
                    ))),
            );
        }
        section
    }

    fn render_location(&self) -> impl IntoElement {
        let mut section = section("location and identity").child(muted(
            "what this device gives away about where and whose it is, without anyone asking.",
        ));
        let Some(intel) = &self.intel else {
            return section.child(muted("looking…"));
        };
        let location = &intel.location;
        let mut facts = Vec::new();
        if let Some(host_name) = &location.host_name {
            facts.push(format!("named {host_name}"));
        }
        if let Some(timezone) = &location.timezone {
            facts.push(format!("clock set to {timezone}"));
        }
        if let Some(locale) = &location.locale {
            facts.push(format!("language {locale}"));
        }
        if !facts.is_empty() {
            section = section.child(muted(facts.join(" · ")));
        }
        for finding in &location.findings {
            section = section.child(render_finding(finding.status, &finding.title, &finding.detail));
        }
        section
    }

    fn render_apps(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut section = section("programs on this device");
        let Some(intel) = &self.intel else {
            return section.child(muted("looking…"));
        };
        let inventory = &intel.apps;
        if let Some(note) = &inventory.note {
            section = section.child(muted(note.clone()));
        }
        let mut sources: Vec<(String, usize)> = Vec::new();
        for app in &inventory.apps {
            match sources.iter_mut().find(|(source, _)| *source == app.source) {
                Some((_, count)) => *count += 1,
                None => sources.push((app.source.clone(), 1)),
            }
        }
        let summary = sources
            .iter()
            .map(|(source, count)| format!("{count} from {source}"))
            .collect::<Vec<_>>()
            .join(" · ");
        section = section.child(muted(format!(
            "{} {}{}{}",
            inventory.apps.len(),
            plural(inventory.apps.len(), "program", "programs"),
            if summary.is_empty() {
                String::new()
            } else {
                format!(": {summary}")
            },
            inventory
                .system_packages
                .map(|count| format!(" · {count} system packages besides"))
                .unwrap_or_default()
        )));
        let shown = if self.show_all_apps {
            inventory.apps.len()
        } else {
            APPS_SHOWN
        };
        for app in inventory.apps.iter().take(shown) {
            let mut detail = Vec::new();
            if let Some(version) = &app.version {
                detail.push(version.clone());
            }
            if let Some(publisher) = &app.publisher {
                detail.push(publisher.clone());
            }
            if let Some(installed_on) = &app.installed_on {
                detail.push(format!("installed {installed_on}"));
            }
            section = section.child(
                h_flex()
                    .gap_2()
                    .child(Label::new(app.name.clone()).size(LabelSize::XSmall).truncate())
                    .child(div().flex_1())
                    .child(muted(detail.join(" · "))),
            );
        }
        if inventory.apps.len() > APPS_SHOWN {
            section = section.child(
                Button::new(
                    "device-apps-toggle",
                    if self.show_all_apps {
                        "show fewer".to_string()
                    } else {
                        format!("show all {}", inventory.apps.len())
                    },
                )
                .label_size(LabelSize::Small)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.show_all_apps = !this.show_all_apps;
                    cx.notify();
                })),
            );
        }
        section
    }

    fn all_findings(&self) -> Vec<&Check> {
        let mut findings: Vec<&Check> = self
            .checks
            .iter()
            .flatten()
            .chain(self.health_findings.iter())
            .collect();
        findings.sort_by_key(|check| check.status);
        findings
    }

    fn render_summary(&self, cx: &Context<Self>) -> impl IntoElement {
        let findings = self.all_findings();
        let count = |status: Status| findings.iter().filter(|check| check.status == status).count();
        let (bad, warning, good) = (count(Status::Bad), count(Status::Warning), count(Status::Good));
        let (headline, color) = if self.checks.is_none() {
            ("checking this device…".to_string(), Color::Muted)
        } else if bad > 0 {
            (
                format!(
                    "{bad} {}",
                    plural(bad, "problem needs attention", "problems need attention")
                ),
                Color::Error,
            )
        } else if warning > 0 {
            (
                format!(
                    "{warning} {}",
                    plural(warning, "warning is worth a look", "warnings are worth a look")
                ),
                Color::Warning,
            )
        } else {
            ("this device looks safe".to_string(), Color::Success)
        };
        let border = cx.theme().colors().border_variant;
        v_flex()
            .gap_1()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(border)
            .child(Label::new(headline).color(color))
            .child(
                Label::new(format!(
                    "{good} passed · {warning} warnings · {bad} problems"
                ))
                .size(LabelSize::Small)
                .color(Color::Muted),
            )
    }

    fn render_health(&self, cx: &Context<Self>) -> impl IntoElement {
        let section = section("health");
        let Some(health) = &self.health else {
            return section.child(muted("measuring…"));
        };
        let memory_percent = percent(health.memory_used_bytes, health.memory_total_bytes);
        let mut section = section
            .child(muted(format!(
                "{} · {} {} · {}",
                health.host_name, health.os_name, health.os_version, health.cpu_name
            )))
            .child(meter(
                "processor",
                health.cpu_usage_percent,
                format!("{:.0}% of {} cores", health.cpu_usage_percent, health.cpu_cores),
                cx,
            ))
            .child(meter(
                "memory",
                memory_percent,
                format!(
                    "{} of {}",
                    format_bytes(health.memory_used_bytes),
                    format_bytes(health.memory_total_bytes)
                ),
                cx,
            ));
        for disk in &health.disks {
            let used = disk.total_bytes.saturating_sub(disk.available_bytes);
            section = section.child(meter(
                format!("disk {}", disk.mount_point.display()),
                percent(used, disk.total_bytes),
                format!(
                    "{} free of {}",
                    format_bytes(disk.available_bytes),
                    format_bytes(disk.total_bytes)
                ),
                cx,
            ));
        }
        for (sensor, celsius) in health.temperatures.iter().take(4) {
            section = section.child(muted(format!("{sensor}: {celsius:.0} °C")));
        }
        section = section.child(muted(format!(
            "running for {}",
            format_duration(health.uptime_seconds)
        )));
        if !health.top_processes.is_empty() {
            section = section.child(
                Label::new("busiest programs")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            );
            for process in &health.top_processes {
                section = section.child(
                    h_flex()
                        .gap_2()
                        .child(Label::new(process.name.clone()).size(LabelSize::Small))
                        .child(div().flex_1())
                        .child(muted(format!(
                            "{:.0}% cpu · {}",
                            process.cpu_percent,
                            format_bytes(process.memory_bytes)
                        ))),
                );
            }
        }
        section
    }

    fn render_checks(&self) -> impl IntoElement {
        let section = section("security");
        if self.checks.is_none() {
            return section.child(muted("running checks…"));
        }
        let mut section = section;
        let categories = [
            Category::Protection,
            Category::Network,
            Category::Access,
            Category::Updates,
            Category::Encryption,
            Category::Hardware,
            Category::Privacy,
            Category::Health,
        ];
        for category in categories {
            let mut in_category: Vec<&Check> = self
                .all_findings()
                .into_iter()
                .filter(|check| check.category == category)
                .collect();
            if in_category.is_empty() {
                continue;
            }
            in_category.sort_by_key(|check| check.status);
            section = section.child(
                Label::new(category_name(category))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            );
            for check in in_category {
                section = section.child(render_check(check));
            }
        }
        section
    }

    fn render_ports(&self) -> impl IntoElement {
        let mut section = section("open to the network");
        if self.checks.is_none() {
            return section.child(muted("looking…"));
        }
        if self.ports.is_empty() {
            let unchecked = self.checks.iter().flatten().find(|check| {
                check.category == Category::Network && check.status == Status::Unknown
            });
            return section.child(muted(match unchecked {
                Some(check) => check.detail.clone(),
                None => "no program is accepting connections from other devices".to_string(),
            }));
        }
        for port in &self.ports {
            section = section.child(
                h_flex()
                    .gap_2()
                    .child(Label::new(format!("{} {}", port.protocol, port.port)).size(LabelSize::Small))
                    .child(muted(port.address.clone()))
                    .child(div().flex_1())
                    .child(muted(port.process.clone().unwrap_or_else(|| "unknown program".into()))),
            );
        }
        section
    }

    fn render_startup(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut section = section("starts with the device");
        if self.checks.is_none() {
            return section.child(muted("looking…"));
        }
        if self.startup.is_empty() {
            return section.child(muted("nothing found"));
        }
        let shown = if self.show_all_startup { self.startup.len() } else { 8 };
        for item in self.startup.iter().take(shown) {
            section = section.child(
                v_flex()
                    .child(Label::new(item.name.clone()).size(LabelSize::Small))
                    .child(muted(format!("{} · {}", item.location, item.command))),
            );
        }
        if self.startup.len() > 8 {
            section = section.child(
                Button::new(
                    "device-startup-toggle",
                    if self.show_all_startup {
                        "show fewer".to_string()
                    } else {
                        format!("show all {}", self.startup.len())
                    },
                )
                .label_size(LabelSize::Small)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.show_all_startup = !this.show_all_startup;
                    cx.notify();
                })),
            );
        }
        section
    }

    fn render_ad_blocker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let section = section("ad blocker").child(muted(
            "blocks ad and tracking domains for every app and browser on this device, using the \
             hosts file. Changing it asks for administrator approval.",
        ));
        match &self.ad_blocker {
            AdBlocker::Checking => section.child(muted("checking…")),
            AdBlocker::Working(status) => section.child(muted(status.clone())),
            AdBlocker::Off => section.child(
                Button::new("device-adblock-on", "block ads on this device")
                    .style(ButtonStyle::Filled)
                    .on_click(cx.listener(|this, _, _, cx| this.set_ad_blocker(true, cx))),
            ),
            AdBlocker::On { blocked_domains } => section
                .child(
                    Label::new(format!("on: {blocked_domains} domains blocked"))
                        .size(LabelSize::Small)
                        .color(Color::Success),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("device-adblock-update", "update the list")
                                .label_size(LabelSize::Small)
                                .on_click(cx.listener(|this, _, _, cx| this.set_ad_blocker(true, cx))),
                        )
                        .child(
                            Button::new("device-adblock-off", "turn off")
                                .label_size(LabelSize::Small)
                                .on_click(cx.listener(|this, _, _, cx| this.set_ad_blocker(false, cx))),
                        ),
                ),
        }
    }

    fn render_duplicates(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let section = section("duplicate files").child(muted(
            "finds files saved more than once in your downloads, documents, desktop, pictures, \
             videos and music. The newest copy is kept; older copies go to the trash.",
        ));
        match &self.duplicates {
            DuplicateScan::Idle => section.child(
                Button::new("device-duplicates-scan", "find duplicate files")
                    .style(ButtonStyle::Filled)
                    .on_click(cx.listener(|this, _, _, cx| this.scan_duplicates(cx))),
            ),
            DuplicateScan::Scanning { progress, .. } => section.child(
                h_flex()
                    .gap_2()
                    .child(muted(format!(
                        "looked at {} files, compared {}",
                        progress.files_seen,
                        format_bytes(progress.bytes_hashed)
                    )))
                    .child(div().flex_1())
                    .child(
                        Button::new("device-duplicates-stop", "stop")
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(|this, _, _, cx| this.stop_scan(cx))),
                    ),
            ),
            DuplicateScan::Done(groups) if groups.is_empty() => section
                .child(muted("no duplicate files found"))
                .child(
                    Button::new("device-duplicates-rescan", "scan again")
                        .label_size(LabelSize::Small)
                        .on_click(cx.listener(|this, _, _, cx| this.scan_duplicates(cx))),
                ),
            DuplicateScan::Done(groups) => {
                let reclaimable: u64 = groups.iter().map(DuplicateGroup::reclaimable_bytes).sum();
                let copies: usize = groups.iter().map(|group| group.older().len()).sum();
                let mut section = section.child(
                    h_flex()
                        .gap_2()
                        .child(Label::new(format!(
                            "{copies} older {} · {} to free",
                            plural(copies, "copy", "copies"),
                            format_bytes(reclaimable)
                        )))
                        .child(div().flex_1())
                        .child(
                            Button::new("device-duplicates-remove-all", "remove all older copies")
                                .style(ButtonStyle::Filled)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let all = match &this.duplicates {
                                        DuplicateScan::Done(groups) => (0..groups.len()).collect(),
                                        _ => Vec::new(),
                                    };
                                    this.remove_older_copies(all, window, cx);
                                })),
                        )
                        .child(
                            IconButton::new("device-duplicates-rescan", IconName::RotateCw)
                                .icon_size(IconSize::Small)
                                .tooltip(Tooltip::text("scan again"))
                                .on_click(cx.listener(|this, _, _, cx| this.scan_duplicates(cx))),
                        ),
                );
                for (index, group) in groups.iter().enumerate().take(SHOWN_DUPLICATE_GROUPS) {
                    let keep = group.keep();
                    let mut entry = v_flex()
                        .gap_0p5()
                        .py_1()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Label::new(file_name(&keep.path))
                                        .size(LabelSize::Small)
                                        .truncate(),
                                )
                                .child(div().flex_1())
                                .child(muted(format!(
                                    "{} × {}",
                                    group.files.len(),
                                    format_bytes(group.size)
                                )))
                                .child(
                                    Button::new(("device-duplicate-remove", index), "keep newest")
                                        .label_size(LabelSize::Small)
                                        .tooltip(Tooltip::text("move the older copies to the trash"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.remove_older_copies(vec![index], window, cx);
                                        })),
                                ),
                        )
                        .child(
                            Label::new(format!("keep {}", keep.path.display()))
                                .size(LabelSize::XSmall)
                                .color(Color::Success),
                        );
                    for older in group.older() {
                        entry = entry.child(
                            Label::new(format!("remove {}", older.path.display()))
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        );
                    }
                    section = section.child(entry);
                }
                if groups.len() > SHOWN_DUPLICATE_GROUPS {
                    section = section.child(muted(format!(
                        "and {} more groups",
                        groups.len() - SHOWN_DUPLICATE_GROUPS
                    )));
                }
                section
            }
        }
    }
}

async fn download_text(http_client: &dyn HttpClient, url: &str) -> anyhow::Result<String> {
    let request = Request::builder()
        .method(Method::GET)
        .uri(url)
        .body(AsyncBody::default())?;
    let mut response = http_client
        .send(request)
        .await
        .with_context(|| format!("couldn't download {url}"))?;
    anyhow::ensure!(
        response.status().is_success(),
        "downloading {url} failed with status {}",
        response.status()
    );
    let mut body = String::new();
    response.body_mut().read_to_string(&mut body).await?;
    Ok(body)
}

fn status_icon(status: Status) -> (IconName, Color) {
    match status {
        Status::Bad => (IconName::XCircle, Color::Error),
        Status::Warning => (IconName::Warning, Color::Warning),
        Status::Unknown => (IconName::Info, Color::Muted),
        Status::Good => (IconName::Check, Color::Success),
    }
}

/// A finding from the intelligence desk, drawn like a check.
fn render_finding(status: Status, title: &str, detail: &str) -> impl IntoElement {
    let (icon, color) = status_icon(status);
    h_flex()
        .items_start()
        .gap_2()
        .py_0p5()
        .child(Icon::new(icon).size(IconSize::Small).color(color))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(Label::new(title.to_string()).size(LabelSize::Small))
                .when(!detail.is_empty(), |this| this.child(muted(detail.to_string()))),
        )
}

fn render_check(check: &Check) -> impl IntoElement {
    let (icon, color) = status_icon(check.status);
    h_flex()
        .items_start()
        .gap_2()
        .py_0p5()
        .child(Icon::new(icon).size(IconSize::Small).color(color))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(Label::new(check.title.clone()).size(LabelSize::Small))
                .when(!check.detail.is_empty(), |this| {
                    this.child(muted(check.detail.clone()))
                })
                .when_some(check.recommendation.clone(), |this, recommendation| {
                    this.child(
                        Label::new(recommendation)
                            .size(LabelSize::XSmall)
                            .color(Color::Accent),
                    )
                }),
        )
}

fn section(title: &'static str) -> gpui::Div {
    v_flex()
        .gap_1()
        .py_2()
        .child(Label::new(title).size(LabelSize::Small).color(Color::Default))
}

fn muted(text: impl Into<SharedString>) -> Label {
    Label::new(text).size(LabelSize::XSmall).color(Color::Muted)
}

fn meter(
    name: impl Into<SharedString>,
    percent_used: f32,
    detail: String,
    cx: &Context<DevicePanel>,
) -> impl IntoElement {
    let colors = cx.theme().colors();
    let status = cx.theme().status();
    let fill = if percent_used >= 90. {
        status.error
    } else if percent_used >= 75. {
        status.warning
    } else {
        colors.text_accent
    };
    v_flex()
        .gap_0p5()
        .child(
            h_flex()
                .child(Label::new(name.into()).size(LabelSize::Small))
                .child(div().flex_1())
                .child(muted(detail)),
        )
        .child(
            div()
                .h(px(4.))
                .w_full()
                .rounded_sm()
                .bg(colors.element_background)
                .child(
                    div()
                        .h_full()
                        .rounded_sm()
                        .bg(fill)
                        .w(gpui::relative(percent_used.clamp(0., 100.) / 100.)),
                ),
        )
}

fn category_name(category: Category) -> &'static str {
    match category {
        Category::Protection => "protection",
        Category::Network => "network",
        Category::Access => "access",
        Category::Updates => "updates",
        Category::Encryption => "encryption",
        Category::Hardware => "hardware",
        Category::Privacy => "privacy",
        Category::Health => "health",
    }
}

fn percent(used: u64, total: u64) -> f32 {
    if total == 0 {
        return 0.;
    }
    (used as f64 / total as f64 * 100.) as f32
}

fn plural(count: usize, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 { one } else { many }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024. && unit < UNITS.len() - 1 {
        value /= 1024.;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn format_duration(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = seconds % 86_400 / 3_600;
    let minutes = seconds % 3_600 / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

impl EventEmitter<PanelEvent> for DevicePanel {}

impl Focusable for DevicePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DevicePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let two_columns = window.viewport_size().width >= TWO_COLUMN_WIDTH;
        // The desk reads left to right: what is happening now, then what the
        // device is and has, then the housekeeping.
        let left = v_flex()
            .flex_1()
            .min_w_0()
            .gap_1()
            .child(self.render_desk_summary(cx))
            .child(self.render_where_data_goes(cx))
            .child(Divider::horizontal())
            .child(self.render_watchers(cx))
            .child(Divider::horizontal())
            .child(self.render_flows(cx))
            .child(Divider::horizontal())
            .child(self.render_wifi())
            .child(Divider::horizontal())
            .child(self.render_neighbours())
            .child(Divider::horizontal())
            .child(self.render_bluetooth())
            .child(Divider::horizontal())
            .child(self.render_location());
        let right = v_flex()
            .flex_1()
            .min_w_0()
            .gap_1()
            .child(self.render_summary(cx))
            .child(self.render_checks())
            .child(Divider::horizontal())
            .child(self.render_apps(cx))
            .child(Divider::horizontal())
            .child(self.render_health(cx))
            .child(Divider::horizontal())
            .child(self.render_ports())
            .child(Divider::horizontal())
            .child(self.render_startup(cx))
            .child(Divider::horizontal())
            .child(self.render_ad_blocker(cx))
            .child(Divider::horizontal())
            .child(self.render_duplicates(cx));
        v_flex()
            .id("device-room")
            .key_context("DevicePanel")
            .track_focus(&self.focus_handle)
            .size_full()
            .px_3()
            .pb_4()
            .overflow_y_scroll()
            .child(
                h_flex()
                    .pt_3()
                    .pb_2()
                    .child(Headline::new("device").size(HeadlineSize::XSmall))
                    .child(div().flex_1())
                    .child(
                        IconButton::new("device-refresh", IconName::RotateCw)
                            .icon_size(IconSize::Small)
                            .tooltip(Tooltip::text("check again"))
                            .on_click(cx.listener(|this, _, _, cx| this.run_checks(cx))),
                    ),
            )
            .when_some(self.message.clone(), |this, message| {
                this.child(Label::new(message).size(LabelSize::Small).color(Color::Accent))
            })
            .map(|this| {
                if two_columns {
                    this.child(h_flex().items_start().gap_6().child(left).child(right))
                } else {
                    this.child(left).child(Divider::horizontal()).child(right)
                }
            })
    }
}

impl Panel for DevicePanel {
    fn persistent_name() -> &'static str {
        "DevicePanel"
    }

    fn panel_key() -> &'static str {
        "DevicePanel"
    }

    fn position(&self, _window: &Window, _cx: &App) -> DockPosition {
        self.position
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        matches!(position, DockPosition::Left | DockPosition::Right)
    }

    fn set_position(&mut self, position: DockPosition, _: &mut Window, cx: &mut Context<Self>) {
        self.position = position;
        cx.notify();
    }

    fn default_size(&self, _window: &Window, _cx: &App) -> Pixels {
        px(480.)
    }

    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        Some(IconName::Lock)
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("device")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleFocus)
    }

    fn is_zoomed(&self, _window: &Window, _cx: &App) -> bool {
        self.zoomed
    }

    fn set_zoomed(&mut self, zoomed: bool, _window: &mut Window, cx: &mut Context<Self>) {
        self.zoomed = zoomed;
        cx.notify();
    }

    fn set_active(&mut self, active: bool, _window: &mut Window, cx: &mut Context<Self>) {
        let was_active = self.active;
        self.active = active;
        if active && !was_active {
            self.start_health_monitor(cx);
            if self.checks.is_none() {
                self.run_checks(cx);
            } else if self.intel.is_some() {
                self.start_traffic_monitor(cx);
            }
        } else if !active {
            self._health_task = None;
            self._traffic_task = None;
        }
    }

    fn activation_priority(&self) -> u32 {
        10
    }
}
