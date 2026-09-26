//! What a security officer would want to know about this device beyond a
//! list of pass/fail checks: which programs are talking to the internet and
//! where their data goes, what is installed, what the wifi and bluetooth
//! around it look like, who else is on the network, and what could be
//! watching. Everything here is answered from the device itself; nothing is
//! sent anywhere.
//!
//! Every function is blocking and meant for a background thread. A missing
//! tool or a refused permission yields an empty report with a `note` that
//! says so, never an error the room can't show.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::checks::{ListeningPort, Status};
use crate::geo::{self, Country};
use crate::startup::StartupItem;
use crate::{COMMAND_TIMEOUT, run_command};

/// Reverse lookups for the destinations are given this long in total.
const REVERSE_LOOKUP_BUDGET: Duration = Duration::from_secs(4);
const REVERSE_LOOKUP_THREADS: usize = 12;
const MOST_DESTINATIONS_SHOWN: usize = 400;

/// One socket this device has open to somewhere else.
#[derive(Clone, Debug)]
pub struct Connection {
    pub protocol: &'static str,
    pub local_address: IpAddr,
    pub local_port: u16,
    pub remote_address: IpAddr,
    pub remote_port: u16,
    pub state: String,
    pub pid: Option<u32>,
    pub process: Option<String>,
    pub exe: Option<PathBuf>,
}

/// What a destination is for, as far as its name gives it away.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DestinationKind {
    /// Advertising networks and audience measurement.
    Advertising,
    /// Analytics, crash reporting, product telemetry.
    Telemetry,
    /// Remote control of this device.
    RemoteAccess,
    /// Push notification and messaging relays.
    Messaging,
    /// Cloud platforms: anyone's service could be behind it.
    Cloud,
    /// Content delivery networks.
    Cdn,
    /// A named company's own service.
    Service,
    /// Nothing known about it.
    Unknown,
}

impl DestinationKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Advertising => "advertising",
            Self::Telemetry => "telemetry",
            Self::RemoteAccess => "remote access",
            Self::Messaging => "messaging",
            Self::Cloud => "cloud",
            Self::Cdn => "content delivery",
            Self::Service => "service",
            Self::Unknown => "unknown",
        }
    }

    /// Kinds worth a person's attention when their data flows there.
    pub fn is_watching(self) -> bool {
        matches!(
            self,
            Self::Advertising | Self::Telemetry | Self::RemoteAccess
        )
    }
}

#[derive(Clone, Debug)]
pub struct Destination {
    pub address: IpAddr,
    pub port: u16,
    pub host: Option<String>,
    pub country: Option<Country>,
    pub owner: Option<&'static str>,
    pub kind: DestinationKind,
    pub connections: usize,
}

impl Destination {
    /// The best short name for where this goes.
    pub fn name(&self) -> String {
        if let Some(owner) = self.owner {
            return owner.to_string();
        }
        if let Some(host) = &self.host {
            return host.clone();
        }
        self.address.to_string()
    }
}

/// One program and everywhere it is talking to.
#[derive(Clone, Debug)]
pub struct DataFlow {
    pub process: String,
    pub exe: Option<PathBuf>,
    pub pids: Vec<u32>,
    pub destinations: Vec<Destination>,
}

impl DataFlow {
    pub fn connection_count(&self) -> usize {
        self.destinations
            .iter()
            .map(|destination| destination.connections)
            .sum()
    }

    pub fn countries(&self) -> Vec<Country> {
        let mut seen = Vec::new();
        for destination in &self.destinations {
            if let Some(country) = destination.country
                && !seen.contains(&country)
            {
                seen.push(country);
            }
        }
        seen
    }
}

#[derive(Clone, Debug, Default)]
pub struct TrafficReport {
    pub flows: Vec<DataFlow>,
    /// Countries the device is talking to, most connections first.
    pub countries: Vec<(Country, usize)>,
    pub total_connections: usize,
    pub note: Option<String>,
}

/// Where this device's data is going right now, grouped by program.
pub fn traffic_report() -> TrafficReport {
    let (connections, note) = match platform::connections() {
        Ok(connections) => (connections, None),
        Err(error) => (Vec::new(), Some(format!("noah couldn't list connections: {error:#}"))),
    };
    let mut report = traffic_report_from(connections, &reverse_lookup_for);
    if report.note.is_none() {
        report.note = note;
    }
    report
}

fn traffic_report_from(
    connections: Vec<Connection>,
    resolve: &dyn Fn(&[IpAddr]) -> HashMap<IpAddr, String>,
) -> TrafficReport {
    let remote: Vec<Connection> = connections
        .into_iter()
        .filter(|connection| is_remote(connection.remote_address))
        .collect();
    let mut addresses: Vec<IpAddr> = remote
        .iter()
        .map(|connection| connection.remote_address)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    addresses.sort();
    addresses.truncate(MOST_DESTINATIONS_SHOWN);
    let hosts = resolve(&addresses);

    let mut by_process: BTreeMap<String, (Option<PathBuf>, Vec<u32>, BTreeMap<(IpAddr, u16), usize>)> =
        BTreeMap::new();
    let mut unknown_program = 0usize;
    for connection in &remote {
        let name = match &connection.process {
            Some(name) => name.clone(),
            None => {
                unknown_program += 1;
                "unknown program".to_string()
            }
        };
        let entry = by_process
            .entry(name)
            .or_insert_with(|| (connection.exe.clone(), Vec::new(), BTreeMap::new()));
        if entry.0.is_none() {
            entry.0 = connection.exe.clone();
        }
        if let Some(pid) = connection.pid
            && !entry.1.contains(&pid)
        {
            entry.1.push(pid);
        }
        *entry
            .2
            .entry((connection.remote_address, connection.remote_port))
            .or_default() += 1;
    }

    let mut flows: Vec<DataFlow> = by_process
        .into_iter()
        .map(|(process, (exe, pids, destinations))| {
            let mut destinations: Vec<Destination> = destinations
                .into_iter()
                .map(|((address, port), connections)| {
                    let host = hosts.get(&address).cloned();
                    let (owner, kind) = classify(host.as_deref(), port);
                    Destination {
                        address,
                        port,
                        host,
                        country: geo::country_of(address),
                        owner,
                        kind,
                        connections,
                    }
                })
                .collect();
            destinations.sort_by(|left, right| {
                left.kind
                    .cmp(&right.kind)
                    .then_with(|| right.connections.cmp(&left.connections))
            });
            DataFlow {
                process,
                exe,
                pids,
                destinations,
            }
        })
        .collect();
    flows.sort_by(|left, right| {
        let watching = |flow: &DataFlow| {
            flow.destinations
                .iter()
                .filter(|destination| destination.kind.is_watching())
                .count()
        };
        watching(right)
            .cmp(&watching(left))
            .then_with(|| right.connection_count().cmp(&left.connection_count()))
    });

    let mut per_country: HashMap<&'static str, (Country, usize)> = HashMap::new();
    for flow in &flows {
        for destination in &flow.destinations {
            if let Some(country) = destination.country {
                per_country
                    .entry(country.code)
                    .or_insert((country, 0))
                    .1 += destination.connections;
            }
        }
    }
    let mut countries: Vec<(Country, usize)> = per_country.into_values().collect();
    countries.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.name.cmp(right.0.name)));

    let note = (unknown_program > 0).then(|| {
        format!(
            "{unknown_program} {} belong to programs noah can't see into; run noah as an \
             administrator to name them",
            if unknown_program == 1 {
                "connection"
            } else {
                "connections"
            }
        )
    });

    TrafficReport {
        total_connections: remote.len(),
        flows,
        countries,
        note,
    }
}

/// An address on the internet rather than this machine or its own network.
pub fn is_remote(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            !(address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_unspecified()
                || address.is_broadcast()
                || address.is_multicast()
                // 100.64.0.0/10, carrier-grade NAT and Tailscale-style overlays.
                || (address.octets()[0] == 100 && (64..128).contains(&address.octets()[1])))
        }
        IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                return is_remote(IpAddr::V4(mapped));
            }
            let first = address.segments()[0];
            !(address.is_loopback()
                || address.is_unspecified()
                || address.is_multicast()
                || (first & 0xffc0) == 0xfe80
                || (first & 0xfe00) == 0xfc00)
        }
    }
}

/// Who a host name belongs to and what it is for, from its suffix. The list
/// is the well-known companies and the trackers people ask about; anything
/// else stays unknown rather than guessed.
pub fn classify(host: Option<&str>, port: u16) -> (Option<&'static str>, DestinationKind) {
    use DestinationKind::*;
    const OWNERS: &[(&str, &str, DestinationKind)] = &[
        // advertising and audience measurement
        ("doubleclick.net", "Google Ads", Advertising),
        ("googlesyndication.com", "Google Ads", Advertising),
        ("googleadservices.com", "Google Ads", Advertising),
        ("adnxs.com", "Xandr (Microsoft) ads", Advertising),
        ("criteo.com", "Criteo ads", Advertising),
        ("criteo.net", "Criteo ads", Advertising),
        ("taboola.com", "Taboola ads", Advertising),
        ("outbrain.com", "Outbrain ads", Advertising),
        ("rubiconproject.com", "Magnite ads", Advertising),
        ("pubmatic.com", "PubMatic ads", Advertising),
        ("openx.net", "OpenX ads", Advertising),
        ("scorecardresearch.com", "Comscore measurement", Advertising),
        ("moatads.com", "Oracle Moat measurement", Advertising),
        ("adsrvr.org", "The Trade Desk ads", Advertising),
        ("casalemedia.com", "Index Exchange ads", Advertising),
        ("amazon-adsystem.com", "Amazon ads", Advertising),
        ("ads-twitter.com", "X ads", Advertising),
        ("adcolony.com", "AdColony ads", Advertising),
        ("unityads.unity3d.com", "Unity ads", Advertising),
        ("applovin.com", "AppLovin ads", Advertising),
        ("ironsrc.com", "ironSource ads", Advertising),
        ("mopub.com", "MoPub ads", Advertising),
        // analytics, crash reporting, telemetry
        ("google-analytics.com", "Google Analytics", Telemetry),
        ("analytics.google.com", "Google Analytics", Telemetry),
        ("app-measurement.com", "Google Firebase analytics", Telemetry),
        ("firebaseinstallations.googleapis.com", "Google Firebase", Telemetry),
        ("crashlytics.com", "Google Crashlytics", Telemetry),
        ("segment.io", "Segment analytics", Telemetry),
        ("segment.com", "Segment analytics", Telemetry),
        ("mixpanel.com", "Mixpanel analytics", Telemetry),
        ("amplitude.com", "Amplitude analytics", Telemetry),
        ("sentry.io", "Sentry crash reports", Telemetry),
        ("bugsnag.com", "Bugsnag crash reports", Telemetry),
        ("datadoghq.com", "Datadog telemetry", Telemetry),
        ("newrelic.com", "New Relic telemetry", Telemetry),
        ("nr-data.net", "New Relic telemetry", Telemetry),
        ("hotjar.com", "Hotjar session recording", Telemetry),
        ("fullstory.com", "FullStory session recording", Telemetry),
        ("appsflyer.com", "AppsFlyer attribution", Telemetry),
        ("adjust.com", "Adjust attribution", Telemetry),
        ("branch.io", "Branch attribution", Telemetry),
        ("kochava.com", "Kochava attribution", Telemetry),
        ("launchdarkly.com", "LaunchDarkly flags", Telemetry),
        ("optimizely.com", "Optimizely experiments", Telemetry),
        ("statsig.com", "Statsig experiments", Telemetry),
        ("posthog.com", "PostHog analytics", Telemetry),
        ("heap.io", "Heap analytics", Telemetry),
        ("intercom.io", "Intercom", Telemetry),
        ("data.microsoft.com", "Microsoft telemetry", Telemetry),
        ("events.data.microsoft.com", "Microsoft telemetry", Telemetry),
        ("vortex.data.microsoft.com", "Microsoft telemetry", Telemetry),
        ("telemetry.mozilla.org", "Mozilla telemetry", Telemetry),
        ("incoming.telemetry.mozilla.org", "Mozilla telemetry", Telemetry),
        ("metrics.apple.com", "Apple telemetry", Telemetry),
        ("xp.apple.com", "Apple telemetry", Telemetry),
        ("telemetry.nvidia.com", "NVIDIA telemetry", Telemetry),
        ("adobe.io", "Adobe telemetry", Telemetry),
        ("demdex.net", "Adobe audience", Telemetry),
        ("omtrdc.net", "Adobe analytics", Telemetry),
        ("graph.facebook.com", "Meta graph", Telemetry),
        ("connect.facebook.net", "Meta pixel", Telemetry),
        ("analytics.tiktok.com", "TikTok analytics", Telemetry),
        ("yandex.ru", "Yandex", Telemetry),
        ("mc.yandex.ru", "Yandex Metrica", Telemetry),
        // remote access
        ("teamviewer.com", "TeamViewer", RemoteAccess),
        ("anydesk.com", "AnyDesk", RemoteAccess),
        ("rustdesk.com", "RustDesk", RemoteAccess),
        ("logmein.com", "LogMeIn", RemoteAccess),
        ("splashtop.com", "Splashtop", RemoteAccess),
        ("remotedesktop.google.com", "Chrome Remote Desktop", RemoteAccess),
        ("parsec.app", "Parsec", RemoteAccess),
        ("ngrok.io", "ngrok tunnel", RemoteAccess),
        ("ngrok.com", "ngrok tunnel", RemoteAccess),
        ("tailscale.com", "Tailscale", RemoteAccess),
        ("zerotier.com", "ZeroTier", RemoteAccess),
        // messaging and push
        ("mtalk.google.com", "Google push", Messaging),
        ("push.apple.com", "Apple push", Messaging),
        ("courier.push.apple.com", "Apple push", Messaging),
        ("wns.windows.com", "Windows push", Messaging),
        ("notify.windows.com", "Windows push", Messaging),
        ("discord.gg", "Discord", Messaging),
        ("discord.com", "Discord", Messaging),
        ("discordapp.com", "Discord", Messaging),
        ("slack.com", "Slack", Messaging),
        ("slack-msgs.com", "Slack", Messaging),
        ("whatsapp.net", "WhatsApp", Messaging),
        ("whatsapp.com", "WhatsApp", Messaging),
        ("telegram.org", "Telegram", Messaging),
        ("signal.org", "Signal", Messaging),
        ("zoom.us", "Zoom", Messaging),
        ("teams.microsoft.com", "Microsoft Teams", Messaging),
        ("skype.com", "Skype", Messaging),
        // cloud
        ("amazonaws.com", "Amazon Web Services", Cloud),
        ("awsglobalaccelerator.com", "Amazon Web Services", Cloud),
        ("compute.amazonaws.com", "Amazon Web Services", Cloud),
        ("googleusercontent.com", "Google Cloud", Cloud),
        ("bc.googleusercontent.com", "Google Cloud", Cloud),
        ("cloudapp.azure.com", "Microsoft Azure", Cloud),
        ("azure.com", "Microsoft Azure", Cloud),
        ("azurewebsites.net", "Microsoft Azure", Cloud),
        ("windows.net", "Microsoft Azure", Cloud),
        ("trafficmanager.net", "Microsoft Azure", Cloud),
        ("digitalocean.com", "DigitalOcean", Cloud),
        ("linode.com", "Akamai Linode", Cloud),
        ("linodeusercontent.com", "Akamai Linode", Cloud),
        ("hetzner.com", "Hetzner", Cloud),
        ("your-server.de", "Hetzner", Cloud),
        ("ovh.net", "OVH", Cloud),
        ("vultr.com", "Vultr", Cloud),
        ("oraclecloud.com", "Oracle Cloud", Cloud),
        ("fly.dev", "Fly.io", Cloud),
        ("vercel-dns.com", "Vercel", Cloud),
        ("vercel.app", "Vercel", Cloud),
        ("herokuapp.com", "Heroku", Cloud),
        ("railway.app", "Railway", Cloud),
        ("render.com", "Render", Cloud),
        ("aliyuncs.com", "Alibaba Cloud", Cloud),
        ("tencentcloud.com", "Tencent Cloud", Cloud),
        ("myqcloud.com", "Tencent Cloud", Cloud),
        // content delivery
        ("cloudfront.net", "Amazon CloudFront", Cdn),
        ("akamaitechnologies.com", "Akamai", Cdn),
        ("akamaiedge.net", "Akamai", Cdn),
        ("akamai.net", "Akamai", Cdn),
        ("edgekey.net", "Akamai", Cdn),
        ("edgesuite.net", "Akamai", Cdn),
        ("fastly.net", "Fastly", Cdn),
        ("fastlylb.net", "Fastly", Cdn),
        ("cloudflare.com", "Cloudflare", Cdn),
        ("cloudflare.net", "Cloudflare", Cdn),
        ("cloudflaressl.com", "Cloudflare", Cdn),
        ("cdn77.com", "CDN77", Cdn),
        ("bunny.net", "Bunny", Cdn),
        ("b-cdn.net", "Bunny", Cdn),
        ("stackpathdns.com", "StackPath", Cdn),
        ("llnwd.net", "Edgio", Cdn),
        ("msedge.net", "Microsoft edge network", Cdn),
        ("azureedge.net", "Microsoft edge network", Cdn),
        ("gvt1.com", "Google downloads", Cdn),
        ("gvt2.com", "Google downloads", Cdn),
        ("jsdelivr.net", "jsDelivr", Cdn),
        ("cloudinary.com", "Cloudinary", Cdn),
        // named services
        ("1e100.net", "Google", Service),
        ("google.com", "Google", Service),
        ("googleapis.com", "Google", Service),
        ("gstatic.com", "Google", Service),
        ("youtube.com", "YouTube", Service),
        ("googlevideo.com", "YouTube", Service),
        ("ggpht.com", "Google", Service),
        ("apple.com", "Apple", Service),
        ("icloud.com", "Apple iCloud", Service),
        ("aaplimg.com", "Apple", Service),
        ("mzstatic.com", "Apple", Service),
        ("microsoft.com", "Microsoft", Service),
        ("live.com", "Microsoft", Service),
        ("office.com", "Microsoft 365", Service),
        ("office365.com", "Microsoft 365", Service),
        ("office.net", "Microsoft 365", Service),
        ("sharepoint.com", "Microsoft SharePoint", Service),
        ("onedrive.com", "Microsoft OneDrive", Service),
        ("msn.com", "Microsoft", Service),
        ("bing.com", "Microsoft Bing", Service),
        ("xboxlive.com", "Xbox Live", Service),
        ("facebook.com", "Meta", Service),
        ("fbcdn.net", "Meta", Service),
        ("instagram.com", "Instagram", Service),
        ("cdninstagram.com", "Instagram", Service),
        ("twitter.com", "X", Service),
        ("twimg.com", "X", Service),
        ("x.com", "X", Service),
        ("tiktokcdn.com", "TikTok", Service),
        ("tiktokv.com", "TikTok", Service),
        ("tiktok.com", "TikTok", Service),
        ("byteoversea.com", "ByteDance", Service),
        ("bytedance.com", "ByteDance", Service),
        ("snapchat.com", "Snapchat", Service),
        ("sc-cdn.net", "Snapchat", Service),
        ("reddit.com", "Reddit", Service),
        ("redd.it", "Reddit", Service),
        ("redditmedia.com", "Reddit", Service),
        ("netflix.com", "Netflix", Service),
        ("nflxvideo.net", "Netflix", Service),
        ("nflximg.net", "Netflix", Service),
        ("spotify.com", "Spotify", Service),
        ("scdn.co", "Spotify", Service),
        ("twitch.tv", "Twitch", Service),
        ("ttvnw.net", "Twitch", Service),
        ("steampowered.com", "Steam", Service),
        ("steamcontent.com", "Steam", Service),
        ("steamstatic.com", "Steam", Service),
        ("epicgames.com", "Epic Games", Service),
        ("riotgames.com", "Riot Games", Service),
        ("github.com", "GitHub", Service),
        ("githubusercontent.com", "GitHub", Service),
        ("gitlab.com", "GitLab", Service),
        ("npmjs.org", "npm", Service),
        ("npmjs.com", "npm", Service),
        ("crates.io", "crates.io", Service),
        ("rust-lang.org", "Rust", Service),
        ("pypi.org", "PyPI", Service),
        ("docker.io", "Docker", Service),
        ("docker.com", "Docker", Service),
        ("dropbox.com", "Dropbox", Service),
        ("dropboxapi.com", "Dropbox", Service),
        ("box.com", "Box", Service),
        ("openai.com", "OpenAI", Service),
        ("anthropic.com", "Anthropic", Service),
        ("venice.ai", "Venice", Service),
        ("asherin.com", "noah", Service),
        ("zed.dev", "Zed", Service),
        ("mozilla.org", "Mozilla", Service),
        ("mozilla.net", "Mozilla", Service),
        ("mozilla.com", "Mozilla", Service),
        ("brave.com", "Brave", Service),
        ("duckduckgo.com", "DuckDuckGo", Service),
        ("wikipedia.org", "Wikipedia", Service),
        ("wikimedia.org", "Wikimedia", Service),
        ("nvidia.com", "NVIDIA", Service),
        ("amd.com", "AMD", Service),
        ("intel.com", "Intel", Service),
        ("ubuntu.com", "Ubuntu", Service),
        ("canonical.com", "Canonical", Service),
        ("debian.org", "Debian", Service),
        ("fedoraproject.org", "Fedora", Service),
        ("archlinux.org", "Arch Linux", Service),
        ("snapcraft.io", "Snapcraft", Service),
        ("flathub.org", "Flathub", Service),
        ("ntp.org", "time servers", Service),
        ("time.apple.com", "Apple time", Service),
        ("time.windows.com", "Microsoft time", Service),
        ("baidu.com", "Baidu", Service),
        ("qq.com", "Tencent", Service),
        ("wechat.com", "WeChat", Service),
        ("alibaba.com", "Alibaba", Service),
        ("aliexpress.com", "AliExpress", Service),
        ("temu.com", "Temu", Service),
        ("shein.com", "Shein", Service),
        ("vk.com", "VK", Service),
        ("mail.ru", "Mail.ru", Service),
        ("kaspersky.com", "Kaspersky", Service),
    ];
    if let Some(host) = host {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        // Longest suffix wins, so "analytics.google.com" beats "google.com".
        let mut best: Option<(&str, &'static str, DestinationKind)> = None;
        for (suffix, owner, kind) in OWNERS {
            let matches = host == *suffix || host.ends_with(&format!(".{suffix}"));
            if matches && best.is_none_or(|(chosen, _, _)| suffix.len() > chosen.len()) {
                best = Some((suffix, owner, *kind));
            }
        }
        if let Some((_, owner, kind)) = best {
            return (Some(owner), kind);
        }
        if host.contains("telemetry") || host.contains("metrics") || host.contains("analytics") {
            return (None, Telemetry);
        }
    }
    let kind = match port {
        853 => Service,
        5228..=5230 => Messaging,
        5223 => Messaging,
        3478 | 3479 | 19302..=19309 => Messaging,
        5938 | 7070 => RemoteAccess,
        _ => Unknown,
    };
    (None, kind)
}

/// Names for the addresses, from reverse DNS, within a fixed time so a slow
/// resolver can't hold the room. Whatever hasn't answered stays a number.
fn reverse_lookup_for(addresses: &[IpAddr]) -> HashMap<IpAddr, String> {
    if addresses.is_empty() {
        return HashMap::new();
    }
    platform::reverse_lookup(addresses)
}

/// Runs `lookup` for each address on a few threads and keeps the answers
/// that arrive within the budget.
#[cfg_attr(windows, allow(dead_code))]
fn reverse_lookup_threaded(
    addresses: &[IpAddr],
    lookup: fn(IpAddr) -> Option<String>,
) -> HashMap<IpAddr, String> {
    let (sender, receiver) = mpsc::channel();
    let queue = std::sync::Arc::new(std::sync::Mutex::new(addresses.to_vec()));
    for _ in 0..REVERSE_LOOKUP_THREADS.min(addresses.len()) {
        let sender = sender.clone();
        let queue = queue.clone();
        // Detached on purpose: a thread stuck in a slow resolver finishes on
        // its own, and its late answer is simply dropped.
        thread::spawn(move || {
            loop {
                let next = match queue.lock() {
                    Ok(mut queue) => queue.pop(),
                    Err(_) => None,
                };
                let Some(address) = next else { break };
                let answer = lookup(address);
                if sender.send((address, answer)).is_err() {
                    break;
                }
            }
        });
    }
    drop(sender);
    let deadline = Instant::now() + REVERSE_LOOKUP_BUDGET;
    let mut answers = HashMap::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match receiver.recv_timeout(remaining) {
            Ok((address, Some(host))) => {
                answers.insert(address, host);
            }
            Ok((_, None)) => {}
            Err(_) => break,
        }
    }
    answers
}

// ---------------------------------------------------------------------------
// Installed applications

#[derive(Clone, Debug)]
pub struct InstalledApp {
    pub name: String,
    pub version: Option<String>,
    pub publisher: Option<String>,
    /// Where it came from: a store, a package manager, a download.
    pub source: String,
    pub installed_on: Option<String>,
    pub location: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct AppInventory {
    pub apps: Vec<InstalledApp>,
    /// Packages that are parts of the system rather than programs a person
    /// opens, counted rather than listed.
    pub system_packages: Option<usize>,
    pub note: Option<String>,
}

/// The programs installed on this device, the ones a person opens first.
pub fn installed_apps() -> AppInventory {
    let mut inventory = platform::installed_apps();
    inventory.apps.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.source.cmp(&right.source))
    });
    inventory
        .apps
        .dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name) && left.source == right.source);
    inventory
}

// ---------------------------------------------------------------------------
// Wifi, the network, and who else is on it

/// Something worth saying about the network or the device, with how bad.
#[derive(Clone, Debug)]
pub struct Finding {
    pub status: Status,
    pub title: String,
    pub detail: String,
}

#[derive(Clone, Debug, Default)]
pub struct WifiLink {
    pub ssid: String,
    pub bssid: Option<String>,
    pub security: Option<String>,
    pub channel: Option<String>,
    pub frequency: Option<String>,
    pub signal: Option<String>,
    pub rate: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct NearbyNetwork {
    pub ssid: String,
    pub bssid: Option<String>,
    pub signal: Option<i32>,
    pub security: Option<String>,
    pub channel: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct WifiReport {
    pub interface: Option<String>,
    pub connected: Option<WifiLink>,
    pub nearby: Vec<NearbyNetwork>,
    pub gateway: Option<IpAddr>,
    pub local_address: Option<IpAddr>,
    pub dns_servers: Vec<IpAddr>,
    pub encrypted_dns: Option<bool>,
    pub mac_address: Option<String>,
    pub mac_randomized: Option<bool>,
    pub findings: Vec<Finding>,
    pub note: Option<String>,
}

/// The wifi this device is on and the ones around it, the way out to the
/// internet, and who answers its DNS.
pub fn wifi_report() -> WifiReport {
    let mut report = platform::wifi_report();
    if report.mac_randomized.is_none()
        && let Some(mac) = &report.mac_address
    {
        report.mac_randomized = locally_administered(mac);
    }
    report.findings = wifi_findings(&report);
    report
}

/// A MAC whose second hex digit has the locally-administered bit set was
/// made up by the device rather than burned in by the maker, which is what
/// randomization does.
fn locally_administered(mac: &str) -> Option<bool> {
    let second = mac.trim().chars().nth(1)?;
    let value = second.to_digit(16)?;
    Some(value & 0x2 != 0)
}

fn wifi_findings(report: &WifiReport) -> Vec<Finding> {
    let mut findings = Vec::new();
    if let Some(link) = &report.connected {
        let security = link
            .security
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase();
        if security.is_empty() || security == "none" || security == "open" || security == "--" {
            findings.push(Finding {
                status: Status::Bad,
                title: "this wifi has no password".into(),
                detail: format!(
                    "anyone in range of \"{}\" can read traffic that isn't itself encrypted and \
                     reach this device directly. Prefer a network with WPA2 or WPA3, or use a VPN here.",
                    link.ssid
                ),
            });
        } else if security.contains("wep") || security.contains("wpa1") || security.contains("tkip") {
            findings.push(Finding {
                status: Status::Warning,
                title: "this wifi uses old encryption".into(),
                detail: format!(
                    "\"{}\" is protected with {}, which can be broken in minutes. Ask for the router \
                     to be set to WPA2-AES or WPA3.",
                    link.ssid, link.security.as_deref().unwrap_or("WEP")
                ),
            });
        }
        // Another access point announcing the same name with weaker security
        // is how an evil twin is set up.
        let twins: Vec<&NearbyNetwork> = report
            .nearby
            .iter()
            .filter(|network| network.ssid == link.ssid)
            .filter(|network| {
                network
                    .security
                    .as_deref()
                    .map(str::to_ascii_lowercase)
                    .is_some_and(|other| other != security)
            })
            .collect();
        if !twins.is_empty() {
            findings.push(Finding {
                status: Status::Warning,
                title: "a second network is using this wifi's name".into(),
                detail: format!(
                    "{} other access point{} called \"{}\" with different security. That is what a \
                     fake hotspot looks like; check with whoever runs the network before typing \
                     passwords here.",
                    twins.len(),
                    if twins.len() == 1 { "" } else { "s" },
                    link.ssid
                ),
            });
        }
    }
    match report.mac_randomized {
        Some(false) if report.connected.is_some() => findings.push(Finding {
            status: Status::Warning,
            title: "this device shows its real hardware address".into(),
            detail: "every network you join sees the same wifi address, which lets shops, \
                     airports and ad companies recognise this device across places. Turn on \
                     random or private wifi addresses in the system's wifi settings."
                .into(),
        }),
        _ => {}
    }
    if !report.dns_servers.is_empty() {
        let owners: Vec<&'static str> = report
            .dns_servers
            .iter()
            .filter_map(|server| dns_owner(*server))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let on_router = report
            .dns_servers
            .iter()
            .all(|server| Some(*server) == report.gateway || !is_remote(*server));
        let detail = if on_router {
            "name lookups go to the router, which usually hands them on to the internet \
             provider. The provider can see every site name this device asks for."
                .to_string()
        } else if owners.is_empty() {
            format!(
                "name lookups go to {}. Whoever runs those servers sees every site name this \
                 device asks for.",
                report
                    .dns_servers
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            format!(
                "name lookups go to {}, which sees every site name this device asks for.",
                owners.join(" and ")
            )
        };
        let status = match report.encrypted_dns {
            Some(true) => Status::Good,
            _ => Status::Unknown,
        };
        findings.push(Finding {
            status,
            title: match report.encrypted_dns {
                Some(true) => "DNS is encrypted".into(),
                Some(false) => "DNS travels in the clear".into(),
                None => "where DNS goes".into(),
            },
            detail: match report.encrypted_dns {
                Some(false) => format!(
                    "{detail} Anyone on the same network can read the lookups too; encrypted DNS \
                     (DNS over HTTPS or TLS) in the system settings closes that."
                ),
                _ => detail,
            },
        });
    }
    findings
}

/// The company behind a public resolver address.
pub fn dns_owner(server: IpAddr) -> Option<&'static str> {
    let text = server.to_string();
    let owner = match text.as_str() {
        "8.8.8.8" | "8.8.4.4" | "2001:4860:4860::8888" | "2001:4860:4860::8844" => "Google",
        "1.1.1.1" | "1.0.0.1" | "2606:4700:4700::1111" | "2606:4700:4700::1001" => "Cloudflare",
        "1.1.1.2" | "1.0.0.2" | "1.1.1.3" | "1.0.0.3" => "Cloudflare",
        "9.9.9.9" | "149.112.112.112" | "2620:fe::fe" | "2620:fe::9" => "Quad9",
        "208.67.222.222" | "208.67.220.220" | "2620:119:35::35" | "2620:119:53::53" => {
            "Cisco OpenDNS"
        }
        "94.140.14.14" | "94.140.15.15" => "AdGuard",
        "185.228.168.9" | "185.228.169.9" => "CleanBrowsing",
        "76.76.2.0" | "76.76.10.0" => "Control D",
        "77.88.8.8" | "77.88.8.1" => "Yandex",
        "114.114.114.114" | "223.5.5.5" | "223.6.6.6" => "a Chinese public resolver",
        _ => return None,
    };
    Some(owner)
}

/// Another device seen on this network.
#[derive(Clone, Debug)]
pub struct Neighbour {
    pub address: IpAddr,
    pub mac: String,
    pub vendor: Option<&'static str>,
    pub is_gateway: bool,
}

/// Devices this machine has recently exchanged packets with on its own
/// network, from the address cache. It only shows who has been talking, not
/// everyone who is there.
pub fn neighbours(gateway: Option<IpAddr>) -> Vec<Neighbour> {
    let mut seen: Vec<Neighbour> = platform::neighbours()
        .into_iter()
        .filter(|(_, mac)| !mac.is_empty() && mac != "00:00:00:00:00:00" && !mac.starts_with("ff:ff"))
        .filter(|(address, _)| !address.is_multicast() && !address.is_unspecified())
        .map(|(address, mac)| Neighbour {
            vendor: vendor_of_mac(&mac),
            is_gateway: Some(address) == gateway,
            address,
            mac,
        })
        .collect();
    seen.sort_by(|left, right| right.is_gateway.cmp(&left.is_gateway).then_with(|| left.address.cmp(&right.address)));
    seen.dedup_by(|left, right| left.address == right.address);
    seen
}

/// The maker behind a hardware address, for the makers people have at home.
pub fn vendor_of_mac(mac: &str) -> Option<&'static str> {
    const VENDORS: &[(&str, &str)] = &[
        ("b8:27:eb", "Raspberry Pi"),
        ("dc:a6:32", "Raspberry Pi"),
        ("e4:5f:01", "Raspberry Pi"),
        ("d8:3a:dd", "Raspberry Pi"),
        ("28:cd:c1", "Raspberry Pi"),
        ("00:17:88", "Philips Hue"),
        ("ec:b5:fa", "Philips Hue"),
        ("18:b4:30", "Google Nest"),
        ("64:16:66", "Google Nest"),
        ("f4:f5:d8", "Google"),
        ("54:60:09", "Google"),
        ("3c:5a:b4", "Google"),
        ("a4:77:33", "Google"),
        ("94:eb:2c", "Google"),
        ("f4:03:2a", "Amazon"),
        ("fc:65:de", "Amazon"),
        ("44:65:0d", "Amazon"),
        ("74:c2:46", "Amazon"),
        ("a0:02:dc", "Amazon"),
        ("0c:47:c9", "Amazon"),
        ("b4:7c:9c", "Amazon"),
        ("48:d6:d5", "Google"),
        ("00:1a:11", "Google"),
        ("94:9f:3e", "Sonos"),
        ("5c:aa:fd", "Sonos"),
        ("b8:e9:37", "Sonos"),
        ("00:0e:58", "Sonos"),
        ("34:7e:5c", "Sonos"),
        ("cc:50:e3", "Espressif (small IoT boards)"),
        ("24:0a:c4", "Espressif (small IoT boards)"),
        ("30:ae:a4", "Espressif (small IoT boards)"),
        ("a4:cf:12", "Espressif (small IoT boards)"),
        ("84:f3:eb", "Espressif (small IoT boards)"),
        ("bc:dd:c2", "Espressif (small IoT boards)"),
        ("ec:fa:bc", "Espressif (small IoT boards)"),
        ("d8:f1:5b", "Espressif (small IoT boards)"),
        ("10:52:1c", "Espressif (small IoT boards)"),
        ("68:c6:3a", "Espressif (small IoT boards)"),
        ("50:02:91", "Espressif (small IoT boards)"),
        ("c8:2b:96", "Espressif (small IoT boards)"),
        ("18:fe:34", "Espressif (small IoT boards)"),
        ("5c:cf:7f", "Espressif (small IoT boards)"),
        ("60:01:94", "Espressif (small IoT boards)"),
        ("a0:20:a6", "Espressif (small IoT boards)"),
        ("2c:f4:32", "Espressif (small IoT boards)"),
        ("48:3f:da", "Espressif (small IoT boards)"),
        ("7c:9e:bd", "Espressif (small IoT boards)"),
        ("d4:d4:da", "Tuya smart home"),
        ("10:d5:61", "Tuya smart home"),
        ("50:8a:06", "Tuya smart home"),
        ("38:1f:8d", "Tuya smart home"),
        ("70:4f:57", "TP-Link"),
        ("50:c7:bf", "TP-Link"),
        ("b0:be:76", "TP-Link"),
        ("c0:06:c3", "TP-Link"),
        ("60:32:b1", "TP-Link"),
        ("a4:2b:b0", "TP-Link"),
        ("1c:61:b4", "TP-Link"),
        ("30:de:4b", "TP-Link"),
        ("98:da:c4", "TP-Link"),
        ("28:80:88", "Netgear"),
        ("a0:40:a0", "Netgear"),
        ("9c:3d:cf", "Netgear"),
        ("c4:04:15", "Netgear"),
        ("e0:46:9a", "Netgear"),
        ("00:14:6c", "Netgear"),
        ("2c:30:33", "Netgear"),
        ("f8:73:94", "Netgear"),
        ("24:5a:4c", "Ubiquiti"),
        ("fc:ec:da", "Ubiquiti"),
        ("78:8a:20", "Ubiquiti"),
        ("74:ac:b9", "Ubiquiti"),
        ("18:e8:29", "Ubiquiti"),
        ("f4:92:bf", "Ubiquiti"),
        ("e0:63:da", "Ubiquiti"),
        ("d0:21:f9", "Ubiquiti"),
        ("04:18:d6", "Ubiquiti"),
        ("00:1e:c2", "Apple"),
        ("3c:22:fb", "Apple"),
        ("f0:18:98", "Apple"),
        ("a4:83:e7", "Apple"),
        ("88:66:5a", "Apple"),
        ("dc:a9:04", "Apple"),
        ("f8:ff:c2", "Apple"),
        ("bc:d0:74", "Apple"),
        ("14:98:77", "Apple"),
        ("38:f9:d3", "Apple"),
        ("a8:5c:2c", "Apple"),
        ("f4:0f:24", "Apple"),
        ("98:01:a7", "Apple"),
        ("ac:bc:32", "Apple"),
        ("d0:03:4b", "Apple"),
        ("48:60:5f", "Apple"),
        ("e0:b5:5f", "Apple"),
        ("94:f6:a3", "Apple"),
        ("70:56:81", "Apple"),
        ("c8:69:cd", "Apple"),
        ("8c:85:90", "Apple"),
        ("f0:99:bf", "Apple"),
        ("5c:f9:38", "Apple"),
        ("7c:d1:c3", "Apple"),
        ("b8:53:ac", "Apple"),
        ("cc:08:8d", "Apple"),
        ("80:e6:50", "Apple"),
        ("28:f0:76", "Apple"),
        ("34:c0:59", "Apple"),
        ("bc:52:b7", "Apple"),
        ("ec:1f:72", "Samsung"),
        ("8c:71:f8", "Samsung"),
        ("50:32:75", "Samsung"),
        ("f4:7b:5e", "Samsung"),
        ("a0:82:1f", "Samsung"),
        ("c8:14:79", "Samsung"),
        ("30:cd:a7", "Samsung"),
        ("b4:79:a7", "Samsung"),
        ("94:35:0a", "Samsung"),
        ("e8:50:8b", "Samsung"),
        ("5c:49:7d", "Samsung"),
        ("64:1c:ae", "Samsung"),
        ("10:d3:8a", "Samsung"),
        ("ac:5f:3e", "Samsung"),
        ("b8:d9:ce", "Samsung"),
        ("f8:04:2e", "Samsung"),
        ("28:39:5e", "Samsung"),
        ("8c:f5:a3", "Samsung"),
        ("64:b5:c6", "Nintendo"),
        ("98:b6:e9", "Nintendo"),
        ("7c:bb:8a", "Nintendo"),
        ("58:2f:40", "Nintendo"),
        ("04:03:d6", "Nintendo"),
        ("dc:68:eb", "Nintendo"),
        ("00:1f:a7", "Sony PlayStation"),
        ("f8:46:1c", "Sony PlayStation"),
        ("70:9e:29", "Sony PlayStation"),
        ("bc:60:a7", "Sony PlayStation"),
        ("2c:cc:44", "Sony PlayStation"),
        ("78:c8:81", "Sony PlayStation"),
        ("98:5f:d3", "Microsoft Xbox"),
        ("7c:ed:8d", "Microsoft Xbox"),
        ("28:18:78", "Microsoft Xbox"),
        ("60:45:bd", "Microsoft"),
        ("c8:3f:26", "Microsoft"),
        ("b8:31:b5", "Microsoft"),
        ("00:15:5d", "Microsoft Hyper-V"),
        ("00:50:56", "VMware"),
        ("00:0c:29", "VMware"),
        ("08:00:27", "VirtualBox"),
        ("52:54:00", "QEMU virtual machine"),
        ("b8:e8:56", "Apple"),
        ("d4:5d:64", "Roku"),
        ("b0:a7:37", "Roku"),
        ("cc:6d:a0", "Roku"),
        ("dc:3a:5e", "Roku"),
        ("00:0d:4b", "Roku"),
        ("ac:3a:7a", "Roku"),
        ("d8:31:34", "Roku"),
        ("c0:a0:0d", "Ring (Amazon)"),
        ("64:9a:63", "Ring (Amazon)"),
        ("34:3e:a4", "Ring (Amazon)"),
        ("54:e0:19", "Ring (Amazon)"),
        ("cc:9e:a2", "Ring (Amazon)"),
        ("00:04:4b", "NVIDIA"),
        ("48:b0:2d", "NVIDIA"),
        ("00:1b:63", "Apple"),
        ("3c:15:c2", "Apple"),
        ("f0:2f:74", "ASUS"),
        ("2c:56:dc", "ASUS"),
        ("04:d4:c4", "ASUS"),
        ("1c:b7:2c", "ASUS"),
        ("a8:5e:45", "ASUS"),
        ("d8:50:e6", "ASUS"),
        ("50:eb:f6", "ASUS"),
        ("7c:10:c9", "ASUS"),
        ("00:23:24", "Xiaomi"),
        ("28:6c:07", "Xiaomi"),
        ("64:cc:2e", "Xiaomi"),
        ("74:23:44", "Xiaomi"),
        ("f8:a4:5f", "Xiaomi"),
        ("50:64:2b", "Xiaomi"),
        ("34:ce:00", "Xiaomi"),
        ("78:11:dc", "Xiaomi"),
        ("40:31:3c", "Xiaomi"),
        ("00:9e:c8", "Xiaomi"),
        ("48:2c:a0", "Xiaomi"),
        ("54:48:e6", "Beijing Xiaomi"),
        ("00:e0:4c", "Realtek"),
        ("b8:ac:6f", "Dell"),
        ("f8:b1:56", "Dell"),
        ("18:03:73", "Dell"),
        ("d4:be:d9", "Dell"),
        ("a4:bb:6d", "Dell"),
        ("c8:f7:50", "Dell"),
        ("34:17:eb", "Dell"),
        ("f0:1f:af", "Dell"),
        ("98:90:96", "Dell"),
        ("3c:52:82", "HP"),
        ("10:e7:c6", "HP"),
        ("80:e8:2c", "HP"),
        ("b0:5c:da", "HP"),
        ("6c:c2:17", "HP"),
        ("f4:39:09", "HP"),
        ("c4:65:16", "HP"),
        ("38:63:bb", "HP"),
        ("e4:e7:49", "HP"),
        ("a0:d3:c1", "HP"),
        ("50:65:f3", "HP"),
        ("00:1e:0b", "HP"),
        ("8c:16:45", "Lenovo"),
        ("54:ee:75", "Lenovo"),
        ("c8:5b:76", "Lenovo"),
        ("98:fa:9b", "Lenovo"),
        ("28:d2:44", "Lenovo"),
        ("50:7b:9d", "Lenovo"),
        ("e8:6a:64", "Lenovo"),
        ("f0:de:f1", "Lenovo"),
        ("00:21:cc", "Lenovo"),
        ("3c:97:0e", "Lenovo"),
        ("00:1a:2b", "Huawei"),
        ("00:e0:fc", "Huawei"),
        ("48:46:fb", "Huawei"),
        ("70:72:3c", "Huawei"),
        ("28:6e:d4", "Huawei"),
        ("a4:c6:4f", "Huawei"),
        ("00:25:9e", "Huawei"),
        ("d4:6e:5c", "Huawei"),
        ("04:bd:70", "Huawei"),
        ("cc:96:a0", "Huawei"),
        ("f4:c7:14", "Huawei"),
        ("e0:24:7f", "Huawei"),
        ("b4:0b:44", "Huawei"),
        ("c8:d1:5e", "Huawei"),
        ("00:1c:bf", "Intel"),
        ("3c:a9:f4", "Intel"),
        ("34:02:86", "Intel"),
        ("8c:8d:28", "Intel"),
        ("a0:88:b4", "Intel"),
        ("48:51:b7", "Intel"),
        ("00:28:f8", "Intel"),
        ("f8:63:3f", "Intel"),
        ("b4:6b:fc", "Intel"),
        ("dc:71:96", "Intel"),
        ("58:96:1d", "Intel"),
        ("3c:e9:f7", "Intel"),
        ("9c:b6:d0", "Intel"),
        ("d8:f2:ca", "Intel"),
        ("74:d8:3e", "Intel"),
        ("7c:b2:7d", "Intel"),
        ("e4:a7:a0", "Intel"),
        ("cc:d9:ac", "Intel"),
        ("a0:d3:7a", "Intel"),
        ("8c:1d:96", "Intel"),
        ("1c:1b:b5", "Intel"),
        ("34:cf:f6", "Intel"),
        ("30:24:a9", "Hon Hai (Foxconn)"),
        ("e0:9d:31", "Intel"),
        ("00:e0:66", "Motorola"),
        ("40:4e:36", "HTC"),
        ("a0:cc:2b", "Murata (wifi module)"),
        ("44:91:60", "Murata (wifi module)"),
        ("14:7d:da", "Apple"),
        ("38:c9:86", "Apple"),
        ("a4:5e:60", "Apple"),
        ("70:ea:1a", "Cisco"),
        ("00:1b:d4", "Cisco"),
        ("58:97:1e", "Cisco"),
        ("00:26:99", "Cisco"),
        ("a8:9d:21", "Cisco"),
        ("00:11:32", "Synology"),
        ("90:09:d0", "Synology"),
        ("24:5e:be", "QNAP"),
        ("00:08:9b", "QNAP"),
        ("e8:9f:80", "Belkin"),
        ("94:10:3e", "Belkin"),
        ("ec:1a:59", "Belkin"),
        ("c0:56:27", "Belkin"),
        ("14:91:82", "Belkin"),
        ("58:ef:68", "Belkin"),
        ("b4:75:0e", "Belkin"),
        ("d8:eb:97", "Trendnet"),
        ("3c:37:86", "Netgear"),
        ("00:18:e7", "Cameo"),
        ("00:1d:d8", "Microsoft"),
        ("c0:ee:fb", "OnePlus"),
        ("94:65:2d", "OnePlus"),
        ("64:a2:f9", "OnePlus"),
        ("d0:c5:d3", "Logitech"),
        ("00:07:ab", "Samsung"),
        ("ac:bc:b5", "Fitbit"),
        ("f0:03:8c", "Garmin"),
        ("90:f1:57", "Garmin"),
        ("00:26:e8", "Murata (wifi module)"),
        ("c8:2e:47", "Suzhou SmartChip"),
        ("2c:aa:8e", "Wyze"),
        ("d0:3f:27", "Wyze"),
        ("7c:78:b2", "Wyze"),
        ("a4:da:22", "Wyze"),
        ("b0:c7:de", "Eufy (Anker)"),
        ("04:17:b6", "Eufy (Anker)"),
        ("cc:4b:73", "AzureWave (wifi module)"),
        ("e0:d4:e8", "Intel"),
        ("00:80:92", "Silex"),
        ("78:d2:94", "Netgear"),
        ("c0:3c:59", "Intel"),
        ("00:24:e4", "Withings"),
        ("00:0f:00", "Legra"),
        ("00:26:ab", "Seiko Epson"),
        ("64:eb:8c", "Seiko Epson"),
        ("9c:ae:d3", "Seiko Epson"),
        ("ac:18:26", "Seiko Epson"),
        ("00:00:85", "Canon"),
        ("2c:9e:fc", "Canon"),
        ("60:12:8b", "Canon"),
        ("18:0c:ac", "Canon"),
        ("00:80:77", "Brother"),
        ("30:05:5c", "Brother"),
        ("00:1b:a9", "Brother"),
        ("00:80:91", "Tokyo Electric"),
        ("48:0f:cf", "HP"),
        ("ec:b1:d7", "HP"),
        ("40:b0:34", "HP"),
        ("00:21:5a", "HP"),
        ("3c:d9:2b", "HP"),
        ("2c:41:38", "HP"),
        ("a4:5d:36", "HP"),
        ("94:57:a5", "HP"),
        ("d0:bf:9c", "HP"),
    ];
    let normalized = mac.trim().to_ascii_lowercase().replace('-', ":");
    let prefix = normalized.get(..8)?;
    VENDORS
        .iter()
        .find(|(candidate, _)| *candidate == prefix)
        .map(|(_, vendor)| *vendor)
}

// ---------------------------------------------------------------------------
// Bluetooth

#[derive(Clone, Debug, Default)]
pub struct BluetoothDevice {
    pub name: String,
    pub address: Option<String>,
    pub connected: bool,
    pub paired: bool,
    pub kind: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct BluetoothReport {
    pub powered: Option<bool>,
    pub devices: Vec<BluetoothDevice>,
    pub findings: Vec<Finding>,
    pub note: Option<String>,
}

/// Bluetooth devices this machine knows about or can see, and the ones that
/// look like trackers.
pub fn bluetooth_report() -> BluetoothReport {
    let mut report = platform::bluetooth_report();
    report.devices.sort_by(|left, right| {
        right
            .connected
            .cmp(&left.connected)
            .then_with(|| right.paired.cmp(&left.paired))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    let mut findings = Vec::new();
    for device in &report.devices {
        if let Some(tracker) = tracker_kind(&device.name) {
            let status = if device.paired {
                Status::Good
            } else {
                Status::Warning
            };
            findings.push(Finding {
                status,
                title: if device.paired {
                    format!("your own {tracker} is nearby")
                } else {
                    format!("a {tracker} that isn't yours is within reach")
                },
                detail: if device.paired {
                    format!("\"{}\" is paired with this device, so it is expected here.", device.name)
                } else {
                    format!(
                        "\"{}\" is in Bluetooth range and not paired with this device. Trackers \
                         report their position to their owner; if it stays with you between \
                         places, look for it in bags, cars and coats.",
                        device.name
                    )
                },
            });
        }
    }
    if report.powered == Some(true) && report.devices.iter().any(|device| device.connected) {
        // Nothing to say: the radio is in use.
    } else if report.powered == Some(true) {
        findings.push(Finding {
            status: Status::Unknown,
            title: "Bluetooth is on with nothing connected".into(),
            detail: "a radio that is on can be seen by shops and trackers that log which devices \
                     pass by. Turning it off when it isn't in use makes this device quieter."
                .into(),
        });
    }
    report.findings = findings;
    report
}

/// The tracker family a device name belongs to, if it is one.
pub fn tracker_kind(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    const TRACKERS: &[(&str, &str)] = &[
        ("airtag", "Apple AirTag"),
        ("smarttag", "Samsung SmartTag"),
        ("smart tag", "Samsung SmartTag"),
        ("tile", "Tile tracker"),
        ("chipolo", "Chipolo tracker"),
        ("pebblebee", "Pebblebee tracker"),
        ("eufy security smarttrack", "Eufy tracker"),
        ("smarttrack", "Eufy tracker"),
        ("moto tag", "Motorola tracker"),
        ("galaxy smarttag", "Samsung SmartTag"),
        ("nutale", "Nut tracker"),
        ("itag", "generic iTag tracker"),
    ];
    TRACKERS
        .iter()
        .find(|(needle, _)| lower.contains(needle))
        .map(|(_, kind)| *kind)
}

// ---------------------------------------------------------------------------
// Location and identity

#[derive(Clone, Debug, Default)]
pub struct LocationReport {
    pub host_name: Option<String>,
    pub timezone: Option<String>,
    pub locale: Option<String>,
    /// Whether the operating system's location service is switched on.
    pub location_services: Option<bool>,
    pub findings: Vec<Finding>,
}

/// What this device gives away about where it is, without asking anyone.
pub fn location_report() -> LocationReport {
    let mut report = platform::location_report();
    if report.host_name.is_none() {
        report.host_name = sysinfo::System::host_name();
    }
    if report.timezone.is_none() {
        report.timezone = std::env::var("TZ").ok().filter(|value| !value.is_empty());
    }
    if report.locale.is_none() {
        report.locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|name| std::env::var(name).ok())
            .filter(|value| !value.is_empty());
    }
    let mut findings = Vec::new();
    if let Some(timezone) = &report.timezone {
        findings.push(Finding {
            status: Status::Unknown,
            title: "the clock says where you are".into(),
            detail: format!(
                "the time zone {timezone} goes out with many web requests and app reports, which \
                 places this device in a band of the world before anyone looks at its address."
            ),
        });
    }
    if let Some(host_name) = &report.host_name {
        let personal = host_name.to_lowercase();
        if personal.contains("macbook")
            || personal.contains("iphone")
            || personal.contains("-pc")
            || personal.contains("laptop")
            || personal.split(['-', '_', '.']).count() >= 2
        {
            findings.push(Finding {
                status: Status::Unknown,
                title: "the device name is visible on every network".into(),
                detail: format!(
                    "\"{host_name}\" is announced to routers and nearby devices when this machine \
                     joins a network. A name without your own name in it gives less away."
                ),
            });
        }
    }
    match report.location_services {
        Some(true) => findings.push(Finding {
            status: Status::Unknown,
            title: "location services are on".into(),
            detail: "programs that were granted location can read a position from wifi names \
                     around the device, even without GPS. Check which programs have it in the \
                     system's privacy settings."
                .into(),
        }),
        Some(false) => findings.push(Finding {
            status: Status::Good,
            title: "location services are off".into(),
            detail: "no program can ask the system where this device is.".into(),
        }),
        None => {}
    }
    report.findings = findings;
    report
}

// ---------------------------------------------------------------------------
// Who is watching

/// Something that watches, controls or reaches this device, ranked.
#[derive(Clone, Debug)]
pub struct Watcher {
    pub status: Status,
    pub title: String,
    pub detail: String,
}

/// Everything from the other reports that amounts to someone watching:
/// programs sending data to trackers and telemetry, remote control tools,
/// doors open to the network, autostarts from odd places, nearby trackers.
pub fn watchers(
    traffic: &TrafficReport,
    startup: &[StartupItem],
    ports: &[ListeningPort],
    bluetooth: &BluetoothReport,
    wifi: &WifiReport,
) -> Vec<Watcher> {
    let mut watchers = Vec::new();

    for flow in &traffic.flows {
        let mut per_owner: BTreeMap<String, (DestinationKind, usize, Vec<&str>)> = BTreeMap::new();
        for destination in &flow.destinations {
            if !destination.kind.is_watching() {
                continue;
            }
            let entry = per_owner
                .entry(destination.name())
                .or_insert((destination.kind, 0, Vec::new()));
            entry.1 += destination.connections;
            if let Some(country) = destination.country
                && !entry.2.contains(&country.name)
            {
                entry.2.push(country.name);
            }
        }
        for (owner, (kind, count, countries)) in per_owner {
            let where_to = if countries.is_empty() {
                String::new()
            } else {
                format!(" in {}", countries.join(" and "))
            };
            let (status, verb) = match kind {
                DestinationKind::RemoteAccess => (Status::Warning, "is reachable through"),
                DestinationKind::Advertising => (Status::Warning, "is talking to"),
                _ => (Status::Unknown, "is reporting to"),
            };
            watchers.push(Watcher {
                status,
                title: format!("{} {verb} {owner}", flow.process),
                detail: format!(
                    "{count} connection{} to {} ({}){where_to}.",
                    if count == 1 { "" } else { "s" },
                    owner,
                    kind.label()
                ),
            });
        }
    }

    const REMOTE_CONTROL: &[(&str, &str)] = &[
        ("teamviewer", "TeamViewer"),
        ("anydesk", "AnyDesk"),
        ("rustdesk", "RustDesk"),
        ("logmein", "LogMeIn"),
        ("splashtop", "Splashtop"),
        ("remoting_host", "Chrome Remote Desktop"),
        ("vncserver", "a VNC server"),
        ("x11vnc", "a VNC server"),
        ("tightvnc", "a VNC server"),
        ("realvnc", "a VNC server"),
        ("winvnc", "a VNC server"),
        ("ammyy", "Ammyy Admin"),
        ("parsecd", "Parsec"),
        ("ngrok", "an ngrok tunnel"),
        ("cloudflared", "a Cloudflare tunnel"),
        ("sshd", "an SSH server"),
        ("dropbear", "an SSH server"),
        ("termsrv", "Remote Desktop"),
        ("rdpclip", "Remote Desktop"),
    ];
    let mut named = HashSet::new();
    for flow in &traffic.flows {
        let lower = flow.process.to_ascii_lowercase();
        for (needle, label) in REMOTE_CONTROL {
            if lower.contains(needle) && named.insert(*label) {
                watchers.push(Watcher {
                    status: Status::Warning,
                    title: format!("{label} is running"),
                    detail: format!(
                        "{} can let someone else see or control this device. Fine if you set it \
                         up; close it if you didn't.",
                        flow.process
                    ),
                });
            }
        }
    }

    for port in ports {
        watchers.push(Watcher {
            status: Status::Unknown,
            title: format!(
                "{} is waiting for connections on port {}",
                port.process.as_deref().unwrap_or("a program"),
                port.port
            ),
            detail: format!(
                "{} {}:{} accepts connections from other devices on the network.",
                port.protocol, port.address, port.port
            ),
        });
    }

    for item in startup {
        let command = item.command.to_ascii_lowercase();
        let odd_place = ["/tmp/", "\\temp\\", "\\appdata\\local\\temp", "/downloads/", "\\downloads\\", "/.cache/"]
            .iter()
            .any(|needle| command.contains(needle));
        let hidden = command.contains("-windowstyle hidden")
            || command.contains("-w hidden")
            || command.contains("-enc ")
            || command.contains("-encodedcommand");
        if odd_place || hidden {
            watchers.push(Watcher {
                status: Status::Warning,
                title: format!("{} starts with the device from an odd place", item.name),
                detail: format!(
                    "{} runs at sign-in from {}. Programs that hide in temporary folders or start \
                     hidden shells are how persistence is usually done; if you don't recognise it, \
                     remove it from {}.",
                    item.name, item.command, item.location
                ),
            });
        }
    }

    for finding in bluetooth.findings.iter().chain(wifi.findings.iter()) {
        if finding.status <= Status::Warning {
            watchers.push(Watcher {
                status: finding.status,
                title: finding.title.clone(),
                detail: finding.detail.clone(),
            });
        }
    }

    watchers.sort_by_key(|watcher| watcher.status);
    watchers
}

// ---------------------------------------------------------------------------
// Shared parsing

/// A `Connection` from address strings, dropping what can't be parsed.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn connection(
    protocol: &'static str,
    local: &str,
    remote: &str,
    state: &str,
    pid: Option<u32>,
    process: Option<String>,
    exe: Option<PathBuf>,
) -> Option<Connection> {
    let (local_address, local_port) = crate::checks::parse_socket_address(local)?;
    let (remote_address, remote_port) = crate::checks::parse_socket_address(remote)?;
    Some(Connection {
        protocol,
        local_address: local_address.parse().ok()?,
        local_port,
        remote_address: remote_address.parse().ok()?,
        remote_port,
        state: state.to_string(),
        pid,
        process,
        exe,
    })
}

/// `Name=value` style lines, as `netsh` and `system_profiler` print them.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
fn labelled_value<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let (key, value) = line.split_once(':')?;
    key.trim().eq_ignore_ascii_case(label).then(|| value.trim())
}

#[cfg(unix)]
fn reverse_lookup_unix(address: IpAddr) -> Option<String> {
    use std::ffi::CStr;
    use std::mem;

    let mut host = [0 as libc::c_char; 256];
    // SAFETY: the sockaddr structs are plain data, fully initialised below
    // before being handed to getnameinfo with their exact size, and the host
    // buffer is NUL-terminated by getnameinfo on success.
    let result = unsafe {
        match address {
            IpAddr::V4(v4) => {
                let mut addr: libc::sockaddr_in = mem::zeroed();
                addr.sin_family = libc::AF_INET as libc::sa_family_t;
                addr.sin_addr = libc::in_addr {
                    s_addr: u32::from_ne_bytes(v4.octets()),
                };
                #[cfg(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd"))]
                {
                    addr.sin_len = mem::size_of::<libc::sockaddr_in>() as u8;
                }
                libc::getnameinfo(
                    &addr as *const libc::sockaddr_in as *const libc::sockaddr,
                    mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
                    host.as_mut_ptr(),
                    host.len() as libc::socklen_t,
                    std::ptr::null_mut(),
                    0,
                    libc::NI_NAMEREQD,
                )
            }
            IpAddr::V6(v6) => {
                let mut addr: libc::sockaddr_in6 = mem::zeroed();
                addr.sin6_family = libc::AF_INET6 as libc::sa_family_t;
                addr.sin6_addr = libc::in6_addr {
                    s6_addr: v6.octets(),
                };
                #[cfg(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd"))]
                {
                    addr.sin6_len = mem::size_of::<libc::sockaddr_in6>() as u8;
                }
                libc::getnameinfo(
                    &addr as *const libc::sockaddr_in6 as *const libc::sockaddr,
                    mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
                    host.as_mut_ptr(),
                    host.len() as libc::socklen_t,
                    std::ptr::null_mut(),
                    0,
                    libc::NI_NAMEREQD,
                )
            }
        }
    };
    if result != 0 {
        return None;
    }
    // SAFETY: getnameinfo returned success, so `host` holds a NUL-terminated string.
    let name = unsafe { CStr::from_ptr(host.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    (!name.is_empty() && name != address.to_string()).then_some(name)
}

// ---------------------------------------------------------------------------
// Linux

#[cfg(target_os = "linux")]
use linux as platform;

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::fs;
    use std::net::{Ipv4Addr, Ipv6Addr};

    pub(super) fn reverse_lookup(addresses: &[IpAddr]) -> HashMap<IpAddr, String> {
        reverse_lookup_threaded(addresses, reverse_lookup_unix)
    }

    pub(super) fn connections() -> anyhow::Result<Vec<Connection>> {
        let owners = socket_owners();
        let mut connections = Vec::new();
        for (path, protocol, v6) in [
            ("/proc/net/tcp", "tcp", false),
            ("/proc/net/tcp6", "tcp", true),
            ("/proc/net/udp", "udp", false),
            ("/proc/net/udp6", "udp", true),
        ] {
            let Ok(text) = fs::read_to_string(path) else {
                continue;
            };
            connections.extend(parse_proc_net(&text, protocol, v6, &owners));
        }
        if connections.is_empty() && !Path::new("/proc/net/tcp").exists() {
            anyhow::bail!("/proc/net isn't available");
        }
        Ok(connections)
    }

    /// Socket inode to the process holding it, for the processes this user
    /// may look into.
    fn socket_owners() -> HashMap<u64, (u32, String, Option<PathBuf>)> {
        let mut owners = HashMap::new();
        let Ok(entries) = fs::read_dir("/proc") else {
            return owners;
        };
        for entry in entries.flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            let Ok(fds) = fs::read_dir(entry.path().join("fd")) else {
                continue;
            };
            let mut name = None;
            let mut exe = None;
            for fd in fds.flatten() {
                let Ok(target) = fs::read_link(fd.path()) else {
                    continue;
                };
                let target = target.to_string_lossy();
                let Some(inode) = target
                    .strip_prefix("socket:[")
                    .and_then(|rest| rest.strip_suffix(']'))
                    .and_then(|inode| inode.parse::<u64>().ok())
                else {
                    continue;
                };
                if name.is_none() {
                    name = Some(
                        fs::read_to_string(entry.path().join("comm"))
                            .map(|comm| comm.trim().to_string())
                            .unwrap_or_else(|_| format!("pid {pid}")),
                    );
                    exe = fs::read_link(entry.path().join("exe")).ok();
                }
                owners.insert(inode, (pid, name.clone().unwrap_or_default(), exe.clone()));
            }
        }
        owners
    }

    pub(super) fn parse_proc_net(
        text: &str,
        protocol: &'static str,
        v6: bool,
        owners: &HashMap<u64, (u32, String, Option<PathBuf>)>,
    ) -> Vec<Connection> {
        text.lines()
            .skip(1)
            .filter_map(|line| {
                let columns: Vec<&str> = line.split_whitespace().collect();
                let local = columns.get(1)?;
                let remote = columns.get(2)?;
                let state = u8::from_str_radix(columns.get(3)?, 16).ok()?;
                let inode: u64 = columns.get(9)?.parse().ok()?;
                let (remote_address, remote_port) = parse_hex_address(remote, v6)?;
                let (local_address, local_port) = parse_hex_address(local, v6)?;
                // 0A is LISTEN; a UDP socket with no peer has state 07 and a
                // zero remote address, which is nothing to report.
                if state == 0x0a || remote_port == 0 || remote_address.is_unspecified() {
                    return None;
                }
                let state_name = match state {
                    0x01 => "established",
                    0x02 => "syn sent",
                    0x03 => "syn received",
                    0x04 | 0x05 | 0x06 | 0x08 | 0x09 | 0x0b => "closing",
                    0x07 => "open",
                    _ => "unknown",
                };
                let owner = owners.get(&inode);
                Some(Connection {
                    protocol,
                    local_address,
                    local_port,
                    remote_address,
                    remote_port,
                    state: state_name.to_string(),
                    pid: owner.map(|(pid, _, _)| *pid),
                    process: owner.map(|(_, name, _)| name.clone()),
                    exe: owner.and_then(|(_, _, exe)| exe.clone()),
                })
            })
            .collect()
    }

    /// `0100007F:0016` is 127.0.0.1:22 (each 32-bit word little-endian);
    /// IPv6 is four such words.
    pub(super) fn parse_hex_address(text: &str, v6: bool) -> Option<(IpAddr, u16)> {
        let (address, port) = text.split_once(':')?;
        let port = u16::from_str_radix(port, 16).ok()?;
        if v6 {
            if address.len() != 32 {
                return None;
            }
            let mut octets = [0u8; 16];
            for (word, chunk) in address.as_bytes().chunks(8).enumerate() {
                let value = u32::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
                let bytes = value.to_le_bytes();
                let start = word * 4;
                octets.get_mut(start..start + 4)?.copy_from_slice(&bytes);
            }
            Some((IpAddr::V6(Ipv6Addr::from(octets)), port))
        } else {
            let value = u32::from_str_radix(address, 16).ok()?;
            Some((IpAddr::V4(Ipv4Addr::from(value.to_le_bytes())), port))
        }
    }

    pub(super) fn installed_apps() -> AppInventory {
        let mut apps = Vec::new();
        let home = dirs::home_dir();
        let mut folders: Vec<(PathBuf, &str)> = vec![
            (PathBuf::from("/usr/share/applications"), "system"),
            (PathBuf::from("/usr/local/share/applications"), "system"),
            (PathBuf::from("/var/lib/flatpak/exports/share/applications"), "flatpak"),
            (PathBuf::from("/var/lib/snapd/desktop/applications"), "snap"),
        ];
        if let Some(home) = &home {
            folders.push((home.join(".local/share/applications"), "user"));
            folders.push((
                home.join(".local/share/flatpak/exports/share/applications"),
                "flatpak",
            ));
        }
        let flatpak_versions = flatpak_versions();
        let snap_versions = snap_versions();
        for (folder, source) in folders {
            let Ok(entries) = fs::read_dir(&folder) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|extension| extension != "desktop") {
                    continue;
                }
                let Ok(text) = fs::read_to_string(&path) else {
                    continue;
                };
                let Some(app) = parse_desktop_app(&text, source, &path, &flatpak_versions, &snap_versions)
                else {
                    continue;
                };
                apps.push(app);
            }
        }
        let system_packages = package_count();
        AppInventory {
            apps,
            system_packages,
            note: None,
        }
    }

    fn parse_desktop_app(
        text: &str,
        source: &str,
        path: &Path,
        flatpak_versions: &HashMap<String, (String, String)>,
        snap_versions: &HashMap<String, (String, String)>,
    ) -> Option<InstalledApp> {
        let mut name = None;
        let mut exec = None;
        let mut hidden = false;
        let mut in_entry = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_entry = line == "[Desktop Entry]";
                continue;
            }
            if !in_entry {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "Name" => name = Some(value.trim().to_string()),
                "Exec" => exec = Some(value.trim().to_string()),
                "NoDisplay" | "Hidden" if value.trim().eq_ignore_ascii_case("true") => hidden = true,
                "Type" if value.trim() != "Application" => return None,
                _ => {}
            }
        }
        if hidden {
            return None;
        }
        let name = name?;
        let file_stem = path.file_stem()?.to_string_lossy().into_owned();
        let (version, publisher) = match source {
            "flatpak" => flatpak_versions
                .get(&file_stem)
                .map(|(version, origin)| (Some(version.clone()), Some(origin.clone())))
                .unwrap_or((None, None)),
            "snap" => {
                let snap_name = file_stem.split('_').next().unwrap_or(&file_stem).to_string();
                snap_versions
                    .get(&snap_name)
                    .map(|(version, publisher)| (Some(version.clone()), Some(publisher.clone())))
                    .unwrap_or((None, None))
            }
            _ => (None, None),
        };
        Some(InstalledApp {
            name,
            version,
            publisher,
            source: source.to_string(),
            installed_on: None,
            location: exec,
        })
    }

    fn flatpak_versions() -> HashMap<String, (String, String)> {
        let Ok(output) = run_command(
            "flatpak",
            &["list", "--app", "--columns=application,version,origin"],
            COMMAND_TIMEOUT,
        ) else {
            return HashMap::new();
        };
        output
            .stdout
            .lines()
            .filter_map(|line| {
                let mut columns = line.split('\t');
                let id = columns.next()?.trim().to_string();
                let version = columns.next().unwrap_or("").trim().to_string();
                let origin = columns.next().unwrap_or("").trim().to_string();
                Some((id, (version, origin)))
            })
            .collect()
    }

    fn snap_versions() -> HashMap<String, (String, String)> {
        let Ok(output) = run_command("snap", &["list"], COMMAND_TIMEOUT) else {
            return HashMap::new();
        };
        output
            .stdout
            .lines()
            .skip(1)
            .filter_map(|line| {
                let columns: Vec<&str> = line.split_whitespace().collect();
                let name = columns.first()?.to_string();
                let version = columns.get(1).unwrap_or(&"").to_string();
                let publisher = columns.get(4).unwrap_or(&"").to_string();
                Some((name, (version, publisher)))
            })
            .collect()
    }

    fn package_count() -> Option<usize> {
        if which::which("dpkg-query").is_ok() {
            let output = run_command("dpkg-query", &["-f", "${binary:Package}\n", "-W"], COMMAND_TIMEOUT).ok()?;
            return Some(output.stdout.lines().filter(|line| !line.trim().is_empty()).count());
        }
        if which::which("rpm").is_ok() {
            let output = run_command("rpm", &["-qa"], COMMAND_TIMEOUT).ok()?;
            return Some(output.stdout.lines().filter(|line| !line.trim().is_empty()).count());
        }
        if which::which("pacman").is_ok() {
            let output = run_command("pacman", &["-Qq"], COMMAND_TIMEOUT).ok()?;
            return Some(output.stdout.lines().filter(|line| !line.trim().is_empty()).count());
        }
        None
    }

    pub(super) fn wifi_report() -> WifiReport {
        let mut report = WifiReport::default();
        report.gateway = default_gateway();
        report.dns_servers = dns_servers();
        report.encrypted_dns = resolved_encryption();

        if which::which("nmcli").is_ok() {
            if let Ok(output) = run_command(
                "nmcli",
                &["-t", "-f", "ACTIVE,SSID,BSSID,CHAN,FREQ,RATE,SIGNAL,SECURITY", "dev", "wifi", "list"],
                COMMAND_TIMEOUT,
            ) && output.success()
            {
                let (connected, nearby) = parse_nmcli_wifi(&output.stdout);
                report.connected = connected;
                report.nearby = nearby;
            }
            if let Ok(output) = run_command(
                "nmcli",
                &["-t", "-f", "DEVICE,TYPE,STATE,IP4-ADDRESS", "dev", "show"],
                COMMAND_TIMEOUT,
            ) {
                // `dev show` prints blocks per device; the wifi one names the interface.
                let mut device = None;
                for line in output.stdout.lines() {
                    if let Some(name) = line.strip_prefix("GENERAL.DEVICE:") {
                        device = Some(name.trim().to_string());
                    } else if line.starts_with("GENERAL.TYPE:") && line.contains("wifi") {
                        report.interface = device.clone();
                    } else if report.interface.is_some()
                        && report.interface == device
                        && let Some(address) = line.strip_prefix("IP4.ADDRESS[1]:")
                    {
                        report.local_address = address.split('/').next().and_then(|ip| ip.parse().ok());
                    }
                }
            }
        }
        if report.interface.is_none() {
            report.interface = wireless_interface();
        }
        if report.connected.is_none()
            && let Some(interface) = &report.interface
            && which::which("iw").is_ok()
            && let Ok(output) = run_command("iw", &["dev", interface, "link"], COMMAND_TIMEOUT)
        {
            report.connected = parse_iw_link(&output.stdout);
        }
        if let Some(interface) = &report.interface {
            report.mac_address = fs::read_to_string(format!("/sys/class/net/{interface}/address"))
                .ok()
                .map(|mac| mac.trim().to_string());
            if report.local_address.is_none() {
                report.local_address = interface_address(interface);
            }
        }
        if report.interface.is_none() {
            report.note = Some("no wifi adapter was found; this device may be on a cable".into());
        } else if report.connected.is_none() && which::which("nmcli").is_err() && which::which("iw").is_err() {
            report.note = Some("install NetworkManager (nmcli) or iw for the wifi details".into());
        }
        report
    }

    fn wireless_interface() -> Option<String> {
        let entries = fs::read_dir("/sys/class/net").ok()?;
        entries
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .find(|name| Path::new(&format!("/sys/class/net/{name}/wireless")).exists())
    }

    fn interface_address(interface: &str) -> Option<IpAddr> {
        let output = run_command("ip", &["-4", "-o", "addr", "show", "dev", interface], COMMAND_TIMEOUT).ok()?;
        output.stdout.split_whitespace().skip_while(|word| *word != "inet").nth(1).and_then(|cidr| {
            cidr.split('/').next()?.parse().ok()
        })
    }

    /// nmcli's terse output escapes the colons inside BSSIDs, so fields are
    /// split on colons that aren't preceded by a backslash.
    fn split_terse(line: &str) -> Vec<String> {
        let mut fields = Vec::new();
        let mut current = String::new();
        let mut escaped = false;
        for character in line.chars() {
            match (escaped, character) {
                (true, other) => {
                    current.push(other);
                    escaped = false;
                }
                (false, '\\') => escaped = true,
                (false, ':') => fields.push(std::mem::take(&mut current)),
                (false, other) => current.push(other),
            }
        }
        fields.push(current);
        fields
    }

    pub(super) fn parse_nmcli_wifi(output: &str) -> (Option<WifiLink>, Vec<NearbyNetwork>) {
        let mut connected = None;
        let mut nearby = Vec::new();
        for line in output.lines() {
            let fields = split_terse(line);
            let [active, ssid, bssid, channel, frequency, rate, signal, security] = fields.as_slice() else {
                continue;
            };
            let none_if_empty = |value: &String| (!value.is_empty()).then(|| value.clone());
            if active == "yes" {
                connected = Some(WifiLink {
                    ssid: ssid.clone(),
                    bssid: none_if_empty(bssid),
                    security: Some(if security.is_empty() { "open".to_string() } else { security.clone() }),
                    channel: none_if_empty(channel),
                    frequency: none_if_empty(frequency),
                    signal: none_if_empty(signal).map(|signal| format!("{signal}%")),
                    rate: none_if_empty(rate),
                });
            } else {
                nearby.push(NearbyNetwork {
                    ssid: if ssid.is_empty() { "(hidden network)".to_string() } else { ssid.clone() },
                    bssid: none_if_empty(bssid),
                    signal: signal.parse().ok(),
                    security: Some(if security.is_empty() { "open".to_string() } else { security.clone() }),
                    channel: none_if_empty(channel),
                });
            }
        }
        nearby.sort_by(|left, right| right.signal.cmp(&left.signal));
        (connected, nearby)
    }

    fn parse_iw_link(output: &str) -> Option<WifiLink> {
        let mut link = WifiLink::default();
        let mut bssid = None;
        for line in output.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("Connected to ") {
                bssid = rest.split_whitespace().next().map(str::to_string);
            } else if let Some(ssid) = line.strip_prefix("SSID: ") {
                link.ssid = ssid.to_string();
            } else if let Some(frequency) = line.strip_prefix("freq: ") {
                link.frequency = Some(format!("{frequency} MHz"));
            } else if let Some(signal) = line.strip_prefix("signal: ") {
                link.signal = Some(signal.to_string());
            } else if let Some(rate) = line.strip_prefix("tx bitrate: ") {
                link.rate = Some(rate.to_string());
            }
        }
        if link.ssid.is_empty() {
            return None;
        }
        link.bssid = bssid;
        Some(link)
    }

    fn default_gateway() -> Option<IpAddr> {
        let text = fs::read_to_string("/proc/net/route").ok()?;
        text.lines().skip(1).find_map(|line| {
            let columns: Vec<&str> = line.split_whitespace().collect();
            if *columns.get(1)? != "00000000" {
                return None;
            }
            let gateway = u32::from_str_radix(columns.get(2)?, 16).ok()?;
            Some(IpAddr::V4(Ipv4Addr::from(gateway.to_le_bytes())))
        })
    }

    fn dns_servers() -> Vec<IpAddr> {
        let mut servers: Vec<IpAddr> = Vec::new();
        if which::which("resolvectl").is_ok()
            && let Ok(output) = run_command("resolvectl", &["status"], COMMAND_TIMEOUT)
        {
            for line in output.stdout.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("DNS Servers:") {
                    servers.extend(rest.split_whitespace().filter_map(|word| word.split('%').next()?.parse::<IpAddr>().ok()));
                }
            }
        }
        if servers.is_empty()
            && let Ok(text) = fs::read_to_string("/etc/resolv.conf")
        {
            for line in text.lines() {
                if let Some(rest) = line.trim().strip_prefix("nameserver") {
                    if let Ok(address) = rest.trim().split('%').next().unwrap_or("").parse::<IpAddr>() {
                        // systemd's stub resolver: the real servers were read above.
                        if address == IpAddr::V4(Ipv4Addr::new(127, 0, 0, 53)) {
                            continue;
                        }
                        servers.push(address);
                    }
                }
            }
        }
        servers.dedup();
        servers
    }

    fn resolved_encryption() -> Option<bool> {
        let output = run_command("resolvectl", &["status"], COMMAND_TIMEOUT).ok()?;
        let line = output.stdout.lines().find(|line| line.trim().starts_with("DNSOverTLS:"))?;
        let value = line.split(':').nth(1)?.trim().to_ascii_lowercase();
        Some(value == "yes" || value == "opportunistic")
    }

    pub(super) fn neighbours() -> Vec<(IpAddr, String)> {
        let Ok(text) = fs::read_to_string("/proc/net/arp") else {
            return Vec::new();
        };
        text.lines()
            .skip(1)
            .filter_map(|line| {
                let columns: Vec<&str> = line.split_whitespace().collect();
                let address: IpAddr = columns.first()?.parse().ok()?;
                // Flags 0x0 means the entry is incomplete: nobody answered.
                if *columns.get(2)? == "0x0" {
                    return None;
                }
                Some((address, columns.get(3)?.to_ascii_lowercase()))
            })
            .collect()
    }

    pub(super) fn bluetooth_report() -> BluetoothReport {
        let mut report = BluetoothReport::default();
        if which::which("bluetoothctl").is_err() {
            report.note = Some("bluetoothctl isn't installed, so Bluetooth can't be listed here".into());
            return report;
        }
        if let Ok(output) = run_command("bluetoothctl", &["show"], COMMAND_TIMEOUT) {
            report.powered = output
                .stdout
                .lines()
                .find_map(|line| line.trim().strip_prefix("Powered:"))
                .map(|value| value.trim() == "yes");
        }
        let Ok(output) = run_command("bluetoothctl", &["devices"], COMMAND_TIMEOUT) else {
            return report;
        };
        for line in output.stdout.lines().take(40) {
            let mut words = line.split_whitespace();
            if words.next() != Some("Device") {
                continue;
            }
            let Some(address) = words.next() else { continue };
            let name = words.collect::<Vec<_>>().join(" ");
            let mut device = BluetoothDevice {
                name: if name.is_empty() { address.to_string() } else { name },
                address: Some(address.to_string()),
                ..Default::default()
            };
            if let Ok(info) = run_command("bluetoothctl", &["info", address], Duration::from_secs(3)) {
                for line in info.stdout.lines() {
                    let line = line.trim();
                    if let Some(value) = line.strip_prefix("Connected:") {
                        device.connected = value.trim() == "yes";
                    } else if let Some(value) = line.strip_prefix("Paired:") {
                        device.paired = value.trim() == "yes";
                    } else if let Some(value) = line.strip_prefix("Icon:") {
                        device.kind = Some(value.trim().replace('-', " "));
                    }
                }
            }
            report.devices.push(device);
        }
        Ok::<(), ()>(()).ok();
        report
    }

    pub(super) fn location_report() -> LocationReport {
        let mut report = LocationReport::default();
        report.timezone = fs::read_to_string("/etc/timezone")
            .ok()
            .map(|zone| zone.trim().to_string())
            .filter(|zone| !zone.is_empty())
            .or_else(|| {
                fs::read_link("/etc/localtime").ok().and_then(|target| {
                    let target = target.to_string_lossy().into_owned();
                    target.split("zoneinfo/").nth(1).map(str::to_string)
                })
            });
        if which::which("systemctl").is_ok()
            && let Ok(output) = run_command("systemctl", &["is-active", "geoclue"], COMMAND_TIMEOUT)
        {
            report.location_services = Some(output.stdout.trim() == "active");
        }
        report
    }
}

// ---------------------------------------------------------------------------
// Windows

#[cfg(windows)]
use windows as platform;

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::run_powershell;

    /// PowerShell resolves all the names in one process, since starting
    /// PowerShell once costs more than the lookups.
    pub(super) fn reverse_lookup(addresses: &[IpAddr]) -> HashMap<IpAddr, String> {
        let list = addresses
            .iter()
            .map(|address| format!("'{address}'"))
            .collect::<Vec<_>>()
            .join(",");
        let script = format!(
            "foreach ($ip in @({list})) {{ try {{ $h = [System.Net.Dns]::GetHostEntry($ip).HostName; \
             if ($h -and $h -ne $ip) {{ Write-Output \"$ip|$h\" }} }} catch {{}} }}"
        );
        let Ok(output) = run_powershell(&script, REVERSE_LOOKUP_BUDGET + Duration::from_secs(6)) else {
            return HashMap::new();
        };
        output
            .stdout
            .lines()
            .filter_map(|line| {
                let (address, host) = line.trim().split_once('|')?;
                Some((address.parse().ok()?, host.to_string()))
            })
            .collect()
    }

    pub(super) fn connections() -> anyhow::Result<Vec<Connection>> {
        let script = "$procs = @{}; Get-Process | ForEach-Object { $procs[$_.Id] = @($_.ProcessName, $_.Path) }; \
            Get-NetTCPConnection -ErrorAction SilentlyContinue | Where-Object { $_.State -ne 'Listen' -and $_.RemoteAddress -ne '0.0.0.0' -and $_.RemoteAddress -ne '::' } | ForEach-Object { \
            $p = $procs[[int]$_.OwningProcess]; \
            Write-Output (\"tcp|\" + $_.LocalAddress + \"|\" + $_.LocalPort + \"|\" + $_.RemoteAddress + \"|\" + $_.RemotePort + \"|\" + $_.State + \"|\" + $_.OwningProcess + \"|\" + $p[0] + \"|\" + $p[1]) }";
        let output = run_powershell(script, COMMAND_TIMEOUT + Duration::from_secs(10))?;
        if !output.success() && output.stdout.trim().is_empty() {
            anyhow::bail!("PowerShell {}", output.error_summary());
        }
        Ok(output
            .stdout
            .lines()
            .filter_map(|line| {
                let columns: Vec<&str> = line.trim().split('|').collect();
                let [_, local, local_port, remote, remote_port, state, pid, name, path] = columns.as_slice() else {
                    return None;
                };
                let strip = |address: &str| address.split('%').next().unwrap_or(address).to_string();
                Some(Connection {
                    protocol: "tcp",
                    local_address: strip(local).parse().ok()?,
                    local_port: local_port.parse().ok()?,
                    remote_address: strip(remote).parse().ok()?,
                    remote_port: remote_port.parse().ok()?,
                    state: state.to_ascii_lowercase(),
                    pid: pid.parse().ok(),
                    process: (!name.is_empty()).then(|| name.to_string()),
                    exe: (!path.is_empty()).then(|| PathBuf::from(path)),
                })
            })
            .collect())
    }

    pub(super) fn installed_apps() -> AppInventory {
        use windows_registry::{CURRENT_USER, Key, LOCAL_MACHINE};
        const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
        const UNINSTALL_32: &str = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";

        fn from_key(root: &Key, path: &str, source: &str, apps: &mut Vec<InstalledApp>) {
            let Ok(key) = root.open(path) else { return };
            let Ok(names) = key.keys() else { return };
            for name in names {
                let Ok(entry) = key.open(&name) else { continue };
                let Ok(display) = entry.get_string("DisplayName") else { continue };
                if display.trim().is_empty() {
                    continue;
                }
                let system_component = entry.get_u32("SystemComponent").map(|value| value == 1).unwrap_or(false);
                if system_component {
                    continue;
                }
                apps.push(InstalledApp {
                    name: display.trim().to_string(),
                    version: entry.get_string("DisplayVersion").ok().filter(|value| !value.is_empty()),
                    publisher: entry.get_string("Publisher").ok().filter(|value| !value.is_empty()),
                    source: source.to_string(),
                    installed_on: entry.get_string("InstallDate").ok().filter(|value| !value.is_empty()).map(|date| {
                        if date.len() == 8 {
                            format!("{}-{}-{}", &date[..4], &date[4..6], &date[6..])
                        } else {
                            date
                        }
                    }),
                    location: entry.get_string("InstallLocation").ok().filter(|value| !value.is_empty()),
                });
            }
        }

        let mut apps = Vec::new();
        from_key(LOCAL_MACHINE, UNINSTALL, "installer", &mut apps);
        from_key(LOCAL_MACHINE, UNINSTALL_32, "installer", &mut apps);
        from_key(CURRENT_USER, UNINSTALL, "installer (this user)", &mut apps);

        let store = run_powershell(
            "Get-AppxPackage -ErrorAction SilentlyContinue | Where-Object { -not $_.IsFramework -and $_.SignatureKind -eq 'Store' } | ForEach-Object { Write-Output ($_.Name + '|' + $_.Version + '|' + $_.Publisher + '|' + $_.InstallLocation) }",
            COMMAND_TIMEOUT + Duration::from_secs(10),
        );
        let mut note = None;
        match store {
            Ok(output) => {
                for line in output.stdout.lines() {
                    let columns: Vec<&str> = line.trim().split('|').collect();
                    let [name, version, publisher, location] = columns.as_slice() else { continue };
                    let publisher = publisher
                        .split(',')
                        .find_map(|part| part.trim().strip_prefix("CN=").or_else(|| part.trim().strip_prefix("O=")))
                        .unwrap_or(publisher)
                        .trim_matches('"')
                        .to_string();
                    apps.push(InstalledApp {
                        name: name.rsplit('.').next().unwrap_or(name).to_string(),
                        version: (!version.is_empty()).then(|| version.to_string()),
                        publisher: (!publisher.is_empty()).then_some(publisher),
                        source: "Microsoft Store".to_string(),
                        installed_on: None,
                        location: (!location.is_empty()).then(|| location.to_string()),
                    });
                }
            }
            Err(error) => note = Some(format!("Store apps couldn't be listed: {error:#}")),
        }
        AppInventory {
            apps,
            system_packages: None,
            note,
        }
    }

    pub(super) fn wifi_report() -> WifiReport {
        let mut report = WifiReport::default();
        if let Ok(output) = run_command("netsh", &["wlan", "show", "interfaces"], COMMAND_TIMEOUT) {
            let mut link = WifiLink::default();
            for line in output.stdout.lines() {
                if let Some(value) = labelled_value(line, "Name") {
                    report.interface = Some(value.to_string());
                } else if let Some(value) = labelled_value(line, "Physical address") {
                    report.mac_address = Some(value.to_string());
                } else if let Some(value) = labelled_value(line, "SSID") {
                    link.ssid = value.to_string();
                } else if let Some(value) = labelled_value(line, "BSSID") {
                    link.bssid = Some(value.to_string());
                } else if let Some(value) = labelled_value(line, "Authentication") {
                    link.security = Some(value.to_string());
                } else if let Some(value) = labelled_value(line, "Channel") {
                    link.channel = Some(value.to_string());
                } else if let Some(value) = labelled_value(line, "Signal") {
                    link.signal = Some(value.to_string());
                } else if let Some(value) = labelled_value(line, "Receive rate (Mbps)") {
                    link.rate = Some(format!("{value} Mbit/s"));
                } else if let Some(value) = labelled_value(line, "Band") {
                    link.frequency = Some(value.to_string());
                }
            }
            if !link.ssid.is_empty() {
                report.connected = Some(link);
            }
        } else {
            report.note = Some("netsh isn't available, so the wifi can't be described".into());
        }
        if let Ok(output) = run_command("netsh", &["wlan", "show", "networks", "mode=bssid"], COMMAND_TIMEOUT) {
            let mut current: Option<NearbyNetwork> = None;
            for line in output.stdout.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("SSID ") {
                    if let Some(network) = current.take() {
                        report.nearby.push(network);
                    }
                    let ssid = trimmed.split_once(':').map(|(_, value)| value.trim()).unwrap_or("");
                    current = Some(NearbyNetwork {
                        ssid: if ssid.is_empty() { "(hidden network)".into() } else { ssid.to_string() },
                        ..Default::default()
                    });
                } else if let Some(network) = current.as_mut() {
                    if let Some(value) = labelled_value(trimmed, "Authentication") {
                        network.security = Some(value.to_string());
                    } else if trimmed.starts_with("BSSID") {
                        if network.bssid.is_none() {
                            network.bssid = trimmed.split_once(':').map(|(_, value)| value.trim().to_string());
                        }
                    } else if let Some(value) = labelled_value(trimmed, "Signal") {
                        if network.signal.is_none() {
                            network.signal = value.trim_end_matches('%').parse().ok();
                        }
                    } else if let Some(value) = labelled_value(trimmed, "Channel") {
                        if network.channel.is_none() {
                            network.channel = Some(value.to_string());
                        }
                    }
                }
            }
            if let Some(network) = current.take() {
                report.nearby.push(network);
            }
            let connected = report.connected.as_ref().map(|link| link.ssid.clone());
            report.nearby.retain(|network| Some(&network.ssid) != connected.as_ref());
            report.nearby.sort_by(|left, right| right.signal.cmp(&left.signal));
        }
        if let Ok(output) = run_powershell(
            "$c = Get-NetIPConfiguration -ErrorAction SilentlyContinue | Where-Object { $_.IPv4DefaultGateway -ne $null } | Select-Object -First 1; \
             if ($c) { Write-Output ('gateway|' + $c.IPv4DefaultGateway.NextHop); Write-Output ('address|' + $c.IPv4Address.IPAddress); foreach ($d in $c.DNSServer.ServerAddresses) { Write-Output ('dns|' + $d) } }; \
             try { $e = Get-DnsClientDohServerAddress -ErrorAction Stop; if ($e) { Write-Output 'doh|yes' } } catch {}",
            COMMAND_TIMEOUT,
        ) {
            for line in output.stdout.lines() {
                let Some((key, value)) = line.trim().split_once('|') else { continue };
                match key {
                    "gateway" => report.gateway = value.parse().ok(),
                    "address" => report.local_address = value.parse().ok(),
                    "dns" => {
                        if let Ok(address) = value.split('%').next().unwrap_or(value).parse() {
                            report.dns_servers.push(address);
                        }
                    }
                    "doh" => report.encrypted_dns = Some(true),
                    _ => {}
                }
            }
        }
        report
    }

    pub(super) fn neighbours() -> Vec<(IpAddr, String)> {
        let Ok(output) = run_command("arp", &["-a"], COMMAND_TIMEOUT) else {
            return Vec::new();
        };
        output
            .stdout
            .lines()
            .filter_map(|line| {
                let columns: Vec<&str> = line.split_whitespace().collect();
                let [address, mac, kind] = columns.as_slice() else { return None };
                if !kind.eq_ignore_ascii_case("dynamic") {
                    return None;
                }
                Some((address.parse().ok()?, mac.replace('-', ":").to_ascii_lowercase()))
            })
            .collect()
    }

    pub(super) fn bluetooth_report() -> BluetoothReport {
        let mut report = BluetoothReport::default();
        let script = "Get-PnpDevice -Class Bluetooth -ErrorAction SilentlyContinue | ForEach-Object { \
            Write-Output ($_.FriendlyName + '|' + $_.Status + '|' + $_.Present + '|' + $_.InstanceId) }";
        match run_powershell(script, COMMAND_TIMEOUT + Duration::from_secs(5)) {
            Ok(output) => {
                for line in output.stdout.lines() {
                    let columns: Vec<&str> = line.trim().split('|').collect();
                    let [name, status, present, instance] = columns.as_slice() else { continue };
                    let lower = name.to_ascii_lowercase();
                    let is_radio = lower.contains("adapter")
                        || lower.contains("radio")
                        || lower.contains("enumerator")
                        || lower.contains("transport")
                        || lower.contains("service")
                        || lower.contains("le generic");
                    if is_radio {
                        if lower.contains("adapter") || lower.contains("radio") {
                            report.powered = Some(status.eq_ignore_ascii_case("OK"));
                        }
                        continue;
                    }
                    let paired = instance.starts_with("BTHENUM") || instance.starts_with("BTHLE");
                    report.devices.push(BluetoothDevice {
                        name: name.to_string(),
                        address: instance
                            .rsplit('_')
                            .next()
                            .filter(|tail| tail.len() == 12)
                            .map(|tail| {
                                tail.as_bytes()
                                    .chunks(2)
                                    .map(|pair| String::from_utf8_lossy(pair).into_owned())
                                    .collect::<Vec<_>>()
                                    .join(":")
                            }),
                        connected: status.eq_ignore_ascii_case("OK") && present.eq_ignore_ascii_case("True"),
                        paired,
                        kind: None,
                    });
                }
            }
            Err(error) => report.note = Some(format!("Bluetooth couldn't be listed: {error:#}")),
        }
        report
    }

    pub(super) fn location_report() -> LocationReport {
        use windows_registry::LOCAL_MACHINE;
        let mut report = LocationReport::default();
        if let Ok(output) = run_command("tzutil", &["/g"], COMMAND_TIMEOUT) {
            let zone = output.stdout.trim();
            if !zone.is_empty() {
                report.timezone = Some(zone.to_string());
            }
        }
        if let Ok(output) = run_powershell("(Get-Culture).Name", COMMAND_TIMEOUT) {
            let culture = output.stdout.trim();
            if !culture.is_empty() {
                report.locale = Some(culture.to_string());
            }
        }
        report.location_services = LOCAL_MACHINE
            .open(r"SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location")
            .and_then(|key| key.get_string("Value"))
            .ok()
            .map(|value| value.eq_ignore_ascii_case("Allow"));
        report
    }
}

// ---------------------------------------------------------------------------
// macOS

#[cfg(target_os = "macos")]
use macos as platform;

#[cfg(target_os = "macos")]
mod macos {
    use super::*;

    pub(super) fn reverse_lookup(addresses: &[IpAddr]) -> HashMap<IpAddr, String> {
        reverse_lookup_threaded(addresses, reverse_lookup_unix)
    }

    pub(super) fn connections() -> anyhow::Result<Vec<Connection>> {
        let output = run_command("lsof", &["-nP", "-iTCP", "-iUDP"], COMMAND_TIMEOUT)?;
        if !output.success() && output.stdout.trim().is_empty() {
            anyhow::bail!("lsof {}", output.error_summary());
        }
        Ok(output
            .stdout
            .lines()
            .skip(1)
            .filter_map(|line| {
                let columns: Vec<&str> = line.split_whitespace().collect();
                let command = columns.first()?.replace("\\x20", " ");
                let pid: u32 = columns.get(1)?.parse().ok()?;
                let protocol_column = columns.iter().position(|column| *column == "TCP" || *column == "UDP")?;
                let protocol = if columns[protocol_column] == "TCP" { "tcp" } else { "udp" };
                let name = columns.get(protocol_column + 1)?;
                let state = columns
                    .get(protocol_column + 2)
                    .map(|state| state.trim_matches(|character| character == '(' || character == ')').to_ascii_lowercase())
                    .unwrap_or_else(|| "open".to_string());
                if state == "listen" {
                    return None;
                }
                let (local, remote) = name.split_once("->")?;
                connection(protocol, local, remote, &state, Some(pid), Some(command), None)
            })
            .collect())
    }

    pub(super) fn installed_apps() -> AppInventory {
        let mut apps = Vec::new();
        let mut folders = vec![
            (PathBuf::from("/Applications"), "Applications"),
            (PathBuf::from("/Applications/Utilities"), "Applications"),
        ];
        if let Some(home) = dirs::home_dir() {
            folders.push((home.join("Applications"), "Applications (this user)"));
        }
        for (folder, source) in folders {
            let Ok(entries) = std::fs::read_dir(&folder) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|extension| extension != "app") {
                    continue;
                }
                let name = path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let plist = path.join("Contents/Info.plist");
                let read = |key: &str| {
                    run_command(
                        "plutil",
                        &["-extract", key, "raw", "-o", "-", &plist.to_string_lossy()],
                        Duration::from_secs(3),
                    )
                    .ok()
                    .filter(|output| output.success())
                    .map(|output| output.stdout.trim().to_string())
                    .filter(|value| !value.is_empty())
                };
                let identifier = read("CFBundleIdentifier");
                let publisher = identifier.as_deref().and_then(|identifier| {
                    let mut parts = identifier.split('.');
                    let top = parts.next()?;
                    let organisation = parts.next()?;
                    if top == "com" || top == "org" || top == "net" || top == "io" || top == "app" {
                        Some(organisation.to_string())
                    } else {
                        None
                    }
                });
                let installed_on = std::fs::metadata(&path)
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .map(|time| {
                        let seconds = time
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|duration| duration.as_secs() as i64)
                            .unwrap_or(0);
                        chrono::DateTime::from_timestamp(seconds, 0)
                            .map(|date| date.format("%Y-%m-%d").to_string())
                            .unwrap_or_default()
                    });
                apps.push(InstalledApp {
                    name,
                    version: read("CFBundleShortVersionString"),
                    publisher,
                    source: source.to_string(),
                    installed_on,
                    location: Some(path.display().to_string()),
                });
            }
        }
        let system_packages = if which::which("brew").is_ok() {
            run_command("brew", &["list", "--formula"], COMMAND_TIMEOUT)
                .ok()
                .map(|output| output.stdout.lines().filter(|line| !line.trim().is_empty()).count())
        } else {
            None
        };
        AppInventory {
            apps,
            system_packages,
            note: None,
        }
    }

    pub(super) fn wifi_report() -> WifiReport {
        let mut report = WifiReport::default();
        if let Ok(output) = run_command("system_profiler", &["SPAirPortDataType", "-json"], Duration::from_secs(20))
            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&output.stdout)
        {
            let interfaces = json
                .get("SPAirPortDataType")
                .and_then(|items| items.get(0))
                .and_then(|item| item.get("spairport_airport_interfaces"))
                .and_then(|interfaces| interfaces.as_array())
                .cloned()
                .unwrap_or_default();
            for interface in interfaces {
                let name = interface.get("_name").and_then(|name| name.as_str()).unwrap_or("").to_string();
                if name.is_empty() {
                    continue;
                }
                report.interface = Some(name.clone());
                if let Some(current) = interface.get("spairport_current_network_information") {
                    let field = |key: &str| current.get(key).and_then(|value| value.as_str()).map(str::to_string);
                    let ssid = field("_name").unwrap_or_default();
                    if !ssid.is_empty() {
                        report.connected = Some(WifiLink {
                            ssid,
                            bssid: None,
                            security: field("spairport_security_mode").map(|mode| mode.replace("spairport_security_mode_", "").replace('_', " ")),
                            channel: field("spairport_network_channel"),
                            frequency: None,
                            signal: field("spairport_signal_noise"),
                            rate: field("spairport_network_rate").map(|rate| format!("{rate} Mbit/s")),
                        });
                    }
                }
                if let Some(others) = interface
                    .get("spairport_airport_other_local_wireless_networks")
                    .and_then(|others| others.as_array())
                {
                    for other in others {
                        let field = |key: &str| other.get(key).and_then(|value| value.as_str()).map(str::to_string);
                        report.nearby.push(NearbyNetwork {
                            ssid: field("_name").unwrap_or_else(|| "(hidden network)".into()),
                            bssid: None,
                            signal: field("spairport_signal_noise")
                                .and_then(|text| text.split_whitespace().next()?.parse().ok()),
                            security: field("spairport_security_mode").map(|mode| mode.replace("spairport_security_mode_", "").replace('_', " ")),
                            channel: field("spairport_network_channel"),
                        });
                    }
                }
                break;
            }
        } else {
            report.note = Some("system_profiler couldn't describe the wifi".into());
        }
        if let Some(interface) = &report.interface
            && let Ok(output) = run_command("ifconfig", &[interface], COMMAND_TIMEOUT)
        {
            for line in output.stdout.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("ether ") {
                    report.mac_address = rest.split_whitespace().next().map(str::to_string);
                } else if let Some(rest) = line.strip_prefix("inet ") {
                    report.local_address = rest.split_whitespace().next().and_then(|ip| ip.parse().ok());
                }
            }
        }
        if let Ok(output) = run_command("route", &["-n", "get", "default"], COMMAND_TIMEOUT) {
            report.gateway = output
                .stdout
                .lines()
                .find_map(|line| labelled_value(line, "gateway"))
                .and_then(|value| value.parse().ok());
        }
        if let Ok(output) = run_command("scutil", &["--dns"], COMMAND_TIMEOUT) {
            for line in output.stdout.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("nameserver[") {
                    if let Some((_, address)) = rest.split_once(':') {
                        if let Ok(address) = address.trim().parse::<IpAddr>() {
                            if !report.dns_servers.contains(&address) {
                                report.dns_servers.push(address);
                            }
                        }
                    }
                }
            }
        }
        report
    }

    pub(super) fn neighbours() -> Vec<(IpAddr, String)> {
        let Ok(output) = run_command("arp", &["-an"], COMMAND_TIMEOUT) else {
            return Vec::new();
        };
        output
            .stdout
            .lines()
            .filter_map(|line| {
                let start = line.find('(')? + 1;
                let end = line.find(')')?;
                let address: IpAddr = line.get(start..end)?.parse().ok()?;
                let mac = line.split_whitespace().skip_while(|word| *word != "at").nth(1)?;
                if mac == "(incomplete)" {
                    return None;
                }
                // macOS drops leading zeros: 0:1a:2b becomes 00:1a:2b here.
                let mac = mac
                    .split(':')
                    .map(|part| if part.len() == 1 { format!("0{part}") } else { part.to_string() })
                    .collect::<Vec<_>>()
                    .join(":")
                    .to_ascii_lowercase();
                Some((address, mac))
            })
            .collect()
    }

    pub(super) fn bluetooth_report() -> BluetoothReport {
        let mut report = BluetoothReport::default();
        let Ok(output) = run_command("system_profiler", &["SPBluetoothDataType", "-json"], Duration::from_secs(20)) else {
            report.note = Some("system_profiler couldn't describe Bluetooth".into());
            return report;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&output.stdout) else {
            return report;
        };
        let Some(item) = json.get("SPBluetoothDataType").and_then(|items| items.get(0)) else {
            return report;
        };
        report.powered = item
            .get("controller_properties")
            .and_then(|controller| controller.get("controller_state"))
            .and_then(|state| state.as_str())
            .map(|state| state.contains("on"));
        for (key, connected) in [("device_connected", true), ("device_not_connected", false)] {
            let Some(devices) = item.get(key).and_then(|devices| devices.as_array()) else { continue };
            for device in devices {
                let Some(object) = device.as_object() else { continue };
                for (name, properties) in object {
                    report.devices.push(BluetoothDevice {
                        name: name.clone(),
                        address: properties.get("device_address").and_then(|value| value.as_str()).map(str::to_string),
                        connected,
                        paired: true,
                        kind: properties
                            .get("device_minorType")
                            .and_then(|value| value.as_str())
                            .map(str::to_string),
                    });
                }
            }
        }
        report
    }

    pub(super) fn location_report() -> LocationReport {
        let mut report = LocationReport::default();
        report.timezone = std::fs::read_link("/etc/localtime").ok().and_then(|target| {
            let target = target.to_string_lossy().into_owned();
            target.split("zoneinfo/").nth(1).map(str::to_string)
        });
        if let Ok(output) = run_command(
            "defaults",
            &[
                "read",
                "/var/db/locationd/Library/Preferences/ByHost/com.apple.locationd",
                "LocationServicesEnabled",
            ],
            COMMAND_TIMEOUT,
        ) && output.success()
        {
            report.location_services = Some(output.stdout.trim() == "1");
        }
        report
    }
}

// ---------------------------------------------------------------------------
// Anything else

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod platform {
    use super::*;

    pub(super) fn reverse_lookup(_addresses: &[IpAddr]) -> HashMap<IpAddr, String> {
        HashMap::new()
    }

    pub(super) fn connections() -> anyhow::Result<Vec<Connection>> {
        anyhow::bail!("listing connections isn't supported on this operating system")
    }

    pub(super) fn installed_apps() -> AppInventory {
        AppInventory {
            note: Some("listing programs isn't supported on this operating system".into()),
            ..Default::default()
        }
    }

    pub(super) fn wifi_report() -> WifiReport {
        WifiReport {
            note: Some("wifi details aren't supported on this operating system".into()),
            ..Default::default()
        }
    }

    pub(super) fn neighbours() -> Vec<(IpAddr, String)> {
        Vec::new()
    }

    pub(super) fn bluetooth_report() -> BluetoothReport {
        BluetoothReport {
            note: Some("Bluetooth details aren't supported on this operating system".into()),
            ..Default::default()
        }
    }

    pub(super) fn location_report() -> LocationReport {
        LocationReport::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection_to(process: &str, remote: &str, port: u16) -> Connection {
        Connection {
            protocol: "tcp",
            local_address: "192.168.1.20".parse().unwrap(),
            local_port: 50000,
            remote_address: remote.parse().unwrap(),
            remote_port: port,
            state: "established".into(),
            pid: Some(42),
            process: Some(process.into()),
            exe: None,
        }
    }

    #[test]
    fn classifies_hosts_by_longest_suffix() {
        assert_eq!(
            classify(Some("www.google-analytics.com"), 443),
            (Some("Google Analytics"), DestinationKind::Telemetry)
        );
        assert_eq!(
            classify(Some("lhr25s33-in-f14.1e100.net"), 443),
            (Some("Google"), DestinationKind::Service)
        );
        assert_eq!(
            classify(Some("ec2-3-4-5-6.compute-1.amazonaws.com"), 443),
            (Some("Amazon Web Services"), DestinationKind::Cloud)
        );
        assert_eq!(classify(None, 5228), (None, DestinationKind::Messaging));
        assert_eq!(classify(Some("something.example"), 443), (None, DestinationKind::Unknown));
    }

    #[test]
    fn groups_connections_by_program_and_country() {
        let connections = vec![
            connection_to("firefox", "8.8.8.8", 443),
            connection_to("firefox", "8.8.8.8", 443),
            connection_to("firefox", "8.8.4.4", 443),
            connection_to("spotify", "127.0.0.1", 4070),
            connection_to("spotify", "192.168.1.1", 53),
        ];
        let resolve = |addresses: &[IpAddr]| {
            addresses
                .iter()
                .filter(|address| address.to_string() == "8.8.8.8")
                .map(|address| (*address, "dns.google".to_string()))
                .collect()
        };
        let report = traffic_report_from(connections, &resolve);
        assert_eq!(report.total_connections, 3);
        assert_eq!(report.flows.len(), 1);
        let firefox = &report.flows[0];
        assert_eq!(firefox.process, "firefox");
        assert_eq!(firefox.destinations.len(), 2);
        assert_eq!(firefox.connection_count(), 3);
        assert_eq!(report.countries.first().map(|(country, count)| (country.code, *count)), Some(("US", 3)));
        assert!(report.note.is_none());
    }

    #[test]
    fn local_addresses_are_not_remote() {
        assert!(!is_remote("10.1.2.3".parse().unwrap()));
        assert!(!is_remote("100.100.1.1".parse().unwrap()));
        assert!(!is_remote("fe80::1".parse().unwrap()));
        assert!(is_remote("93.184.216.34".parse().unwrap()));
        assert!(is_remote("2606:4700::1".parse().unwrap()));
    }

    #[test]
    fn recognises_trackers_and_vendors() {
        assert_eq!(tracker_kind("Ash's AirTag"), Some("Apple AirTag"));
        assert_eq!(tracker_kind("Galaxy SmartTag2"), Some("Samsung SmartTag"));
        assert_eq!(tracker_kind("WH-1000XM4"), None);
        assert_eq!(vendor_of_mac("B8:27:EB:12:34:56"), Some("Raspberry Pi"));
        assert_eq!(vendor_of_mac("b8-27-eb-12-34-56"), Some("Raspberry Pi"));
        assert_eq!(vendor_of_mac("00:00:00:00:00:00"), None);
        assert_eq!(locally_administered("02:42:ac:11:00:02"), Some(true));
        assert_eq!(locally_administered("b8:27:eb:12:34:56"), Some(false));
    }

    #[test]
    fn open_wifi_is_a_problem() {
        let report = WifiReport {
            connected: Some(WifiLink {
                ssid: "Cafe Free".into(),
                security: Some("open".into()),
                ..Default::default()
            }),
            nearby: vec![NearbyNetwork {
                ssid: "Cafe Free".into(),
                security: Some("WPA2".into()),
                ..Default::default()
            }],
            dns_servers: vec!["8.8.8.8".parse().unwrap()],
            encrypted_dns: Some(false),
            ..Default::default()
        };
        let findings = wifi_findings(&report);
        assert!(findings.iter().any(|finding| finding.status == Status::Bad));
        assert!(findings.iter().any(|finding| finding.title.contains("second network")));
        assert!(findings.iter().any(|finding| finding.detail.contains("Google")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parses_proc_net_lines() {
        let text = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
                    0: 1400A8C0:C350 08080808:01BB 01 00000000:00000000 00:00000000 00000000  1000        0 12345 1 0000000000000000 20 4 30 10 -1\n\
                    1: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 3096 1 0000000000000000 100 0 0 10 0\n";
        let mut owners = HashMap::new();
        owners.insert(12345u64, (7u32, "curl".to_string(), None));
        let connections = linux::parse_proc_net(text, "tcp", false, &owners);
        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].remote_address.to_string(), "8.8.8.8");
        assert_eq!(connections[0].remote_port, 443);
        assert_eq!(connections[0].local_address.to_string(), "192.168.0.20");
        assert_eq!(connections[0].process.as_deref(), Some("curl"));
        let (address, port) = linux::parse_hex_address("0000000000000000FFFF00000100007F:0016", true).unwrap();
        assert_eq!(address.to_string(), "::ffff:127.0.0.1");
        assert_eq!(port, 22);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parses_nmcli_wifi_rows() {
        let output = "yes:Home:AA\\:BB\\:CC\\:DD\\:EE\\:FF:36:5180 MHz:540 Mbit/s:82:WPA2\n\
                      no:Cafe:11\\:22\\:33\\:44\\:55\\:66:6:2437 MHz:130 Mbit/s:40:\n";
        let (connected, nearby) = linux::parse_nmcli_wifi(output);
        let connected = connected.unwrap();
        assert_eq!(connected.ssid, "Home");
        assert_eq!(connected.bssid.as_deref(), Some("AA:BB:CC:DD:EE:FF"));
        assert_eq!(nearby.len(), 1);
        assert_eq!(nearby[0].security.as_deref(), Some("open"));
        assert_eq!(nearby[0].signal, Some(40));
    }
}
