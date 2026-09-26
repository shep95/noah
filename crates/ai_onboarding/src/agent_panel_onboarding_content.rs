use std::sync::Arc;

use gpui::{AppContext as _, IntoElement, ParentElement, Task};
use language_model::{LanguageModelRegistry, ZED_CLOUD_PROVIDER_ID};
use ui::{Tooltip, prelude::*};

use crate::{AgentPanelOnboardingCard, ApiKeysWithoutProviders};

pub struct AgentPanelOnboarding {
    has_configured_providers: bool,
    dismiss: Arc<dyn Fn(&mut Window, &mut App)>,
    /// What this machine already has for shepherd: keys in the environment
    /// and local model servers, so the first run shows what was found rather
    /// than every provider noah knows.
    found: Vec<String>,
    _probe: Task<()>,
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

        let probe = cx.spawn(async move |this, cx| {
            let found = cx.background_spawn(async { detect_local_setup() }).await;
            this.update(cx, |this, cx| {
                this.found = found;
                cx.notify();
            })
            .ok();
        });

        Self {
            has_configured_providers: Self::has_configured_providers(cx),
            dismiss: Arc::new(dismiss),
            found: Vec::new(),
            _probe: probe,
        }
    }

    fn has_configured_providers(cx: &App) -> bool {
        LanguageModelRegistry::read_global(cx)
            .visible_providers()
            .iter()
            .any(|provider| provider.is_authenticated(cx) && provider.id() != ZED_CLOUD_PROVIDER_ID)
    }
}

/// Keys the providers read from the environment, and the local model servers
/// people run, probed by their usual ports. Nothing is read from the keys but
/// their presence.
fn detect_local_setup() -> Vec<String> {
    let mut found = Vec::new();
    for (variable, provider) in [
        ("VENICE_API_KEY", "venice"),
        ("ANTHROPIC_API_KEY", "anthropic"),
        ("OPENAI_API_KEY", "openai"),
        ("GEMINI_API_KEY", "google"),
        ("GOOGLE_AI_API_KEY", "google"),
        ("MISTRAL_API_KEY", "mistral"),
        ("DEEPSEEK_API_KEY", "deepseek"),
        ("OPENROUTER_API_KEY", "openrouter"),
        ("XAI_API_KEY", "x.ai"),
        ("GROQ_API_KEY", "groq"),
    ] {
        if std::env::var_os(variable).is_some_and(|value| !value.is_empty()) {
            let entry = format!("{provider} key ({variable})");
            if !found.contains(&entry) {
                found.push(entry);
            }
        }
    }
    let timeout = std::time::Duration::from_millis(250);
    for (port, server) in [(11434u16, "ollama"), (1234, "lm studio")] {
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        if std::net::TcpStream::connect_timeout(&address, timeout).is_ok() {
            found.push(format!("{server} running on localhost:{port}"));
        }
    }
    found
}

impl Render for AgentPanelOnboarding {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let dismiss = self.dismiss.clone();
        let found = self.found.clone();

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
                    .when(!self.has_configured_providers && !found.is_empty(), |this| {
                        this.child(
                            h_flex()
                                .pt_1()
                                .gap_2()
                                .flex_wrap()
                                .child(
                                    Label::new(format!("found on this machine: {}", found.join(", ")))
                                        .size(LabelSize::Small),
                                )
                                .child(
                                    Button::new("use-found-setup", "use it")
                                        .style(ButtonStyle::Filled)
                                        .label_size(LabelSize::Small)
                                        .on_click(|_, window, cx| {
                                            window.dispatch_action(
                                                Box::new(zed_actions::agent::OpenSettings),
                                                cx,
                                            )
                                        }),
                                ),
                        )
                    })
                    .child(
                        h_flex().absolute().top_0().right_0().child(
                            IconButton::new("dismiss_onboarding", IconName::Close)
                                .icon_size(IconSize::Small)
                                .tooltip(Tooltip::text("Dismiss"))
                                .on_click(move |_, window, cx| dismiss(window, cx)),
                        ),
                    ),
            )
            // With something found, the one "use it" replaces the full list of
            // providers; "add another" is what the settings page is.
            .when(!self.has_configured_providers && found.is_empty(), |this| {
                this.child(ApiKeysWithoutProviders::new())
            })
    }
}
