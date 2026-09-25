//! Spend: estimates before a request is sent, a running total per month, and
//! the budget the person set. Prices come from the provider (Venice lists
//! them with its models), in US dollars per million tokens.

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BudgetState {
    Unlimited,
    Within { remaining: f64 },
    /// Past 80% of the budget.
    Near { remaining: f64 },
    Exceeded { over: f64 },
}

pub fn budget_state(spent: f64, monthly_budget: Option<f64>, next: f64) -> BudgetState {
    let Some(budget) = monthly_budget.filter(|budget| *budget > 0.0) else {
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
