use agent_client_protocol::schema::v1 as acp;
use anyhow::Result;
use gpui::{App, SharedString, Task};
use language_model::LanguageModelToolResultContent;
use noah_trust::cost::{self, Pricing, RunShape, Spend};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{AgentTool, ToolCallEventStream, ToolInput};

/// Before a big task, what it is likely to cost, and the person's yes to
/// that number. Call this before starting work you expect to take more than
/// a handful of steps (a refactor across files, a migration, a long
/// investigation). Give the number of separate tasks and what the work is.
/// The estimate comes from the model's price and from the requests this
/// machine has actually made (tokens per request seen so far), as a low and
/// a high figure. The person is then asked to agree to the high figure; if
/// they decline, do not start the work, and say so.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CostOfToolInput {
    /// What the work is, in one line, as the person will see it.
    pub what: String,
    /// How many separate tasks the work breaks into (at least 1).
    #[serde(default)]
    pub tasks: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CostOfToolOutput(pub String);

impl From<CostOfToolOutput> for LanguageModelToolResultContent {
    fn from(output: CostOfToolOutput) -> Self {
        LanguageModelToolResultContent::Text(output.0.into())
    }
}

/// What the spend log says a request to this model has looked like: the
/// mean input and output tokens over the last requests.
pub(crate) struct Observed {
    pub requests: usize,
    pub input_per_request: u64,
    pub output_per_request: u64,
}

pub(crate) fn observe(entries: &[Spend], model: &str) -> Option<Observed> {
    const WINDOW: usize = 200;
    let recent: Vec<&Spend> = entries
        .iter()
        .rev()
        .filter(|entry| entry.model == model)
        .take(WINDOW)
        .collect();
    if recent.is_empty() {
        return None;
    }
    let requests = recent.len();
    let input: u64 = recent.iter().map(|entry| entry.input_tokens).sum();
    let output: u64 = recent.iter().map(|entry| entry.output_tokens).sum();
    Some(Observed {
        requests,
        input_per_request: input / requests as u64,
        output_per_request: output / requests as u64,
    })
}

pub(crate) struct Projection {
    pub low: f64,
    pub high: f64,
    pub basis: String,
}

/// The low figure is the library's light run from the model's current
/// context; the high figure is the heavier of the library's heavy run and
/// the observed average request repeated for every step.
pub(crate) fn project(
    tasks: u64,
    context_tokens: u64,
    max_tokens: Option<u64>,
    pricing: Pricing,
    observed: Option<&Observed>,
) -> Projection {
    let light = cost::project_run(context_tokens, max_tokens, RunShape::light(tasks), pricing);
    let heavy = cost::project_run(context_tokens, max_tokens, RunShape::heavy(tasks), pricing);
    let mut high = heavy.usd;
    let mut basis = format!(
        "from the model's price and a context of about {} tokens, {} to {} steps",
        context_tokens,
        RunShape::light(tasks).steps,
        RunShape::heavy(tasks).steps
    );
    if let Some(observed) = observed {
        let steps = RunShape::heavy(tasks).steps;
        let observed_usd = pricing.cost(
            observed.input_per_request.saturating_mul(steps),
            observed.output_per_request.saturating_mul(steps),
        );
        if observed_usd > high {
            high = observed_usd;
        }
        basis.push_str(&format!(
            "; the last {} requests to this model averaged {} tokens in and {} out",
            observed.requests, observed.input_per_request, observed.output_per_request
        ));
    }
    Projection {
        low: light.usd.min(high),
        high,
        basis,
    }
}

pub struct CostOfTool;

impl AgentTool for CostOfTool {
    type Input = CostOfToolInput;
    type Output = CostOfToolOutput;

    const NAME: &'static str = "cost_of";

    fn kind() -> acp::ToolKind {
        acp::ToolKind::Think
    }

    fn initial_title(
        &self,
        input: Result<Self::Input, serde_json::Value>,
        _cx: &mut App,
    ) -> SharedString {
        match input {
            Ok(input) => format!("cost of: {}", input.what).into(),
            Err(_) => "cost of".into(),
        }
    }

    fn run(
        self: Arc<Self>,
        input: ToolInput<Self::Input>,
        event_stream: ToolCallEventStream,
        cx: &mut App,
    ) -> Task<Result<Self::Output, Self::Output>> {
        cx.spawn(async move |cx| {
            let input = input
                .recv()
                .await
                .map_err(|error| CostOfToolOutput(error.to_string()))?;
            let what = input.what.trim().to_string();
            if what.is_empty() {
                return Err(CostOfToolOutput("say what the work is".to_string()));
            }
            let tasks = input.tasks.unwrap_or(1).clamp(1, 500);
            let model = cx
                .update(|cx| {
                    event_stream
                        .thread_entity_id()
                        .and_then(|thread| crate::trust::thread_model(thread, cx))
                })
                .ok_or_else(|| {
                    CostOfToolOutput(
                        "no model has answered in this thread yet, so there is nothing to project from"
                            .to_string(),
                    )
                })?;
            if model.local {
                return Ok(CostOfToolOutput(format!(
                    "{} runs on this machine, so \"{what}\" costs nothing beyond electricity; go ahead",
                    model.name
                )));
            }
            let Some(pricing) = model.pricing else {
                return Ok(CostOfToolOutput(format!(
                    "{} doesn't publish prices, so noah can't put a number on \"{what}\"; tell the person that before starting",
                    model.name
                )));
            };
            let log = crate::trust::spend_log_path();
            let model_name = model.name.clone();
            let observed = cx
                .background_executor()
                .spawn(async move { observe(&cost::read(&log), &model_name) })
                .await;
            let projection = project(
                tasks,
                model.context_tokens,
                Some(model.max_tokens),
                pricing,
                observed.as_ref(),
            );
            let high = cost::format_usd(projection.high);
            let low = cost::format_usd(projection.low);
            let title = format!("spend up to {high} on: {what}");
            let authorize = cx.update(|cx| {
                let context = crate::ToolPermissionContext::new(
                    Self::NAME,
                    vec![what.clone(), high.clone()],
                );
                event_stream.authorize_always_prompt(title, context, cx)
            });
            match authorize.await {
                Ok(()) => Ok(CostOfToolOutput(format!(
                    "\"{what}\" will likely cost {low}–{high} on {} ({}). the person agreed to up to {high}; stop and ask again if the work looks like it will pass that.",
                    model.name, projection.basis
                ))),
                Err(error) => Err(CostOfToolOutput(format!(
                    "the person did not agree to spend up to {high} on \"{what}\" ({error}); do not start it",
                ))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spend(model: &str, input: u64, output: u64) -> Spend {
        Spend {
            time: "2026-10-02T00:00:00Z".into(),
            provider: "p".into(),
            model: model.into(),
            input_tokens: input,
            output_tokens: output,
            usd: 0.0,
            thread: None,
        }
    }

    #[test]
    fn observed_averages_only_this_model() {
        let entries = vec![
            spend("a", 1000, 100),
            spend("b", 9000, 900),
            spend("a", 3000, 300),
        ];
        let observed = observe(&entries, "a").expect("some");
        assert_eq!(observed.requests, 2);
        assert_eq!(observed.input_per_request, 2000);
        assert_eq!(observed.output_per_request, 200);
        assert!(observe(&entries, "c").is_none());
    }

    #[test]
    fn the_high_figure_never_sits_below_what_was_seen() {
        let pricing = Pricing {
            input_per_million: 10.0,
            output_per_million: 30.0,
        };
        let seen = Observed {
            requests: 50,
            input_per_request: 100_000,
            output_per_request: 2_000,
        };
        let with = project(2, 5_000, Some(200_000), pricing, Some(&seen));
        let without = project(2, 5_000, Some(200_000), pricing, None);
        assert!(with.high >= without.high);
        assert!(with.low <= with.high);
        assert!(with.basis.contains("last 50 requests"));
    }
}
