use anyhow::{Context as _, Result};
use credentials_provider::CredentialsProvider;
use futures::{AsyncReadExt as _, FutureExt, StreamExt, future::BoxFuture};
use gpui::{App, AppContext, AsyncApp, Context, Entity, SharedString, Task};
use http_client::{AsyncBody, HttpClient, Method, Request as HttpRequest};
use language_model::chat_completion::ChatCompletionEventMapper;
use language_model::{
    ApiKeyConfiguration, ApiKeyState, ApiKeyStatus, AuthenticateError, EnvVar, IconOrSvg, LanguageModel,
    LanguageModelCompletionError, LanguageModelCompletionEvent, LanguageModelId,
    LanguageModelName, LanguageModelProvider, LanguageModelProviderId, LanguageModelProviderName,
    LanguageModelEffortLevel, LanguageModelProviderState, LanguageModelRequest,
    LanguageModelToolChoice, ProviderSettingsView, RateLimiter, ReasoningEffort, env_var,
};
use open_ai::ResponseStreamEvent;
use open_ai::completion::{ChatCompletionMaxTokensParameter, into_open_ai};
use serde::{Deserialize, Serialize};
use settings::{Settings, SettingsStore};
use std::sync::{Arc, LazyLock};
use ui::IconName;

pub const PROVIDER_ID: LanguageModelProviderId = LanguageModelProviderId::new("venice");
const PROVIDER_NAME: LanguageModelProviderName = LanguageModelProviderName::new("Venice");
pub const VENICE_API_URL: &str = "https://api.venice.ai/api/v1";

const API_KEY_ENV_VAR_NAME: &str = "VENICE_API_KEY";
static API_KEY_ENV_VAR: LazyLock<EnvVar> = env_var!(API_KEY_ENV_VAR_NAME);

/// Venice marks its recommended models with traits. shepherd needs tool calls,
/// so the function-calling default is preferred over the general default.
const DEFAULT_MODEL_TRAITS: &[&str] = &["function_calling_default", "default_code", "default"];

#[derive(Default, Clone, Debug, PartialEq)]
pub struct VeniceSettings {
    pub api_url: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VeniceModel {
    pub id: String,
    pub display_name: String,
    pub context_tokens: u64,
    pub max_output_tokens: Option<u64>,
    pub supports_tools: bool,
    pub supports_images: bool,
    /// Whether the model thinks before answering, which makes its reasoning
    /// level selectable.
    pub supports_reasoning: bool,
    pub traits: Vec<String>,
    /// US dollars per million input and output tokens.
    pub pricing: Option<(f64, f64)>,
}

#[derive(Deserialize)]
struct ListModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
    #[serde(rename = "type", default)]
    model_type: Option<String>,
    #[serde(default)]
    model_spec: ModelSpec,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelSpec {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    available_context_tokens: Option<u64>,
    #[serde(default)]
    max_completion_tokens: Option<u64>,
    #[serde(default)]
    capabilities: ModelCapabilities,
    #[serde(default)]
    traits: Vec<String>,
    #[serde(default)]
    offline: bool,
    #[serde(default)]
    pricing: Option<ModelPricing>,
}

#[derive(Default, Deserialize)]
struct ModelPricing {
    #[serde(default)]
    input: Option<Price>,
    #[serde(default)]
    output: Option<Price>,
}

#[derive(Default, Deserialize)]
struct Price {
    #[serde(default)]
    usd: Option<f64>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelCapabilities {
    #[serde(default)]
    supports_function_calling: bool,
    #[serde(default)]
    supports_vision: bool,
    #[serde(default)]
    supports_reasoning: bool,
}

/// Venice-only request options, sent alongside the OpenAI-shaped body.
#[derive(Serialize)]
struct VeniceParameters {
    /// Venice otherwise prepends its own system prompt, which would sit on top
    /// of shepherd's brain and change how it answers.
    include_venice_system_prompt: bool,
    /// Reasoning models stream their thinking inside the reply text; stripping
    /// it keeps the thread to the answer itself.
    strip_thinking_response: bool,
    /// Lets a reasoning model answer straight away when thinking is off.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    disable_thinking: bool,
}

#[derive(Serialize)]
struct VeniceRequest {
    #[serde(flatten)]
    request: open_ai::Request,
    venice_parameters: VeniceParameters,
}

fn parse_models(body: &str) -> Result<Vec<VeniceModel>> {
    let response: ListModelsResponse =
        serde_json::from_str(body).context("Venice returned a model list noah could not read")?;
    Ok(response
        .data
        .into_iter()
        .filter(|entry| entry.model_type.as_deref().is_none_or(|kind| kind == "text"))
        .filter(|entry| !entry.model_spec.offline)
        .map(|entry| {
            let spec = entry.model_spec;
            VeniceModel {
                display_name: spec.name.unwrap_or_else(|| entry.id.clone()),
                id: entry.id,
                context_tokens: spec.available_context_tokens.unwrap_or(32_768),
                max_output_tokens: spec.max_completion_tokens,
                supports_tools: spec.capabilities.supports_function_calling,
                supports_images: spec.capabilities.supports_vision,
                supports_reasoning: spec.capabilities.supports_reasoning,
                traits: spec.traits,
                pricing: spec.pricing.and_then(|pricing| {
                    Some((pricing.input?.usd?, pricing.output?.usd?))
                }),
            }
        })
        .collect())
}

async fn list_models(
    http_client: &dyn HttpClient,
    api_url: &str,
    api_key: &str,
) -> Result<Vec<VeniceModel>> {
    let request = HttpRequest::builder()
        .method(Method::GET)
        .uri(format!("{api_url}/models?type=text"))
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {api_key}"))
        .body(AsyncBody::default())?;
    let mut response = http_client
        .send(request)
        .await
        .with_context(|| format!("couldn't reach Venice at {api_url}"))?;
    let mut body = String::new();
    response.body_mut().read_to_string(&mut body).await?;
    if matches!(response.status().as_u16(), 401 | 403) {
        anyhow::bail!("Venice didn't accept this API key. Check it at venice.ai/settings/api.");
    }
    if !response.status().is_success() {
        anyhow::bail!(
            "Venice refused the model list request ({}): {}",
            response.status(),
            body.trim()
        );
    }
    parse_models(&body)
}

fn default_model_of(models: &[VeniceModel]) -> Option<&VeniceModel> {
    DEFAULT_MODEL_TRAITS
        .iter()
        .find_map(|wanted| {
            models
                .iter()
                .find(|model| model.traits.iter().any(|model_trait| model_trait == wanted))
        })
        .or_else(|| models.iter().find(|model| model.supports_tools))
        .or_else(|| models.first())
}

pub struct State {
    api_key_state: ApiKeyState,
    credentials_provider: Arc<dyn CredentialsProvider>,
    http_client: Arc<dyn HttpClient>,
    available_models: Vec<VeniceModel>,
    /// Why the model list is empty when a key is set, shown under the key in
    /// settings so a rejected key or unreachable server is not silent.
    models_error: Option<SharedString>,
    fetch_models_task: Option<Task<()>>,
}

impl State {
    fn is_authenticated(&self) -> bool {
        self.api_key_state.has_key()
    }

    fn set_api_key(&mut self, api_key: Option<String>, cx: &mut Context<Self>) -> Task<Result<()>> {
        let credentials_provider = self.credentials_provider.clone();
        let api_url = VeniceLanguageModelProvider::api_url(cx);
        let task = self.api_key_state.store(
            api_url,
            api_key,
            |this| &mut this.api_key_state,
            credentials_provider,
            cx,
        );
        cx.spawn(async move |this, cx| {
            let result = task.await?;
            this.update(cx, |this, cx| this.restart_fetch_models_task(cx))?;
            Ok(result)
        })
    }

    fn authenticate(&mut self, cx: &mut Context<Self>) -> Task<Result<(), AuthenticateError>> {
        let credentials_provider = self.credentials_provider.clone();
        let api_url = VeniceLanguageModelProvider::api_url(cx);
        let task = self.api_key_state.load_if_needed(
            api_url,
            |this| &mut this.api_key_state,
            credentials_provider,
            cx,
        );
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| this.restart_fetch_models_task(cx))
                .ok();
            result
        })
    }

    fn restart_fetch_models_task(&mut self, cx: &mut Context<Self>) {
        let api_url = VeniceLanguageModelProvider::api_url(cx);
        self.models_error = None;
        if crate::AllLanguageModelSettings::get_global(cx).offline {
            self.available_models.clear();
            self.fetch_models_task = None;
            self.models_error = Some("offline mode is on, so noah doesn't contact Venice".into());
            cx.notify();
            return;
        }
        let Some(api_key) = self.api_key_state.key(&api_url) else {
            self.available_models.clear();
            self.fetch_models_task = None;
            cx.notify();
            return;
        };
        let http_client = self.http_client.clone();
        self.fetch_models_task = Some(cx.spawn(async move |this, cx| {
            let models = list_models(http_client.as_ref(), &api_url, &api_key).await;
            this.update(cx, |this, cx| {
                match models {
                    Ok(models) if models.is_empty() => {
                        this.available_models.clear();
                        this.models_error = Some("Venice returned no text models.".into());
                    }
                    Ok(models) => this.available_models = models,
                    Err(error) => {
                        log::error!("failed to load Venice models: {error:#}");
                        this.available_models.clear();
                        this.models_error = Some(format!("{error:#}").into());
                    }
                }
                this.fetch_models_task = None;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }
}

pub struct VeniceLanguageModelProvider {
    http_client: Arc<dyn HttpClient>,
    state: Entity<State>,
}

impl VeniceLanguageModelProvider {
    pub fn new(
        http_client: Arc<dyn HttpClient>,
        credentials_provider: Arc<dyn CredentialsProvider>,
        cx: &mut App,
    ) -> Self {
        let state = cx.new(|cx| {
            cx.observe_global::<SettingsStore>({
                let mut last_settings = Self::settings(cx).clone();
                let mut last_offline = crate::AllLanguageModelSettings::get_global(cx).offline;
                move |this: &mut State, cx| {
                    let current_settings = Self::settings(cx);
                    let offline = crate::AllLanguageModelSettings::get_global(cx).offline;
                    if current_settings != &last_settings || offline != last_offline {
                        last_settings = current_settings.clone();
                        last_offline = offline;
                        this.authenticate(cx).detach();
                        cx.notify();
                    }
                }
            })
            .detach();
            State {
                api_key_state: ApiKeyState::new(Self::api_url(cx), (*API_KEY_ENV_VAR).clone()),
                credentials_provider,
                http_client: http_client.clone(),
                available_models: Vec::new(),
                models_error: None,
                fetch_models_task: None,
            }
        });
        Self { http_client, state }
    }

    fn settings(cx: &App) -> &VeniceSettings {
        &crate::AllLanguageModelSettings::get_global(cx).venice
    }

    fn api_url(cx: &App) -> SharedString {
        let api_url = Self::settings(cx).api_url.trim_end_matches('/');
        if api_url.is_empty() {
            VENICE_API_URL.into()
        } else {
            SharedString::new(api_url)
        }
    }

    fn create_language_model(&self, model: VeniceModel) -> Arc<dyn LanguageModel> {
        Arc::new(VeniceLanguageModel {
            id: LanguageModelId::from(model.id.clone()),
            model,
            state: self.state.clone(),
            http_client: self.http_client.clone(),
            request_limiter: RateLimiter::new(4),
        })
    }
}

impl LanguageModelProviderState for VeniceLanguageModelProvider {
    type ObservableEntity = State;

    fn observable_entity(&self) -> Option<Entity<Self::ObservableEntity>> {
        Some(self.state.clone())
    }
}

impl LanguageModelProvider for VeniceLanguageModelProvider {
    fn id(&self) -> LanguageModelProviderId {
        PROVIDER_ID
    }

    fn speech_api(&self, cx: &App) -> Option<language_model::SpeechApi> {
        let api_url = Self::api_url(cx);
        let api_key = self.state.read(cx).api_key_state.key(&api_url)?;
        Some(language_model::SpeechApi {
            provider: "Venice".into(),
            api_url: api_url.to_string(),
            api_key,
            transcription_model: "nvidia/parakeet-tdt-0.6b-v3".into(),
            speech_model: "tts-kokoro".into(),
            voice: "af_sky".into(),
        })
    }

    fn name(&self) -> LanguageModelProviderName {
        PROVIDER_NAME
    }

    fn icon(&self) -> IconOrSvg {
        IconOrSvg::Icon(IconName::AiVenice)
    }

    fn default_model(&self, cx: &App) -> Option<Arc<dyn LanguageModel>> {
        default_model_of(&self.state.read(cx).available_models)
            .cloned()
            .map(|model| self.create_language_model(model))
    }

    fn default_fast_model(&self, cx: &App) -> Option<Arc<dyn LanguageModel>> {
        let models = &self.state.read(cx).available_models;
        models
            .iter()
            .find(|model| model.traits.iter().any(|model_trait| model_trait == "fastest"))
            .cloned()
            .map(|model| self.create_language_model(model))
    }

    fn provided_models(&self, cx: &App) -> Vec<Arc<dyn LanguageModel>> {
        self.state
            .read(cx)
            .available_models
            .iter()
            .cloned()
            .map(|model| self.create_language_model(model))
            .collect()
    }

    fn is_authenticated(&self, cx: &App) -> bool {
        self.state.read(cx).is_authenticated()
    }

    fn authenticate(&self, cx: &mut App) -> Task<Result<(), AuthenticateError>> {
        self.state.update(cx, |state, cx| state.authenticate(cx))
    }

    fn settings_view(&self, cx: &mut App) -> Option<ProviderSettingsView> {
        let state = self.state.read(cx);
        let status = if let Some(error) = &state.models_error {
            ApiKeyStatus::Failed(error.clone())
        } else if state.fetch_models_task.is_some() {
            ApiKeyStatus::Checking
        } else {
            let count = state.available_models.len();
            ApiKeyStatus::Working(
                format!(
                    "Connected to Venice · {count} model{}",
                    if count == 1 { "" } else { "s" }
                )
                .into(),
            )
        };
        Some(ProviderSettingsView::ApiKey(
            ApiKeyConfiguration::new(
                state.api_key_state.has_key(),
                state.api_key_state.is_from_env_var(),
                state.api_key_state.env_var_name().clone(),
                "https://venice.ai/settings/api".into(),
            )
            .status(status),
        ))
    }

    fn set_api_key(&self, api_key: Option<String>, cx: &mut App) -> Task<Result<()>> {
        self.state
            .update(cx, |state, cx| state.set_api_key(api_key, cx))
    }
}

pub struct VeniceLanguageModel {
    id: LanguageModelId,
    model: VeniceModel,
    state: Entity<State>,
    http_client: Arc<dyn HttpClient>,
    request_limiter: RateLimiter,
}

impl VeniceLanguageModel {
    fn stream_completion(
        &self,
        request: VeniceRequest,
        cx: &AsyncApp,
    ) -> BoxFuture<
        'static,
        Result<
            futures::stream::BoxStream<'static, Result<ResponseStreamEvent>>,
            LanguageModelCompletionError,
        >,
    > {
        let http_client = self.http_client.clone();
        let (api_key, api_url) = self.state.read_with(cx, |state, cx| {
            let api_url = VeniceLanguageModelProvider::api_url(cx);
            (state.api_key_state.key(&api_url), api_url)
        });
        let future = self.request_limiter.stream(async move {
            let Some(api_key) = api_key else {
                return Err(LanguageModelCompletionError::NoApiKey {
                    provider: PROVIDER_NAME,
                });
            };
            let response = open_ai::stream_completion(
                http_client.as_ref(),
                PROVIDER_NAME.0.as_str(),
                &api_url,
                &api_key,
                request,
                &Default::default(),
            )
            .await?;
            Ok(response)
        });
        async move { Ok(future.await?.boxed()) }.boxed()
    }
}

impl LanguageModel for VeniceLanguageModel {
    fn id(&self) -> LanguageModelId {
        self.id.clone()
    }

    fn name(&self) -> LanguageModelName {
        LanguageModelName::from(self.model.display_name.clone())
    }

    fn provider_id(&self) -> LanguageModelProviderId {
        PROVIDER_ID
    }

    fn provider_name(&self) -> LanguageModelProviderName {
        PROVIDER_NAME
    }

    fn supports_tools(&self) -> bool {
        self.model.supports_tools
    }

    fn supports_images(&self) -> bool {
        self.model.supports_images
    }

    fn supports_tool_choice(&self, choice: LanguageModelToolChoice) -> bool {
        match choice {
            LanguageModelToolChoice::Auto | LanguageModelToolChoice::Any => {
                self.model.supports_tools
            }
            LanguageModelToolChoice::None => true,
        }
    }

    fn supports_streaming_tools(&self) -> bool {
        true
    }

    fn supports_thinking(&self) -> bool {
        self.model.supports_reasoning
    }

    fn supported_effort_levels(&self) -> Vec<LanguageModelEffortLevel> {
        reasoning_levels(&self.model)
    }

    fn supports_split_token_display(&self) -> bool {
        true
    }

    fn telemetry_id(&self) -> String {
        format!("venice/{}", self.model.id)
    }

    fn price_per_million_tokens(&self) -> Option<(f64, f64)> {
        self.model.pricing
    }

    fn max_token_count(&self) -> u64 {
        self.model.context_tokens
    }

    fn max_output_tokens(&self) -> Option<u64> {
        self.model.max_output_tokens
    }

    fn stream_completion(
        &self,
        mut request: LanguageModelRequest,
        cx: &AsyncApp,
    ) -> BoxFuture<
        'static,
        Result<
            futures::stream::BoxStream<
                'static,
                Result<LanguageModelCompletionEvent, LanguageModelCompletionError>,
            >,
            LanguageModelCompletionError,
        >,
    > {
        request.speed = None;
        let (reasoning_effort, disable_thinking) = reasoning_for(&self.model, &request);
        let request = match into_open_ai(
            request,
            &self.model.id,
            false,
            false,
            self.max_output_tokens(),
            ChatCompletionMaxTokensParameter::MaxCompletionTokens,
            reasoning_effort,
            false,
        ) {
            Ok(request) => VeniceRequest {
                request,
                venice_parameters: VeniceParameters {
                    include_venice_system_prompt: false,
                    strip_thinking_response: true,
                    disable_thinking,
                },
            },
            Err(error) => return async move { Err(error.into()) }.boxed(),
        };
        let completions = self.stream_completion(request, cx);
        let executor = cx.background_executor().clone();
        async move {
            let mapper = ChatCompletionEventMapper::new();
            Ok(language_model::stream_in_background(
                mapper.map_stream(completions.await?).boxed(),
                executor,
            ))
        }
        .boxed()
    }
}

const REASONING_LEVELS: [ReasoningEffort; 3] = [
    ReasoningEffort::Low,
    ReasoningEffort::Medium,
    ReasoningEffort::High,
];

fn reasoning_levels(model: &VeniceModel) -> Vec<LanguageModelEffortLevel> {
    if !model.supports_reasoning {
        return Vec::new();
    }
    REASONING_LEVELS
        .iter()
        .map(|effort| LanguageModelEffortLevel {
            name: effort.label().into(),
            value: effort.value().into(),
            is_default: *effort == ReasoningEffort::Medium,
        })
        .collect()
}

/// The reasoning effort to ask Venice for, and whether to turn thinking off,
/// from the level chosen in the thread.
fn reasoning_for(
    model: &VeniceModel,
    request: &LanguageModelRequest,
) -> (Option<ReasoningEffort>, bool) {
    if !model.supports_reasoning {
        return (None, false);
    }
    if !request.thinking_allowed {
        return (None, true);
    }
    let effort = request
        .thinking_effort
        .as_deref()
        .and_then(|effort| effort.parse::<ReasoningEffort>().ok())
        .filter(|effort| REASONING_LEVELS.contains(effort))
        .unwrap_or(ReasoningEffort::Medium);
    (Some(effort), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODELS: &str = r#"{
        "object": "list",
        "type": "text",
        "data": [
            {
                "id": "llama-3.2-3b",
                "type": "text",
                "object": "model",
                "owned_by": "venice.ai",
                "model_spec": {
                    "name": "Llama 3.2 3B",
                    "availableContextTokens": 131072,
                    "capabilities": { "supportsFunctionCalling": true, "supportsVision": false },
                    "traits": ["fastest"]
                }
            },
            {
                "id": "qwen3-235b",
                "type": "text",
                "model_spec": {
                    "name": "Qwen3 235B",
                    "availableContextTokens": 131072,
                    "maxCompletionTokens": 32768,
                    "capabilities": { "supportsFunctionCalling": true, "supportsVision": false },
                    "traits": ["function_calling_default"]
                }
            },
            {
                "id": "retired-model",
                "type": "text",
                "model_spec": { "offline": true }
            },
            { "id": "flux-dev", "type": "image", "model_spec": {} }
        ]
    }"#;

    #[test]
    fn parses_text_models_and_skips_the_rest() {
        let models = parse_models(MODELS).expect("parses");
        let ids: Vec<_> = models.iter().map(|model| model.id.as_str()).collect();
        assert_eq!(ids, ["llama-3.2-3b", "qwen3-235b"]);
        assert_eq!(models[1].display_name, "Qwen3 235B");
        assert_eq!(models[1].context_tokens, 131_072);
        assert_eq!(models[1].max_output_tokens, Some(32_768));
        assert!(models[1].supports_tools);
    }

    #[test]
    fn prefers_the_function_calling_default() {
        let models = parse_models(MODELS).expect("parses");
        assert_eq!(
            default_model_of(&models).map(|model| model.id.as_str()),
            Some("qwen3-235b")
        );
    }

    #[test]
    fn request_turns_off_venice_system_prompt() {
        let request = VeniceRequest {
            request: open_ai::Request {
                model: "qwen3-235b".into(),
                messages: Vec::new(),
                stream: true,
                stream_options: None,
                max_completion_tokens: None,
                max_tokens: None,
                stop: Vec::new(),
                temperature: None,
                tool_choice: None,
                parallel_tool_calls: None,
                tools: Vec::new(),
                prompt_cache_key: None,
                reasoning_effort: None,
                service_tier: None,
            },
            venice_parameters: VeniceParameters {
                include_venice_system_prompt: false,
                strip_thinking_response: true,
                disable_thinking: false,
            },
        };
        let json = serde_json::to_value(&request).expect("serializes");
        assert_eq!(json["model"], "qwen3-235b");
        assert_eq!(
            json["venice_parameters"]["include_venice_system_prompt"],
            false
        );
        assert!(json["venice_parameters"].get("disable_thinking").is_none());
    }

    fn reasoning_model(supports_reasoning: bool) -> VeniceModel {
        VeniceModel {
            id: "qwen3-235b".into(),
            display_name: "Qwen3".into(),
            context_tokens: 32_768,
            max_output_tokens: None,
            supports_tools: true,
            supports_images: false,
            supports_reasoning,
            traits: Vec::new(),
            pricing: None,
        }
    }

    #[test]
    fn reasoning_models_offer_three_levels() {
        let levels = reasoning_levels(&reasoning_model(true));
        let values: Vec<_> = levels.iter().map(|level| level.value.as_ref()).collect();
        assert_eq!(values, ["low", "medium", "high"]);
        assert!(levels.iter().any(|level| level.is_default && level.value == "medium"));
        assert!(reasoning_levels(&reasoning_model(false)).is_empty());
    }

    #[test]
    fn chosen_level_reaches_the_request() {
        let model = reasoning_model(true);
        let mut request = LanguageModelRequest {
            thinking_allowed: true,
            thinking_effort: Some("high".into()),
            ..Default::default()
        };
        assert_eq!(reasoning_for(&model, &request), (Some(ReasoningEffort::High), false));

        request.thinking_effort = Some("xhigh".into());
        assert_eq!(reasoning_for(&model, &request), (Some(ReasoningEffort::Medium), false));

        request.thinking_allowed = false;
        assert_eq!(reasoning_for(&model, &request), (None, true));

        assert_eq!(reasoning_for(&reasoning_model(false), &request), (None, false));
    }

    #[test]
    fn reasoning_capability_is_read_from_the_model_list() {
        let models = parse_models(
            r#"{"data": [{"id": "deepseek-r1", "type": "text", "model_spec": {
                "capabilities": {"supportsFunctionCalling": true, "supportsReasoning": true}
            }}]}"#,
        )
        .expect("parses");
        assert!(models[0].supports_reasoning);
    }
}
