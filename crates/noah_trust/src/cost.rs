//! Spend: estimates before a request is sent, projections before a larger
//! run, running totals per month and per thread, and the budgets the person
//! set. Prices come from the provider (Venice lists them with its models),
//! in US dollars per million tokens.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::path::Path;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Pricing {
    pub input_per_million: f64,
    pub output_per_million: f64,
}

impl Pricing {
    pub fn cost(&self, input_tokens: u64, output_tokens: u64) -> f64 {
        (input_tokens as f64 * self.input_per_million + output_tokens as f64 * self.output_per_million)
            / 1_000_000.0
    }
}

/// A rough token count for text not yet tokenized: about four characters
/// per token for English and code.
pub fn estimate_tokens(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Estimate {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub usd: f64,
}

/// What one more turn is likely to cost: the whole context is sent again,
/// and replies are assumed to run to a quarter of the context or 4k tokens,
/// whichever is smaller.
pub fn estimate_turn(context_tokens: u64, pricing: Pricing) -> Estimate {
    let output_tokens = (context_tokens / 4).clamp(256, 4_096);
    Estimate {
        input_tokens: context_tokens,
        output_tokens,
        usd: pricing.cost(context_tokens, output_tokens),
    }
}

/// How a multi-step run is assumed to go: every step sends the whole
/// context again, the context grows by what each step adds (tool output and
/// the reply), and each step replies with about the same number of tokens.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RunShape {
    pub steps: u64,
    pub growth_per_step: u64,
    pub output_per_step: u64,
}

impl RunShape {
    /// A quick run of `tasks` independent tasks: a few steps each, small
    /// tool outputs.
    pub fn light(tasks: u64) -> Self {
        Self {
            steps: tasks.max(1) * 4,
            growth_per_step: 1_500,
            output_per_step: 600,
        }
    }

    /// The same tasks when they need exploration, retries and long outputs.
    pub fn heavy(tasks: u64) -> Self {
        Self {
            steps: tasks.max(1) * 12,
            growth_per_step: 4_000,
            output_per_step: 1_500,
        }
    }
}

/// What a run shaped like `shape` is likely to cost, starting from a
/// context of `context_tokens`. `max_context_tokens` caps how large the
/// context gets, since noah compacts threads that reach the model's limit.
pub fn project_run(
    context_tokens: u64,
    max_context_tokens: Option<u64>,
    shape: RunShape,
    pricing: Pricing,
) -> Estimate {
    let cap = max_context_tokens.filter(|cap| *cap > 0).unwrap_or(u64::MAX);
    let mut input_tokens = 0u64;
    for step in 0..shape.steps {
        let context = context_tokens
            .saturating_add(step.saturating_mul(shape.growth_per_step))
            .min(cap);
        input_tokens = input_tokens.saturating_add(context);
    }
    let output_tokens = shape.steps.saturating_mul(shape.output_per_step);
    Estimate {
        input_tokens,
        output_tokens,
        usd: pricing.cost(input_tokens, output_tokens),
    }
}

/// "this plan will likely cost $0.40–$1.10 on opus", from a light and a
/// heavy projection. `pricing` is `None` for models without published
/// prices; `local` models run on this machine and cost nothing.
pub fn describe_projection(
    model: &str,
    context_tokens: u64,
    max_context_tokens: Option<u64>,
    tasks: u64,
    pricing: Option<Pricing>,
    local: bool,
) -> String {
    if local {
        return format!("this plan runs on {model}, a local model, so it costs nothing beyond electricity");
    }
    let Some(pricing) = pricing else {
        return format!("{model} doesn't publish prices, so noah can't project what this plan costs");
    };
    let low = project_run(context_tokens, max_context_tokens, RunShape::light(tasks), pricing);
    let high = project_run(context_tokens, max_context_tokens, RunShape::heavy(tasks), pricing);
    format!(
        "this plan will likely cost {}–{} on {model}",
        format_usd(low.usd),
        format_usd(high.usd)
    )
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spend {
    pub time: String,
    pub provider: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub usd: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
}

pub fn record(log: &Path, spend: &Spend) -> Result<()> {
    if let Some(directory) = log.parent() {
        std::fs::create_dir_all(directory)?;
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
    writeln!(file, "{}", serde_json::to_string(spend)?)?;
    Ok(())
}

pub fn read(log: &Path) -> Vec<Spend> {
    std::fs::read_to_string(log)
        .map(|text| text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect())
        .unwrap_or_default()
}

/// Dollars spent in the month `month` (`2026-09`).
pub fn month_total(entries: &[Spend], month: &str) -> f64 {
    entries
        .iter()
        .filter(|entry| entry.time.starts_with(month))
        .map(|entry| entry.usd)
        .sum()
}

/// Dollars spent in one thread, across months.
pub fn thread_total(entries: &[Spend], thread: &str) -> f64 {
    entries
        .iter()
        .filter(|entry| entry.thread.as_deref() == Some(thread))
        .map(|entry| entry.usd)
        .sum()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BudgetState {
    Unlimited,
    Within { remaining: f64 },
    /// Past 80% of the budget.
    Near { remaining: f64 },
    Exceeded { over: f64 },
}

pub fn budget_state(spent: f64, budget: Option<f64>, next: f64) -> BudgetState {
    let Some(budget) = budget.filter(|budget| *budget > 0.0) else {
        return BudgetState::Unlimited;
    };
    let after = spent + next;
    if after > budget {
        BudgetState::Exceeded { over: after - budget }
    } else if after > budget * 0.8 {
        BudgetState::Near { remaining: budget - after }
    } else {
        BudgetState::Within { remaining: budget - after }
    }
}

pub fn format_usd(amount: f64) -> String {
    // Summing no entries gives -0.0, which would print as "$-0.00".
    let amount = if amount.abs() < 1e-12 { 0.0 } else { amount };
    if amount < 0.01 && amount > 0.0 {
        format!("${amount:.4}")
    } else {
        format!("${amount:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_and_budgets() {
        let pricing = Pricing { input_per_million: 0.5, output_per_million: 2.0 };
        assert!((pricing.cost(1_000_000, 500_000) - 1.5).abs() < 1e-9);
        let estimate = estimate_turn(40_000, pricing);
        assert_eq!(estimate.output_tokens, 4_096);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
        assert_eq!(budget_state(5.0, None, 1.0), BudgetState::Unlimited);
        assert!(matches!(budget_state(5.0, Some(10.0), 1.0), BudgetState::Within { .. }));
        assert!(matches!(budget_state(8.0, Some(10.0), 0.5), BudgetState::Near { .. }));
        assert!(matches!(budget_state(9.9, Some(10.0), 0.5), BudgetState::Exceeded { .. }));
        assert_eq!(format_usd(0.004), "$0.0040");
    }

    #[test]
    fn totals_by_thread() {
        let entry = |thread: Option<&str>, usd: f64| Spend {
            time: "2026-09-01T00:00:00Z".into(),
            provider: "venice".into(),
            model: "m".into(),
            input_tokens: 0,
            output_tokens: 0,
            usd,
            thread: thread.map(str::to_string),
        };
        let entries = [entry(Some("a"), 0.25), entry(Some("b"), 1.0), entry(Some("a"), 0.5), entry(None, 3.0)];
        assert!((thread_total(&entries, "a") - 0.75).abs() < 1e-9);
        assert_eq!(thread_total(&entries, "c"), 0.0);
        assert!(matches!(budget_state(0.75, Some(1.0), 0.0), BudgetState::Within { .. }));
        assert!(matches!(budget_state(1.0, Some(1.0), 0.01), BudgetState::Exceeded { .. }));
    }

    #[test]
    fn projects_runs() {
        let pricing = Pricing { input_per_million: 5.0, output_per_million: 25.0 };
        let shape = RunShape { steps: 3, growth_per_step: 1_000, output_per_step: 100 };
        let estimate = project_run(10_000, None, shape, pricing);
        assert_eq!(estimate.input_tokens, 10_000 + 11_000 + 12_000);
        assert_eq!(estimate.output_tokens, 300);
        let capped = project_run(10_000, Some(10_500), shape, pricing);
        assert_eq!(capped.input_tokens, 10_000 + 10_500 + 10_500);

        let low = project_run(20_000, Some(200_000), RunShape::light(2), pricing);
        let high = project_run(20_000, Some(200_000), RunShape::heavy(2), pricing);
        assert!(low.usd < high.usd);
        let text = describe_projection("opus", 20_000, Some(200_000), 2, Some(pricing), false);
        assert_eq!(
            text,
            format!("this plan will likely cost {}–{} on opus", format_usd(low.usd), format_usd(high.usd))
        );
        assert!(describe_projection("qwen", 20_000, None, 2, None, true).contains("local model"));
        assert!(describe_projection("mystery", 20_000, None, 2, None, false).contains("doesn't publish prices"));
    }

    #[test]
    fn totals_by_month() {
        let entry = |time: &str, usd: f64| Spend {
            time: time.into(),
            provider: "venice".into(),
            model: "m".into(),
            input_tokens: 0,
            output_tokens: 0,
            usd,
            thread: None,
        };
        let entries = [entry("2026-09-01T00:00:00Z", 1.0), entry("2026-09-20T00:00:00Z", 2.5), entry("2026-08-31T00:00:00Z", 9.0)];
        assert!((month_total(&entries, "2026-09") - 3.5).abs() < 1e-9);
    }
}
