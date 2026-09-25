//! Guards against hallucinated and look-alike dependencies. When shepherd
//! adds a package, noah checks that it exists in its registry, how old it is
//! and how widely it's used, and whether its name is one typo away from a
//! popular package, a common way attackers catch AI-written code.

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
        },
        Ecosystem::Npm => RegistryInfo {
            exists: json["name"].is_string(),
            created: json["time"]["created"].as_str().map(str::to_string),
            downloads: None,
            latest_version: json["dist-tags"]["latest"].as_str().map(str::to_string),
        },
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
            }
        }
    }
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
    Assessment {
        dependency: dependency.clone(),
        verdict,
        notes,
    }
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
