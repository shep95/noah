//! asherin.board: a whiteboard the person and shepherd share. The person
//! draws with the mouse and keyboard; shepherd draws by editing `board.json`
//! in the board folder (see assets/shepherd/board_guide.md). Both see the
//! same file, which the view reloads within a second of a change.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::cell::Cell;
use std::time::{Duration, SystemTime};

use anyhow::{Context as _, Result};
use gpui::{
    App, AppContext as _, Bounds, ClipboardEntry, Context, Entity, EventEmitter, FocusHandle,
    Focusable, Hsla, ImageSource, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ObjectFit, PathBuilder, PathPromptOptions, Pixels, Point, SharedString,
    StyledImage as _, Task, TextRun, Window, canvas, fill, img, outline, point, px, quad, size,
};
use serde::{Deserialize, Serialize};
use settings::Settings as _;
use ui::{IconButton, IconName, IconSize, Tooltip, prelude::*};
use util::ResultExt as _;
use workspace::item::{Item, ItemEvent};
use workspace::{Workspace, WorkspaceSettings};

pub const BOARD_FILE: &str = "board.json";
const IMAGES_DIRECTORY: &str = "images";
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_TEXT_SIZE: f32 = 16.0;
const DEFAULT_LINE_WIDTH: f32 = 2.0;
const DEFAULT_IMAGE_RADIUS: f32 = 12.0;
const MAX_IMAGE_WIDTH: f32 = 360.0;

fn default_radius() -> f32 {
    DEFAULT_IMAGE_RADIUS
}

fn default_width() -> f32 {
    DEFAULT_LINE_WIDTH
}

fn default_text_size() -> f32 {
    DEFAULT_TEXT_SIZE
}

fn default_color() -> String {
    "ink".to_string()
}

/// Everything on the board, as shepherd reads and writes it.
#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
pub struct Board {
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub items: Vec<Shape>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shape {
    Stroke {
        points: Vec<[f32; 2]>,
        #[serde(default = "default_color")]
        color: String,
        #[serde(default = "default_width")]
        width: f32,
    },
    Line {
        from: [f32; 2],
        to: [f32; 2],
        #[serde(default = "default_color")]
        color: String,
        #[serde(default = "default_width")]
        width: f32,
    },
    Rect {
        origin: [f32; 2],
        size: [f32; 2],
        #[serde(default = "default_color")]
        color: String,
        #[serde(default = "default_width")]
        width: f32,
        #[serde(default)]
        fill: bool,
    },
    Ellipse {
        origin: [f32; 2],
        size: [f32; 2],
        #[serde(default = "default_color")]
        color: String,
        #[serde(default = "default_width")]
        width: f32,
        #[serde(default)]
        fill: bool,
    },
    Text {
        origin: [f32; 2],
        text: String,
        #[serde(default = "default_color")]
        color: String,
        #[serde(default = "default_text_size")]
        size: f32,
    },
    Image {
        origin: [f32; 2],
        size: [f32; 2],
        path: String,
        #[serde(default = "default_radius")]
        radius: f32,
    },
}

impl Shape {
    /// The box the shape occupies, for hit tests and moving.
    fn bounds(&self) -> (Point<f32>, Point<f32>) {
        match self {
            Shape::Stroke { points, width, .. } => {
                let mut min = point(f32::MAX, f32::MAX);
                let mut max = point(f32::MIN, f32::MIN);
                for [x, y] in points {
                    min = point(min.x.min(*x), min.y.min(*y));
                    max = point(max.x.max(*x), max.y.max(*y));
                }
                if points.is_empty() {
                    return (point(0.0, 0.0), point(0.0, 0.0));
                }
                let pad = width.max(4.0);
                (point(min.x - pad, min.y - pad), point(max.x + pad, max.y + pad))
            }
            Shape::Line { from, to, width, .. } => {
                let pad = width.max(4.0);
                (
                    point(from[0].min(to[0]) - pad, from[1].min(to[1]) - pad),
                    point(from[0].max(to[0]) + pad, from[1].max(to[1]) + pad),
                )
            }
            Shape::Rect { origin, size, .. }
            | Shape::Ellipse { origin, size, .. }
            | Shape::Image { origin, size, .. } => normalized(*origin, *size),
            Shape::Text { origin, text, size, .. } => {
                let size = *size;
                let width = text.chars().count() as f32 * size * 0.55;
                (
                    point(origin[0], origin[1]),
                    point(origin[0] + width.max(size), origin[1] + size * 1.4),
                )
            }
        }
    }

    fn contains(&self, position: Point<f32>) -> bool {
        let (min, max) = self.bounds();
        position.x >= min.x && position.x <= max.x && position.y >= min.y && position.y <= max.y
    }

    fn translate(&mut self, delta: Point<f32>) {
        match self {
            Shape::Stroke { points, .. } => {
                for [x, y] in points.iter_mut() {
                    *x += delta.x;
                    *y += delta.y;
                }
            }
            Shape::Line { from, to, .. } => {
                from[0] += delta.x;
                from[1] += delta.y;
                to[0] += delta.x;
                to[1] += delta.y;
            }
            Shape::Rect { origin, .. }
            | Shape::Ellipse { origin, .. }
            | Shape::Text { origin, .. }
            | Shape::Image { origin, .. } => {
                origin[0] += delta.x;
                origin[1] += delta.y;
            }
        }
    }
}

/// A rectangle drawn in any direction, as its top-left corner and size.
fn normalized(origin: [f32; 2], size: [f32; 2]) -> (Point<f32>, Point<f32>) {
    let (x0, x1) = if size[0] >= 0.0 {
        (origin[0], origin[0] + size[0])
    } else {
        (origin[0] + size[0], origin[0])
    };
    let (y0, y1) = if size[1] >= 0.0 {
        (origin[1], origin[1] + size[1])
    } else {
        (origin[1] + size[1], origin[1])
    };
    (point(x0, y0), point(x1, y1))
}

pub fn load_board(path: &Path) -> Result<Board> {
    let json = std::fs::read_to_string(path)
        .with_context(|| format!("couldn't read {}", path.display()))?;
    serde_json::from_str(&json).with_context(|| format!("{} isn't a board", path.display()))
}

/// Writes the board through a temporary file, so a reader never sees half.
pub fn save_board(path: &Path, board: &Board) -> Result<()> {
    let json = serde_json::to_string_pretty(board)?;
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, json)?;
    std::fs::rename(&temporary, path)?;
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    Select,
    Pen,
    Line,
    Rect,
    Ellipse,
    Text,
    Eraser,
}

impl Tool {
    const ALL: [Tool; 7] = [
        Tool::Select,
        Tool::Pen,
        Tool::Line,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Text,
        Tool::Eraser,
    ];

    fn label(self) -> &'static str {
        match self {
            Tool::Select => "move",
            Tool::Pen => "pen",
            Tool::Line => "line",
            Tool::Rect => "box",
            Tool::Ellipse => "circle",
            Tool::Text => "text",
            Tool::Eraser => "erase",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Tool::Select => IconName::CursorIBeam,
            Tool::Pen => IconName::Pencil,
            Tool::Line => IconName::Dash,
            Tool::Rect => IconName::SquareDot,
            Tool::Ellipse => IconName::Circle,
            Tool::Text => IconName::Font,
            Tool::Eraser => IconName::Eraser,
        }
    }
}

const PALETTE: [&str; 7] = ["ink", "muted", "accent", "red", "green", "blue", "yellow"];

/// A color name from the palette, or a hex color, as the theme paints it.
fn resolve_color(name: &str, cx: &App) -> Hsla {
    let colors = cx.theme().colors();
    let status = cx.theme().status();
    match name {
        "ink" => colors.text,
        "muted" => colors.text_muted,
        "accent" => colors.text_accent,
        "red" => status.error,
        "green" => status.success,
        "blue" => status.info,
        "yellow" => status.warning,
        "white" => gpui::white(),
        other => gpui::Rgba::try_from(other)
            .map(Hsla::from)
            .unwrap_or(colors.text),
    }
}

/// Where a mouse press turned into a shape that is still being drawn.
enum Draft {
    Stroke(Vec<[f32; 2]>),
    Line { from: [f32; 2], to: [f32; 2] },
    Rect { origin: [f32; 2], size: [f32; 2] },
    Ellipse { origin: [f32; 2], size: [f32; 2] },
    Move { index: usize, last: Point<f32> },
}

pub struct BoardView {
    root: PathBuf,
    board: Board,
    tool: Tool,
    color: String,
    draft: Option<Draft>,
    selected: Option<usize>,
    /// The text item being typed into, if any.
    typing: Option<usize>,
    focus_handle: FocusHandle,
    /// Where the drawing surface sits in the window, kept by the canvas so
    /// mouse positions can be turned into board coordinates.
    surface: Rc<Cell<Bounds<Pixels>>>,
    /// The file's modification time as of the last read or write. A newer
    /// file was written by someone else and is reloaded.
    modified: Option<SystemTime>,
    save: Option<Task<()>>,
    _poll: Task<()>,
}

impl BoardView {
    pub fn new(root: PathBuf, cx: &mut Context<Self>) -> Self {
        let path = root.join(BOARD_FILE);
        let board = load_board(&path).unwrap_or_default();
        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        let poll = cx.spawn({
            let path = path.clone();
            async move |this, cx| {
                loop {
                    cx.background_executor().timer(POLL_INTERVAL).await;
                    let path = path.clone();
                    let modified = cx
                        .background_spawn(async move {
                            std::fs::metadata(&path).and_then(|m| m.modified()).ok()
                        })
                        .await;
                    let alive = this
                        .update(cx, |this, cx| {
                            if modified != this.modified && this.draft.is_none() {
                                this.reload(cx);
                            }
                        })
                        .is_ok();
                    if !alive {
                        break;
                    }
                }
            }
        });
        Self {
            root,
            board,
            tool: Tool::Pen,
            color: "ink".to_string(),
            draft: None,
            selected: None,
            typing: None,
            focus_handle: cx.focus_handle(),
            surface: Rc::new(Cell::new(Bounds::default())),
            modified,
            save: None,
            _poll: poll,
        }
    }

    fn path(&self) -> PathBuf {
        self.root.join(BOARD_FILE)
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let path = self.path();
        match load_board(&path) {
            Ok(board) => {
                self.board = board;
                self.selected = None;
                self.typing = None;
            }
            Err(error) => log::warn!("asherin.board: {error:#}"),
        }
        self.modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        cx.notify();
    }

    /// Records a change: bumps the revision and writes the file in the
    /// background, remembering the resulting time so the poll doesn't reload
    /// our own write.
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.board.revision += 1;
        let path = self.path();
        let board = self.board.clone();
        self.save = Some(cx.spawn(async move |this, cx| {
            let saved = cx
                .background_spawn({
                    let path = path.clone();
                    async move {
                        save_board(&path, &board)?;
                        std::fs::metadata(&path)?.modified().context("no modification time")
                    }
                })
                .await;
            this.update(cx, |this, _| match saved {
                Ok(modified) => this.modified = Some(modified),
                Err(error) => log::error!("asherin.board: couldn't save: {error:#}"),
            })
            .ok();
        }));
        cx.notify();
    }

    fn board_position(&self, window_position: Point<Pixels>) -> Point<f32> {
        let origin = self.surface.get().origin;
        point(
            f32::from(window_position.x - origin.x),
            f32::from(window_position.y - origin.y),
        )
    }

    fn hit(&self, position: Point<f32>) -> Option<usize> {
        self.board
            .items
            .iter()
            .rposition(|shape| shape.contains(position))
    }

    fn finish_typing(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.typing.take() {
            let empty = matches!(self.board.items.get(index), Some(Shape::Text { text, .. }) if text.trim().is_empty());
            if empty {
                self.board.items.remove(index);
            }
            self.changed(cx);
        }
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.button != MouseButton::Left {
            return;
        }
        self.focus_handle.focus(window, cx);
        self.finish_typing(cx);
        let position = self.board_position(event.position);
        let at = [position.x, position.y];
        match self.tool {
            Tool::Pen => self.draft = Some(Draft::Stroke(vec![at])),
            Tool::Line => self.draft = Some(Draft::Line { from: at, to: at }),
            Tool::Rect => self.draft = Some(Draft::Rect { origin: at, size: [0.0, 0.0] }),
            Tool::Ellipse => self.draft = Some(Draft::Ellipse { origin: at, size: [0.0, 0.0] }),
            Tool::Text => {
                self.board.items.push(Shape::Text {
                    origin: at,
                    text: String::new(),
                    color: self.color.clone(),
                    size: DEFAULT_TEXT_SIZE,
                });
                self.typing = Some(self.board.items.len() - 1);
                self.selected = self.typing;
            }
            Tool::Eraser => {
                if let Some(index) = self.hit(position) {
                    self.board.items.remove(index);
                    self.selected = None;
                    self.changed(cx);
                }
            }
            Tool::Select => {
                self.selected = self.hit(position);
                if let Some(index) = self.selected {
                    self.draft = Some(Draft::Move { index, last: position });
                }
            }
        }
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if !event.dragging() {
            return;
        }
        let position = self.board_position(event.position);
        let at = [position.x, position.y];
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        // A move changes an item rather than the draft, so it is applied
        // once the draft borrow is over.
        let mut moved = None;
        match draft {
            Draft::Stroke(points) => {
                let far_enough = points
                    .last()
                    .is_none_or(|last| (last[0] - at[0]).abs() + (last[1] - at[1]).abs() >= 1.5);
                if far_enough {
                    points.push(at);
                }
            }
            Draft::Line { to, .. } => *to = at,
            Draft::Rect { origin, size } | Draft::Ellipse { origin, size } => {
                *size = [at[0] - origin[0], at[1] - origin[1]];
            }
            Draft::Move { index, last } => {
                let delta = point(position.x - last.x, position.y - last.y);
                *last = position;
                moved = Some((*index, delta));
            }
        }
        if let Some((index, delta)) = moved
            && let Some(shape) = self.board.items.get_mut(index)
        {
            shape.translate(delta);
        }
        cx.notify();
    }

    fn on_mouse_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.draft.take() else {
            return;
        };
        let color = self.color.clone();
        let shape = match draft {
            Draft::Stroke(points) if points.len() >= 2 => Some(Shape::Stroke {
                points,
                color,
                width: DEFAULT_LINE_WIDTH,
            }),
            Draft::Stroke(_) => None,
            Draft::Line { from, to } if from != to => Some(Shape::Line {
                from,
                to,
                color,
                width: DEFAULT_LINE_WIDTH,
            }),
            Draft::Line { .. } => None,
            Draft::Rect { origin, size } if size[0].abs() >= 2.0 && size[1].abs() >= 2.0 => {
                let (min, max) = normalized(origin, size);
                Some(Shape::Rect {
                    origin: [min.x, min.y],
                    size: [max.x - min.x, max.y - min.y],
                    color,
                    width: DEFAULT_LINE_WIDTH,
                    fill: false,
                })
            }
            Draft::Ellipse { origin, size } if size[0].abs() >= 2.0 && size[1].abs() >= 2.0 => {
                let (min, max) = normalized(origin, size);
                Some(Shape::Ellipse {
                    origin: [min.x, min.y],
                    size: [max.x - min.x, max.y - min.y],
                    color,
                    width: DEFAULT_LINE_WIDTH,
                    fill: false,
                })
            }
            Draft::Rect { .. } | Draft::Ellipse { .. } => None,
            Draft::Move { .. } => {
                self.changed(cx);
                None
            }
        };
        if let Some(shape) = shape {
            self.board.items.push(shape);
            self.changed(cx);
        }
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let command = keystroke.modifiers.platform || keystroke.modifiers.control;
        if command && keystroke.key == "v" {
            self.paste(cx);
            return;
        }
        if command && keystroke.key == "z" {
            self.undo(cx);
            return;
        }
        if let Some(index) = self.typing {
            match keystroke.key.as_str() {
                "enter" | "escape" => self.finish_typing(cx),
                "backspace" => {
                    if let Some(Shape::Text { text, .. }) = self.board.items.get_mut(index) {
                        text.pop();
                    }
                }
                _ => {
                    if let Some(typed) = keystroke.key_char.as_deref()
                        && !command
                        && let Some(Shape::Text { text, .. }) = self.board.items.get_mut(index)
                    {
                        text.push_str(typed);
                    }
                }
            }
            cx.notify();
            return;
        }
        match keystroke.key.as_str() {
            "delete" | "backspace" => {
                if let Some(index) = self.selected.take()
                    && index < self.board.items.len()
                {
                    self.board.items.remove(index);
                    self.changed(cx);
                }
            }
            "escape" => {
                self.selected = None;
                cx.notify();
            }
            _ => {}
        }
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        self.typing = None;
        if self.board.items.pop().is_some() {
            self.selected = None;
            self.changed(cx);
        }
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.typing = None;
        self.selected = None;
        if !self.board.items.is_empty() {
            self.board.items.clear();
            self.changed(cx);
        }
    }

    /// Adds a picture from the clipboard, if there is one.
    fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        for entry in item.entries() {
            match entry {
                ClipboardEntry::Image(image) => {
                    let extension = match image.format() {
                        gpui::ImageFormat::Png => "png",
                        gpui::ImageFormat::Jpeg => "jpg",
                        gpui::ImageFormat::Webp => "webp",
                        gpui::ImageFormat::Gif => "gif",
                        gpui::ImageFormat::Bmp => "bmp",
                        gpui::ImageFormat::Tiff => "tiff",
                        _ => "png",
                    };
                    let name = format!(
                        "pasted-{}.{extension}",
                        chrono::Local::now().format("%Y-%m-%d-%H%M%S")
                    );
                    let target = self.root.join(IMAGES_DIRECTORY).join(&name);
                    let written = std::fs::create_dir_all(self.root.join(IMAGES_DIRECTORY))
                        .and_then(|()| std::fs::write(&target, image.bytes()));
                    if written.log_err().is_some() {
                        self.place_image(&target, cx);
                    }
                    return;
                }
                ClipboardEntry::ExternalPaths(paths) => {
                    for path in paths.paths() {
                        if is_image_file(path) {
                            self.add_image_file(path.clone(), cx);
                        }
                    }
                    return;
                }
                ClipboardEntry::String(_) => {}
            }
        }
    }

    /// Asks for a picture file and places it on the board.
    fn choose_image(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("add to the board".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            this.update(cx, |this, cx| {
                for path in paths {
                    this.add_image_file(path, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Copies a picture into the board's own folder, so the board still shows
    /// it when the original moves, and places it.
    fn add_image_file(&mut self, source: PathBuf, cx: &mut Context<Self>) {
        let Some(file_name) = source.file_name() else {
            return;
        };
        let images = self.root.join(IMAGES_DIRECTORY);
        let target = images.join(file_name);
        let copied = std::fs::create_dir_all(&images).and_then(|()| {
            if target != source {
                std::fs::copy(&source, &target)?;
            }
            Ok(())
        });
        if copied.log_err().is_some() {
            self.place_image(&target, cx);
        }
    }

    fn place_image(&mut self, path: &Path, cx: &mut Context<Self>) {
        let (width, height) = image::image_dimensions(path)
            .map(|(w, h)| (w as f32, h as f32))
            .unwrap_or((320.0, 240.0));
        let scale = (MAX_IMAGE_WIDTH / width.max(1.0)).min(1.0);
        let size = [width * scale, height * scale];
        let surface = self.surface.get();
        let origin = [
            (f32::from(surface.size.width) / 2.0 - size[0] / 2.0).max(16.0),
            (f32::from(surface.size.height) / 2.0 - size[1] / 2.0).max(16.0),
        ];
        let relative = path
            .strip_prefix(&self.root)
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| path.to_string_lossy().to_string());
        self.board.items.push(Shape::Image {
            origin,
            size,
            path: relative,
            radius: DEFAULT_IMAGE_RADIUS,
        });
        self.selected = Some(self.board.items.len() - 1);
        self.tool = Tool::Select;
        self.changed(cx);
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        h_flex()
            .w_full()
            .flex_none()
            .px_2()
            .py_1()
            .gap_1()
            .border_b_1()
            .border_color(colors.border_variant)
            .bg(colors.title_bar_background)
            .child(
                Label::new("asherin.board")
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .mr_2(),
            )
            .children(Tool::ALL.into_iter().map(|tool| {
                let active = tool == self.tool;
                IconButton::new(tool.label(), tool.icon())
                    .icon_size(IconSize::Small)
                    .icon_color(if active { Color::Default } else { Color::Muted })
                    .toggle_state(active)
                    .tooltip(Tooltip::text(tool.label()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.finish_typing(cx);
                        this.tool = tool;
                        cx.notify();
                    }))
            }))
            .child(
                IconButton::new("board-photo", IconName::Image)
                    .icon_size(IconSize::Small)
                    .icon_color(Color::Muted)
                    .tooltip(Tooltip::text("add a photo (or paste one with ctrl-v)"))
                    .on_click(cx.listener(|this, _, _, cx| this.choose_image(cx))),
            )
            .child(div().w_2())
            .children(PALETTE.into_iter().enumerate().map(|(index, name)| {
                let current = self.color == name;
                div()
                    .id(("board-color", index))
                    .size(px(14.0))
                    .rounded_full()
                    .bg(resolve_color(name, cx))
                    .border_2()
                    .border_color(if current { colors.text } else { gpui::transparent_black() })
                    .cursor_pointer()
                    .tooltip(Tooltip::text(name))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.color = name.to_string();
                        if let Some(index) = this.selected
                            && let Some(shape) = this.board.items.get_mut(index)
                        {
                            match shape {
                                Shape::Stroke { color, .. }
                                | Shape::Line { color, .. }
                                | Shape::Rect { color, .. }
                                | Shape::Ellipse { color, .. }
                                | Shape::Text { color, .. } => *color = name.to_string(),
                                Shape::Image { .. } => {}
                            }
                            this.changed(cx);
                        }
                        cx.notify();
                    }))
            }))
            .child(div().flex_1())
            .child(
                IconButton::new("board-undo", IconName::Undo)
                    .icon_size(IconSize::Small)
                    .icon_color(Color::Muted)
                    .tooltip(Tooltip::text("undo the last thing (ctrl-z)"))
                    .on_click(cx.listener(|this, _, _, cx| this.undo(cx))),
            )
            .child(
                IconButton::new("board-clear", IconName::Trash)
                    .icon_size(IconSize::Small)
                    .icon_color(Color::Muted)
                    .tooltip(Tooltip::text("clear the board"))
                    .on_click(cx.listener(|this, _, _, cx| this.clear(cx))),
            )
            .child(
                Label::new(format!("ask shepherd to draw · {}", self.root.display()))
                    .size(LabelSize::XSmall)
                    .color(Color::Muted)
                    .ml_2(),
            )
    }

    fn render_surface(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let wallpaper = WorkspaceSettings::get_global(cx);
        let wallpaper_source: ImageSource = match wallpaper.wallpaper.as_deref() {
            Some(path) => PathBuf::from(path).into(),
            None => "images/noah/wallpaper.jpg".into(),
        };
        let colors = cx.theme().colors();
        let surface = self.surface.clone();
        let items = self.board.items.clone();
        let draft_shape = self.draft_shape();
        let selected = self.selected;
        let typing = self.typing;
        let painted: Vec<(Shape, Hsla)> = items
            .iter()
            .chain(draft_shape.iter())
            .map(|shape| {
                let color = match shape {
                    Shape::Stroke { color, .. }
                    | Shape::Line { color, .. }
                    | Shape::Rect { color, .. }
                    | Shape::Ellipse { color, .. }
                    | Shape::Text { color, .. } => resolve_color(color, cx),
                    Shape::Image { .. } => colors.border,
                };
                (shape.clone(), color)
            })
            .collect();
        let selection_color = colors.text_accent;
        let font = theme_settings::ThemeSettings::get_global(cx).ui_font.clone();

        div()
            .id("board-surface")
            .relative()
            .flex_1()
            .size_full()
            .overflow_hidden()
            .cursor_crosshair()
            .child(
                img(wallpaper_source)
                    .absolute()
                    .inset_0()
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .opacity(wallpaper.wallpaper_opacity),
            )
            .children(items.iter().enumerate().filter_map(|(index, shape)| {
                let Shape::Image { origin, size: image_size, path, radius } = shape else {
                    return None;
                };
                let absolute = self.root.join(path);
                Some(
                    div()
                        .absolute()
                        .left(px(origin[0]))
                        .top(px(origin[1]))
                        .w(px(image_size[0]))
                        .h(px(image_size[1]))
                        .rounded(px(*radius))
                        .overflow_hidden()
                        .when(selected == Some(index), |this| {
                            this.border_1().border_color(selection_color)
                        })
                        .child(
                            img(ImageSource::from(absolute))
                                .size_full()
                                .rounded(px(*radius))
                                .object_fit(ObjectFit::Cover),
                        ),
                )
            }))
            .child(
                canvas(
                    move |bounds, _, _| {
                        surface.set(bounds);
                        bounds
                    },
                    move |bounds, _, window, cx| {
                        let origin = bounds.origin;
                        let to_window = |p: [f32; 2]| point(origin.x + px(p[0]), origin.y + px(p[1]));
                        for (index, (shape, color)) in painted.iter().enumerate() {
                            match shape {
                                Shape::Stroke { points, width, .. } => {
                                    if points.len() < 2 {
                                        continue;
                                    }
                                    let mut builder = PathBuilder::stroke(px(*width));
                                    builder.move_to(to_window(points[0]));
                                    for p in &points[1..] {
                                        builder.line_to(to_window(*p));
                                    }
                                    if let Ok(path) = builder.build() {
                                        window.paint_path(path, *color);
                                    }
                                }
                                Shape::Line { from, to, width, .. } => {
                                    let mut builder = PathBuilder::stroke(px(*width));
                                    builder.move_to(to_window(*from));
                                    builder.line_to(to_window(*to));
                                    if let Ok(path) = builder.build() {
                                        window.paint_path(path, *color);
                                    }
                                }
                                Shape::Rect { origin: o, size: s, width, fill: filled, .. } => {
                                    let rect = Bounds::new(to_window(*o), size(px(s[0]), px(s[1])));
                                    if *filled {
                                        window.paint_quad(fill(rect, *color));
                                    } else {
                                        window.paint_quad(quad(
                                            rect,
                                            px(3.0),
                                            gpui::transparent_black(),
                                            px(*width),
                                            *color,
                                            gpui::BorderStyle::Solid,
                                        ));
                                    }
                                }
                                Shape::Ellipse { origin: o, size: s, width, fill: filled, .. } => {
                                    let center = point(o[0] + s[0] / 2.0, o[1] + s[1] / 2.0);
                                    let (rx, ry) = (s[0] / 2.0, s[1] / 2.0);
                                    let mut builder = if *filled {
                                        PathBuilder::fill()
                                    } else {
                                        PathBuilder::stroke(px(*width))
                                    };
                                    const SEGMENTS: usize = 64;
                                    for step in 0..=SEGMENTS {
                                        let angle = step as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
                                        let at = [center.x + rx * angle.cos(), center.y + ry * angle.sin()];
                                        if step == 0 {
                                            builder.move_to(to_window(at));
                                        } else {
                                            builder.line_to(to_window(at));
                                        }
                                    }
                                    if *filled {
                                        builder.close();
                                    }
                                    if let Ok(path) = builder.build() {
                                        window.paint_path(path, *color);
                                    }
                                }
                                Shape::Text { origin: o, text, size: text_size, .. } => {
                                    let shown: SharedString = if typing == Some(index) {
                                        format!("{text}|").into()
                                    } else {
                                        text.clone().into()
                                    };
                                    if shown.is_empty() {
                                        continue;
                                    }
                                    let run = TextRun {
                                        len: shown.len(),
                                        font: font.clone(),
                                        color: *color,
                                        background_color: None,
                                        underline: None,
                                        strikethrough: None,
                                    };
                                    let line = window.text_system().shape_line(
                                        shown,
                                        px(*text_size),
                                        &[run],
                                        None,
                                    );
                                    line.paint(
                                        to_window(*o),
                                        px(*text_size * 1.4),
                                        gpui::TextAlign::Left,
                                        None,
                                        window,
                                        cx,
                                    )
                                    .log_err();
                                }
                                Shape::Image { .. } => {}
                            }
                            if selected == Some(index) && !matches!(shape, Shape::Image { .. }) {
                                let (min, max) = shape.bounds();
                                let rect = Bounds::new(
                                    to_window([min.x - 2.0, min.y - 2.0]),
                                    size(px(max.x - min.x + 4.0), px(max.y - min.y + 4.0)),
                                );
                                window.paint_quad(outline(rect, selection_color, gpui::BorderStyle::Dashed));
                            }
                        }
                    },
                )
                .absolute()
                .inset_0()
                .size_full(),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
    }

    /// The shape being drawn right now, so it shows before the mouse is
    /// released.
    fn draft_shape(&self) -> Option<Shape> {
        let color = self.color.clone();
        match self.draft.as_ref()? {
            Draft::Stroke(points) => Some(Shape::Stroke {
                points: points.clone(),
                color,
                width: DEFAULT_LINE_WIDTH,
            }),
            Draft::Line { from, to } => Some(Shape::Line {
                from: *from,
                to: *to,
                color,
                width: DEFAULT_LINE_WIDTH,
            }),
            Draft::Rect { origin, size } => {
                let (min, max) = normalized(*origin, *size);
                Some(Shape::Rect {
                    origin: [min.x, min.y],
                    size: [max.x - min.x, max.y - min.y],
                    color,
                    width: DEFAULT_LINE_WIDTH,
                    fill: false,
                })
            }
            Draft::Ellipse { origin, size } => {
                let (min, max) = normalized(*origin, *size);
                Some(Shape::Ellipse {
                    origin: [min.x, min.y],
                    size: [max.x - min.x, max.y - min.y],
                    color,
                    width: DEFAULT_LINE_WIDTH,
                    fill: false,
                })
            }
            Draft::Move { .. } => None,
        }
    }
}

fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp"
            )
        })
}

impl Render for BoardView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("AsherinBoard")
            .track_focus(&self.focus_handle)
            .size_full()
            .on_key_down(cx.listener(Self::on_key_down))
            .child(self.render_toolbar(cx))
            .child(self.render_surface(cx))
    }
}

impl EventEmitter<ItemEvent> for BoardView {}

impl Focusable for BoardView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Item for BoardView {
    type Event = ItemEvent;

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        "asherin.board".into()
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<ui::Icon> {
        Some(ui::Icon::new(IconName::Pencil))
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        None
    }

    fn to_item_events(event: &Self::Event, f: &mut dyn FnMut(ItemEvent)) {
        f(*event)
    }
}

/// Shows the board in `workspace`, reusing an open one.
pub fn open_in(workspace: &mut Workspace, root: PathBuf, window: &mut Window, cx: &mut Context<Workspace>) {
    let existing = workspace.items_of_type::<BoardView>(cx).next();
    if let Some(existing) = existing {
        workspace.activate_item(&existing, true, true, window, cx);
        return;
    }
    let view: Entity<BoardView> = cx.new(|cx| BoardView::new(root, cx));
    workspace.add_item_to_active_pane(Box::new(view), None, true, window, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_board_round_trips_through_its_file() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let path = directory.path().join(BOARD_FILE);
        assert_eq!(load_board(&path).ok(), None);
        let board = Board {
            revision: 3,
            items: vec![
                Shape::Stroke { points: vec![[0.0, 0.0], [10.0, 5.0]], color: "ink".into(), width: 2.0 },
                Shape::Text { origin: [20.0, 20.0], text: "hello".into(), color: "accent".into(), size: 16.0 },
                Shape::Image { origin: [0.0, 0.0], size: [100.0, 50.0], path: "images/a.png".into(), radius: 12.0 },
            ],
        };
        save_board(&path, &board).expect("saves");
        assert_eq!(load_board(&path).expect("loads"), board);
    }

    #[test]
    fn shepherds_minimal_items_get_the_defaults() {
        let board: Board = serde_json::from_str(
            r#"{"items": [
                {"kind": "rect", "origin": [1, 2], "size": [30, 40]},
                {"kind": "text", "origin": [5, 5], "text": "hi"},
                {"kind": "image", "origin": [0, 0], "size": [10, 10], "path": "images/x.png"}
            ]}"#,
        )
        .expect("parses");
        assert_eq!(board.revision, 0);
        assert!(matches!(&board.items[0], Shape::Rect { color, width, fill: false, .. } if color == "ink" && *width == 2.0));
        assert!(matches!(&board.items[1], Shape::Text { size, .. } if *size == 16.0));
        assert!(matches!(&board.items[2], Shape::Image { radius, .. } if *radius == 12.0));
    }

    #[test]
    fn hit_tests_and_moves_follow_the_shape() {
        let mut rect = Shape::Rect {
            origin: [10.0, 10.0],
            size: [20.0, 20.0],
            color: "ink".into(),
            width: 2.0,
            fill: false,
        };
        assert!(rect.contains(point(15.0, 15.0)));
        assert!(!rect.contains(point(35.0, 15.0)));
        rect.translate(point(20.0, 0.0));
        assert!(rect.contains(point(35.0, 15.0)));

        let backwards = Shape::Ellipse {
            origin: [50.0, 50.0],
            size: [-20.0, -20.0],
            color: "ink".into(),
            width: 2.0,
            fill: false,
        };
        assert!(backwards.contains(point(40.0, 40.0)));
    }
}
