use gpui::{ClipboardItem, DismissEvent, EventEmitter, FocusHandle, Focusable};
use theme::ActiveTheme;
use ui::{AlertModal, prelude::*};

use crate::ModalView;

pub const BITCOIN_ADDRESS: &str = "bc1qwhu7xz88spa56mfvmg0zhtwlr4sny0qel2k2t8";

pub struct DonateModal {
    focus_handle: FocusHandle,
    copied: bool,
}

impl DonateModal {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            copied: false,
        }
    }

    fn copy_address(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(BITCOIN_ADDRESS.to_string()));
        self.copied = true;
        cx.notify();
    }
}

impl Focusable for DonateModal {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for DonateModal {}

impl ModalView for DonateModal {
    fn fade_out_background(&self) -> bool {
        true
    }
}

impl Render for DonateModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        AlertModal::new("donate-modal")
            .width(rems(32.))
            .key_context("DonateModal")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &menu::Confirm, _window, cx| this.copy_address(cx)))
            .on_action(cx.listener(|_, _: &menu::Cancel, _window, cx| cx.emit(DismissEvent)))
            .header(
                v_flex()
                    .p_3()
                    .gap_1()
                    .child(Label::new("support noah").size(LabelSize::Large))
                    .child(
                        Label::new(
                            "noah is free and has no ads or tracking. if it helps you, a bitcoin donation keeps it going.",
                        )
                        .color(Color::Muted),
                    ),
            )
            .child(
                v_flex()
                    .p_3()
                    .gap_2()
                    .child(Label::new("bitcoin").size(LabelSize::Small).color(Color::Muted))
                    .child(
                        div()
                            .id("bitcoin-address")
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .border_1()
                            .border_color(colors.border_focused)
                            .bg(colors.editor_background)
                            .font_buffer(cx)
                            .child(Label::new(BITCOIN_ADDRESS).single_line()),
                    ),
            )
            .footer(
                h_flex()
                    .p_3()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("close-donate", "close")
                            .on_click(cx.listener(|_, _, _window, cx| cx.emit(DismissEvent))),
                    )
                    .child(
                        Button::new(
                            "copy-bitcoin-address",
                            if self.copied { "copied" } else { "copy address" },
                        )
                        .style(ButtonStyle::Filled)
                        .on_click(cx.listener(|this, _, _window, cx| this.copy_address(cx))),
                    ),
            )
    }
}
