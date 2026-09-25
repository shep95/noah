use anyhow::{Context as _, Result};
use client::Client;
use db::kvp::KeyValueStore;
use futures_lite::StreamExt;
use gpui::{
    App, AppContext as _, AsyncApp, Context, Entity, EventEmitter, Global, Task, TaskExt, Window,
    actions,
};
use http_client::{HttpClient, HttpClientWithUrl};
use paths::remote_servers_dir;
use release_channel::ReleaseChannel;
use semver::Version;
use serde::{Deserialize, Serialize};
use settings::{RegisterSetting, Settings, SettingsStore};
use smol::fs::File;
use smol::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
};
use std::{
    env::{
        self,
        consts::{ARCH, OS},
    },
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};
use util::command::new_command;
use workspace::Workspace;

pub mod noah_release;

const SHOULD_SHOW_UPDATE_NOTIFICATION_KEY: &str = "auto-updater-should-show-updated-notification";

/// A newer noah exists but this install can't update itself (a .deb, a folder
/// noah can't write to, or a system without a build yet). Unlike a failed
/// network check, this is shown even when the check ran on its own.
#[derive(Debug)]
struct ManualUpdateNeeded(String);

impl std::fmt::Display for ManualUpdateNeeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ManualUpdateNeeded {}
const POLL_INTERVAL: Duration = Duration::from_secs(60 * 60);
const NIGHTLY_POLL_INTERVAL: Duration = Duration::from_secs(15 * 60);
const REMOTE_SERVER_CACHE_LIMIT: usize = 5;

actions!(
    auto_update,
    [
        /// Checks for available updates.
        Check,
        /// Dismisses the update error message.
        DismissMessage,
        /// Opens the release notes for the current version in a browser.
        ViewReleaseNotes,
    ]
);

#[derive(Serialize, Debug)]
pub struct AssetQuery<'a> {
    asset: &'a str,
    os: &'a str,
    arch: &'a str,
    metrics_id: Option<&'a str>,
    system_id: Option<&'a str>,
    is_staff: Option<bool>,
}

#[derive(Clone, Debug)]
pub enum AutoUpdateStatus {
    Idle,
    Checking,
    Downloading {
        version: Version,
        /// Download progress as a fraction in the range `0.0..=1.0`, or `None`
        /// when the total download size is not yet known.
        progress: Option<f32>,
    },
    Installing {
        version: Version,
    },
    Updated {
        version: Version,
    },
    Errored {
        error: Arc<anyhow::Error>,
    },
}

impl PartialEq for AutoUpdateStatus {
    // `progress` is deliberately not compared: two `Downloading` statuses for
    // the same version are equal regardless of how far the download is.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (AutoUpdateStatus::Idle, AutoUpdateStatus::Idle) => true,
            (AutoUpdateStatus::Checking, AutoUpdateStatus::Checking) => true,
            (
                AutoUpdateStatus::Downloading { version: v1, .. },
                AutoUpdateStatus::Downloading { version: v2, .. },
            ) => v1 == v2,
            (
                AutoUpdateStatus::Installing { version: v1 },
                AutoUpdateStatus::Installing { version: v2 },
            ) => v1 == v2,
            (
                AutoUpdateStatus::Updated { version: v1 },
                AutoUpdateStatus::Updated { version: v2 },
            ) => v1 == v2,
            (AutoUpdateStatus::Errored { error: e1 }, AutoUpdateStatus::Errored { error: e2 }) => {
                e1.to_string() == e2.to_string()
            }
            _ => false,
        }
    }
}

impl AutoUpdateStatus {
    pub fn is_updated(&self) -> bool {
        matches!(self, Self::Updated { .. })
    }
}

pub enum AutoUpdateEvent {
    /// A manual check received a release response and found no newer version.
    UpToDate,
}

pub struct AutoUpdater {
    status: AutoUpdateStatus,
    current_version: Version,
    client: Arc<Client>,
    pending_poll: Option<Task<Option<()>>>,
    quit_subscription: Option<gpui::Subscription>,
    update_check_type: UpdateCheckType,
    _wake_subscription: gpui::Subscription,
    dismissed_status: Option<AutoUpdateStatus>,
    /// This copy's release build; `None` for builds that don't update.
    installed_build: Option<u64>,
    /// The release already downloaded and installed this session, if any.
    downloaded_build: Option<u64>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ReleaseAsset {
    pub version: String,
    pub url: String,
}

#[derive(Clone, Copy, Debug, RegisterSetting)]
struct AutoUpdateSetting(bool);

/// Whether or not to automatically check for updates.
///
/// Default: true
impl Settings for AutoUpdateSetting {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        Self(content.auto_update.unwrap())
    }
}

#[derive(Default)]
struct GlobalAutoUpdate(Option<Entity<AutoUpdater>>);

impl Global for GlobalAutoUpdate {}

pub fn init(client: Arc<Client>, cx: &mut App) {
    #[cfg(target_os = "windows")]
    if let Some(install_directory) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        cx.background_spawn(async move {
            noah_release::remove_set_aside_files(&install_directory);
            // The installer from the last update, which has already run.
            std::fs::remove_dir_all(install_directory.join("updates")).ok();
        })
        .detach();
    }

    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        workspace.register_action(|_, action, window, cx| check(action, window, cx));

        workspace.register_action(|_, action, _, cx| {
            view_release_notes(action, cx);
        });
    })
    .detach();

    let version = release_channel::AppVersion::global(cx);
    let auto_updater = cx.new(|cx| {
        let updater = AutoUpdater::new(version, client, cx);

        let poll_for_updates = ReleaseChannel::try_global(cx)
            .map(|channel| channel.poll_for_updates())
            .unwrap_or(false);

        if option_env!("ZED_UPDATE_EXPLANATION").is_none()
            && env::var("ZED_UPDATE_EXPLANATION").is_err()
            && poll_for_updates
        {
            let mut update_subscription = AutoUpdateSetting::get_global(cx)
                .0
                .then(|| updater.start_polling(cx));

            cx.observe_global::<SettingsStore>(move |updater: &mut AutoUpdater, cx| {
                if AutoUpdateSetting::get_global(cx).0 {
                    if update_subscription.is_none() {
                        update_subscription = Some(updater.start_polling(cx))
                    }
                } else {
                    update_subscription.take();
                }
            })
            .detach();
        }

        updater
    });
    cx.set_global(GlobalAutoUpdate(Some(auto_updater)));
}

pub fn check(_: &Check, window: &mut Window, cx: &mut App) {
    if let Some(message) = option_env!("ZED_UPDATE_EXPLANATION")
        .map(ToOwned::to_owned)
        .or_else(|| env::var("ZED_UPDATE_EXPLANATION").ok())
    {
        drop(window.prompt(
            gpui::PromptLevel::Info,
            "Zed was installed via a package manager.",
            Some(&message),
            &["OK"],
            cx,
        ));
        return;
    }

    if !ReleaseChannel::try_global(cx)
        .map(|channel| channel.poll_for_updates())
        .unwrap_or(false)
    {
        return;
    }

    if let Some(updater) = AutoUpdater::get(cx) {
        updater.update(cx, |updater, cx| updater.poll(UpdateCheckType::Manual, cx));
    } else {
        drop(window.prompt(
            gpui::PromptLevel::Info,
            "Could not check for updates",
            Some("Auto-updates disabled for non-bundled app."),
            &["OK"],
            cx,
        ));
    }
}

pub fn release_notes_url(cx: &mut App) -> Option<String> {
    let release_channel = ReleaseChannel::try_global(cx)?;
    let url = match release_channel {
        ReleaseChannel::Stable | ReleaseChannel::Preview => {
            let auto_updater = AutoUpdater::get(cx)?;
            let auto_updater = auto_updater.read(cx);
            let mut current_version = auto_updater.current_version.clone();
            current_version.pre = semver::Prerelease::EMPTY;
            current_version.build = semver::BuildMetadata::EMPTY;
            let release_channel = release_channel.dev_name();
            let path = format!("/releases/{release_channel}/{current_version}");
            auto_updater.client.http_client().build_url(&path)
        }
        ReleaseChannel::Nightly => {
            "https://github.com/zed-industries/zed/commits/nightly/".to_string()
        }
        ReleaseChannel::Dev => "https://github.com/zed-industries/zed/commits/main/".to_string(),
    };
    Some(url)
}

pub fn view_release_notes(_: &ViewReleaseNotes, cx: &mut App) -> Option<()> {
    let url = release_notes_url(cx)?;
    cx.open_url(&url);
    None
}

#[cfg(not(target_os = "windows"))]
const INSTALLER_DIR_PREFIX: &str = "zed-auto-update";

#[cfg(not(target_os = "windows"))]
struct InstallerDir(tempfile::TempDir);

#[cfg(not(target_os = "windows"))]
impl InstallerDir {
    async fn new() -> Result<Self> {
        Ok(Self(
            tempfile::Builder::new()
                .prefix(INSTALLER_DIR_PREFIX)
                .tempdir()?,
        ))
    }

    fn path(&self) -> &Path {
        self.0.path()
    }
}

#[cfg(target_os = "windows")]
struct InstallerDir(PathBuf);

#[cfg(target_os = "windows")]
impl InstallerDir {
    async fn new() -> Result<Self> {
        let installer_dir = std::env::current_exe()?
            .parent()
            .context("No parent dir for Zed.exe")?
            .join("updates");
        if smol::fs::metadata(&installer_dir).await.is_ok() {
            smol::fs::remove_dir_all(&installer_dir).await?;
        }
        smol::fs::create_dir(&installer_dir).await?;
        Ok(Self(installer_dir))
    }

    fn path(&self) -> &Path {
        self.0.as_path()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UpdateCheckType {
    Automatic,
    Manual,
}

impl UpdateCheckType {
    pub fn is_manual(self) -> bool {
        self == Self::Manual
    }
}

impl EventEmitter<AutoUpdateEvent> for AutoUpdater {}

impl AutoUpdater {
    pub fn get(cx: &mut App) -> Option<Entity<Self>> {
        cx.default_global::<GlobalAutoUpdate>().0.clone()
    }

    fn new(current_version: Version, client: Arc<Client>, cx: &mut Context<Self>) -> Self {
        // noah installs an update while it runs (see `install_release_windows`),
        // so nothing has to happen on quit.
        let quit_subscription = None;

        cx.on_app_restart(|this, _| {
            this.quit_subscription.take();
        })
        .detach();

        // A download or check that was in flight when the machine went to sleep
        // is almost certainly riding a TCP connection that silently died during
        // suspend, so it would otherwise appear to stall indefinitely.
        let wake_subscription = cx.on_system_wake({
            let this = cx.entity().downgrade();
            move |cx| {
                this.update(cx, |this, cx| this.restart_after_wake(cx)).ok();
            }
        });

        Self {
            status: AutoUpdateStatus::Idle,
            current_version,
            client,
            pending_poll: None,
            quit_subscription,
            update_check_type: UpdateCheckType::Automatic,
            _wake_subscription: wake_subscription,
            dismissed_status: None,
            installed_build: noah_release::installed_build(),
            downloaded_build: None,
        }
    }

    fn restart_after_wake(&mut self, cx: &mut Context<Self>) {
        // Only network phases can be safely restarted. `Installing` is a local
        // operation (mounting a dmg, rsync, etc.) that must not be interrupted.
        if !matches!(
            self.status,
            AutoUpdateStatus::Checking | AutoUpdateStatus::Downloading { .. }
        ) {
            return;
        }

        let check_type = self.update_check_type;
        self.pending_poll.take();
        self.status = AutoUpdateStatus::Idle;
        self.poll(check_type, cx);
    }

    pub fn start_polling(&self, cx: &mut Context<Self>) -> Task<Result<()>> {
        let poll_interval =
            ReleaseChannel::try_global(cx).map_or(POLL_INTERVAL, |channel| match channel {
                ReleaseChannel::Nightly => NIGHTLY_POLL_INTERVAL,
                _ => POLL_INTERVAL,
            });

        cx.spawn(async move |this, cx| {
            #[cfg(all(not(target_os = "windows"), not(test)))]
            cx.background_spawn(cleanup_stale_installer_dirs()).detach();

            loop {
                this.update(cx, |this, cx| this.poll(UpdateCheckType::Automatic, cx))?;
                cx.background_executor().timer(poll_interval).await;
            }
        })
    }

    pub fn update_check_type(&self) -> UpdateCheckType {
        self.update_check_type
    }

    pub fn poll(&mut self, check_type: UpdateCheckType, cx: &mut Context<Self>) {
        if check_type.is_manual() {
            self.dismissed_status = None;
        }
        if self.pending_poll.is_some() {
            if self.update_check_type == UpdateCheckType::Automatic {
                self.update_check_type = check_type;
                cx.notify();
            }
            return;
        }
        self.update_check_type = check_type;

        cx.notify();

        self.pending_poll = Some(cx.spawn(async move |this, cx| {
            let result = Self::update(this.upgrade()?, cx).await;
            this.update(cx, |this, cx| {
                this.pending_poll = None;
                if let Err(error) = result {
                    let needs_manual_update = error.downcast_ref::<ManualUpdateNeeded>().is_some();
                    this.status = match check_type {
                        UpdateCheckType::Automatic if needs_manual_update => {
                            log::warn!("auto-update: {}", error);
                            AutoUpdateStatus::Errored {
                                error: Arc::new(error),
                            }
                        }
                        // Be quiet if the check was automated (e.g. when offline)
                        UpdateCheckType::Automatic => {
                            log::info!("auto-update check failed: error:{:?}", error);
                            AutoUpdateStatus::Idle
                        }
                        UpdateCheckType::Manual => {
                            log::error!("auto-update failed: error:{:?}", error);
                            AutoUpdateStatus::Errored {
                                error: Arc::new(error),
                            }
                        }
                    };

                    cx.notify();
                }
            })
            .ok()
        }));
    }

    pub fn current_version(&self) -> Version {
        self.current_version.clone()
    }

    pub fn status(&self) -> AutoUpdateStatus {
        self.status.clone()
    }

    pub fn dismissed_status(&self) -> Option<AutoUpdateStatus> {
        self.dismissed_status.clone()
    }

    pub fn dismiss_status(&mut self, status: AutoUpdateStatus, cx: &mut Context<Self>) {
        self.dismissed_status = Some(status);
        cx.notify();
    }

    pub fn dismiss(&mut self, cx: &mut Context<Self>) -> bool {
        if let AutoUpdateStatus::Idle = self.status {
            return false;
        }
        self.status = AutoUpdateStatus::Idle;
        cx.notify();
        true
    }

    // If you are packaging Zed and need to override the place it downloads SSH remotes from,
    // you can override this function. You should also update get_remote_server_release_url to return
    // Ok(None).
    pub async fn download_remote_server_release(
        release_channel: ReleaseChannel,
        version: Option<Version>,
        os: &str,
        arch: &str,
        set_status: impl Fn(&str, &mut AsyncApp) + Send + 'static,
        cx: &mut AsyncApp,
    ) -> Result<PathBuf> {
        let this = cx.update(|cx| {
            cx.default_global::<GlobalAutoUpdate>()
                .0
                .clone()
                .context("auto-update not initialized")
        })?;

        set_status("Fetching remote server release", cx);
        let release = Self::get_release_asset(
            &this,
            release_channel,
            version,
            "zed-remote-server",
            os,
            arch,
            cx,
        )
        .await?;

        let servers_dir = paths::remote_servers_dir();
        let channel_dir = servers_dir.join(release_channel.dev_name());
        let platform_dir = channel_dir.join(format!("{}-{}", os, arch));
        let version_path = platform_dir.join(format!("{}.gz", release.version));
        smol::fs::create_dir_all(&platform_dir).await.ok();

        let client = this.read_with(cx, |this, _| this.client.http_client());

        if smol::fs::metadata(&version_path).await.is_err() {
            log::info!(
                "downloading zed-remote-server {os} {arch} version {}",
                release.version
            );
            set_status("Downloading remote server", cx);
            download_remote_server_binary(&version_path, release, client).await?;
        }

        if let Err(error) =
            cleanup_remote_server_cache(&platform_dir, &version_path, REMOTE_SERVER_CACHE_LIMIT)
                .await
        {
            log::warn!(
                "Failed to clean up remote server cache in {:?}: {error:#}",
                platform_dir
            );
        }

        Ok(version_path)
    }

    pub async fn get_remote_server_release_url(
        channel: ReleaseChannel,
        version: Option<Version>,
        os: &str,
        arch: &str,
        cx: &mut AsyncApp,
    ) -> Result<Option<String>> {
        let this = cx.update(|cx| {
            cx.default_global::<GlobalAutoUpdate>()
                .0
                .clone()
                .context("auto-update not initialized")
        })?;

        let release =
            Self::get_release_asset(&this, channel, version, "zed-remote-server", os, arch, cx)
                .await?;

        Ok(Some(release.url))
    }

    async fn get_release_asset(
        this: &Entity<Self>,
        release_channel: ReleaseChannel,
        version: Option<Version>,
        asset: &str,
        os: &str,
        arch: &str,
        cx: &mut AsyncApp,
    ) -> Result<ReleaseAsset> {
        let client = this.read_with(cx, |this, _| this.client.clone());

        let (system_id, metrics_id, is_staff) = if client.telemetry().metrics_enabled() {
            (
                client.telemetry().system_id(),
                client.telemetry().metrics_id(),
                client.telemetry().is_staff(),
            )
        } else {
            (None, None, None)
        };

        let version = if let Some(mut version) = version {
            version.pre = semver::Prerelease::EMPTY;
            version.build = semver::BuildMetadata::EMPTY;
            version.to_string()
        } else {
            "latest".to_string()
        };
        let http_client = client.http_client();

        let path = format!("/releases/{}/{}/asset", release_channel.dev_name(), version,);
        let url = http_client.build_zed_cloud_url_with_query(
            &path,
            AssetQuery {
                os,
                arch,
                asset,
                metrics_id: metrics_id.as_deref(),
                system_id: system_id.as_deref(),
                is_staff,
            },
        )?;

        let mut response = http_client
            .get(url.as_str(), Default::default(), true)
            .await?;
        let mut body = Vec::new();
        response.body_mut().read_to_end(&mut body).await?;

        anyhow::ensure!(
            response.status().is_success(),
            "failed to fetch release: {:?}",
            String::from_utf8_lossy(&body),
        );

        serde_json::from_slice(body.as_slice()).with_context(|| {
            format!(
                "error deserializing release {:?}",
                String::from_utf8_lossy(&body),
            )
        })
    }

    async fn update(this: Entity<Self>, cx: &mut AsyncApp) -> Result<()> {
        let (client, previous_status) = this.read_with(cx, |this, _| {
            (this.client.http_client(), this.status.clone())
        });

        this.update(cx, |this, cx| {
            this.status = AutoUpdateStatus::Checking;
            log::info!("Auto Update: checking for updates");
            cx.notify();
        });

        let (installed_build, downloaded_build) =
            this.read_with(cx, |this, _| (this.installed_build, this.downloaded_build));
        let Some(installed_build) = installed_build else {
            anyhow::bail!(
                "this copy of noah was built from source, so it doesn't update itself; \
                 get releases from {}",
                noah_release::DOWNLOAD_PAGE
            );
        };
        let release_channel = cx.update(|cx| ReleaseChannel::try_global(cx));
        let manifest = Self::fetch_noah_manifest(&client, release_channel).await?;
        if !noah_release::is_newer(manifest.build, installed_build, downloaded_build) {
            this.update(cx, |this, cx| {
                this.status = match previous_status {
                    AutoUpdateStatus::Updated { .. } => previous_status,
                    _ => {
                        if this.update_check_type.is_manual() {
                            cx.emit(AutoUpdateEvent::UpToDate);
                        }
                        AutoUpdateStatus::Idle
                    }
                };
                cx.notify();
            });
            return Ok(());
        }
        manifest.verify_signature()?;
        let newer_version = manifest.version();
        let asset = manifest.asset_for(OS, ARCH).cloned().ok_or_else(|| {
            ManualUpdateNeeded(format!(
                "noah {} is out, but not built for this system yet; see {}",
                manifest.version,
                noah_release::DOWNLOAD_PAGE
            ))
        })?;
        let fetched_release_data = ReleaseAsset {
            version: manifest.version.clone(),
            url: asset.url.clone(),
        };

        this.update(cx, |this, cx| {
            this.status = AutoUpdateStatus::Downloading {
                version: newer_version.clone(),
                progress: None,
            };
            cx.notify();
        });

        let installer_dir = InstallerDir::new()
            .await
            .context("Failed to create installer dir")?;
        let target_path = Self::target_path(&installer_dir).await?;
        let progress_entity = this.clone();
        let mut progress_cx = cx.clone();
        download_release(
            &target_path,
            fetched_release_data,
            client,
            move |progress| {
                progress_entity.update(&mut progress_cx, |this, cx| {
                    if let AutoUpdateStatus::Downloading {
                        progress: current_progress,
                        ..
                    } = &mut this.status
                    {
                        *current_progress = progress;
                        cx.notify();
                    }
                });
            },
        )
        .await
        .with_context(|| format!("Failed to download update to {}", target_path.display()))?;

        cx.background_spawn({
            let target_path = target_path.clone();
            let expected = asset.sha256.clone();
            async move { noah_release::verify_sha256(&target_path, &expected) }
        })
        .await?;

        this.update(cx, |this, cx| {
            this.status = AutoUpdateStatus::Installing {
                version: newer_version.clone(),
            };
            cx.notify();
        });

        #[cfg(test)]
        let install_result = match cx
            .try_read_global::<tests::InstallOverride, _>(|g, _| g.0.clone())
            .map(|test_install| test_install(&target_path, cx))
        {
            Some(result) => result,
            None => return Ok(()),
        };

        #[cfg(not(test))]
        let install_result = {
            let running_app_path = cx.update(|cx| cx.app_path())?;
            cx.background_spawn(Self::install_release(
                installer_dir,
                target_path.clone(),
                running_app_path,
            ))
            .await
        };
        let new_binary_path = install_result
            .with_context(|| format!("Failed to install update at: {}", target_path.display()))?;
        if let Some(new_binary_path) = new_binary_path {
            cx.update(|cx| cx.set_restart_path(new_binary_path));
        }

        this.update(cx, |this, cx| {
            this.set_should_show_update_notification(true, cx)
                .detach_and_log_err(cx);
            this.downloaded_build = Some(manifest.build);
            this.status = AutoUpdateStatus::Updated {
                version: newer_version,
            };
            cx.notify();
        });
        Ok(())
    }

    async fn fetch_noah_manifest(
        client: &HttpClientWithUrl,
        release_channel: Option<ReleaseChannel>,
    ) -> Result<noah_release::Manifest> {
        noah_release::fetch_manifest(client, release_channel).await
    }

    async fn target_path(installer_dir: &InstallerDir) -> Result<PathBuf> {
        let filename = match OS {
            "macos" => anyhow::Ok("noah.dmg"),
            "linux" => Ok("noah.tar.xz"),
            "windows" => Ok("noah-setup.exe"),
            unsupported_os => anyhow::bail!("not supported: {unsupported_os}"),
        }?;

        Ok(installer_dir.path().join(filename))
    }

    #[cfg_attr(test, allow(dead_code))]
    async fn install_release(
        // Owned so the downloaded installer stays on disk until installing ends.
        _installer_dir: InstallerDir,
        target_path: PathBuf,
        running_app_path: PathBuf,
    ) -> Result<Option<PathBuf>> {
        match OS {
            // noah has no signed macOS release to update from yet.
            "macos" => Err(ManualUpdateNeeded(format!(
                "a new noah is out; install it from {}",
                noah_release::DOWNLOAD_PAGE
            ))
            .into()),
            "linux" => install_release_linux(&target_path, running_app_path).await,
            "windows" => install_release_windows(&target_path).await,
            unsupported_os => anyhow::bail!("not supported: {unsupported_os}"),
        }
    }

    pub fn set_should_show_update_notification(
        &self,
        should_show: bool,
        cx: &App,
    ) -> Task<Result<()>> {
        let kvp = KeyValueStore::global(cx);
        cx.background_spawn(async move {
            if should_show {
                kvp.write_kvp(
                    SHOULD_SHOW_UPDATE_NOTIFICATION_KEY.to_string(),
                    "".to_string(),
                )
                .await?;
            } else {
                kvp.delete_kvp(SHOULD_SHOW_UPDATE_NOTIFICATION_KEY.to_string())
                    .await?;
            }
            Ok(())
        })
    }

    pub fn should_show_update_notification(&self, cx: &App) -> Task<Result<bool>> {
        let kvp = KeyValueStore::global(cx);
        cx.background_spawn(async move {
            Ok(kvp.read_kvp(SHOULD_SHOW_UPDATE_NOTIFICATION_KEY)?.is_some())
        })
    }
}

async fn download_remote_server_binary(
    target_path: &PathBuf,
    release: ReleaseAsset,
    client: Arc<HttpClientWithUrl>,
) -> Result<()> {
    let temp = tempfile::Builder::new().tempfile_in(remote_servers_dir())?;
    let mut temp_file = File::create(&temp).await?;

    let mut response = client.get(&release.url, Default::default(), true).await?;
    anyhow::ensure!(
        response.status().is_success(),
        "failed to download remote server release: {:?}",
        response.status()
    );
    smol::io::copy(response.body_mut(), &mut temp_file).await?;
    smol::fs::rename(&temp, &target_path).await?;

    Ok(())
}

async fn cleanup_remote_server_cache(
    platform_dir: &Path,
    keep_path: &Path,
    limit: usize,
) -> Result<()> {
    if limit == 0 {
        return Ok(());
    }

    let mut entries = smol::fs::read_dir(platform_dir).await?;
    let now = SystemTime::now();
    let mut candidates = Vec::new();

    while let Some(entry) = entries.next().await {
        let entry = entry?;
        let path = entry.path();
        if path.extension() != Some(OsStr::new("gz")) {
            continue;
        }

        let mtime = if path == keep_path {
            now
        } else {
            smol::fs::metadata(&path)
                .await
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        };

        candidates.push((path, mtime));
    }

    if candidates.len() <= limit {
        return Ok(());
    }

    candidates.sort_by(|(path_a, time_a), (path_b, time_b)| {
        time_b.cmp(time_a).then_with(|| path_a.cmp(path_b))
    });

    for (index, (path, _)) in candidates.into_iter().enumerate() {
        if index < limit || path == keep_path {
            continue;
        }

        if let Err(error) = smol::fs::remove_file(&path).await {
            log::warn!(
                "Failed to remove old remote server archive {:?}: {}",
                path,
                error
            );
        }
    }

    Ok(())
}

async fn download_release(
    target_path: &Path,
    release: ReleaseAsset,
    client: Arc<HttpClientWithUrl>,
    mut on_progress: impl FnMut(Option<f32>),
) -> Result<()> {
    let mut target_file = File::create(&target_path).await?;

    let mut response = client.get(&release.url, Default::default(), true).await?;
    anyhow::ensure!(
        response.status().is_success(),
        "failed to download update: {:?}",
        response.status()
    );

    let total_bytes = response
        .headers()
        .get(http_client::http::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|total_bytes| *total_bytes > 0);

    let mut downloaded_bytes: u64 = 0;
    let mut last_reported_percent: Option<u8> = None;
    let mut buffer = [0u8; 8192];
    let body = response.body_mut();
    loop {
        let bytes_read = body.read(&mut buffer).await?;
        if bytes_read == 0 {
            break;
        }
        target_file.write_all(&buffer[..bytes_read]).await?;
        downloaded_bytes += bytes_read as u64;

        if let Some(total_bytes) = total_bytes {
            let fraction = (downloaded_bytes as f32 / total_bytes as f32).clamp(0.0, 1.0);
            // Only report when the whole-number percentage changes to avoid notifying the UI on every chunk.
            let percent = (fraction * 100.0) as u8;
            if last_reported_percent != Some(percent) {
                last_reported_percent = Some(percent);
                on_progress(Some(fraction));
            }
        }
    }
    target_file.flush().await?;
    if total_bytes.is_some() && last_reported_percent != Some(100) {
        on_progress(Some(1.0));
    }
    log::info!("downloaded update. path:{:?}", target_path);

    Ok(())
}

async fn install_release_linux(
    downloaded_archive: &Path,
    running_app_path: PathBuf,
) -> Result<Option<PathBuf>> {
    let prefix = noah_release::linux_install_prefix(&running_app_path)
        .map_err(|error| ManualUpdateNeeded(format!("a new noah is out, but {error:#}")))?;
    // Unpacking beside the current install keeps both on one disk, so the
    // swap below is two renames rather than a copy that could be interrupted.
    let staging = prefix.join(format!(".noah-update-{}", std::process::id()));
    fs::remove_dir_all(&staging).await.ok();
    fs::create_dir_all(&staging)
        .await
        .context("failed to create a folder for the noah update")?;

    let output = new_command("tar")
        .arg("-xJf")
        .arg(downloaded_archive)
        .arg("-C")
        .arg(&staging)
        .output()
        .await
        .context("failed to run tar on the noah update")?;
    if !output.status.success() {
        fs::remove_dir_all(&staging).await.ok();
        anyhow::bail!(
            "failed to unpack the noah update: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let staged_app = staging.join("noah.app");
    let installed_app = prefix.join("noah.app");
    let previous_app = staging.join("previous.app");
    anyhow::ensure!(
        staged_app.join("libexec").join("zed-editor").is_file(),
        "the noah update doesn't contain the editor"
    );
    fs::rename(&installed_app, &previous_app)
        .await
        .context("failed to move the current noah aside")?;
    if let Err(error) = fs::rename(&staged_app, &installed_app).await {
        fs::rename(&previous_app, &installed_app).await.ok();
        return Err(error).context("failed to put the new noah in place");
    }
    // The running editor keeps its files open, so removing them now is safe.
    fs::remove_dir_all(&staging).await.ok();

    Ok(Some(installed_app.join("libexec").join("zed-editor")))
}

/// Runs the new installer silently over the running copy. Windows refuses to
/// overwrite a program that is running but allows renaming it, so noah's
/// programs and libraries are moved aside first; they are deleted at the next
/// start, and put back if the installer fails.
async fn install_release_windows(downloaded_installer: &Path) -> Result<Option<PathBuf>> {
    let current_exe = std::env::current_exe()?;
    let install_directory = current_exe
        .parent()
        .context("noah.exe has no parent folder")?
        .to_path_buf();
    let moved = noah_release::set_aside_program_files(&install_directory)?;
    let output = new_command(downloaded_installer)
        .arg("/S")
        .arg("/UPDATE")
        .output()
        .await;
    match output {
        Ok(output) if output.status.success() && current_exe.exists() => Ok(Some(current_exe)),
        Ok(output) => {
            noah_release::restore_set_aside(&moved);
            anyhow::bail!(
                "the noah installer failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )
        }
        Err(error) => {
            noah_release::restore_set_aside(&moved);
            Err(error).context("couldn't start the noah installer")
        }
    }
}

#[cfg(not(target_os = "windows"))]
async fn cleanup_stale_installer_dirs() {
    const STALE_INSTALLER_DIR_AGE: Duration = Duration::from_secs(24 * 60 * 60);

    let temp_dir = std::env::temp_dir();
    let Ok(mut entries) = fs::read_dir(&temp_dir).await else {
        log::warn!("failed to read temp dir {temp_dir:?} while cleaning up installer dirs");
        return;
    };
    while let Some(entry) = entries.next().await {
        let Ok(entry) = entry else {
            continue;
        };
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(INSTALLER_DIR_PREFIX)
        {
            continue;
        }
        // Leave recent dirs alone, as they may belong to an update currently
        // in progress in another Zed instance.
        let is_stale = entry.metadata().await.ok().is_some_and(|metadata| {
            metadata.is_dir()
                && metadata.modified().ok().is_some_and(|modified| {
                    SystemTime::now()
                        .duration_since(modified)
                        .is_ok_and(|age| age > STALE_INSTALLER_DIR_AGE)
                })
        });
        if is_stale {
            if let Err(error) = fs::remove_dir_all(entry.path()).await {
                log::warn!(
                    "failed to remove stale installer dir {:?}: {error}",
                    entry.path()
                );
            } else {
                log::info!("removed stale installer dir {:?}", entry.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use client::Client;
    use clock::FakeSystemClock;
    use futures::channel::oneshot;
    use gpui::TestAppContext;
    use http_client::{FakeHttpClient, Response};
    use settings::default_settings;
    use std::{
        rc::Rc,
        sync::{
            Arc,
            atomic::{self, AtomicBool},
        },
    };
    use tempfile::tempdir;

    use sha2::Digest as _;

    #[ctor::ctor(unsafe)]
    fn init_logger() {
        zlog::init_test();
    }

    use super::*;

    pub(super) struct InstallOverride(pub Rc<dyn Fn(&Path, &AsyncApp) -> Result<Option<PathBuf>>>);
    impl Global for InstallOverride {}

    #[gpui::test]
    fn test_auto_update_defaults_to_true(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let mut store = SettingsStore::new(cx, &settings::default_settings());
            store
                .set_default_settings(&default_settings(), cx)
                .expect("Unable to set default settings");
            store
                .set_user_settings("{}", cx)
                .expect("Unable to set user settings");
            cx.set_global(store);
            assert!(AutoUpdateSetting::get_global(cx).0);
        });
    }

    #[gpui::test]
    async fn test_auto_update_downloads(cx: &mut TestAppContext) {
        cx.background_executor.allow_parking();
        zlog::init_test();
        let release_available = Arc::new(AtomicBool::new(false));
        let update_contents = "<fake-noah-update>";
        let update_checksum: String = sha2::Sha256::digest(update_contents.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();

        let (installer_tx, installer_rx) = oneshot::channel::<String>();

        cx.update(|cx| {
            settings::init(cx);

            let current_version = semver::Version::new(0, 100, 0);
            release_channel::init_test(current_version, ReleaseChannel::Stable, cx);

            let clock = Arc::new(FakeSystemClock::new());
            let release_available = Arc::clone(&release_available);
            let installer_rx = Arc::new(parking_lot::Mutex::new(Some(installer_rx)));
            let asset_key = format!("{OS}-{ARCH}");
            let fake_client_http = FakeHttpClient::create(move |req| {
                let release_available = release_available.load(atomic::Ordering::Relaxed);
                let installer_rx = installer_rx.clone();
                let manifest = serde_json::json!({
                    "build": if release_available { 200 } else { 100 },
                    "version": if release_available { "0.100.1" } else { "0.100.0" },
                    "assets": {
                        asset_key.clone(): {
                            "url": "https://test.example/new-download",
                            "sha256": update_checksum.clone(),
                        }
                    }
                })
                .to_string();
                async move {
                    if req.uri().path() == "/downloads/latest.json" {
                        return Ok(Response::builder()
                            .status(200)
                            .body(manifest.into())
                            .unwrap());
                    } else if req.uri().path() == "/new-download" {
                        return Ok(Response::builder()
                            .status(200)
                            .body({
                                let installer_rx = installer_rx.lock().take().unwrap();
                                installer_rx.await.unwrap().into()
                            })
                            .unwrap());
                    }
                    Ok(Response::builder().status(404).body("".into()).unwrap())
                }
            });
            let client = Client::new(clock, fake_client_http, cx);
            crate::init(client, cx);
            AutoUpdater::get(cx)
                .expect("auto updater should exist")
                .update(cx, |updater, _| updater.installed_build = Some(100));
        });

        let auto_updater = cx.update(|cx| AutoUpdater::get(cx).expect("auto updater should exist"));

        cx.background_executor.run_until_parked();

        auto_updater.read_with(cx, |updater, _| {
            assert_eq!(updater.status(), AutoUpdateStatus::Idle);
        });

        release_available.store(true, atomic::Ordering::SeqCst);
        cx.background_executor.advance_clock(POLL_INTERVAL);
        cx.background_executor.run_until_parked();

        loop {
            cx.background_executor.timer(Duration::from_millis(0)).await;
            cx.run_until_parked();
            let status = auto_updater.read_with(cx, |updater, _| updater.status());
            if !matches!(status, AutoUpdateStatus::Idle) {
                break;
            }
        }
        let status = auto_updater.read_with(cx, |updater, _| updater.status());
        assert_eq!(
            status,
            AutoUpdateStatus::Downloading {
                version: semver::Version::new(0, 100, 1),
                progress: None,
            }
        );

        installer_tx.send(update_contents.to_owned()).unwrap();

        let tmp_dir = Arc::new(tempdir().unwrap());

        cx.update(|cx| {
            let tmp_dir = tmp_dir.clone();
            cx.set_global(InstallOverride(Rc::new(move |target_path, _cx| {
                let tmp_dir = tmp_dir.clone();
                let dest_path = tmp_dir.path().join("noah");
                std::fs::copy(&target_path, &dest_path)?;
                Ok(Some(dest_path))
            })));
        });

        loop {
            cx.background_executor.timer(Duration::from_millis(0)).await;
            cx.run_until_parked();
            let status = auto_updater.read_with(cx, |updater, _| updater.status());
            if !matches!(status, AutoUpdateStatus::Downloading { .. }) {
                break;
            }
        }
        let status = auto_updater.read_with(cx, |updater, _| updater.status());
        assert_eq!(
            status,
            AutoUpdateStatus::Updated {
                version: semver::Version::new(0, 100, 1)
            }
        );
        let will_restart = cx.expect_restart();
        cx.update(|cx| cx.restart());
        let (path, arguments) = will_restart.await.unwrap();
        assert!(arguments.is_empty());
        let path = path.unwrap();
        assert_eq!(path, tmp_dir.path().join("noah"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), update_contents);
    }

    #[gpui::test]
    async fn test_download_release_reports_progress(cx: &mut TestAppContext) {
        cx.background_executor.allow_parking();

        let body = vec![0u8; 20_000];
        let content_length = body.len();

        let client = FakeHttpClient::create(move |_req| {
            let body = body.clone();
            async move {
                Ok(Response::builder()
                    .status(200)
                    .header(
                        http_client::http::header::CONTENT_LENGTH,
                        body.len().to_string(),
                    )
                    .body(body.into())
                    .unwrap())
            }
        });

        let temp_dir = tempdir().unwrap();
        let target_path = temp_dir.path().join("zed-download");
        let release = ReleaseAsset {
            version: "1.0.0".to_string(),
            url: "https://test.example/download".to_string(),
        };

        let reported = Rc::new(std::cell::RefCell::new(Vec::<f32>::new()));
        download_release(&target_path, release, client, {
            let reported = reported.clone();
            move |fraction| {
                if let Some(fraction) = fraction {
                    reported.borrow_mut().push(fraction);
                }
            }
        })
        .await
        .unwrap();

        let reported = reported.borrow();
        assert!(
            reported.len() >= 2,
            "expected progress to be reported across multiple reads, got {reported:?}"
        );
        assert_eq!(
            reported.last().copied(),
            Some(1.0),
            "download should finish at 100%"
        );
        for fraction in reported.iter() {
            assert!(
                (0.0..=1.0).contains(fraction),
                "progress {fraction} out of range"
            );
        }
        for pair in reported.windows(2) {
            assert!(
                pair[0] <= pair[1],
                "progress must not decrease: {reported:?}"
            );
        }

        let downloaded_len = std::fs::metadata(&target_path).unwrap().len();
        assert_eq!(downloaded_len, content_length as u64);
    }

    #[gpui::test]
    async fn test_download_release_without_content_length_reports_no_progress(
        cx: &mut TestAppContext,
    ) {
        cx.background_executor.allow_parking();

        let body = vec![0u8; 20_000];
        let content_length = body.len();

        let client = FakeHttpClient::create(move |_req| {
            let body = body.clone();
            async move { Ok(Response::builder().status(200).body(body.into()).unwrap()) }
        });

        let temp_dir = tempdir().unwrap();
        let target_path = temp_dir.path().join("zed-download");
        let release = ReleaseAsset {
            version: "1.0.0".to_string(),
            url: "https://test.example/download".to_string(),
        };

        let reported = Rc::new(std::cell::RefCell::new(Vec::<Option<f32>>::new()));
        download_release(&target_path, release, client, {
            let reported = reported.clone();
            move |fraction| {
                reported.borrow_mut().push(fraction);
            }
        })
        .await
        .unwrap();

        assert!(
            reported.borrow().is_empty(),
            "progress should not be reported when the total size is unknown, got {:?}",
            reported.borrow()
        );

        let downloaded_len = std::fs::metadata(&target_path).unwrap().len();
        assert_eq!(downloaded_len, content_length as u64);
    }
}
