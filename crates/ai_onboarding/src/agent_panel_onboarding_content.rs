use std::sync::Arc;

use gpui::{IntoElement, ParentElement};
use language_model::{LanguageModelRegistry, ZED_CLOUD_PROVIDER_ID};
use ui::{Tooltip, prelude::*};

use crate::{AgentPanelOnboardingCard, ApiKeysWithoutProviders};

pub struct AgentPanelOnboarding {
    has_configured_providers: bool,
    dismiss: Arc<dyn Fn(&mut Window, &mut App)>,
}

impl AgentPanelOnboarding {
    pub fn new(dismiss: impl Fn(&mut Window, &mut App) + 'static, cx: &mut Context<Self>) -> Self {
        cx.subscribe(
            &LanguageModelRegistry::global(cx),
            |this: &mut Self, _registry, event: &language_model::Event, cx| match event {
                language_model::Event::ProviderStateChanged(_)
                | language_model::Event::AddedProvider(_)
                | language_model::Event::RemovedProvider(_)
                | language_model::Event::ProvidersChanged => {
                    this.has_configured_providers = Self::has_configured_providers(cx)
                }
                _ => {}
            },
        )
        .detach();

        Self {
            has_configured_providers: Self::has_configured_providers(cx),
            dismiss: Arc::new(dismiss),
        }
    }

    fn has_configured_providers(cx: &App) -> bool {
        LanguageModelRegistry::read_global(cx)
            .visible_providers()
            .iter()
            .any(|provider| provider.is_authenticated(cx) && provider.id() != ZED_CLOUD_PROVIDER_ID)
    }
}

impl Render for AgentPanelOnboarding {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let dismiss = self.dismiss.clone();

        AgentPanelOnboardingCard::new()
            .child(
                v_flex()
                    .w_full()
                    .relative()
                    .gap_1()
                    .child(Headline::new("shepherd"))
                    .child(
                        Label::new(
                            "Research, then plan, then build. Give it an API key from any provider \
                             or a local model and it starts reading your project.",
                        )
                        .color(Color::Muted),
                    )
                    .child(
                        Label::new("Keys stay on this machine. noah keeps no record of your work.")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .child(
                        h_flex().absolute().top_0().right_0().child(
                            IconButton::new("dismiss_onboarding", IconName::Close)
                                .icon_size(IconSize::Small)
                                .tooltip(Tooltip::text("Dismiss"))
                                .on_click(move |_, window, cx| dismiss(window, cx)),
                        ),
                    ),
            )
            .when(!self.has_configured_providers, |this| {
                this.child(ApiKeysWithoutProviders::new())
            })
    }
}
