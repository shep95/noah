use anyhow::{Result, anyhow};
use collections::HashSet;
use credentials_provider::CredentialsProvider;
use futures::{AsyncReadExt as _, FutureExt, StreamExt, future::BoxFuture};
use gpui::{App, AppContext, AsyncApp, Context, Entity, SharedString, Task};
use http_client::{AsyncBody, HttpClient, Method, Request as HttpRequest};
use language_model::chat_completion::ChatCompletionEventMapper;
use language_model::{
    ApiKeyConfiguration, ApiKeyState, ApiKeyStatus, AuthenticateError, EnvVar, IconOrSvg,
    LanguageModel, LanguageModelCompletionError, LanguageModelCompletionEvent, LanguageModelId,
    LanguageModelName, LanguageModelProvider, LanguageModelProviderId, LanguageModelProviderName,
    LanguageModelProviderState, LanguageModelRequest, LanguageModelToolChoice,
    ProviderSettingsView, RateLimiter,
};
use open_ai::ResponseStreamEvent;
use open_ai::completion::{ChatCompletionMaxTokensParameter, into_open_ai};
use settings::{Settings, SettingsStore};
use std::sync::Arc;
use ui::IconName;

const DEFAULT_CONTEXT_TOKENS: u64 = 128_000;

/// Fields that OpenAI-compatible servers use to report a model's context
/// window; each vendor picked its own name.
const CONTEXT_FIELDS: &[&str] = &[
    "context_length",
    "context_window",
    "max_context_length",
    "max_model_len",
];

/// Model list entries whose id contains one of these are not chat models, and
/// would fail if picked for a thread.
const NON_CHAT_ID_MARKERS: &[&str] = &[
    "embed",
    "whisper",
    "tts",
    "audio",
    "image",
    "rerank",
    "moderation",
    "dall-e",
    "transcribe",
    "-ocr",
];

/// Values of the `type` field some servers (Together AI, for one) attach to
/// each model, for models that can't hold a conversation.
const NON_CHAT_TYPES: &[&str] = &[
    "audio",
    "embedding",
    "embeddings",
    "image",
    "moderation",
    "rerank",
    "stt",
    "transcribe",
    "tts",
    "video",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Region {
    Western,
    Chinese,
}

pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub region: Region,
    pub api_url: &'static str,
    pub env_var: &'static str,
    pub api_key_url: &'static str,
    /// False for servers without a `GET /models` endpoint, so noah doesn't
    /// send a request it knows will fail.
    pub lists_models: bool,
    /// Model ids and their context windows, offered when the server can't
    /// list its own models. The first one present is the default model.
    pub fallback_models: &'static [(&'static str, u64)],
}

/// Ordered Western first, then Chinese, alphabetically by name within each.
pub const PRESETS: &[Preset] = &[
    Preset {
        id: "cerebras",
        name: "Cerebras",
        region: Region::Western,
        api_url: "https://api.cerebras.ai/v1",
        env_var: "CEREBRAS_API_KEY",
        api_key_url: "https://cloud.cerebras.ai/platform",
        lists_models: true,
        fallback_models: &[
            ("gpt-oss-120b", 131_072),
            ("qwen-3-235b-a22b-instruct-2507", 131_072),
            ("llama-3.3-70b", 65_536),
            ("llama3.1-8b", 32_768),
        ],
    },
    Preset {
        id: "cohere",
        name: "Cohere",
        region: Region::Western,
        api_url: "https://api.cohere.ai/compatibility/v1",
        env_var: "COHERE_API_KEY",
        api_key_url: "https://dashboard.cohere.com/api-keys",
        lists_models: true,
        fallback_models: &[
            ("command-a-03-2025", 256_000),
            ("command-r-plus-08-2024", 128_000),
            ("command-r-08-2024", 128_000),
            ("command-r7b-12-2024", 128_000),
        ],
    },
    Preset {
        id: "deepinfra",
        name: "DeepInfra",
        region: Region::Western,
        api_url: "https://api.deepinfra.com/v1/openai",
        env_var: "DEEPINFRA_API_KEY",
        api_key_url: "https://deepinfra.com/dash/api_keys",
        lists_models: true,
        fallback_models: &[
            ("deepseek-ai/DeepSeek-V3.1", 163_840),
            ("Qwen/Qwen3-Coder-480B-A35B-Instruct", 262_144),
            ("moonshotai/Kimi-K2-Instruct-0905", 262_144),
            ("meta-llama/Llama-3.3-70B-Instruct", 131_072),
        ],
    },
    Preset {
        id: "fireworks",
        name: "Fireworks AI",
        region: Region::Western,
        api_url: "https://api.fireworks.ai/inference/v1",
        env_var: "FIREWORKS_API_KEY",
        api_key_url: "https://fireworks.ai/account/api-keys",
        lists_models: true,
        fallback_models: &[
            ("accounts/fireworks/models/deepseek-v3p1", 163_840),
            (
                "accounts/fireworks/models/qwen3-coder-480b-a35b-instruct",
                262_144,
            ),
            ("accounts/fireworks/models/kimi-k2-instruct-0905", 262_144),
            ("accounts/fireworks/models/llama-v3p3-70b-instruct", 131_072),
        ],
    },
    Preset {
        id: "groq",
        name: "Groq",
        region: Region::Western,
        api_url: "https://api.groq.com/openai/v1",
        env_var: "GROQ_API_KEY",
        api_key_url: "https://console.groq.com/keys",
        lists_models: true,
        fallback_models: &[
            ("openai/gpt-oss-120b", 131_072),
            ("moonshotai/kimi-k2-instruct-0905", 262_144),
            ("llama-3.3-70b-versatile", 131_072),
            ("qwen/qwen3-32b", 131_072),
            ("llama-3.1-8b-instant", 131_072),
        ],
    },
    Preset {
        id: "huggingface",
        name: "Hugging Face",
        region: Region::Western,
        api_url: "https://router.huggingface.co/v1",
        env_var: "HF_TOKEN",
        api_key_url: "https://huggingface.co/settings/tokens",
        lists_models: true,
        fallback_models: &[
            ("openai/gpt-oss-120b", 131_072),
            ("deepseek-ai/DeepSeek-V3.1", 131_072),
            ("Qwen/Qwen3-Coder-480B-A35B-Instruct", 262_144),
            ("moonshotai/Kimi-K2-Instruct-0905", 262_144),
        ],
    },
    Preset {
        id: "hyperbolic",
        name: "Hyperbolic",
        region: Region::Western,
        api_url: "https://api.hyperbolic.xyz/v1",
        env_var: "HYPERBOLIC_API_KEY",
        api_key_url: "https://app.hyperbolic.xyz/settings",
        lists_models: true,
        fallback_models: &[
            ("openai/gpt-oss-120b", 131_072),
            ("deepseek-ai/DeepSeek-V3", 131_072),
            ("Qwen/Qwen3-Coder-480B-A35B-Instruct", 262_144),
            ("meta-llama/Llama-3.3-70B-Instruct", 131_072),
        ],
    },
    Preset {
        id: "nebius",
        name: "Nebius AI Studio",
        region: Region::Western,
        api_url: "https://api.studio.nebius.com/v1",
        env_var: "NEBIUS_API_KEY",
        api_key_url: "https://studio.nebius.com/settings/api-keys",
        lists_models: true,
        fallback_models: &[
            ("openai/gpt-oss-120b", 131_072),
            ("deepseek-ai/DeepSeek-V3-0324", 131_072),
            ("Qwen/Qwen3-Coder-480B-A35B-Instruct", 262_144),
            ("meta-llama/Llama-3.3-70B-Instruct", 131_072),
        ],
    },
    Preset {
        id: "novita",
        name: "Novita AI",
        region: Region::Western,
        api_url: "https://api.novita.ai/v3/openai",
        env_var: "NOVITA_API_KEY",
        api_key_url: "https://novita.ai/settings/key-management",
        lists_models: true,
        fallback_models: &[
            ("deepseek/deepseek-v3.1", 131_072),
            ("qwen/qwen3-coder-480b-a35b-instruct", 262_144),
            ("moonshotai/kimi-k2-instruct", 131_072),
            ("meta-llama/llama-3.3-70b-instruct", 131_072),
        ],
    },
    Preset {
        id: "nvidia",
        name: "NVIDIA NIM",
        region: Region::Western,
        api_url: "https://integrate.api.nvidia.com/v1",
        env_var: "NVIDIA_API_KEY",
        api_key_url: "https://build.nvidia.com/settings/api-keys",
        lists_models: true,
        fallback_models: &[
            ("deepseek-ai/deepseek-v3.1", 131_072),
            ("qwen/qwen3-coder-480b-a35b-instruct", 262_144),
            ("moonshotai/kimi-k2-instruct", 131_072),
            ("meta/llama-3.3-70b-instruct", 131_072),
        ],
    },
    Preset {
        id: "perplexity",
        name: "Perplexity",
        region: Region::Western,
        api_url: "https://api.perplexity.ai",
        env_var: "PERPLEXITY_API_KEY",
        api_key_url: "https://www.perplexity.ai/account/api/keys",
        lists_models: false,
        fallback_models: &[
            ("sonar-pro", 200_000),
            ("sonar", 128_000),
            ("sonar-reasoning-pro", 128_000),
        ],
    },
    Preset {
        id: "sambanova",
        name: "SambaNova",
        region: Region::Western,
        api_url: "https://api.sambanova.ai/v1",
        env_var: "SAMBANOVA_API_KEY",
        api_key_url: "https://cloud.sambanova.ai/apis",
        lists_models: true,
        fallback_models: &[
            ("DeepSeek-V3.1", 131_072),
            ("gpt-oss-120b", 131_072),
            ("Meta-Llama-3.3-70B-Instruct", 131_072),
            ("Qwen3-32B", 32_768),
        ],
    },
    Preset {
        id: "together",
        name: "Together AI",
        region: Region::Western,
        api_url: "https://api.together.xyz/v1",
        env_var: "TOGETHER_API_KEY",
        api_key_url: "https://api.together.ai/settings/api-keys",
        lists_models: true,
        fallback_models: &[
            ("openai/gpt-oss-120b", 131_072),
            ("deepseek-ai/DeepSeek-V3", 131_072),
            ("Qwen/Qwen3-Coder-480B-A35B-Instruct-FP8", 262_144),
            ("moonshotai/Kimi-K2-Instruct-0905", 262_144),
            ("meta-llama/Llama-3.3-70B-Instruct-Turbo", 131_072),
        ],
    },
    Preset {
        id: "yi",
        name: "01.AI Yi",
        region: Region::Chinese,
        api_url: "https://api.lingyiwanwu.com/v1",
        env_var: "YI_API_KEY",
        api_key_url: "https://platform.lingyiwanwu.com/apikeys",
        lists_models: true,
        fallback_models: &[("yi-lightning", 16_384), ("yi-large", 32_768)],
    },
    Preset {
        id: "dashscope",
        name: "Alibaba Qwen",
        region: Region::Chinese,
        api_url: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        env_var: "DASHSCOPE_API_KEY",
        api_key_url: "https://modelstudio.console.alibabacloud.com/?tab=model#/api-key",
        lists_models: true,
        fallback_models: &[
            ("qwen3-max", 262_144),
            ("qwen3-coder-plus", 1_000_000),
            ("qwen-plus", 131_072),
            ("qwen-turbo", 131_072),
        ],
    },
    Preset {
        id: "dashscope_cn",
        name: "Alibaba Qwen (China)",
        region: Region::Chinese,
        api_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        env_var: "DASHSCOPE_API_KEY",
        api_key_url: "https://bailian.console.aliyun.com/?tab=model#/api-key",
        lists_models: true,
        fallback_models: &[
            ("qwen3-max", 262_144),
            ("qwen3-coder-plus", 1_000_000),
            ("qwen-plus", 131_072),
            ("qwen-turbo", 131_072),
        ],
    },
    Preset {
        id: "baichuan",
        name: "Baichuan",
        region: Region::Chinese,
        api_url: "https://api.baichuan-ai.com/v1",
        env_var: "BAICHUAN_API_KEY",
        api_key_url: "https://platform.baichuan-ai.com/console/apikey",
        lists_models: true,
        fallback_models: &[
            ("Baichuan4-Turbo", 32_768),
            ("Baichuan4-Air", 32_768),
            ("Baichuan4", 32_768),
        ],
    },
    Preset {
        id: "qianfan",
        name: "Baidu Qianfan",
        region: Region::Chinese,
        api_url: "https://qianfan.baidubce.com/v2",
        env_var: "QIANFAN_API_KEY",
        api_key_url: "https://console.bce.baidu.com/iam/#/iam/apikey/list",
        lists_models: true,
        fallback_models: &[
            ("ernie-4.5-turbo-128k", 131_072),
            ("ernie-4.5-turbo-32k", 32_768),
            ("ernie-x1-turbo-32k", 32_768),
        ],
    },
    Preset {
        id: "minimax",
        name: "MiniMax",
        region: Region::Chinese,
        api_url: "https://api.minimax.io/v1",
        env_var: "MINIMAX_API_KEY",
        api_key_url: "https://www.minimax.io/platform/user-center/basic-information/interface-key",
        lists_models: true,
        fallback_models: &[
            ("MiniMax-M2", 204_800),
            ("MiniMax-M1", 1_000_000),
            ("MiniMax-Text-01", 1_000_000),
        ],
    },
    Preset {
        id: "minimax_cn",
        name: "MiniMax (China)",
        region: Region::Chinese,
        api_url: "https://api.minimaxi.com/v1",
        env_var: "MINIMAX_API_KEY",
        api_key_url: "https://platform.minimaxi.com/user-center/basic-information/interface-key",
        lists_models: true,
        fallback_models: &[
            ("MiniMax-M2", 204_800),
            ("MiniMax-M1", 1_000_000),
            ("MiniMax-Text-01", 1_000_000),
        ],
    },
    Preset {
        id: "moonshot",
        name: "Moonshot Kimi",
        region: Region::Chinese,
        api_url: "https://api.moonshot.ai/v1",
        env_var: "MOONSHOT_API_KEY",
        api_key_url: "https://platform.moonshot.ai/console/api-keys",
        lists_models: true,
        fallback_models: &[
            ("kimi-k2-0905-preview", 262_144),
            ("kimi-k2-turbo-preview", 262_144),
            ("kimi-k2-thinking", 262_144),
            ("moonshot-v1-128k", 131_072),
        ],
    },
    Preset {
        id: "moonshot_cn",
        name: "Moonshot Kimi (China)",
        region: Region::Chinese,
        api_url: "https://api.moonshot.cn/v1",
        env_var: "MOONSHOT_API_KEY",
        api_key_url: "https://platform.moonshot.cn/console/api-keys",
        lists_models: true,
        fallback_models: &[
            ("kimi-k2-0905-preview", 262_144),
            ("kimi-k2-turbo-preview", 262_144),
            ("kimi-k2-thinking", 262_144),
            ("moonshot-v1-128k", 131_072),
        ],
    },
    Preset {
        id: "siliconflow",
        name: "SiliconFlow",
        region: Region::Chinese,
        api_url: "https://api.siliconflow.com/v1",
        env_var: "SILICONFLOW_API_KEY",
        api_key_url: "https://cloud.siliconflow.com/account/ak",
        lists_models: true,
        fallback_models: &[
            ("deepseek-ai/DeepSeek-V3.1", 163_840),
            ("Qwen/Qwen3-Coder-480B-A35B-Instruct", 262_144),
            ("moonshotai/Kimi-K2-Instruct-0905", 262_144),
            ("zai-org/GLM-4.5", 131_072),
        ],
    },
    Preset {
        id: "siliconflow_cn",
        name: "SiliconFlow (China)",
        region: Region::Chinese,
        api_url: "https://api.siliconflow.cn/v1",
        env_var: "SILICONFLOW_API_KEY",
        api_key_url: "https://cloud.siliconflow.cn/account/ak",
        lists_models: true,
        fallback_models: &[
            ("deepseek-ai/DeepSeek-V3.1", 163_840),
            ("Qwen/Qwen3-Coder-480B-A35B-Instruct", 262_144),
            ("moonshotai/Kimi-K2-Instruct-0905", 262_144),
            ("zai-org/GLM-4.5", 131_072),
        ],
    },
    Preset {
        id: "stepfun",
        name: "StepFun",
        region: Region::Chinese,
        api_url: "https://api.stepfun.com/v1",
        env_var: "STEPFUN_API_KEY",
        api_key_url: "https://platform.stepfun.com/interface-key",
        lists_models: true,
        fallback_models: &[
            ("step-3", 65_536),
            ("step-2-16k", 16_384),
            ("step-1-32k", 32_768),
        ],
    },
    Preset {
        id: "hunyuan",
        name: "Tencent Hunyuan",
        region: Region::Chinese,
        api_url: "https://api.hunyuan.cloud.tencent.com/v1",
        env_var: "HUNYUAN_API_KEY",
        api_key_url: "https://console.cloud.tencent.com/hunyuan/api-key",
        lists_models: true,
        fallback_models: &[
            ("hunyuan-turbos-latest", 32_768),
            ("hunyuan-t1-latest", 65_536),
            ("hunyuan-functioncall", 32_768),
        ],
    },
    Preset {
        id: "volcengine",
        name: "Volcengine Ark (Doubao)",
        region: Region::Chinese,
        api_url: "https://ark.cn-beijing.volces.com/api/v3",
        env_var: "ARK_API_KEY",
        api_key_url: "https://console.volcengine.com/ark/region:ark+cn-beijing/apiKey",
        lists_models: true,
        fallback_models: &[
            ("doubao-seed-1-6-250615", 262_144),
            ("doubao-seed-1-6-flash-250615", 262_144),
            ("deepseek-v3-250324", 131_072),
        ],
    },
    Preset {
        id: "zai",
        name: "Z.ai",
        region: Region::Chinese,
        api_url: "https://api.z.ai/api/paas/v4",
        env_var: "ZAI_API_KEY",
        api_key_url: "https://z.ai/manage-apikey/apikey-list",
        lists_models: true,
        fallback_models: &[
            ("glm-4.6", 200_000),
            ("glm-4.5", 131_072),
            ("glm-4.5-air", 131_072),
        ],
    },
    Preset {
        id: "zhipu",
        name: "Zhipu GLM",
        region: Region::Chinese,
        api_url: "https://open.bigmodel.cn/api/paas/v4",
        env_var: "ZHIPUAI_API_KEY",
        api_key_url: "https://open.bigmodel.cn/usercenter/apikeys",
        lists_models: true,
        fallback_models: &[
            ("glm-4.6", 200_000),
            ("glm-4.5", 131_072),
            ("glm-4.5-air", 131_072),
        ],
    },
];

#[derive(Clone, Debug, PartialEq)]
pub struct PresetModel {
    pub id: String,
    pub context_tokens: u64,
}

impl Preset {
    fn provider_id(&self) -> LanguageModelProviderId {
        LanguageModelProviderId::new(self.id)
    }

    fn provider_name(&self) -> LanguageModelProviderName {
        LanguageModelProviderName::new(self.name)
    }

    fn fallback(&self) -> Vec<PresetModel> {
        self.fallback_models
            .iter()
            .map(|(id, context_tokens)| PresetModel {
                id: (*id).to_string(),
                context_tokens: *context_tokens,
            })
            .collect()
    }
}

fn is_chat_model(entry: &serde_json::Value, id: &str) -> bool {
    let id = id.to_lowercase();
    if NON_CHAT_ID_MARKERS.iter().any(|marker| id.contains(marker)) {
        return false;
    }
    let model_type = entry
        .get("type")
        .and_then(|model_type| model_type.as_str())
        .map(|model_type| model_type.to_lowercase());
    !model_type.is_some_and(|model_type| NON_CHAT_TYPES.contains(&model_type.as_str()))
}

fn context_tokens_of(entry: &serde_json::Value) -> u64 {
    CONTEXT_FIELDS
        .iter()
        .filter_map(|field| entry.get(*field))
        .find_map(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
                .filter(|tokens| *tokens > 0)
        })
        .unwrap_or(DEFAULT_CONTEXT_TOKENS)
}

/// Reads an OpenAI-style model list. Most servers wrap the entries in
/// `{"data": [...]}`, but Together AI returns the bare array.
fn parse_models(body: &str) -> Result<Vec<PresetModel>> {
    let response: serde_json::Value = serde_json::from_str(body)?;
    let entries = match &response {
        serde_json::Value::Array(entries) => entries,
        serde_json::Value::Object(object) => object
            .get("data")
            .and_then(|data| data.as_array())
            .ok_or_else(|| anyhow!("the response has no `data` list"))?,
        _ => return Err(anyhow!("the response is not a model list")),
    };
    let mut seen = HashSet::default();
    Ok(entries
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_str()?.trim();
            if id.is_empty() || !is_chat_model(entry, id) || !seen.insert(id.to_string()) {
                return None;
            }
            Some(PresetModel {
                id: id.to_string(),
                context_tokens: context_tokens_of(entry),
            })
        })
        .collect())
}

enum ModelListResponse {
    Listed(Vec<PresetModel>),
    KeyRejected,
    Unavailable(String),
}

fn read_model_list_response(status: u16, body: &str) -> ModelListResponse {
    if matches!(status, 401 | 403) {
        return ModelListResponse::KeyRejected;
    }
    if !(200..300).contains(&status) {
        return ModelListResponse::Unavailable(format!("status {status}"));
    }
    match parse_models(body) {
        Ok(models) => ModelListResponse::Listed(models),
        Err(error) => ModelListResponse::Unavailable(format!("{error:#}")),
    }
}

async fn fetch_model_list(
    http_client: &dyn HttpClient,
    api_url: &str,
    api_key: &str,
) -> ModelListResponse {
    let result: Result<(u16, String)> = async {
        let request = HttpRequest::builder()
            .method(Method::GET)
            .uri(format!("{api_url}/models"))
            .header("Accept", "application/json")
            .header("Authorization", format!("Bearer {api_key}"))
            .body(AsyncBody::default())?;
        let mut response = http_client.send(request).await?;
        let mut body = String::new();
        response.body_mut().read_to_string(&mut body).await?;
        Ok((response.status().as_u16(), body))
    }
    .await;
    match result {
        Ok((status, body)) => read_model_list_response(status, &body),
        Err(error) => ModelListResponse::Unavailable(format!("{error:#}")),
    }
}

#[derive(Clone, Debug, PartialEq)]
enum ModelListState {
    /// No key yet, so nothing has been asked of the server.
    Idle,
    Offline,
    Fetching,
    Listed,
    /// The fallback list is in use, with the reason the server's own list
    /// couldn't be used, if it was asked for one.
    BuiltIn {
        reason: Option<SharedString>,
    },
    KeyRejected,
}

fn resolve_model_list(
    preset: &Preset,
    response: ModelListResponse,
) -> (Vec<PresetModel>, ModelListState) {
    match response {
        ModelListResponse::Listed(models) if !models.is_empty() => (models, ModelListState::Listed),
        ModelListResponse::Listed(_) => (
            preset.fallback(),
            ModelListState::BuiltIn {
                reason: Some("it listed no chat models".into()),
            },
        ),
        ModelListResponse::KeyRejected => (Vec::new(), ModelListState::KeyRejected),
        ModelListResponse::Unavailable(reason) => (
            preset.fallback(),
            ModelListState::BuiltIn {
                reason: Some(reason.into()),
            },
        ),
    }
}

fn default_model_of<'a>(preset: &Preset, models: &'a [PresetModel]) -> Option<&'a PresetModel> {
    preset
        .fallback_models
        .iter()
        .find_map(|(fallback_id, _)| models.iter().find(|model| model.id == *fallback_id))
        .or_else(|| models.first())
}

fn is_offline(cx: &App) -> bool {
    crate::AllLanguageModelSettings::get_global(cx).offline
}

pub struct State {
    preset: &'static Preset,
    api_key_state: ApiKeyState,
    credentials_provider: Arc<dyn CredentialsProvider>,
    http_client: Arc<dyn HttpClient>,
    available_models: Vec<PresetModel>,
    model_list_state: ModelListState,
    fetch_models_task: Option<Task<()>>,
}

impl State {
    fn api_url(&self) -> SharedString {
        SharedString::new_static(self.preset.api_url)
    }

    fn is_authenticated(&self) -> bool {
        self.api_key_state.has_key()
    }

    fn set_api_key(&mut self, api_key: Option<String>, cx: &mut Context<Self>) -> Task<Result<()>> {
        let credentials_provider = self.credentials_provider.clone();
        let task = self.api_key_state.store(
            self.api_url(),
            api_key,
            |this| &mut this.api_key_state,
            credentials_provider,
            cx,
        );
        cx.spawn(async move |this, cx| {
            task.await?;
            this.update(cx, |this, cx| this.restart_fetch_models_task(cx))?;
            Ok(())
        })
    }

    fn authenticate(&mut self, cx: &mut Context<Self>) -> Task<Result<(), AuthenticateError>> {
        let credentials_provider = self.credentials_provider.clone();
        let task = self.api_key_state.load_if_needed(
            self.api_url(),
            |this| &mut this.api_key_state,
            credentials_provider,
            cx,
        );
        cx.spawn(async move |this, cx| {
            let result = task.await;
            // Authentication is requested again whenever the UI looks at the
            // provider, so the list is only fetched the first time a key shows up.
            this.update(cx, |this, cx| {
                if this.model_list_state == ModelListState::Idle {
                    this.restart_fetch_models_task(cx);
                }
            })
            .ok();
            result
        })
    }

    fn restart_fetch_models_task(&mut self, cx: &mut Context<Self>) {
        let preset = self.preset;
        let api_url = self.api_url();
        self.available_models.clear();
        self.fetch_models_task = None;
        if is_offline(cx) {
            self.model_list_state = ModelListState::Offline;
            cx.notify();
            return;
        }
        let Some(api_key) = self.api_key_state.key(&api_url) else {
            self.model_list_state = ModelListState::Idle;
            cx.notify();
            return;
        };
        if !preset.lists_models {
            self.available_models = preset.fallback();
            self.model_list_state = ModelListState::BuiltIn { reason: None };
            cx.notify();
            return;
        }
        self.model_list_state = ModelListState::Fetching;
        let http_client = self.http_client.clone();
        self.fetch_models_task = Some(cx.spawn(async move |this, cx| {
            let response = fetch_model_list(http_client.as_ref(), &api_url, &api_key).await;
            if let ModelListResponse::Unavailable(reason) = &response {
                log::warn!(
                    "{} didn't list its models, using noah's built-in list: {reason}",
                    preset.name
                );
            }
            let (models, model_list_state) = resolve_model_list(preset, response);
            this.update(cx, |this, cx| {
                this.available_models = models;
                this.model_list_state = model_list_state;
                this.fetch_models_task = None;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn status(&self) -> ApiKeyStatus {
        let name = self.preset.name;
        let count = self.available_models.len();
        let models = if count == 1 { "model" } else { "models" };
        match &self.model_list_state {
            ModelListState::Idle | ModelListState::Fetching => ApiKeyStatus::Checking,
            ModelListState::Offline => ApiKeyStatus::Failed(
                format!("offline mode is on, so noah doesn't contact {name}").into(),
            ),
            ModelListState::Listed => {
                ApiKeyStatus::Working(format!("Connected to {name} · {count} {models}").into())
            }
            ModelListState::BuiltIn { reason: None } => {
                ApiKeyStatus::Working(format!("{count} built-in {models}").into())
            }
            ModelListState::BuiltIn {
                reason: Some(reason),
            } => ApiKeyStatus::Working(
                format!("{count} built-in {models} ({name} didn't list its own models: {reason})")
                    .into(),
            ),
            ModelListState::KeyRejected => ApiKeyStatus::Failed(
                format!(
                    "{name} didn't accept this API key. Check it at {}.",
                    self.preset.api_key_url
                )
                .into(),
            ),
        }
    }
}

pub struct PresetLanguageModelProvider {
    preset: &'static Preset,
    http_client: Arc<dyn HttpClient>,
    state: Entity<State>,
}

impl PresetLanguageModelProvider {
    pub fn new(
        preset: &'static Preset,
        http_client: Arc<dyn HttpClient>,
        credentials_provider: Arc<dyn CredentialsProvider>,
        cx: &mut App,
    ) -> Self {
        let state = cx.new(|cx| {
            cx.observe_global::<SettingsStore>({
                let mut last_offline = is_offline(cx);
                move |this: &mut State, cx| {
                    let offline = is_offline(cx);
                    if offline != last_offline {
                        last_offline = offline;
                        this.restart_fetch_models_task(cx);
                    }
                }
            })
            .detach();
            State {
                preset,
                api_key_state: ApiKeyState::new(
                    SharedString::new_static(preset.api_url),
                    EnvVar::new(SharedString::new_static(preset.env_var)),
                ),
                credentials_provider,
                http_client: http_client.clone(),
                available_models: Vec::new(),
                model_list_state: ModelListState::Idle,
                fetch_models_task: None,
            }
        });
        Self {
            preset,
            http_client,
            state,
        }
    }

    fn create_language_model(&self, model: PresetModel) -> Arc<dyn LanguageModel> {
        Arc::new(PresetLanguageModel {
            id: LanguageModelId::from(model.id.clone()),
            preset: self.preset,
            model,
            state: self.state.clone(),
            http_client: self.http_client.clone(),
            request_limiter: RateLimiter::new(4),
        })
    }
}

impl LanguageModelProviderState for PresetLanguageModelProvider {
    type ObservableEntity = State;

    fn observable_entity(&self) -> Option<Entity<Self::ObservableEntity>> {
        Some(self.state.clone())
    }
}

impl LanguageModelProvider for PresetLanguageModelProvider {
    fn id(&self) -> LanguageModelProviderId {
        self.preset.provider_id()
    }

    fn name(&self) -> LanguageModelProviderName {
        self.preset.provider_name()
    }

    fn icon(&self) -> IconOrSvg {
        IconOrSvg::Icon(IconName::AiOpenAiCompat)
    }

    fn default_model(&self, cx: &App) -> Option<Arc<dyn LanguageModel>> {
        default_model_of(self.preset, &self.state.read(cx).available_models)
            .cloned()
            .map(|model| self.create_language_model(model))
    }

    fn default_fast_model(&self, _cx: &App) -> Option<Arc<dyn LanguageModel>> {
        None
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
        let mut configuration = ApiKeyConfiguration::new(
            state.api_key_state.has_key(),
            state.api_key_state.is_from_env_var(),
            state.api_key_state.env_var_name().clone(),
            SharedString::new_static(self.preset.api_key_url),
        );
        if state.api_key_state.has_key() || state.model_list_state == ModelListState::Offline {
            configuration = configuration.status(state.status());
        }
        Some(ProviderSettingsView::ApiKey(configuration))
    }

    fn set_api_key(&self, api_key: Option<String>, cx: &mut App) -> Task<Result<()>> {
        self.state
            .update(cx, |state, cx| state.set_api_key(api_key, cx))
    }
}

pub struct PresetLanguageModel {
    id: LanguageModelId,
    preset: &'static Preset,
    model: PresetModel,
    state: Entity<State>,
    http_client: Arc<dyn HttpClient>,
    request_limiter: RateLimiter,
}

impl PresetLanguageModel {
    fn stream_completion(
        &self,
        request: open_ai::Request,
        cx: &AsyncApp,
    ) -> BoxFuture<
        'static,
        Result<
            futures::stream::BoxStream<'static, Result<ResponseStreamEvent>>,
            LanguageModelCompletionError,
        >,
    > {
        let http_client = self.http_client.clone();
        let preset = self.preset;
        let (api_key, offline) = self.state.read_with(cx, |state, cx| {
            (state.api_key_state.key(preset.api_url), is_offline(cx))
        });
        let future = self.request_limiter.stream(async move {
            if offline {
                return Err(LanguageModelCompletionError::Other(anyhow!(
                    "offline mode is on, so noah doesn't contact {}",
                    preset.name
                )));
            }
            let Some(api_key) = api_key else {
                return Err(LanguageModelCompletionError::NoApiKey {
                    provider: preset.provider_name(),
                });
            };
            let response = open_ai::stream_completion(
                http_client.as_ref(),
                preset.name,
                preset.api_url,
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

impl LanguageModel for PresetLanguageModel {
    fn id(&self) -> LanguageModelId {
        self.id.clone()
    }

    fn name(&self) -> LanguageModelName {
        LanguageModelName::from(self.model.id.clone())
    }

    fn provider_id(&self) -> LanguageModelProviderId {
        self.preset.provider_id()
    }

    fn provider_name(&self) -> LanguageModelProviderName {
        self.preset.provider_name()
    }

    /// Model lists don't say which models call tools, and these servers are
    /// used for agent work, so tools are offered and a model that rejects them
    /// reports its own error.
    fn supports_tools(&self) -> bool {
        true
    }

    fn supports_images(&self) -> bool {
        false
    }

    fn supports_tool_choice(&self, _choice: LanguageModelToolChoice) -> bool {
        true
    }

    fn supports_streaming_tools(&self) -> bool {
        true
    }

    fn supports_split_token_display(&self) -> bool {
        true
    }

    fn telemetry_id(&self) -> String {
        format!("{}/{}", self.preset.id, self.model.id)
    }

    fn max_token_count(&self) -> u64 {
        self.model.context_tokens
    }

    fn max_output_tokens(&self) -> Option<u64> {
        None
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
        // Service tiers are an OpenAI feature the other servers reject.
        request.speed = None;
        // `max_tokens` is the parameter every one of these servers accepts;
        // several reject OpenAI's newer `max_completion_tokens`.
        let request = match into_open_ai(
            request,
            &self.model.id,
            false,
            false,
            self.max_output_tokens(),
            ChatCompletionMaxTokensParameter::MaxTokens,
            None,
            false,
        ) {
            Ok(request) => request,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_well_formed() {
        let mut ids = HashSet::default();
        let mut api_urls = HashSet::default();
        for preset in PRESETS {
            assert!(ids.insert(preset.id), "duplicate preset id {}", preset.id);
            assert!(
                api_urls.insert(preset.api_url),
                "duplicate api url {}",
                preset.api_url
            );
            assert!(preset.api_url.starts_with("https://"), "{}", preset.id);
            assert!(!preset.api_url.ends_with('/'), "{}", preset.id);
            assert!(preset.api_key_url.starts_with("https://"), "{}", preset.id);
            assert!(!preset.env_var.is_empty(), "{}", preset.id);
            assert!(!preset.name.is_empty(), "{}", preset.id);
            assert!(!preset.fallback_models.is_empty(), "{}", preset.id);
            for (model_id, context_tokens) in preset.fallback_models {
                assert!(!model_id.is_empty(), "{}", preset.id);
                assert!(*context_tokens > 0, "{}/{model_id}", preset.id);
            }
        }
    }

    #[test]
    fn presets_are_ordered_western_then_chinese_alphabetically() {
        let keys: Vec<_> = PRESETS
            .iter()
            .map(|preset| (preset.region, preset.name.to_lowercase()))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn parses_context_fields_and_drops_non_chat_models() {
        let models = parse_models(
            r#"{"object": "list", "data": [
                {"id": "llama-3.3-70b-versatile", "context_window": 131072},
                {"id": "qwen3-32b", "context_length": "40960"},
                {"id": "mistral-large", "max_context_length": 262144},
                {"id": "vllm-model", "max_model_len": 32768},
                {"id": "no-context"},
                {"id": "zero-context", "context_length": 0},
                {"id": "text-embedding-3-small"},
                {"id": "BAAI/bge-reranker-v2-m3"},
                {"id": "whisper-large-v3"},
                {"id": "playai-tts"},
                {"id": "gpt-4o-audio-preview"},
                {"id": "flux-image-pro"},
                {"id": "omni-moderation-latest"},
                {"id": "dall-e-3"},
                {"id": "gpt-4o-transcribe"},
                {"id": "deepseek-ocr"},
                {"id": "flux-schnell", "type": "image"},
                {"id": "chat-typed-as-chat", "type": "chat"},
                {"id": "llama-3.3-70b-versatile", "context_window": 1},
                {"object": "model"}
            ]}"#,
        )
        .expect("parses");
        let listed: Vec<_> = models
            .iter()
            .map(|model| (model.id.as_str(), model.context_tokens))
            .collect();
        assert_eq!(
            listed,
            [
                ("llama-3.3-70b-versatile", 131_072),
                ("qwen3-32b", 40_960),
                ("mistral-large", 262_144),
                ("vllm-model", 32_768),
                ("no-context", DEFAULT_CONTEXT_TOKENS),
                ("zero-context", DEFAULT_CONTEXT_TOKENS),
                ("chat-typed-as-chat", DEFAULT_CONTEXT_TOKENS),
            ]
        );
    }

    #[test]
    fn parses_a_bare_model_array() {
        let models = parse_models(r#"[{"id": "meta-llama/Llama-3.3-70B-Instruct-Turbo", "type": "chat", "context_length": 131072}]"#)
            .expect("parses");
        assert_eq!(
            models,
            [PresetModel {
                id: "meta-llama/Llama-3.3-70B-Instruct-Turbo".into(),
                context_tokens: 131_072,
            }]
        );
    }

    fn preset(id: &str) -> &'static Preset {
        PRESETS
            .iter()
            .find(|preset| preset.id == id)
            .expect("preset exists")
    }

    #[test]
    fn falls_back_when_the_model_list_is_unreadable() {
        let groq = preset("groq");
        for (status, body) in [
            (200, "<html>not json</html>"),
            (200, r#"{"models": []}"#),
            (200, r#"{"data": [{"id": "text-embedding-3-small"}]}"#),
            (404, r#"{"error": "not found"}"#),
            (500, ""),
        ] {
            let (models, state) = resolve_model_list(groq, read_model_list_response(status, body));
            assert_eq!(models, groq.fallback(), "status {status}, body {body}");
            assert!(
                matches!(state, ModelListState::BuiltIn { reason: Some(_) }),
                "status {status}, body {body}"
            );
        }
    }

    #[test]
    fn uses_the_listed_models_when_readable() {
        let groq = preset("groq");
        let (models, state) = resolve_model_list(
            groq,
            read_model_list_response(200, r#"{"data": [{"id": "llama-3.1-8b-instant"}]}"#),
        );
        assert_eq!(state, ModelListState::Listed);
        assert_eq!(models.len(), 1);
    }

    #[test]
    fn rejected_key_offers_no_models() {
        for status in [401, 403] {
            let (models, state) =
                resolve_model_list(preset("groq"), read_model_list_response(status, ""));
            assert!(models.is_empty());
            assert_eq!(state, ModelListState::KeyRejected);
        }
    }

    #[test]
    fn default_model_prefers_the_fallback_order() {
        let groq = preset("groq");
        let models = vec![
            PresetModel {
                id: "some-new-model".into(),
                context_tokens: DEFAULT_CONTEXT_TOKENS,
            },
            PresetModel {
                id: "llama-3.3-70b-versatile".into(),
                context_tokens: 131_072,
            },
        ];
        assert_eq!(
            default_model_of(groq, &models).map(|model| model.id.as_str()),
            Some("llama-3.3-70b-versatile")
        );
        assert_eq!(
            default_model_of(groq, &models[..1]).map(|model| model.id.as_str()),
            Some("some-new-model")
        );
    }
}
