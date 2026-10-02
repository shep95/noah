//! What noah shield told noah about a site. The shield's popup has "tell
//! noah about this site"; the native host files the record here, and the
//! fetch and browser tools read it so shepherd sees the site the way the
//! shield did before it trusts it.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// A report is a few KB; a file past this is not one.
const MAX_REPORT_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Deserialize)]
pub struct Report {
    pub site: String,
    #[serde(default)]
    pub at: String,
    #[serde(default)]
    pub grade: Option<String>,
    #[serde(default)]
    pub score: Option<u32>,
    #[serde(default)]
    pub third_parties: Option<u32>,
    #[serde(default, rename = "thirdParties")]
    pub third_parties_camel: Option<u32>,
    #[serde(default)]
    pub advertising: Option<u32>,
    #[serde(default)]
    pub https: bool,
    #[serde(default)]
    pub lookalike: Option<Lookalike>,
    #[serde(default)]
    pub breaches: Option<Vec<Breach>>,
    #[serde(default)]
    pub stopped: Vec<Stopped>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Lookalike {
    #[serde(default)]
    pub brand: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Breach {
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub classes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Stopped {
    #[serde(default)]
    pub site: String,
    #[serde(default)]
    pub count: u32,
    #[serde(default)]
    pub owner: String,
}

/// A site name as a file name: only the characters a host name has.
pub fn file_name_for(site: &str) -> Option<String> {
    let site = site.trim().to_ascii_lowercase();
    if site.is_empty()
        || site.len() > 253
        || !site
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        || site.starts_with('.')
        || site.contains("..")
    {
        return None;
    }
    Some(format!("{site}.json"))
}

fn read(directory: &Path, site: &str) -> Option<Report> {
    let path = directory.join(file_name_for(site)?);
    let metadata = std::fs::metadata(&path).ok()?;
    if metadata.len() > MAX_REPORT_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// The report for a host, or for the nearest parent domain that has one:
/// `shop.example.co.uk` finds `example.co.uk`.
pub fn report_for_host(directory: &Path, host: &str) -> Option<Report> {
    let host = host.trim().to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').filter(|label| !label.is_empty()).collect();
    for start in 0..labels.len().saturating_sub(1) {
        let candidate = labels[start..].join(".");
        if let Some(report) = read(directory, &candidate) {
            return Some(report);
        }
    }
    None
}

pub fn directory() -> PathBuf {
    paths::shield_sites_directory()
}

/// One line for the model, from the person's own shield: what it graded the
/// site, who the site talks to, and whether it is a lookalike or has leaked.
pub fn note_for_host(host: &str) -> Option<String> {
    let report = report_for_host(&directory(), host)?;
    Some(describe(&report))
}

pub fn describe(report: &Report) -> String {
    let mut parts = Vec::new();
    if let Some(grade) = &report.grade {
        match report.score {
            Some(score) => parts.push(format!("graded {grade} ({score}/100)")),
            None => parts.push(format!("graded {grade}")),
        }
    }
    let third_parties = report.third_parties.or(report.third_parties_camel);
    if let Some(third_parties) = third_parties {
        match report.advertising {
            Some(advertising) if advertising > 0 => {
                parts.push(format!("{third_parties} third parties, {advertising} advertising"))
            }
            _ => parts.push(format!("{third_parties} third parties")),
        }
    }
    if !report.https {
        parts.push("no https".to_string());
    }
    if let Some(lookalike) = &report.lookalike
        && !lookalike.brand.is_empty()
    {
        parts.push(format!("looks like {} but is not", lookalike.brand));
    }
    if let Some(breaches) = &report.breaches
        && !breaches.is_empty()
    {
        let last = breaches
            .iter()
            .map(|breach| breach.date.get(..4).unwrap_or(""))
            .max()
            .unwrap_or("");
        parts.push(format!(
            "leaked its users' data {} time{}{}",
            breaches.len(),
            if breaches.len() == 1 { "" } else { "s" },
            if last.is_empty() { String::new() } else { format!(", last in {last}") }
        ));
    }
    let stopped: u32 = report.stopped.iter().map(|group| group.count).sum();
    if stopped > 0 {
        let names: Vec<&str> = report
            .stopped
            .iter()
            .take(4)
            .map(|group| group.site.as_str())
            .filter(|name| !name.is_empty())
            .collect();
        parts.push(format!("{stopped} tracker requests stopped ({})", names.join(", ")));
    }
    if parts.is_empty() {
        parts.push("no findings".to_string());
    }
    format!(
        "[noah shield on {}{}: {}]",
        report.site,
        if report.at.is_empty() { String::new() } else { format!(", {}", report.at.chars().take(10).collect::<String>()) },
        parts.join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_only_from_host_names() {
        assert_eq!(file_name_for("Example.COM").as_deref(), Some("example.com.json"));
        assert!(file_name_for("../etc/passwd").is_none());
        assert!(file_name_for("a/b").is_none());
        assert!(file_name_for("").is_none());
    }

    #[test]
    fn a_parent_domain_report_serves_its_subdomains() {
        let directory = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            directory.path().join("example.co.uk.json"),
            r#"{"site":"example.co.uk","at":"2026-10-02T00:00:00Z","grade":"D","score":41,"thirdParties":12,"advertising":7,"https":true,"lookalike":null,"breaches":[{"date":"2024-05-01","classes":["Email addresses"]}],"stopped":[{"site":"doubleclick.net","count":9,"owner":"Google"}]}"#,
        )
        .expect("write");
        let report = report_for_host(directory.path(), "shop.example.co.uk").expect("found");
        let note = describe(&report);
        assert!(note.contains("graded D (41/100)"), "{note}");
        assert!(note.contains("12 third parties, 7 advertising"), "{note}");
        assert!(note.contains("leaked its users' data 1 time, last in 2024"), "{note}");
        assert!(note.contains("9 tracker requests stopped (doubleclick.net)"), "{note}");
        assert!(report_for_host(directory.path(), "other.org").is_none());
    }
}
