use anyhow::{Context as _, Result};
use async_tungstenite::tungstenite::Message;
use base64::Engine as _;
use editor::Editor;
use futures::{StreamExt as _, channel::mpsc};
use gpui::{
    Action, App, AsyncWindowContext, Bounds, Context, Entity, EventEmitter, FocusHandle,
    Focusable, KeyDownEvent, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ObjectFit, Pixels, RenderImage, ScrollWheelEvent, SharedString, Subscription, Task, WeakEntity,
    Window, actions, canvas, img, px,
};
use serde_json::{Value, json};
use std::{cell::Cell, rc::Rc, sync::Arc};
use ui::{IconButton, IconName, IconSize, Tooltip, prelude::*};
use util::ResultExt as _;
use workspace::{
    Workspace,
    dock::{DockPosition, Panel, PanelEvent},
};

use crate::{AgentBrowser, BrowserEvent, address_to_url};

actions!(
    browser_panel,
    [
        /// Opens the browser room, where shepherd's browser is shown live.
        ToggleFocus,
    ]
);

pub(crate) fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
            workspace.toggle_panel_focus::<BrowserPanel>(window, cx);
        });
    })
    .detach();
}

enum Connection {
    Disconnected,
    Connecting,
    Connected {
        input: mpsc::UnboundedSender<String>,
        _task: Task<()>,
    },
}

struct Frame {
    image: Arc<RenderImage>,
    viewport_width: f32,
    viewport_height: f32,
}

pub struct BrowserPanel {
    focus_handle: FocusHandle,
    address: Entity<Editor>,
    connection: Connection,
    frame: Option<Frame>,
    /// The page size last requested to match the room, in CSS pixels.
    requested_viewport: Option<(u32, u32)>,
    error: Option<SharedString>,
    image_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Set once the room has attached to the stream on its own. After that it
    /// only reattaches after a command, so closing the browser keeps it closed.
    attached_on_open: bool,
    position: DockPosition,
    zoomed: bool,
    _subscriptions: Vec<Subscription>,
}

impl BrowserPanel {
    pub async fn load(
        workspace: WeakEntity<Workspace>,
        mut cx: AsyncWindowContext,
    ) -> Result<Entity<Self>> {
        workspace.update_in(&mut cx, |_workspace, window, cx| {
            cx.new(|cx| Self::new(window, cx))
        })
    }

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let address = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("search or enter an address", window, cx);
            editor
        });
        let mut subscriptions = Vec::new();
        if let Some(browser) = AgentBrowser::global(cx) {
            subscriptions.push(cx.observe(&browser, |_, _, cx| cx.notify()));
            subscriptions.push(cx.subscribe_in(
                &browser,
                window,
                |this, _, event: &BrowserEvent, window, cx| match event {
                    BrowserEvent::CommandFinished => {
                        if matches!(this.connection, Connection::Disconnected) {
                            this.connect(window, cx);
                        }
                    }
                },
            ));
        }
        Self {
            focus_handle: cx.focus_handle(),
            address,
            connection: Connection::Disconnected,
            frame: None,
            requested_viewport: None,
            error: None,
            image_bounds: Rc::new(Cell::new(None)),
            attached_on_open: false,
            position: DockPosition::Right,
            zoomed: false,
            _subscriptions: subscriptions,
        }
    }

    fn run(&mut self, arguments: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(browser) = AgentBrowser::global(cx) else {
            return;
        };
        self.error = None;
        let command = browser.update(cx, |browser, cx| browser.run(arguments, cx));
        cx.spawn_in(window, async move |this, cx| {
            if let Err(error) = command.await {
                this.update(cx, |this, cx| {
                    this.error = Some(format!("{error:#}").into());
                    cx.notify();
                })?;
            }
            anyhow::Ok(())
        })
        .detach_and_log_err(cx);
        cx.notify();
    }

    fn navigate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.address.read(cx).text(cx);
        if typed.trim().is_empty() {
            return;
        }
        let url = address_to_url(&typed);
        self.address.update(cx, |address, cx| {
            address.set_text(url.as_ref(), window, cx);
        });
        self.run(vec!["open".into(), url.to_string()], window, cx);
        window.focus(&self.focus_handle, cx);
    }

    /// Attaches to the session's live stream. Starting the stream also starts
    /// the browser if it isn't running yet.
    fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.connection, Connection::Disconnected) {
            return;
        }
        self.connection = Connection::Connecting;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = Self::open_stream(this.clone(), cx).await;
            if let Err(error) = result {
                this.update(cx, |this, cx| {
                    this.connection = Connection::Disconnected;
                    this.error = Some(format!("{error:#}").into());
                    cx.notify();
                })?;
            }
            anyhow::Ok(())
        })
        .detach_and_log_err(cx);
    }

    async fn open_stream(this: WeakEntity<Self>, cx: &mut AsyncWindowContext) -> Result<()> {
        let status = crate::run_command(vec!["--json".into(), "stream".into(), "status".into()])
            .await?;
        let status: Value =
            serde_json::from_str(&status).context("agent-browser sent an unreadable status")?;
        let port = status["data"]["port"]
            .as_u64()
            .context("agent-browser did not report a stream port")?;
        let stream = smol::net::TcpStream::connect(("127.0.0.1", port as u16)).await?;
        let (socket, _) = async_tungstenite::client_async(
            format!("ws://127.0.0.1:{port}/?pacing=ack&maxFps=20"),
            stream,
        )
        .await?;
        let (mut sink, mut messages) = socket.split();
        let (input, mut outgoing) = mpsc::unbounded::<String>();

        let writer = cx.background_spawn(async move {
            while let Some(message) = outgoing.next().await {
                if sink.send(Message::text(message)).await.is_err() {
                    break;
                }
            }
        });
        let acks = input.clone();
        let task = cx.spawn({
            let this = this.clone();
            async move |cx| {
                while let Some(Ok(message)) = messages.next().await {
                    let Message::Text(text) = message else {
                        continue;
                    };
                    let Some(value) = serde_json::from_str::<Value>(text.as_str()).log_err() else {
                        continue;
                    };
                    match value["type"].as_str() {
                        Some("frame") => {
                            let Some(frame) = decode_frame(&value, cx).await else {
                                continue;
                            };
                            let seq = value["seq"].as_u64().unwrap_or_default();
                            let updated = this.update(cx, |this, cx| {
                                if let Some(previous) = this.frame.replace(frame) {
                                    cx.drop_image(previous.image, None);
                                }
                                cx.notify();
                            });
                            if updated.is_err() {
                                break;
                            }
                            acks.unbounded_send(json!({"type": "ack", "seq": seq}).to_string())
                                .ok();
                        }
                        Some("url") => {
                            let url = value["url"].as_str().unwrap_or_default().to_string();
                            this.update_in(cx, |this, window, cx| {
                                let editing = this.address.focus_handle(cx).is_focused(window);
                                if !editing {
                                    this.address.update(cx, |address, cx| {
                                        address.set_text(url, window, cx);
                                    });
                                }
                            })
                            .ok();
                        }
                        _ => {}
                    }
                }
                drop(writer);
                this.update(cx, |this, cx| {
                    this.connection = Connection::Disconnected;
                    this.requested_viewport = None;
                    if let Some(previous) = this.frame.take() {
                        cx.drop_image(previous.image, None);
                    }
                    cx.notify();
                })
                .ok();
            }
        });
        this.update(cx, |this, cx| {
            this.connection = Connection::Connected { input, _task: task };
            cx.notify();
        })?;
        Ok(())
    }

    /// Resizes the page to the room, so it fills the space instead of being
    /// letterboxed. Runs quietly: it isn't shepherd's work, so it doesn't mark
    /// the browser busy.
    fn fit_viewport(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.connection, Connection::Connected { .. }) || self.frame.is_none() {
            return;
        }
        let Some(bounds) = self.image_bounds.get() else {
            return;
        };
        let width = f32::from(bounds.size.width).round() as u32;
        let height = f32::from(bounds.size.height).round() as u32;
        if width < 200 || height < 150 {
            return;
        }
        let close_enough = self.requested_viewport.is_some_and(|(requested_width, requested_height)| {
            requested_width.abs_diff(width) < 8 && requested_height.abs_diff(height) < 8
        });
        if close_enough {
            return;
        }
        self.requested_viewport = Some((width, height));
        cx.background_spawn(crate::run_command(vec![
            "set".into(),
            "viewport".into(),
            width.to_string(),
            height.to_string(),
        ]))
        .detach_and_log_err(cx);
    }

    fn send(&self, message: Value) {
        if let Connection::Connected { input, .. } = &self.connection {
            input.unbounded_send(message.to_string()).ok();
        }
    }

    /// Maps a point in the window to the page's own coordinates, accounting
    /// for the frame being letterboxed inside the room.
    fn page_point(&self, position: gpui::Point<Pixels>) -> Option<(f32, f32)> {
        let frame = self.frame.as_ref()?;
        let bounds = self.image_bounds.get()?;
        let scale = (f32::from(bounds.size.width) / frame.viewport_width)
            .min(f32::from(bounds.size.height) / frame.viewport_height);
        if scale <= 0.0 {
            return None;
        }
        let drawn_width = frame.viewport_width * scale;
        let drawn_height = frame.viewport_height * scale;
        let left = f32::from(bounds.origin.x) + (f32::from(bounds.size.width) - drawn_width) / 2.0;
        let top = f32::from(bounds.origin.y) + (f32::from(bounds.size.height) - drawn_height) / 2.0;
        let x = (f32::from(position.x) - left) / scale;
        let y = (f32::from(position.y) - top) / scale;
        (x >= 0.0 && y >= 0.0 && x <= frame.viewport_width && y <= frame.viewport_height)
            .then_some((x, y))
    }

    fn send_mouse(
        &self,
        event_type: &str,
        position: gpui::Point<Pixels>,
        button: Option<MouseButton>,
        click_count: usize,
        modifiers: &Modifiers,
    ) {
        let Some((x, y)) = self.page_point(position) else {
            return;
        };
        let button = match button {
            Some(MouseButton::Left) => "left",
            Some(MouseButton::Right) => "right",
            Some(MouseButton::Middle) => "middle",
            _ => "none",
        };
        self.send(json!({
            "type": "input_mouse",
            "eventType": event_type,
            "x": x,
            "y": y,
            "button": button,
            "clickCount": click_count,
            "modifiers": cdp_modifiers(modifiers),
        }));
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = cdp_modifiers(&keystroke.modifiers);
        if (keystroke.modifiers.control || keystroke.modifiers.platform)
            && keystroke.key == "v"
        {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                self.run(vec!["keyboard".into(), "inserttext".into(), text], window, cx);
            }
            cx.stop_propagation();
            return;
        }
        let (key, text) = match dom_key(&keystroke.key) {
            Some(named) => (named.to_string(), None),
            None => match keystroke.key_char.as_ref().filter(|_| {
                !keystroke.modifiers.control && !keystroke.modifiers.platform
            }) {
                Some(character) => (character.clone(), Some(character.clone())),
                None => (keystroke.key.clone(), None),
            },
        };
        let mut down = json!({
            "type": "input_keyboard",
            "eventType": "keyDown",
            "key": key,
            "modifiers": modifiers,
        });
        if let Some(text) = text {
            down["text"] = json!(text);
        }
        self.send(down);
        self.send(json!({
            "type": "input_keyboard",
            "eventType": "keyUp",
            "key": key,
            "modifiers": modifiers,
        }));
        cx.stop_propagation();
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = AgentBrowser::global(cx).is_some_and(|browser| browser.read(cx).is_busy());
        h_flex()
            .gap_1()
            .px_2()
            .py_1p5()
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(
                IconButton::new("browser-back", IconName::ArrowLeft)
                    .icon_size(IconSize::Small)
                    .tooltip(Tooltip::text("back"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.run(vec!["back".into()], window, cx)
                    })),
            )
            .child(
                IconButton::new("browser-forward", IconName::ArrowRight)
                    .icon_size(IconSize::Small)
                    .tooltip(Tooltip::text("forward"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.run(vec!["forward".into()], window, cx)
                    })),
            )
            .child(
                IconButton::new("browser-reload", IconName::RotateCw)
                    .icon_size(IconSize::Small)
                    .tooltip(Tooltip::text("reload"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.run(vec!["reload".into()], window, cx)
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .bg(cx.theme().colors().element_background)
                    .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| {
                        this.navigate(window, cx)
                    }))
                    .child(self.address.clone()),
            )
            .when(busy, |this| {
                this.child(
                    Label::new("shepherd is browsing")
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
            })
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let connecting = matches!(self.connection, Connection::Connecting);
        let missing_binary = crate::find_binary().is_none();
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .p_6()
            .child(Label::new("browser").color(Color::Muted))
            .child(
                Label::new(if missing_binary {
                    "agent-browser isn't installed with this copy of noah."
                } else if connecting {
                    "starting the browser…"
                } else {
                    "Type an address above, or ask shepherd to look something up. The page is live: click, scroll and type in it."
                })
                .size(LabelSize::Small)
                .color(Color::Muted),
            )
            .when(!connecting && !missing_binary, |this| {
                this.child(
                    Button::new("browser-start", "start the browser")
                        .style(ButtonStyle::Outlined)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.run(vec!["open".into(), "about:blank".into()], window, cx)
                        })),
                )
            })
            .when_some(
                self.error
                    .clone()
                    .filter(|error| error.contains("No Chrome, Edge or Chromium")),
                |this, _| {
                    this.child(
                        Button::new("browser-install", "download a browser")
                            .style(ButtonStyle::Outlined)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.run(vec!["install".into()], window, cx)
                            })),
                    )
                },
            )
    }

    fn render_page(&self, frame: &Frame, cx: &mut Context<Self>) -> impl IntoElement {
        let image_bounds = self.image_bounds.clone();
        div()
            .id("browser-page")
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                canvas(
                    move |bounds, _, _| image_bounds.set(Some(bounds)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                img(frame.image.clone())
                    .size_full()
                    .object_fit(ObjectFit::Contain),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus_handle, cx);
                    this.send_mouse(
                        "mousePressed",
                        event.position,
                        Some(event.button),
                        event.click_count,
                        &event.modifiers,
                    );
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _, _| {
                    this.send_mouse(
                        "mouseReleased",
                        event.position,
                        Some(event.button),
                        event.click_count,
                        &event.modifiers,
                    );
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, _| {
                this.send_mouse(
                    "mouseMoved",
                    event.position,
                    event.pressed_button,
                    0,
                    &event.modifiers,
                );
            }))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, _| {
                let Some((x, y)) = this.page_point(event.position) else {
                    return;
                };
                let delta = event.delta.pixel_delta(px(20.));
                // gpui reports scrolling towards the top as positive; the page
                // expects the opposite.
                this.send(json!({
                    "type": "input_mouse",
                    "eventType": "mouseWheel",
                    "x": x,
                    "y": y,
                    "deltaX": -f32::from(delta.x),
                    "deltaY": -f32::from(delta.y),
                }));
            }))
    }
}

async fn decode_frame(value: &Value, cx: &mut AsyncWindowContext) -> Option<Frame> {
    let data = value["data"].as_str()?.to_string();
    let viewport_width = value["metadata"]["deviceWidth"].as_f64()? as f32;
    let (image, image_width, image_height) = cx
        .background_spawn(async move {
            let bytes = base64::engine::general_purpose::STANDARD.decode(data)?;
            let mut pixels = image::load_from_memory(&bytes)?.into_rgba8();
            // gpui textures are BGRA.
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            let (width, height) = pixels.dimensions();
            anyhow::Ok((
                Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])),
                width,
                height,
            ))
        })
        .await
        .log_err()?;
    if image_width == 0 {
        return None;
    }
    // The frame covers only the page's visible area, which is shorter than
    // the `deviceHeight` Chrome reports, so the page's height in CSS pixels
    // comes from the image itself.
    let viewport_height = image_height as f32 * viewport_width / image_width as f32;
    Some(Frame {
        image,
        viewport_width,
        viewport_height,
    })
}

/// Chrome DevTools modifier bits: Alt 1, Ctrl 2, Meta 4, Shift 8.
fn cdp_modifiers(modifiers: &Modifiers) -> u8 {
    u8::from(modifiers.alt)
        | (u8::from(modifiers.control) << 1)
        | (u8::from(modifiers.platform) << 2)
        | (u8::from(modifiers.shift) << 3)
}

fn dom_key(key: &str) -> Option<&'static str> {
    Some(match key {
        "enter" => "Enter",
        "backspace" => "Backspace",
        "tab" => "Tab",
        "escape" => "Escape",
        "delete" => "Delete",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        _ => return None,
    })
}

impl EventEmitter<PanelEvent> for BrowserPanel {}

impl Focusable for BrowserPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for BrowserPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.attached_on_open {
            self.attached_on_open = true;
            self.connect(window, cx);
        }
        self.fit_viewport(cx);
        v_flex()
            .key_context("BrowserPanel")
            .track_focus(&self.focus_handle)
            .size_full()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.focus_handle.is_focused(window) {
                    this.handle_key(event, window, cx);
                }
            }))
            .child(self.render_toolbar(cx))
            .child(match &self.frame {
                Some(frame) => self.render_page(frame, cx).into_any_element(),
                None => self.render_empty(cx).into_any_element(),
            })
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    div().px_3().py_1p5().child(
                        Label::new(error)
                            .size(LabelSize::Small)
                            .color(Color::Error),
                    ),
                )
            })
    }
}

impl Panel for BrowserPanel {
    fn persistent_name() -> &'static str {
        "BrowserPanel"
    }

    fn panel_key() -> &'static str {
        "BrowserPanel"
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
        px(720.)
    }

    fn icon(&self, _window: &Window, _cx: &App) -> Option<IconName> {
        Some(IconName::ToolWeb)
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("browser")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleFocus)
    }

    fn icon_label(&self, _window: &Window, cx: &App) -> Option<String> {
        AgentBrowser::global(cx)
            .filter(|browser| browser.read(cx).is_busy())
            .map(|_| "working".to_string())
    }

    fn is_zoomed(&self, _window: &Window, _cx: &App) -> bool {
        self.zoomed
    }

    fn set_zoomed(&mut self, zoomed: bool, _window: &mut Window, cx: &mut Context<Self>) {
        self.zoomed = zoomed;
        cx.notify();
    }

    fn activation_priority(&self) -> u32 {
        8
    }
}
