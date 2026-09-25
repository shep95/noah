use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use futures::{FutureExt as _, StreamExt as _, channel::mpsc};
use gpui::{App, AppContext as _, Task};
use model_files::{MetadataEdit, Runner};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ui::SharedString;
use util::markdown::MarkdownInlineCode;

use crate::{AgentTool, ToolCallEventStream, ToolInput};

/// Works with large AI model files (GGUF and safetensors) of any size without
/// loading their weights: only headers are read, and edits stream the tensor
/// data into a new file unchanged.
///
/// - `inspect`: format, size, parameter count, quantization types and all
///   metadata (architecture, context length, chat template, tokenizer...).
/// - `edit_metadata`: change or add metadata keys with `set`, delete them with
///   `remove`. The original file is never modified; the edited copy goes to
///   `output_path` or next to it as `<name>.edited.<ext>`. Existing keys keep
///   their type (a number key needs a number). Safetensors metadata is strings.
/// - `import`: make the model available in a local runner, `ollama` or
///   `lmstudio`, under `name`; it then appears in noah's model picker.
///
/// To change the weights themselves (quantize, convert from Hugging Face,
/// merge a LoRA), use llama.cpp's tools in the terminal, such as
/// `llama-quantize model.gguf out.gguf Q4_K_M` or `convert_hf_to_gguf.py`,
/// then inspect and import the result with this tool.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ModelFileToolInput {
    /// What to do with the file.
    pub action: ModelFileAction,
    /// Absolute path to the .gguf or .safetensors file.
    pub path: String,
    /// For `edit_metadata`: keys to set and their new values, as text.
    #[serde(default)]
    pub set: BTreeMap<String, String>,
    /// For `edit_metadata`: keys to delete.
    #[serde(default)]
    pub remove: Vec<String>,
    /// For `edit_metadata`: where to write the edited copy.
    #[serde(default)]
    pub output_path: Option<String>,
    /// For `import`: which runner to import into.
    #[serde(default)]
    pub runner: Option<ModelRunner>,
    /// For `import`: the model's name in the runner, such as `my-coder:7b`.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelFileAction {
    Inspect,
    EditMetadata,
    Import,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModelRunner {
    Ollama,
    Lmstudio,
}

pub struct ModelFileTool;

impl AgentTool for ModelFileTool {
    type Input = ModelFileToolInput;
    type Output = String;

    const NAME: &'static str = "model_file";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Other
    }

    fn allow_in_restricted_mode() -> bool {
        false
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(input) => {
                let verb = match input.action {
                    ModelFileAction::Inspect => "inspect",
                    ModelFileAction::EditMetadata => "edit metadata of",
                    ModelFileAction::Import => "import",
                };
                format!("model file: {verb} {}", MarkdownInlineCode(&input.path)).into()
            }
            Err(_) => "model file".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.spawn(async move |cx| {
            let input = input.recv().await.map_err(|error| error.to_string())?;
            let path = PathBuf::from(&input.path);

            let (description, work): (
                String,
                Box<dyn FnOnce(&mut dyn FnMut(f64)) -> anyhow::Result<String> + Send>,
            ) = match input.action {
                // Reading a header changes nothing, so it runs without asking.
                ModelFileAction::Inspect => {
                    return cx
                        .background_spawn(async move { model_files::describe(&path, 600) })
                        .await
                        .map_err(|error| format!("{error:#}"));
                }
                ModelFileAction::EditMetadata => {
                    let mut edits: Vec<MetadataEdit> = input
                        .set
                        .iter()
                        .map(|(key, value)| MetadataEdit::Set {
                            key: key.clone(),
                            value: value.clone(),
                        })
                        .collect();
                    edits.extend(
                        input
                            .remove
                            .iter()
                            .map(|key| MetadataEdit::Remove { key: key.clone() }),
                    );
                    if edits.is_empty() {
                        return Err("give keys to `set` or `remove`".to_string());
                    }
                    let destination = input
                        .output_path
                        .as_ref()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| model_files::default_edited_path(&path));
                    let described = format!(
                        "write an edited copy of {} to {}",
                        path.display(),
                        destination.display()
                    );
                    (
                        described,
                        Box::new(move |progress| {
                            model_files::edit_metadata(&path, &destination, &edits, progress)?;
                            Ok(format!(
                                "wrote {}\n\n{}",
                                destination.display(),
                                model_files::describe(&destination, 300)?
                            ))
                        }),
                    )
                }
                ModelFileAction::Import => {
                    let runner = match input.runner {
                        Some(ModelRunner::Ollama) => Runner::Ollama,
                        Some(ModelRunner::Lmstudio) => Runner::LmStudio,
                        None => return Err("choose a `runner`: ollama or lmstudio".to_string()),
                    };
                    let Some(name) = input.name.clone() else {
                        return Err("give the model a `name`".to_string());
                    };
                    let described = format!(
                        "import {} into {} as {name}",
                        path.display(),
                        if runner == Runner::Ollama {
                            "Ollama"
                        } else {
                            "LM Studio"
                        }
                    );
                    (
                        described,
                        Box::new(move |progress| {
                            model_files::import(&path, runner, &name, progress)
                        }),
                    )
                }
            };

            let authorize = cx.update(|cx| {
                let context =
                    crate::ToolPermissionContext::new(Self::NAME, vec![description.clone()]);
                event_stream.authorize(MarkdownInlineCode(&description).to_string(), context, cx)
            });
            futures::select! {
                result = authorize.fuse() => result.map_err(|error| error.to_string())?,
                _ = event_stream.cancelled_by_user().fuse() => {
                    return Err("cancelled".to_string());
                }
            };

            // Multi-gigabyte copies take minutes, so progress is shown in the
            // tool call's title as it goes.
            let (progress_sender, mut progress_updates) = mpsc::unbounded::<u32>();
            let job = cx.background_spawn(async move {
                let mut last_percent = u32::MAX;
                let mut report = move |fraction: f64| {
                    let percent = (fraction * 100.0).floor() as u32;
                    if percent != last_percent {
                        last_percent = percent;
                        progress_sender.unbounded_send(percent).ok();
                    }
                };
                work(&mut report)
            });
            let mut job = job.fuse();
            loop {
                futures::select! {
                    percent = progress_updates.next() => {
                        if let Some(percent) = percent {
                            event_stream.update_fields(
                                acp::ToolCallUpdateFields::new()
                                    .title(format!("{description} · {percent}%")),
                            );
                        }
                    }
                    result = job => {
                        return result.map_err(|error| format!("{error:#}"));
                    }
                }
            }
        })
    }
}
