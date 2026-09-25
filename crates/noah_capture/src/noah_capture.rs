//! Screenshots and screen recordings of noah's own window, from buttons in
//! the room rail. Screenshots are saved as PNG in the person's Pictures
//! folder and copied to the clipboard; recordings are saved in their Videos
//! folder as MP4 when ffmpeg is installed, otherwise as Motion-JPEG AVI.
//!
//! Linux and Windows capture the screen through GPUI's screen capture and
//! crop to the window; macOS uses the system's `screencapture`.

mod avi;

use anyhow::{Context as _, Result, anyhow, bail};
use gpui::{App, AppContext as _, Global, Task, Window, actions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub use avi::AviWriter;

actions!(
    capture,
    [
        /// Saves a screenshot of the noah window and copies it to the clipboard.
        TakeScreenshot,
        /// Starts or stops recording the noah window.
        ToggleScreenRecording,
    ]
);

/// Recordings stop on their own after this long.
const MAX_RECORDING: Duration = Duration::from_secs(30 * 60);
#[cfg(not(target_os = "macos"))]
const RECORDING_FRAME_INTERVAL: Duration = Duration::from_millis(100);

/// The part of the screen to keep, in the captured frame's pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl CropRect {
    /// Clamps the rectangle to a frame of the given size, keeping width and
    /// height even so video players accept the result.
    pub fn clamped(self, frame_width: u32, frame_height: u32) -> Option<Self> {
        let x = self.x.min(frame_width);
        let y = self.y.min(frame_height);
        let width = self.width.min(frame_width - x) & !1;
        let height = self.height.min(frame_height - y) & !1;
        (width >= 2 && height >= 2).then_some(Self { x, y, width, height })
    }
}

#[derive(Default)]
struct CaptureState {
    recording: Option<Recording>,
    /// Starting a recording is asynchronous; a second toggle meanwhile must
    /// not start another one.
    starting: bool,
}

impl Global for CaptureState {}

struct Recording {
    started: Instant,
    path: PathBuf,
    #[cfg(not(target_os = "macos"))]
    session: Session,
    #[cfg(target_os = "macos")]
    child: std::process::Child,
}

pub fn is_recording(cx: &App) -> bool {
    cx.try_global::<CaptureState>()
        .is_some_and(|state| state.recording.is_some())
}

pub fn recording_elapsed(cx: &App) -> Option<Duration> {
    cx.try_global::<CaptureState>()?
        .recording
        .as_ref()
        .map(|recording| recording.started.elapsed())
}

fn timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H.%M.%S").to_string()
}

fn output_directory(videos: bool) -> Result<PathBuf> {
    let base = if videos {
        dirs::video_dir()
    } else {
        dirs::picture_dir()
    }
    .unwrap_or_else(|| paths::home_dir().join(if videos { "Videos" } else { "Pictures" }));
    let directory = base.join("noah");
    std::fs::create_dir_all(&directory)
        .with_context(|| format!("couldn't create {}", directory.display()))?;
    Ok(directory)
}

/// The window's rectangle on its display, in the display's physical pixels.
#[cfg(not(target_os = "macos"))]
fn window_rect(window: &Window, cx: &App) -> CropRect {
    let bounds = window.bounds();
    let scale = window.scale_factor();
    let display_origin = window
        .display(cx)
        .map(|display| display.bounds().origin)
        .unwrap_or_default();
    let x = (f32::from(bounds.origin.x - display_origin.x) * scale).max(0.0);
    let y = (f32::from(bounds.origin.y - display_origin.y) * scale).max(0.0);
    CropRect {
        x: x.round() as u32,
        y: y.round() as u32,
        width: (f32::from(bounds.size.width) * scale).round() as u32,
        height: (f32::from(bounds.size.height) * scale).round() as u32,
    }
}

/// The window's screen, as the capture backend identifies it. On Windows both
/// are the monitor handle; elsewhere the ids don't correspond.
#[cfg(not(target_os = "macos"))]
fn window_display(window: &Window, cx: &App) -> Option<u32> {
    if !cfg!(windows) {
        return None;
    }
    window
        .display(cx)
        .map(|display| u64::from(display.id()) as u32)
}

/// Saves a PNG of the window and copies it to the clipboard. Returns the
/// file's path.
pub fn take_screenshot(window: &Window, cx: &mut App) -> Task<Result<PathBuf>> {
    let directory = match output_directory(false) {
        Ok(directory) => directory,
        Err(error) => return Task::ready(Err(error)),
    };
    let path = directory.join(format!("noah {}.png", timestamp()));
    #[cfg(target_os = "macos")]
    let capture = {
        let bounds = window.bounds();
        platform_mac::screenshot(&path, bounds, cx)
    };
    #[cfg(target_os = "linux")]
    let capture = match x11::window_id(window) {
        Some(window_id) => x11::screenshot(path.clone(), window_id, cx),
        None => platform::screenshot(
            path.clone(),
            window_rect(window, cx),
            window_display(window, cx),
            cx,
        ),
    };
    #[cfg(target_os = "windows")]
    let capture = platform::screenshot(
            path.clone(),
            window_rect(window, cx),
            window_display(window, cx),
            cx,
        );
    cx.spawn(async move |cx| {
        capture.await?;
        let bytes = std::fs::read(&path).with_context(|| format!("couldn't read {}", path.display()))?;
        cx.update(|cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(
                gpui::ImageFormat::Png,
                bytes,
            )));
        });
        Ok(path)
    })
}

/// Starts recording the window, or stops the recording in progress. Starting
/// resolves to `None`; stopping resolves to the saved video's path.
pub fn toggle_recording(window: &Window, cx: &mut App) -> Task<Result<Option<PathBuf>>> {
    if cx.default_global::<CaptureState>().recording.is_some() {
        return stop_recording(cx);
    }
    if cx.default_global::<CaptureState>().starting {
        return Task::ready(Ok(None));
    }
    let directory = match output_directory(true) {
        Ok(directory) => directory,
        Err(error) => return Task::ready(Err(error)),
    };
    #[cfg(target_os = "macos")]
    {
        let path = directory.join(format!("noah {}.mov", timestamp()));
        let child = match platform_mac::start_recording(&path, window.bounds()) {
            Ok(child) => child,
            Err(error) => return Task::ready(Err(error)),
        };
        cx.default_global::<CaptureState>().recording = Some(Recording {
            started: Instant::now(),
            path,
            child,
        });
        schedule_automatic_stop(cx);
        cx.refresh_windows();
        Task::ready(Ok(None))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let path = directory.join(format!("noah {}.avi", timestamp()));
        #[cfg(target_os = "linux")]
        let start = match x11::window_id(window) {
            Some(window_id) => Task::ready(x11::start_recording(path.clone(), window_id)),
            None => {
                let start = platform::start_recording(
                    path.clone(),
                    window_rect(window, cx),
                    window_display(window, cx),
                    cx,
                );
                cx.spawn(async move |_| start.await.map(Session::Scap))
            }
        };
        #[cfg(target_os = "windows")]
        let start = {
            let start = platform::start_recording(
                    path.clone(),
                    window_rect(window, cx),
                    window_display(window, cx),
                    cx,
                );
            cx.spawn(async move |_| start.await.map(Session::Scap))
        };
        cx.default_global::<CaptureState>().starting = true;
        cx.spawn(async move |cx| {
            let session = start.await;
            cx.update(|cx| cx.default_global::<CaptureState>().starting = false);
            let session = session?;
            cx.update(|cx| {
                cx.default_global::<CaptureState>().recording = Some(Recording {
                    started: Instant::now(),
                    path,
                    session,
                });
                schedule_automatic_stop(cx);
                cx.refresh_windows();
            });
            Ok(None)
        })
    }
}

fn schedule_automatic_stop(cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(MAX_RECORDING).await;
        let task = cx.update(|cx| {
            let long_enough = recording_elapsed(cx).is_some_and(|elapsed| elapsed >= MAX_RECORDING);
            long_enough.then(|| stop_recording(cx))
        });
        if let Some(task) = task
            && let Err(error) = task.await
        {
            log::error!("couldn't stop the recording: {error:#}");
        }
    })
    .detach();
}

fn stop_recording(cx: &mut App) -> Task<Result<Option<PathBuf>>> {
    let Some(recording) = cx.default_global::<CaptureState>().recording.take() else {
        return Task::ready(Ok(None));
    };
    cx.refresh_windows();
    let duration = recording.started.elapsed();
    let path = recording.path.clone();
    #[cfg(target_os = "macos")]
    let finished = platform_mac::stop_recording(recording.child, cx);
    #[cfg(not(target_os = "macos"))]
    let finished = recording.session.finish(duration, cx);
    cx.background_spawn(async move {
        finished.await?;
        Ok(Some(convert_to_mp4(&path).unwrap_or(path)))
    })
}

#[cfg(not(target_os = "macos"))]
enum Session {
    Scap(platform::RecordingSession),
    #[cfg(target_os = "linux")]
    X11(x11::RecordingThread),
}

#[cfg(not(target_os = "macos"))]
impl Session {
    fn finish(self, duration: Duration, cx: &mut App) -> Task<Result<()>> {
        match self {
            Session::Scap(session) => session.finish(duration, cx),
            #[cfg(target_os = "linux")]
            Session::X11(thread) => cx.background_spawn(async move { thread.finish(duration) }),
        }
    }
}

/// Re-encodes a recording as MP4 with ffmpeg, when it's installed, and
/// returns the new file. MP4 plays everywhere and is far smaller.
fn convert_to_mp4(path: &Path) -> Option<PathBuf> {
    if path.extension().is_some_and(|extension| extension == "mov") {
        return None;
    }
    let ffmpeg = which::which("ffmpeg").ok()?;
    let output = path.with_extension("mp4");
    let status = gpui_util::new_std_command(ffmpeg)
        .args(["-y", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-movflags", "+faststart"])
        .arg(&output)
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    std::fs::remove_file(path).ok();
    Some(output)
}

/// Converts a captured frame's pixels to RGB, keeping only `rect`.
/// `layout` names where red, green and blue sit in each pixel.
pub fn crop_to_rgb(
    data: &[u8],
    frame_width: u32,
    frame_height: u32,
    bytes_per_pixel: usize,
    layout: [usize; 3],
    rect: CropRect,
) -> Result<image::RgbImage> {
    let rect = rect
        .clamped(frame_width, frame_height)
        .ok_or_else(|| anyhow!("the window isn't on the captured screen"))?;
    let stride = frame_width as usize * bytes_per_pixel;
    if data.len() < stride * frame_height as usize {
        bail!("the captured frame is smaller than it says");
    }
    let mut pixels = Vec::with_capacity(rect.width as usize * rect.height as usize * 3);
    for row in rect.y..rect.y + rect.height {
        let start = row as usize * stride + rect.x as usize * bytes_per_pixel;
        let end = start + rect.width as usize * bytes_per_pixel;
        for pixel in data[start..end].chunks_exact(bytes_per_pixel) {
            pixels.extend_from_slice(&[pixel[layout[0]], pixel[layout[1]], pixel[layout[2]]]);
        }
    }
    image::RgbImage::from_raw(rect.width, rect.height, pixels)
        .ok_or_else(|| anyhow!("couldn't assemble the captured image"))
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::*;
    use futures::channel::oneshot;
    use gpui::{ScreenCaptureFrame, ScreenCaptureSource, ScreenCaptureStream};
    use std::rc::Rc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    fn frame_to_rgb(frame: &ScreenCaptureFrame, rect: CropRect) -> Result<image::RgbImage> {
        use scap::frame::Frame;
        let (data, width, height, bytes_per_pixel, layout) = match &frame.0 {
            Frame::BGRx(frame) => (&frame.data, frame.width, frame.height, 4, [2, 1, 0]),
            Frame::BGRA(frame) => (&frame.data, frame.width, frame.height, 4, [2, 1, 0]),
            Frame::BGR0(frame) => (&frame.data, frame.width, frame.height, 4, [2, 1, 0]),
            Frame::RGBx(frame) => (&frame.data, frame.width, frame.height, 4, [0, 1, 2]),
            Frame::XBGR(frame) => (&frame.data, frame.width, frame.height, 4, [3, 2, 1]),
            Frame::RGB(frame) => (&frame.data, frame.width, frame.height, 3, [0, 1, 2]),
            Frame::YUVFrame(_) => bail!("the screen came back in a video format noah can't read"),
        };
        crop_to_rgb(
            data,
            width.max(0) as u32,
            height.max(0) as u32,
            bytes_per_pixel,
            layout,
            rect,
        )
    }

    /// The screen to capture: the window's own when it can be identified, else
    /// the main one, else the first.
    async fn pick_source(
        display: Option<u32>,
        cx: &mut gpui::AsyncApp,
    ) -> Result<Rc<dyn ScreenCaptureSource>> {
        let supported = cx.update(|cx| cx.is_screen_capture_supported());
        if !supported {
            bail!("screen capture isn't available on this system");
        }
        let sources = cx
            .update(|cx| cx.screen_capture_sources())
            .await
            .context("screen capture stopped unexpectedly")??;
        let own = display.and_then(|display| {
            sources.iter().find(|source| {
                source
                    .metadata()
                    .is_ok_and(|metadata| metadata.id as u32 == display)
            })
        });
        let main = sources.iter().find(|source| {
            source
                .metadata()
                .is_ok_and(|metadata| metadata.is_main == Some(true))
        });
        own.or(main)
            .or(sources.first())
            .cloned()
            .ok_or_else(|| anyhow!("no screen is available to capture"))
    }

    pub(super) fn screenshot(
        path: PathBuf,
        rect: CropRect,
        display: Option<u32>,
        cx: &mut App,
    ) -> Task<Result<()>> {
        cx.spawn(async move |cx| {
            let source = pick_source(display, cx).await?;
            let (sender, receiver) = oneshot::channel::<Result<image::RgbImage>>();
            let sender = Mutex::new(Some(sender));
            let stream = cx
                .update(|cx| {
                    source.stream(
                        cx.foreground_executor(),
                        Box::new(move |frame| {
                            let Ok(mut sender) = sender.lock() else {
                                return;
                            };
                            if let Some(sender) = sender.take() {
                                sender.send(frame_to_rgb(&frame, rect)).ok();
                            }
                        }),
                    )
                })
                .await
                .context("screen capture stopped unexpectedly")??;
            let image = receiver.await.context("no frame arrived from the screen")??;
            drop(stream);
            cx.background_spawn(async move {
                image
                    .save_with_format(&path, image::ImageFormat::Png)
                    .with_context(|| format!("couldn't save {}", path.display()))
            })
            .await
        })
    }

    struct Encoder {
        writer: Option<AviWriter>,
        last_frame: Option<Instant>,
        error: Option<String>,
    }

    pub(super) struct RecordingSession {
        _stream: Box<dyn ScreenCaptureStream>,
        stopped: Arc<AtomicBool>,
        encoder: Arc<Mutex<Encoder>>,
    }

    impl RecordingSession {
        pub(super) fn finish(self, duration: Duration, cx: &mut App) -> Task<Result<()>> {
            self.stopped.store(true, Ordering::SeqCst);
            drop(self._stream);
            let encoder = self.encoder;
            cx.background_spawn(async move {
                let mut encoder = encoder
                    .lock()
                    .map_err(|_| anyhow!("the recording was interrupted"))?;
                if let Some(error) = encoder.error.take() {
                    bail!("the recording failed: {error}");
                }
                let writer = encoder
                    .writer
                    .take()
                    .ok_or_else(|| anyhow!("the recording has no frames"))?;
                if writer.frame_count() == 0 {
                    bail!("no frames were captured");
                }
                writer.finish(duration)
            })
        }
    }

    pub(super) fn start_recording(
        path: PathBuf,
        rect: CropRect,
        display: Option<u32>,
        cx: &mut App,
    ) -> Task<Result<RecordingSession>> {
        cx.spawn(async move |cx| {
            let source = pick_source(display, cx).await?;
            let stopped = Arc::new(AtomicBool::new(false));
            let encoder = Arc::new(Mutex::new(Encoder {
                writer: None,
                last_frame: None,
                error: None,
            }));
            let stream = cx
                .update(|cx| {
                    let stopped = stopped.clone();
                    let encoder = encoder.clone();
                    source.stream(
                        cx.foreground_executor(),
                        Box::new(move |frame| {
                            if stopped.load(Ordering::SeqCst) {
                                return;
                            }
                            let Ok(mut encoder) = encoder.lock() else {
                                return;
                            };
                            if encoder.error.is_some()
                                || encoder
                                    .last_frame
                                    .is_some_and(|last| last.elapsed() < RECORDING_FRAME_INTERVAL)
                            {
                                return;
                            }
                            encoder.last_frame = Some(Instant::now());
                            let written = (|| -> Result<()> {
                                let image = frame_to_rgb(&frame, rect)?;
                                if encoder.writer.is_none() {
                                    encoder.writer =
                                        Some(AviWriter::create(&path, image.width(), image.height())?);
                                }
                                let Some(writer) = encoder.writer.as_mut() else {
                                    return Ok(());
                                };
                                let (width, height) = writer.dimensions();
                                let image = if (image.width(), image.height()) == (width, height) {
                                    image
                                } else {
                                    image::imageops::resize(
                                        &image,
                                        width,
                                        height,
                                        image::imageops::FilterType::Triangle,
                                    )
                                };
                                let mut jpeg = Vec::new();
                                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 82)
                                    .encode_image(&image)?;
                                writer.write_frame(&jpeg)
                            })();
                            if let Err(error) = written {
                                encoder.error = Some(format!("{error:#}"));
                            }
                        }),
                    )
                })
                .await
                .context("screen capture stopped unexpectedly")??;
            Ok(RecordingSession {
                _stream: stream,
                stopped,
                encoder,
            })
        })
    }
}

/// X11 lets a program read its own window's pixels directly, which works on
/// every X11 desktop (with or without a compositor or window manager).
#[cfg(target_os = "linux")]
mod x11 {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat};

    pub(super) fn window_id(window: &Window) -> Option<u32> {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        match HasWindowHandle::window_handle(window).ok()?.as_raw() {
            RawWindowHandle::Xlib(handle) => u32::try_from(handle.window).ok(),
            RawWindowHandle::Xcb(handle) => Some(handle.window.get()),
            _ => None,
        }
    }

    struct Grabber {
        connection: x11rb::rust_connection::RustConnection,
        window: u32,
    }

    impl Grabber {
        fn connect(window: u32) -> Result<Self> {
            let (connection, _) =
                x11rb::connect(None).context("couldn't reach the X server to capture the window")?;
            Ok(Self { connection, window })
        }

        fn grab(&self) -> Result<image::RgbImage> {
            let geometry = self
                .connection
                .get_geometry(self.window)?
                .reply()
                .context("couldn't read the window's size")?;
            let image = self
                .connection
                .get_image(
                    ImageFormat::Z_PIXMAP,
                    self.window,
                    0,
                    0,
                    geometry.width,
                    geometry.height,
                    u32::MAX,
                )?
                .reply()
                .context("the X server didn't return the window's pixels")?;
            let width = u32::from(geometry.width);
            let height = u32::from(geometry.height);
            let bytes_per_pixel = image.data.len() / (width as usize * height as usize).max(1);
            if bytes_per_pixel < 3 {
                bail!("the window uses a color depth noah can't capture");
            }
            let big_endian = self.connection.setup().image_byte_order
                == x11rb::protocol::xproto::ImageOrder::MSB_FIRST;
            let layout = if big_endian { [1, 2, 3] } else { [2, 1, 0] };
            crop_to_rgb(
                &image.data,
                width,
                height,
                bytes_per_pixel,
                layout,
                CropRect { x: 0, y: 0, width, height },
            )
        }
    }

    pub(super) fn screenshot(path: PathBuf, window: u32, cx: &mut App) -> Task<Result<()>> {
        cx.background_spawn(async move {
            let image = Grabber::connect(window)?.grab()?;
            image
                .save_with_format(&path, image::ImageFormat::Png)
                .with_context(|| format!("couldn't save {}", path.display()))
        })
    }

    pub(super) struct RecordingThread {
        stop: Arc<AtomicBool>,
        thread: std::thread::JoinHandle<Result<Option<AviWriter>>>,
    }

    impl RecordingThread {
        pub(super) fn finish(self, duration: Duration) -> Result<()> {
            self.stop.store(true, Ordering::SeqCst);
            let writer = self
                .thread
                .join()
                .map_err(|_| anyhow!("the recording stopped unexpectedly"))??;
            let writer = writer.ok_or_else(|| anyhow!("no frames were captured"))?;
            writer.finish(duration)
        }
    }

    pub(super) fn start_recording(path: PathBuf, window: u32) -> Result<Session> {
        let grabber = Grabber::connect(window)?;
        let first = grabber.grab()?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread = std::thread::Builder::new()
            .name("noah-screen-recording".into())
            .spawn({
                let stop = stop.clone();
                move || -> Result<Option<AviWriter>> {
                    let (width, height) = (first.width() & !1, first.height() & !1);
                    let mut writer = AviWriter::create(&path, width, height)?;
                    let mut next = Some(first);
                    while !stop.load(Ordering::SeqCst) {
                        let started = Instant::now();
                        let image = match next.take() {
                            Some(image) => image,
                            None => grabber.grab()?,
                        };
                        let image = if (image.width(), image.height()) == (width, height) {
                            image
                        } else {
                            image::imageops::resize(
                                &image,
                                width,
                                height,
                                image::imageops::FilterType::Triangle,
                            )
                        };
                        let mut jpeg = Vec::new();
                        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 82)
                            .encode_image(&image)?;
                        writer.write_frame(&jpeg)?;
                        if let Some(rest) = RECORDING_FRAME_INTERVAL.checked_sub(started.elapsed()) {
                            std::thread::sleep(rest);
                        }
                    }
                    Ok(Some(writer))
                }
            })
            .context("couldn't start recording")?;
        Ok(Session::X11(RecordingThread { stop, thread }))
    }
}

#[cfg(target_os = "macos")]
mod platform_mac {
    use super::*;
    use gpui::{Bounds, Pixels};

    fn region(bounds: Bounds<Pixels>) -> String {
        format!(
            "-R{},{},{},{}",
            f32::from(bounds.origin.x).round(),
            f32::from(bounds.origin.y).round(),
            f32::from(bounds.size.width).round(),
            f32::from(bounds.size.height).round()
        )
    }

    pub(super) fn screenshot(path: &Path, bounds: Bounds<Pixels>, cx: &mut App) -> Task<Result<()>> {
        let path = path.to_path_buf();
        cx.background_spawn(async move {
            let status = std::process::Command::new("/usr/sbin/screencapture")
                .arg("-x")
                .arg(region(bounds))
                .arg(&path)
                .status()
                .context("couldn't run screencapture")?;
            if !status.success() || !path.exists() {
                bail!(
                    "macOS didn't allow the screenshot; allow noah under System Settings > Privacy & Security > Screen Recording"
                );
            }
            Ok(())
        })
    }

    pub(super) fn start_recording(path: &Path, bounds: Bounds<Pixels>) -> Result<std::process::Child> {
        std::process::Command::new("/usr/sbin/screencapture")
            .arg("-v")
            .arg("-x")
            .arg(format!("-V{}", MAX_RECORDING.as_secs()))
            .arg(region(bounds))
            .arg(path)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("couldn't start screencapture")
    }

    pub(super) fn stop_recording(mut child: std::process::Child, cx: &mut App) -> Task<Result<()>> {
        cx.background_spawn(async move {
            // screencapture finishes and saves the movie on an interrupt.
            let interrupted = std::process::Command::new("/bin/kill")
                .arg("-INT")
                .arg(child.id().to_string())
                .status()
                .context("couldn't stop screencapture")?;
            if !interrupted.success() {
                child.kill().ok();
            }
            child.wait().context("screencapture didn't finish")?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_and_reorders_pixels() {
        // A 3x2 BGRx frame whose pixels count up in their blue channel.
        let mut data = Vec::new();
        for index in 0..6u8 {
            data.extend_from_slice(&[index, 10, 20, 255]);
        }
        let rect = CropRect { x: 1, y: 0, width: 2, height: 2 };
        let image = crop_to_rgb(&data, 3, 2, 4, [2, 1, 0], rect).expect("crop");
        assert_eq!(image.dimensions(), (2, 2));
        assert_eq!(image.get_pixel(0, 0).0, [20, 10, 1]);
        assert_eq!(image.get_pixel(1, 1).0, [20, 10, 5]);
    }

    #[test]
    fn keeps_crops_inside_the_frame() {
        let rect = CropRect { x: 100, y: 50, width: 5000, height: 5000 };
        assert_eq!(
            rect.clamped(1920, 1080),
            Some(CropRect { x: 100, y: 50, width: 1820, height: 1030 })
        );
        assert_eq!(CropRect { x: 3000, y: 0, width: 10, height: 10 }.clamped(1920, 1080), None);
    }
}
