//! The notepad: a small window of its own, so it can be dragged anywhere,
//! onto any screen, and sized as the person likes, on top of or beside
//! noah. What is typed saves itself to ~/.noah/notes.md, plain markdown
//! they can open anywhere, and the window comes back where it was left.

use std::path::PathBuf;
use std::time::Duration;

use editor::{Editor, EditorEvent, EditorMode};
use gpui::{
    App, AppContext as _, Bounds, Context, Entity, Focusable as _, ImageSource, ObjectFit,
    Pixels, Subscription, Task, TitlebarOptions, Window, WindowBounds, WindowHandle,
    WindowOptions, img, point, px, size,
};
use language::language_settings::SoftWrap;
use serde::{Deserialize, Serialize};
use settings::Settings as _;
use ui::prelude::*;
use util::ResultExt as _;
use workspace::WorkspaceSettings;

/// Typing pauses this long before the file is written.
const SAVE_DELAY: Duration = Duration::from_millis(600);

pub fn init(cx: &mut App) {
    cx.on_action(|_: &zed_actions::ToggleNotes, cx| toggle(cx));
}

struct NotesWindow(WindowHandle<Notes>);

impl gpui::Global for NotesWindow {}

/// Opens the notepad, brings it forward if it is already open, or closes it
/// when it is the active window: one button, one idea.
pub fn toggle(cx: &mut App) {
    if let Some(handle) = cx.try_global::<NotesWindow>().map(|window| window.0) {
        let outcome = handle.update(cx, |_, window, _| {
            if window.is_window_active() {
                window.remove_window();
            } else {
                window.activate_window();
            }
        });
        if outcome.is_ok() {
            return;
        }
    }
    open(cx);
}

fn bounds_file() -> PathBuf {
    paths::data_dir().join("notes_window.json")
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct SavedBounds {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

fn saved_bounds() -> Option<Bounds<Pixels>> {
    let saved: SavedBounds = serde_json::from_slice(&std::fs::read(bounds_file()).ok()?).ok()?;
    (saved.width >= 120.0 && saved.height >= 100.0).then(|| {
        Bounds::new(
            point(px(saved.x), px(saved.y)),
            size(px(saved.width), px(saved.height)),
        )
    })
}

fn open(cx: &mut App) {
    let window_bounds = match saved_bounds() {
        Some(bounds) => WindowBounds::Windowed(bounds),
        None => WindowBounds::centered(size(px(420.0), px(520.0)), cx),
    };
    let app_id = release_channel::ReleaseChannel::global(cx).app_id().to_owned();
    let opened = cx.open_window(
        WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("noah — notes".into()),
                appears_transparent: false,
                traffic_light_position: None,
            }),
            focus: true,
            show: true,
            is_movable: true,
            kind: gpui::WindowKind::Normal,
            window_background: cx.theme().window_background_appearance(),
            app_id: Some(app_id),
            window_min_size: Some(size(px(180.0), px(140.0))),
            window_bounds: Some(window_bounds),
            ..Default::default()
        },
        |window, cx| cx.new(|cx| Notes::new(window, cx)),
    );
    match opened {
        Ok(handle) => cx.set_global(NotesWindow(handle)),
        Err(error) => log::error!("couldn't open the notepad: {error:#}"),
    }
}

pub struct Notes {
    editor: Entity<Editor>,
    path: PathBuf,
    /// The text as last written, so an unchanged buffer isn't rewritten.
    written: String,
    saving: bool,
    save: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Notes {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let path = paths::notes_file();
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let editor = cx.new(|cx| {
            let buffer = cx.new(|cx| language::Buffer::local(text.clone(), cx));
            let buffer = cx.new(|cx| multi_buffer::MultiBuffer::singleton(buffer, cx));
            let mut editor = Editor::new(EditorMode::full(), buffer, None, window, cx);
            editor.set_placeholder_text("write anything. it saves itself.", window, cx);
            editor.set_show_gutter(false, cx);
            editor.set_show_line_numbers(false, cx);
            editor.set_show_wrap_guides(false, cx);
            editor.set_show_indent_guides(false, cx);
            editor.set_soft_wrap_mode(SoftWrap::EditorWidth, cx);
            editor
        });
        let mut subscriptions = vec![cx.subscribe(&editor, |this, _, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::BufferEdited) {
                this.schedule_save(cx);
            }
        })];
        subscriptions.push(cx.observe_window_bounds(window, |_, window, cx| {
            let bounds = window.bounds();
            let saved = SavedBounds {
                x: f32::from(bounds.origin.x),
                y: f32::from(bounds.origin.y),
                width: f32::from(bounds.size.width),
                height: f32::from(bounds.size.height),
            };
            cx.background_spawn(async move {
                let file = bounds_file();
                if let Some(directory) = file.parent() {
                    std::fs::create_dir_all(directory)?;
                }
                std::fs::write(file, serde_json::to_vec(&saved)?)?;
                anyhow::Ok(())
            })
            .detach_and_log_err(cx);
        }));
        window.focus(&editor.focus_handle(cx), cx);
        Self {
            editor,
            path,
            written: text,
            saving: false,
            save: None,
            _subscriptions: subscriptions,
        }
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        self.saving = true;
        cx.notify();
        self.save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let Ok(Some((path, text))) = this.update(cx, |this, cx| {
                let text = this.editor.read(cx).text(cx);
                (text != this.written).then(|| (this.path.clone(), text))
            }) else {
                this.update(cx, |this, cx| {
                    this.saving = false;
                    cx.notify();
                })
                .ok();
                return;
            };
            let written = cx
                .background_spawn({
                    let path = path.clone();
                    let text = text.clone();
                    async move {
                        if let Some(directory) = path.parent() {
                            std::fs::create_dir_all(directory)?;
                        }
                        let temporary = path.with_extension("md.tmp");
                        std::fs::write(&temporary, text.as_bytes())?;
                        std::fs::rename(&temporary, &path)?;
                        anyhow::Ok(())
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                match written {
                    Ok(()) => this.written = text,
                    Err(error) => log::error!("couldn't save the notepad: {error:#}"),
                }
                this.saving = false;
                cx.notify();
            })
            .ok();
        }));
    }
}

impl Render for Notes {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let wallpaper = WorkspaceSettings::get_global(cx);
        let wallpaper_source: ImageSource = match wallpaper.wallpaper.as_deref() {
            Some(path) => PathBuf::from(path).into(),
            None => "images/noah/wallpaper.jpg".into(),
        };
        let status = if self.saving { "saving" } else { "saved" };
        div()
            .relative()
            .size_full()
            .bg(colors.background)
            .child(
                img(wallpaper_source)
                    .absolute()
                    .inset_0()
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .opacity(wallpaper.wallpaper_opacity * 0.6),
            )
            .child(
                v_flex()
                    .absolute()
                    .inset_0()
                    .size_full()
                    .child(
                        h_flex()
                            .flex_none()
                            .px_3()
                            .py_1p5()
                            .gap_2()
                            .border_b_1()
                            .border_color(colors.border_variant)
                            .child(Label::new("notes").size(LabelSize::Small).color(Color::Muted))
                            .child(div().flex_1())
                            .child(Label::new(status).size(LabelSize::XSmall).color(Color::Muted))
                            .child(
                                Label::new(self.path.display().to_string())
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .child(div().flex_1().min_h_0().px_3().py_2().child(self.editor.clone())),
            )
    }
}
