//! Whether shepherd's confidence can be trusted in this project. Each time
//! the person keeps or rejects a file shepherd changed, the confidence it
//! gave that file is recorded; comparing the two over time shows whether a
//! "0.9" really means right nine times out of ten here.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub time: String,
    pub path: String,
    pub confidence: f32,
    pub kept: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

pub fn record(log: &Path, outcome: &Outcome) -> Result<()> {
    if let Some(directory) = log.parent() {
        std::fs::create_dir_all(directory)?;
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
    writeln!(file, "{}", serde_json::to_string(outcome)?)?;
    Ok(())
}

pub fn read(log: &Path) -> Vec<Outcome> {
    std::fs::read_to_string(log)
        .map(|text| {
            text.lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bucket {
    pub low: f32,
    pub high: f32,
    pub count: usize,
    pub kept: usize,
}

impl Bucket {
    pub fn kept_rate(&self) -> Option<f32> {
        (self.count > 0).then(|| self.kept as f32 / self.count as f32)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    pub buckets: Vec<Bucket>,
    /// Mean squared gap between confidence and what happened (0 is perfect,
    /// 0.25 is no better than a coin).
    pub brier: Option<f32>,
    pub samples: usize,
}

impl Calibration {
    /// Whether a stated confidence has held up here: `Some(false)` when
    /// changes given this confidence were kept noticeably less often.
    pub fn trustworthy_at(&self, confidence: f32) -> Option<bool> {
        let bucket = self
            .buckets
            .iter()
            .find(|bucket| confidence >= bucket.low && confidence < bucket.high + f32::EPSILON)?;
        if bucket.count < 5 {
            return None;
        }
        let rate = bucket.kept_rate()?;
        Some(rate + 0.15 >= (bucket.low + bucket.high) / 2.0)
    }

    pub fn summary(&self) -> String {
        if self.samples == 0 {
            return "no kept or rejected changes recorded yet".to_string();
        }
        let mut out = format!("{} outcomes", self.samples);
        if let Some(brier) = self.brier {
            out.push_str(&format!(", Brier score {brier:.3}"));
        }
        for bucket in &self.buckets {
            if let Some(rate) = bucket.kept_rate() {
                out.push_str(&format!(
                    "; said {:.1}–{:.1}: kept {:.0}% of {}",
                    bucket.low,
                    bucket.high,
                    rate * 100.0,
                    bucket.count
                ));
            }
        }
        out
    }
}

pub fn calibrate(outcomes: &[Outcome]) -> Calibration {
    let edges = [0.0, 0.5, 0.7, 0.9, 1.0];
    let mut buckets: Vec<Bucket> = edges
        .windows(2)
        .map(|pair| Bucket {
            low: pair[0],
            high: pair[1],
            count: 0,
            kept: 0,
        })
        .collect();
    let mut squared_error = 0.0;
    for outcome in outcomes {
        let confidence = outcome.confidence.clamp(0.0, 1.0);
        let index = buckets
            .iter()
            .position(|bucket| confidence < bucket.high)
            .unwrap_or(buckets.len() - 1);
        if let Some(bucket) = buckets.get_mut(index) {
            bucket.count += 1;
            bucket.kept += usize::from(outcome.kept);
        }
        let actual = if outcome.kept { 1.0 } else { 0.0 };
        squared_error += (confidence - actual).powi(2);
    }
    Calibration {
        brier: (!outcomes.is_empty()).then(|| squared_error / outcomes.len() as f32),
        samples: outcomes.len(),
        buckets,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(confidence: f32, kept: bool) -> Outcome {
        Outcome {
            time: String::new(),
            path: "a.rs".into(),
            confidence,
            kept,
            model: None,
        }
    }

    #[test]
    fn overconfidence_shows() {
        let mut outcomes: Vec<Outcome> = (0..6).map(|index| outcome(0.95, index < 2)).collect();
        outcomes.extend((0..6).map(|index| outcome(0.3, index < 2)));
        let calibration = calibrate(&outcomes);
        assert_eq!(calibration.samples, 12);
        assert_eq!(calibration.trustworthy_at(0.95), Some(false));
        assert_eq!(calibration.trustworthy_at(0.3), Some(true));
        assert_eq!(calibration.trustworthy_at(0.6), None);
        assert!(calibration.summary().contains("kept 33% of 6"));
    }

    #[test]
    fn records_and_reads() {
        let directory = tempfile::tempdir().expect("tempdir");
        let log = directory.path().join("calibration.jsonl");
        record(&log, &outcome(0.8, true)).expect("record");
        record(&log, &outcome(0.4, false)).expect("record");
        assert_eq!(read(&log).len(), 2);
    }
}
