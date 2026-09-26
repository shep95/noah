//! The notepad: a pad that floats over noah's window. Drag its title bar to
//! put it anywhere, drag its corner to size it, and it stays above the rooms
//! as you move between them. What is typed saves itself to ~/.noah/notes.md,
//! plain markdown the person can open anywhere, and the pad comes back where
//! it was left.

use std::path::PathBuf;
use std::time::Duration;

use editor::{Editor, EditorEvent, EditorMode};
use gpui::{
    App, AppContext as _, Bounds, Context, DragMoveEvent, Entity, Focusable as _, ImageSource,
    ObjectFit, Pixels, Point, Render, Size, Subscription, Task, WeakEntity, Window, img, point,
    px,
};
use language::language_settings::SoftWrap;
use serde::{Deserialize, Serialize};
use settings::Settings as _;
use ui::{IconButton, IconName, IconSize, Tooltip, prelude::*};
use workspace::{Workspace, WorkspaceSettings};

/// Typing pauses this long before the file is written.
const SAVE_DELAY: Duration = Duration::from_millis(600);
const FLOATING_NAME: &str = "notes";
const MIN_SIZE: Size<Pixels> = Size {
    width: px(220.0),
    height: px(160.0),
};

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        workspace.register_action(|workspace, _: &zed_actions::ToggleNotes, window, cx| {
            let weak = workspace.weak_handle();
            workspace.toggle_floating_view(
                FLOATING_NAME,
                move |_, window, cx| cx.new(|cx| Notes::new(weak, window, cx)).into(),
                window,
                cx,
            );
        });
    })
    .detach();
}

fn placement_file() -> PathBuf {
    paths::data_dir().join("notes_window.json")
}

/// Where the pad sits in the window, from its top-left corner.
#[derive(Serialize, Deserialize, Clone, Copy)]
struct Placement {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            x: 120.0,
            y: 80.0,
            width: 420.0,
            height: 480.0,
        }
    }
}

fn saved_placement() -> Placement {
    std::fs::read(placement_file())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Placement>(&bytes).ok())
        .filter(|saved| {
            saved.width >= f32::from(MIN_SIZE.width) && saved.height >= f32::from(MIN_SIZE.height)
        })
        .unwrap_or_default()
}

/// What is being dragged: the pad by its title bar, or its size by the corner.
struct DraggedNotes {
    resizing: bool,
    /// Where in the pad the drag started, so it doesn't jump under the cursor.
    grip: Point<Pixels>,
}

impl Render for DraggedNotes {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

pub struct Notes {
    workspace: WeakEntity<Workspace>,
    editor: Entity<Editor>,
    path: PathBuf,
    placement: Placement,
    /// Where the pad's container sits in the window, so drags can be
    /// measured from its origin.
    origin: Point<Pixels>,
    /// The text as last written, so an unchanged buffer isn't rewritten.
    written: String,
    saving: bool,
    save: Option<Task<()>>,
    place: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Notes {
    fn new(workspace: WeakEntity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
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
        let subscriptions = vec![cx.subscribe(&editor, |this, _, event: &EditorEvent, cx| {
            if matches!(event, EditorEvent::BufferEdited) {
                this.schedule_save(cx);
            }
        })];
        window.focus(&editor.focus_handle(cx), cx);
        Self {
            workspace,
            editor,
            path,
            placement: saved_placement(),
            origin: Point::default(),
            written: text,
            saving: false,
            save: None,
            place: None,
            _subscriptions: subscriptions,
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.workspace
            .update(cx, |workspace, cx| workspace.hide_floating_view(FLOATING_NAME, cx))
            .ok();
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

    /// Remembers where the pad is, a moment after it stops moving.
    fn schedule_place(&mut self, cx: &mut Context<Self>) {
        let placement = self.placement;
        self.place = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(Duration::from_millis(400)).await;
            let saved = cx
                .background_spawn(async move {
                    let file = placement_file();
                    if let Some(directory) = file.parent() {
                        std::fs::create_dir_all(directory)?;
                    }
                    std::fs::write(file, serde_json::to_vec(&placement)?)?;
                    anyhow::Ok(())
                })
                .await;
            if let Err(error) = saved {
                log::error!("couldn't remember where the notepad is: {error:#}");
            }
        }));
    }

    fn on_drag_move(&mut self, event: &DragMoveEvent<DraggedNotes>, window: &mut Window, cx: &mut Context<Self>) {
        let drag = event.drag(cx);
        let container = window.viewport_size();
        let cursor = point(event.event.position.x - self.origin.x, event.event.position.y - self.origin.y);
        if drag.resizing {
            let width = f32::from(cursor.x - px(self.placement.x) + drag.grip.x);
            let height = f32::from(cursor.y - px(self.placement.y) + drag.grip.y);
            self.placement.width = width.max(f32::from(MIN_SIZE.width));
            self.placement.height = height.max(f32::from(MIN_SIZE.height));
        } else {
            // The title bar can't leave the window, so the pad can always be
            // reached again.
            let max_x = (f32::from(container.width) - 60.0).max(0.0);
            let max_y = (f32::from(container.height) - 40.0).max(0.0);
            self.placement.x = f32::from(cursor.x - drag.grip.x).clamp(0.0, max_x);
            self.placement.y = f32::from(cursor.y - drag.grip.y).clamp(0.0, max_y);
        }
        self.schedule_place(cx);
        cx.notify();
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
        let placement = self.placement;
        let origin_entity = cx.entity().downgrade();
        // The container spans the window without drawing or catching clicks;
        // it is what measures drags and keeps the pad's coordinates local.
        div()
            .id("notes-overlay")
            .absolute()
            .inset_0()
            .on_drag_move(cx.listener(Self::on_drag_move))
            .child(
                gpui::canvas(
                    move |bounds: Bounds<Pixels>, _, cx| {
                        origin_entity
                            .update(cx, |this, _| this.origin = bounds.origin)
                            .ok();
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0()
                .size_full(),
            )
            .child(
                v_flex()
                    .id("notes-pad")
                    .absolute()
                    .left(px(placement.x))
                    .top(px(placement.y))
                    .w(px(placement.width))
                    .h(px(placement.height))
                    .overflow_hidden()
                    .rounded_lg()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.elevated_surface_background)
                    .shadow_lg()
                    .occlude()
                    .child(
                        img(wallpaper_source)
                            .absolute()
                            .inset_0()
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .opacity(wallpaper.wallpaper_opacity * 0.5),
                    )
                    .child(
                        h_flex()
                            .id("notes-title")
                            .relative()
                            .flex_none()
                            .px_3()
                            .py_1p5()
                            .gap_2()
                            .border_b_1()
                            .border_color(colors.border_variant)
                            .cursor_grab()
                            .on_drag(
                                DraggedNotes {
                                    resizing: false,
                                    grip: Point::default(),
                                },
                                |_, grip, _, cx| {
                                    cx.new(|_| DraggedNotes {
                                        resizing: false,
                                        grip,
                                    })
                                },
                            )
                            .child(Label::new("notes").size(LabelSize::Small).color(Color::Muted))
                            .child(
                                Label::new("drag to move · corner to size")
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            )
                            .child(div().flex_1())
                            .child(Label::new(status).size(LabelSize::XSmall).color(Color::Muted))
                            .child(
                                IconButton::new("notes-close", IconName::Close)
                                    .icon_size(IconSize::XSmall)
                                    .icon_color(Color::Muted)
                                    .tooltip(Tooltip::text("hide the notepad; it keeps its text"))
                                    .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .px_3()
                            .py_2()
                            .child(self.editor.clone()),
                    )
                    .child(
                        div()
                            .id("notes-resize")
                            .absolute()
                            .right_0()
                            .bottom_0()
                            .size(px(18.0))
                            .cursor_nwse_resize()
                            .on_drag(
                                DraggedNotes {
                                    resizing: true,
                                    grip: Point::default(),
                                },
                                move |_, grip, _, cx| {
                                    cx.new(move |_| DraggedNotes {
                                        resizing: true,
                                        grip: point(px(18.0) - grip.x, px(18.0) - grip.y),
                                    })
                                },
                            )
                            .child(
                                div()
                                    .absolute()
                                    .right(px(4.0))
                                    .bottom(px(4.0))
                                    .size(px(8.0))
                                    .border_r_1()
                                    .border_b_1()
                                    .border_color(colors.text_muted),
                            ),
                    ),
            )
    }
}
