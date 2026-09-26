//! Add-ons: small JavaScript programs shepherd writes and noah keeps, so a
//! computation the person relies on runs the same, tested way every time
//! instead of being estimated by the model.
//!
//! An add-on is a folder holding `addon.toml`, `main.js` and `tests.json`.
//! Global add-ons live in `~/.noah/addons/<name>/`, project add-ons in
//! `<project>/.noah/addons/<name>/`; a project add-on hides a global one with
//! the same name. Every earlier version is kept in `versions/v<N>/`.

mod sandbox;
pub mod tool_api;

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use anyhow::{Context as _, Result, anyhow, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sha2::{Digest as _, Sha256};

pub use sandbox::{Execution, ExecutionError, MAX_INPUT_BYTES, MAX_OUTPUT_BYTES, execute};

pub const MANIFEST_FILE_NAME: &str = "addon.toml";
pub const TESTS_FILE_NAME: &str = "tests.json";
pub const VERSIONS_DIRECTORY_NAME: &str = "versions";
pub const DEFAULT_ENTRY: &str = "main.js";
pub const MAX_TEST_CASES: usize = 100;
pub const MAX_NAME_LENGTH: usize = 64;
pub const MAX_USE_WHEN_LENGTH: usize = 500;
pub const MAX_CODE_BYTES: usize = 256 * 1024;
pub const DEFAULT_CPU_MS: u64 = 2_000;
pub const MAX_CPU_MS: u64 = 10_000;
pub const MIN_CPU_MS: u64 = 10;
pub const DEFAULT_MEMORY_MB: u64 = 64;
pub const MAX_MEMORY_MB: u64 = 256;
// QuickJS needs a few megabytes for its own intrinsics before the script
// allocates anything.
pub const MIN_MEMORY_MB: u64 = 8;

pub fn global_addons_directory() -> PathBuf {
    paths::home_dir().join(".noah").join("addons")
}

pub fn project_addons_directory(project_root: &Path) -> PathBuf {
    project_root.join(".noah").join("addons")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Global,
    Project,
}

impl fmt::Display for Scope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Global => "global",
            Self::Project => "project",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Limits {
    pub cpu_ms: u64,
    pub memory_mb: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            cpu_ms: DEFAULT_CPU_MS,
            memory_mb: DEFAULT_MEMORY_MB,
        }
    }
}

impl Limits {
    fn validate(&self) -> Result<()> {
        if !(MIN_CPU_MS..=MAX_CPU_MS).contains(&self.cpu_ms) {
            bail!(
                "limits.cpu_ms is {}; it must be between {MIN_CPU_MS} and {MAX_CPU_MS}",
                self.cpu_ms
            );
        }
        if !(MIN_MEMORY_MB..=MAX_MEMORY_MB).contains(&self.memory_mb) {
            bail!(
                "limits.memory_mb is {}; it must be between {MIN_MEMORY_MB} and {MAX_MEMORY_MB}",
                self.memory_mb
            );
        }
        Ok(())
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PermissionsFile {
    #[serde(default)]
    network: Vec<String>,
    #[serde(default)]
    secrets: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    name: String,
    version: u32,
    use_when: String,
    #[serde(default)]
    entry: Option<String>,
    #[serde(default)]
    input: Option<toml::Table>,
    #[serde(default)]
    permissions: Option<PermissionsFile>,
    #[serde(default)]
    limits: Option<Limits>,
}

#[derive(Serialize)]
struct ManifestFileOut<'a> {
    name: &'a str,
    version: u32,
    use_when: &'a str,
    entry: &'a str,
    input: toml::Value,
    permissions: PermissionsFile,
    limits: Limits,
}

/// A parsed and validated `addon.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub name: String,
    pub version: u32,
    pub use_when: String,
    pub entry: String,
    /// The JSON schema a call's input must satisfy. `{}` accepts anything.
    pub input_schema: JsonValue,
    pub limits: Limits,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self> {
        let file: ManifestFile =
            toml::from_str(text).map_err(|error| anyhow!("addon.toml is invalid: {error}"))?;
        let permissions = file.permissions.unwrap_or_default();
        if !permissions.network.is_empty() {
            bail!(
                "addon.toml asks for network access ({}). noah doesn't grant add-ons network access yet; add-ons must be pure computation, so pass the data in as input instead",
                permissions.network.join(", ")
            );
        }
        if !permissions.secrets.is_empty() {
            bail!(
                "addon.toml asks for secrets ({}). noah doesn't give add-ons secrets yet; add-ons must be pure computation",
                permissions.secrets.join(", ")
            );
        }
        let input_schema = match file.input {
            Some(table) => serde_json::to_value(table).map_err(|error| {
                anyhow!("addon.toml [input] isn't a valid JSON schema: {error}")
            })?,
            None => JsonValue::Object(Default::default()),
        };
        let manifest = Self {
            name: file.name,
            version: file.version,
            use_when: file.use_when,
            entry: file.entry.unwrap_or_else(|| DEFAULT_ENTRY.to_string()),
            input_schema,
            limits: file.limits.unwrap_or_default(),
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<()> {
        validate_name(&self.name)?;
        if self.version == 0 {
            bail!("addon.toml version must be 1 or more");
        }
        let use_when = self.use_when.trim();
        if use_when.is_empty() {
            bail!("addon.toml use_when is empty; say when the add-on should be used");
        }
        if use_when.chars().count() > MAX_USE_WHEN_LENGTH {
            bail!("addon.toml use_when is longer than {MAX_USE_WHEN_LENGTH} characters");
        }
        validate_entry(&self.entry)?;
        self.limits.validate()?;
        compile_schema(&self.input_schema)?;
        Ok(())
    }

    pub fn to_toml(&self) -> Result<String> {
        let input = toml::Value::try_from(&self.input_schema).map_err(|error| {
            anyhow!("the input schema can't be written as TOML (it can't contain null): {error}")
        })?;
        if !input.is_table() {
            bail!("the input schema must be a JSON object");
        }
        let file = ManifestFileOut {
            name: &self.name,
            version: self.version,
            use_when: self.use_when.trim(),
            entry: &self.entry,
            input,
            permissions: PermissionsFile::default(),
            limits: self.limits,
        };
        toml::to_string(&file).context("writing addon.toml")
    }
}

/// Everything people make in noah is named `asherin.<name>`, add-ons
/// included. Takes a name with or without the prefix and gives it with.
pub fn canonical_name(name: &str) -> Result<String> {
    let slug = name.trim().strip_prefix(NAME_PREFIX).unwrap_or(name.trim());
    validate_slug(slug)?;
    Ok(format!("{NAME_PREFIX}{slug}"))
}

pub const NAME_PREFIX: &str = "asherin.";

pub fn validate_name(name: &str) -> Result<()> {
    validate_slug(name.strip_prefix(NAME_PREFIX).unwrap_or(name))
}

fn validate_slug(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= MAX_NAME_LENGTH
        && name.starts_with(|character: char| character.is_ascii_lowercase())
        && !name.ends_with('-')
        && name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        });
    if valid {
        Ok(())
    } else {
        Err(anyhow!(
            "`{name}` isn't a valid add-on name: use 1 to {MAX_NAME_LENGTH} lowercase letters, digits and hyphens, starting with a letter (for example `candidate-score`)"
        ))
    }
}

fn validate_entry(entry: &str) -> Result<()> {
    let plain_file_name = !entry.is_empty()
        && !entry.starts_with('.')
        && !entry.contains(['/', '\\', ':'])
        && entry.ends_with(".js");
    if plain_file_name {
        Ok(())
    } else {
        Err(anyhow!(
            "addon.toml entry `{entry}` must be a .js file name in the add-on's folder, such as `main.js`"
        ))
    }
}

fn compile_schema(schema: &JsonValue) -> Result<jsonschema::Validator> {
    if !schema.is_object() {
        bail!("the input schema must be a JSON object");
    }
    jsonschema::options()
        .with_pattern_options(jsonschema::PatternOptions::regex())
        .build(schema)
        .map_err(|error| anyhow!("the input schema is invalid: {error}"))
}

fn validate_input(validator: &jsonschema::Validator, input: &JsonValue) -> Result<(), String> {
    let problems: Vec<String> = validator
        .iter_errors(input)
        .take(10)
        .map(|error| {
            let location = error.instance_path().to_string();
            if location.is_empty() {
                error.to_string()
            } else {
                format!("at {location}: {error}")
            }
        })
        .collect();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "the input doesn't match the add-on's input schema: {}",
            problems.join("; ")
        ))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expectation {
    Output(JsonValue),
    /// The call must fail with an error containing this text.
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TestCase {
    pub name: Option<String>,
    pub input: JsonValue,
    pub expectation: Expectation,
}

impl TestCase {
    fn label(&self, index: usize) -> String {
        match &self.name {
            Some(name) => format!("#{} {name}", index + 1),
            None => format!("#{}", index + 1),
        }
    }
}

/// Parses `tests.json`: an array of `{"input": ..., "expect": ...}` or
/// `{"input": ..., "expect_error": "text"}` objects, each with an optional
/// `name`.
pub fn parse_tests(text: &str) -> Result<Vec<TestCase>> {
    let value: JsonValue = serde_json::from_str(text)
        .map_err(|error| anyhow!("tests.json isn't valid JSON: {error}"))?;
    parse_test_value(&value)
}

fn parse_test_value(value: &JsonValue) -> Result<Vec<TestCase>> {
    let Some(cases) = value.as_array() else {
        bail!("tests.json must be a JSON array of test cases");
    };
    if cases.is_empty() {
        bail!("an add-on needs at least one test case");
    }
    if cases.len() > MAX_TEST_CASES {
        bail!("an add-on can have at most {MAX_TEST_CASES} test cases");
    }
    cases
        .iter()
        .enumerate()
        .map(|(index, case)| {
            let position = index + 1;
            let Some(case) = case.as_object() else {
                bail!("test case #{position} must be an object");
            };
            if let Some(key) = case
                .keys()
                .find(|key| !matches!(key.as_str(), "name" | "input" | "expect" | "expect_error"))
            {
                bail!("test case #{position} has an unknown field `{key}`; use name, input, expect or expect_error");
            }
            let name = match case.get("name") {
                None => None,
                Some(JsonValue::String(name)) => Some(name.clone()),
                Some(_) => bail!("test case #{position}: `name` must be a string"),
            };
            let input = case
                .get("input")
                .cloned()
                .ok_or_else(|| anyhow!("test case #{position} has no `input`"))?;
            let expectation = match (case.get("expect"), case.get("expect_error")) {
                (Some(expected), None) => Expectation::Output(expected.clone()),
                (None, Some(JsonValue::String(text))) if !text.trim().is_empty() => {
                    Expectation::Error(text.clone())
                }
                (None, Some(_)) => {
                    bail!("test case #{position}: `expect_error` must be the non-empty text the error contains")
                }
                (Some(_), Some(_)) => {
                    bail!("test case #{position} has both `expect` and `expect_error`; use one")
                }
                (None, None) => bail!("test case #{position} needs `expect` or `expect_error`"),
            };
            Ok(TestCase {
                name,
                input,
                expectation,
            })
        })
        .collect()
}

/// Whether `actual` is the output `expected` describes. Numbers compare by
/// value, so `1` and `1.0` are equal, and object key order doesn't matter.
pub fn json_matches(expected: &JsonValue, actual: &JsonValue) -> bool {
    match (expected, actual) {
        (JsonValue::Number(expected), JsonValue::Number(actual)) => {
            match (expected.as_f64(), actual.as_f64()) {
                (Some(expected), Some(actual)) => expected == actual,
                _ => expected == actual,
            }
        }
        (JsonValue::Array(expected), JsonValue::Array(actual)) => {
            expected.len() == actual.len()
                && expected
                    .iter()
                    .zip(actual)
                    .all(|(expected, actual)| json_matches(expected, actual))
        }
        (JsonValue::Object(expected), JsonValue::Object(actual)) => {
            expected.len() == actual.len()
                && expected.iter().all(|(key, expected)| {
                    actual
                        .get(key)
                        .is_some_and(|actual| json_matches(expected, actual))
                })
        }
        (expected, actual) => expected == actual,
    }
}

/// An add-on's three files, parsed and validated.
#[derive(Debug, Clone)]
pub struct AddonSource {
    pub manifest: Manifest,
    pub manifest_text: String,
    pub code: String,
    pub tests: Vec<TestCase>,
    pub tests_text: String,
}

impl AddonSource {
    pub fn from_texts(manifest_text: String, code: String, tests_text: String) -> Result<Self> {
        let manifest = Manifest::parse(&manifest_text)?;
        if code.trim().is_empty() {
            bail!("{} is empty", manifest.entry);
        }
        if code.len() > MAX_CODE_BYTES {
            bail!("{} is larger than {MAX_CODE_BYTES} bytes", manifest.entry);
        }
        let tests = parse_tests(&tests_text)?;
        Ok(Self {
            manifest,
            manifest_text,
            code,
            tests,
            tests_text,
        })
    }

    /// Loads an add-on folder (or one of its `versions/v<N>` folders).
    pub fn load(directory: &Path) -> Result<Self> {
        let manifest_text = read_text(&directory.join(MANIFEST_FILE_NAME))?;
        let manifest = Manifest::parse(&manifest_text)?;
        let code = read_text(&directory.join(&manifest.entry))?;
        let tests_text = read_text(&directory.join(TESTS_FILE_NAME))?;
        Self::from_texts(manifest_text, code, tests_text)
    }

    fn content_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        for part in [&self.manifest_text, &self.code, &self.tests_text] {
            hasher.update((part.len() as u64).to_le_bytes());
            hasher.update(part.as_bytes());
        }
        hasher.finalize().into()
    }

    fn same_behavior_as(&self, other: &AddonSource) -> bool {
        self.manifest.use_when.trim() == other.manifest.use_when.trim()
            && self.manifest.input_schema == other.manifest.input_schema
            && self.manifest.limits == other.manifest.limits
            && self.code == other.code
            && self.tests == other.tests
    }
}

fn read_text(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaseResult {
    pub label: String,
    pub passed: bool,
    /// Why the case failed.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TestReport {
    pub cases: Vec<CaseResult>,
}

impl TestReport {
    pub fn total(&self) -> usize {
        self.cases.len()
    }

    pub fn passed(&self) -> usize {
        self.cases.iter().filter(|case| case.passed).count()
    }

    pub fn all_passed(&self) -> bool {
        !self.cases.is_empty() && self.passed() == self.total()
    }

    pub fn summary(&self) -> String {
        format!("tests: {} cases, {} passed", self.total(), self.passed())
    }

    /// The summary followed by one line per failing case.
    pub fn describe(&self) -> String {
        let mut text = self.summary();
        for case in self.cases.iter().filter(|case| !case.passed) {
            text.push_str(&format!(
                "\n- {} failed: {}",
                case.label,
                case.detail.as_deref().unwrap_or("no detail")
            ));
        }
        text
    }
}

/// Runs every case in `tests.json` through the same path a real call takes:
/// input validation, then the sandbox.
pub fn run_tests(source: &AddonSource) -> TestReport {
    let validator = match compile_schema(&source.manifest.input_schema) {
        Ok(validator) => validator,
        Err(error) => {
            return TestReport {
                cases: vec![CaseResult {
                    label: "input schema".into(),
                    passed: false,
                    detail: Some(format!("{error:#}")),
                }],
            };
        }
    };
    let cases = source
        .tests
        .iter()
        .enumerate()
        .map(|(index, case)| {
            let label = case.label(index);
            let result = validate_input(&validator, &case.input).and_then(|()| {
                execute(&source.code, &case.input, source.manifest.limits)
                    .map_err(|error| error.to_string())
            });
            let detail = match (&case.expectation, result) {
                (Expectation::Output(expected), Ok(execution)) => {
                    (!json_matches(expected, &execution.output)).then(|| {
                        format!(
                            "expected {}, got {}",
                            clip(&expected.to_string()),
                            clip(&execution.output.to_string())
                        )
                    })
                }
                (Expectation::Output(_), Err(error)) => Some(format!("error: {}", clip(&error))),
                (Expectation::Error(expected), Ok(execution)) => Some(format!(
                    "expected an error containing \"{expected}\", got {}",
                    clip(&execution.output.to_string())
                )),
                (Expectation::Error(expected), Err(error)) => (!error.contains(expected.as_str()))
                    .then(|| {
                        format!(
                            "expected an error containing \"{expected}\", got: {}",
                            clip(&error)
                        )
                    }),
            };
            CaseResult {
                label,
                passed: detail.is_none(),
                detail,
            }
        })
        .collect();
    TestReport { cases }
}

fn clip(text: &str) -> String {
    const LIMIT: usize = 300;
    if text.chars().count() <= LIMIT {
        text.to_string()
    } else {
        let clipped: String = text.chars().take(LIMIT).collect();
        format!("{clipped}…")
    }
}

// Content hashes of add-ons whose tests passed in this process, so a call
// doesn't re-run the whole suite every time.
static VERIFIED: LazyLock<Mutex<HashSet<[u8; 32]>>> = LazyLock::new(Default::default);

fn is_verified(hash: &[u8; 32]) -> bool {
    VERIFIED
        .lock()
        .map(|verified| verified.contains(hash))
        .unwrap_or(false)
}

fn mark_verified(hash: [u8; 32]) {
    if let Ok(mut verified) = VERIFIED.lock() {
        verified.insert(hash);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunError {
    /// The add-on's own tests don't pass, so it isn't used.
    TestsFailing(TestReport),
    InvalidInput(String),
    Failed(ExecutionError),
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TestsFailing(report) => write!(
                formatter,
                "the add-on's own tests don't pass, so noah won't run it until it's fixed. {}",
                report.describe()
            ),
            Self::InvalidInput(message) => write!(formatter, "{message}"),
            Self::Failed(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for RunError {}

/// Runs an add-on on `input`. Its tests must pass first (checked once per
/// content), and the input must satisfy its schema.
pub fn run(source: &AddonSource, input: &JsonValue) -> Result<Execution, RunError> {
    let hash = source.content_hash();
    if !is_verified(&hash) {
        let report = run_tests(source);
        if !report.all_passed() {
            return Err(RunError::TestsFailing(report));
        }
        mark_verified(hash);
    }
    let validator = compile_schema(&source.manifest.input_schema)
        .map_err(|error| RunError::InvalidInput(format!("{error:#}")))?;
    validate_input(&validator, input).map_err(RunError::InvalidInput)?;
    execute(&source.code, input, source.manifest.limits).map_err(RunError::Failed)
}

/// Where add-ons are looked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddonRoots {
    pub global: PathBuf,
    pub project: Option<PathBuf>,
}

impl AddonRoots {
    pub fn new(global: PathBuf, project_root: Option<&Path>) -> Self {
        Self {
            global,
            project: project_root.map(project_addons_directory),
        }
    }

    pub fn directory(&self, scope: Scope) -> Option<&Path> {
        match scope {
            Scope::Global => Some(&self.global),
            Scope::Project => self.project.as_deref(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstalledAddon {
    pub scope: Scope,
    pub directory: PathBuf,
    pub manifest: Manifest,
    /// A project add-on with the same name as a global one, which it hides.
    pub overrides_global: bool,
}

impl InstalledAddon {
    pub fn load_source(&self) -> Result<AddonSource> {
        let source = AddonSource::load(&self.directory)?;
        check_folder_name(&self.directory, &source.manifest)?;
        Ok(source)
    }

    /// Earlier versions kept in `versions/`, oldest first.
    pub fn earlier_versions(&self) -> Vec<u32> {
        archived_versions(&self.directory)
    }

    pub fn load_version(&self, version: u32) -> Result<AddonSource> {
        if version == self.manifest.version {
            return self.load_source();
        }
        let directory = version_directory(&self.directory, version);
        if !directory.is_dir() {
            bail!(
                "{} has no version {version}; versions: {}",
                self.manifest.name,
                self.version_list()
            );
        }
        AddonSource::load(&directory)
    }

    pub fn version_list(&self) -> String {
        let mut versions = self.earlier_versions();
        versions.push(self.manifest.version);
        versions
            .iter()
            .map(|version| format!("v{version}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn check_folder_name(directory: &Path, manifest: &Manifest) -> Result<()> {
    let folder = directory
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if folder != manifest.name {
        bail!(
            "addon.toml says the name is `{}` but the folder is `{folder}`; they must match",
            manifest.name
        );
    }
    Ok(())
}

fn version_directory(addon_directory: &Path, version: u32) -> PathBuf {
    addon_directory
        .join(VERSIONS_DIRECTORY_NAME)
        .join(format!("v{version}"))
}

fn archived_versions(addon_directory: &Path) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir(addon_directory.join(VERSIONS_DIRECTORY_NAME)) else {
        return Vec::new();
    };
    let mut versions: Vec<u32> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_prefix('v'))
                .and_then(|number| number.parse().ok())
        })
        .collect();
    versions.sort_unstable();
    versions
}

/// An add-on folder that couldn't be loaded.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub scope: Scope,
    pub directory: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Registry {
    /// Sorted by name; a project add-on replaces a global one of the same name.
    pub addons: Vec<InstalledAddon>,
    pub problems: Vec<Problem>,
}

impl Registry {
    pub fn get(&self, name: &str) -> Option<&InstalledAddon> {
        // Accepts the name with or without the asherin. prefix, and still
        // finds add-ons saved before the prefix existed.
        let slug = name.trim().strip_prefix(NAME_PREFIX).unwrap_or(name.trim());
        self.addons.iter().find(|addon| {
            addon.manifest.name.strip_prefix(NAME_PREFIX).unwrap_or(&addon.manifest.name) == slug
        })
    }

    /// Add-ons whose name or `use_when` share words with `query`, best first.
    pub fn search(&self, query: &str) -> Vec<&InstalledAddon> {
        let words: Vec<String> = query
            .split(|character: char| !character.is_alphanumeric())
            .filter(|word| word.len() > 2)
            .map(|word| word.to_lowercase())
            .collect();
        let mut scored: Vec<(usize, &InstalledAddon)> = self
            .addons
            .iter()
            .filter_map(|addon| {
                let haystack =
                    format!("{} {}", addon.manifest.name, addon.manifest.use_when).to_lowercase();
                let score = words
                    .iter()
                    .filter(|word| haystack.contains(word.as_str()))
                    .count();
                (score > 0).then_some((score, addon))
            })
            .collect();
        scored.sort_by(|left, right| right.0.cmp(&left.0));
        scored.into_iter().map(|(_, addon)| addon).collect()
    }
}

/// Finds the add-ons in the global and project folders.
pub fn discover(roots: &AddonRoots) -> Registry {
    let mut registry = Registry::default();
    let (global, global_problems) = scan(&roots.global, Scope::Global);
    registry.problems.extend(global_problems);
    let (project, project_problems) = match &roots.project {
        Some(directory) => scan(directory, Scope::Project),
        None => (Vec::new(), Vec::new()),
    };
    registry.problems.extend(project_problems);
    let project_names: HashSet<String> = project
        .iter()
        .map(|addon| addon.manifest.name.clone())
        .collect();
    let global_names: HashSet<String> = global
        .iter()
        .map(|addon| addon.manifest.name.clone())
        .collect();
    registry.addons.extend(
        global
            .into_iter()
            .filter(|addon| !project_names.contains(&addon.manifest.name)),
    );
    registry
        .addons
        .extend(project.into_iter().map(|addon| InstalledAddon {
            overrides_global: global_names.contains(&addon.manifest.name),
            ..addon
        }));
    registry
        .addons
        .sort_by(|left, right| left.manifest.name.cmp(&right.manifest.name));
    registry
}

fn scan(root: &Path, scope: Scope) -> (Vec<InstalledAddon>, Vec<Problem>) {
    let mut addons = Vec::new();
    let mut problems = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return (addons, problems);
    };
    let mut directories: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| !name.starts_with('.'))
        })
        .collect();
    directories.sort();
    for directory in directories {
        let result = read_text(&directory.join(MANIFEST_FILE_NAME))
            .and_then(|text| Manifest::parse(&text))
            .and_then(|manifest| {
                check_folder_name(&directory, &manifest)?;
                Ok(manifest)
            });
        match result {
            Ok(manifest) => addons.push(InstalledAddon {
                scope,
                directory,
                manifest,
                overrides_global: false,
            }),
            Err(error) => problems.push(Problem {
                scope,
                directory,
                message: format!("{error:#}"),
            }),
        }
    }
    (addons, problems)
}

/// What shepherd asks to save: new content for an add-on.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub name: String,
    pub use_when: String,
    pub input_schema: JsonValue,
    pub code: String,
    /// The `tests.json` array.
    pub tests: JsonValue,
    pub limits: Limits,
}

impl Draft {
    fn from_source(source: &AddonSource) -> Result<Self> {
        let tests: JsonValue = serde_json::from_str(&source.tests_text)?;
        Ok(Self {
            name: source.manifest.name.clone(),
            use_when: source.manifest.use_when.clone(),
            input_schema: source.manifest.input_schema.clone(),
            code: source.code.clone(),
            tests,
            limits: source.manifest.limits,
        })
    }
}

/// A save that has been validated and tested but not written yet, so the
/// person can review it first.
#[derive(Debug, Clone)]
pub struct PreparedSave {
    pub scope: Scope,
    pub directory: PathBuf,
    pub source: AddonSource,
    /// The version being replaced, when the add-on already exists.
    pub previous_version: Option<u32>,
    pub previous: Option<AddonSource>,
    pub report: TestReport,
    /// Set when this save restores an earlier version.
    pub restored_from: Option<u32>,
}

impl PreparedSave {
    pub fn is_unchanged(&self) -> bool {
        self.previous
            .as_ref()
            .is_some_and(|previous| previous.same_behavior_as(&self.source))
    }

    /// A unified diff of the code against the version being replaced.
    pub fn code_diff(&self) -> Option<String> {
        let previous = self.previous.as_ref()?;
        Some(unified_diff(&previous.code, &self.source.code))
    }

    /// Test cases whose input or expectation differ from the version being
    /// replaced, by label.
    pub fn changed_tests(&self) -> Vec<String> {
        let Some(previous) = &self.previous else {
            return Vec::new();
        };
        self.source
            .tests
            .iter()
            .enumerate()
            .filter(|(index, case)| previous.tests.get(*index) != Some(case))
            .map(|(index, case)| case.label(index))
            .collect()
    }
}

pub fn unified_diff(before: &str, after: &str) -> String {
    use imara_diff::{Algorithm, BasicLineDiffPrinter, Diff, InternedInput, UnifiedDiffConfig};
    let input = InternedInput::new(before, after);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    diff.unified_diff(
        &BasicLineDiffPrinter(&input.interner),
        UnifiedDiffConfig::default(),
        &input,
    )
    .to_string()
}

/// Validates a draft, gives it the next version number and runs its tests.
/// Nothing is written.
pub fn prepare_save(mut draft: Draft, scope: Scope, roots: &AddonRoots) -> Result<PreparedSave> {
    draft.name = canonical_name(&draft.name)?;
    let root = roots.directory(scope).ok_or_else(|| {
        anyhow!("there's no project folder open, so the add-on can't be saved to the project; save it globally instead")
    })?;
    let directory = root.join(&draft.name);
    let previous_version = installed_version(&directory)?;
    let previous = previous_version.and_then(|_| AddonSource::load(&directory).ok());
    let highest = archived_versions(&directory)
        .into_iter()
        .chain(previous_version)
        .max()
        .unwrap_or(0);
    let manifest = Manifest {
        name: draft.name,
        version: highest + 1,
        use_when: draft.use_when.trim().to_string(),
        entry: DEFAULT_ENTRY.to_string(),
        input_schema: draft.input_schema,
        limits: draft.limits,
    };
    manifest.validate()?;
    let tests_text = serde_json::to_string_pretty(&draft.tests)? + "\n";
    let source = AddonSource::from_texts(manifest.to_toml()?, draft.code, tests_text)?;
    let report = run_tests(&source);
    Ok(PreparedSave {
        scope,
        directory,
        source,
        previous_version,
        previous,
        report,
        restored_from: None,
    })
}

/// Prepares a save whose content is an earlier version of `addon`. The result
/// gets a new version number, so history only moves forward.
pub fn prepare_restore(
    addon: &InstalledAddon,
    version: u32,
    roots: &AddonRoots,
) -> Result<PreparedSave> {
    if version == addon.manifest.version {
        bail!("{} is already at v{version}", addon.manifest.name);
    }
    let old = addon.load_version(version)?;
    let mut prepared = prepare_save(Draft::from_source(&old)?, addon.scope, roots)?;
    prepared.restored_from = Some(version);
    Ok(prepared)
}

// The version in the installed addon.toml, even when the rest of that file is
// invalid, so a broken add-on can still be replaced and its files archived.
fn installed_version(directory: &Path) -> Result<Option<u32>> {
    let path = directory.join(MANIFEST_FILE_NAME);
    if !path.exists() {
        return Ok(None);
    }
    let text = read_text(&path)?;
    let version = toml::from_str::<toml::Table>(&text)
        .ok()
        .and_then(|table| {
            table
                .get("version")
                .and_then(|version| version.as_integer())
        })
        .and_then(|version| u32::try_from(version).ok())
        .unwrap_or(0);
    Ok(Some(version))
}

/// Writes a prepared save: the current files move to `versions/v<N>/` and
/// the new ones take their place. Refuses when the tests didn't pass or the
/// add-on changed since the save was prepared.
pub fn install(prepared: &PreparedSave) -> Result<InstalledAddon> {
    if !prepared.report.all_passed() {
        bail!(
            "not saved: the add-on's tests must all pass first. {}",
            prepared.report.describe()
        );
    }
    let current_version = installed_version(&prepared.directory)?;
    if current_version != prepared.previous_version {
        bail!(
            "not saved: {} changed while this save was waiting; prepare it again",
            prepared.source.manifest.name
        );
    }
    if let Some(version) = current_version {
        archive_current(&prepared.directory, version)?;
    }
    std::fs::create_dir_all(&prepared.directory)
        .with_context(|| format!("creating {}", prepared.directory.display()))?;
    write_file(
        &prepared.directory.join(&prepared.source.manifest.entry),
        &prepared.source.code,
    )?;
    write_file(
        &prepared.directory.join(TESTS_FILE_NAME),
        &prepared.source.tests_text,
    )?;
    // The manifest goes last: until it's written the folder still describes
    // the version being replaced.
    write_file(
        &prepared.directory.join(MANIFEST_FILE_NAME),
        &prepared.source.manifest_text,
    )?;
    mark_verified(prepared.source.content_hash());
    Ok(InstalledAddon {
        scope: prepared.scope,
        directory: prepared.directory.clone(),
        manifest: prepared.source.manifest.clone(),
        overrides_global: false,
    })
}

fn archive_current(directory: &Path, version: u32) -> Result<()> {
    let mut target = version_directory(directory, version);
    if target.exists() {
        // A version number can only repeat when files were edited by hand;
        // keep both rather than overwrite history.
        let mut attempt = 2;
        while target.exists() {
            target = directory
                .join(VERSIONS_DIRECTORY_NAME)
                .join(format!("v{version}-{attempt}"));
            attempt += 1;
        }
    }
    std::fs::create_dir_all(&target).with_context(|| format!("creating {}", target.display()))?;
    for entry in
        std::fs::read_dir(directory).with_context(|| format!("reading {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            std::fs::copy(&path, target.join(entry.file_name()))
                .with_context(|| format!("keeping {} as version {version}", path.display()))?;
        }
    }
    Ok(())
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("invalid path {}", path.display()))?;
    let temporary = path.with_file_name(format!(".{file_name}.tmp"));
    std::fs::write(&temporary, contents)
        .with_context(|| format!("writing {}", temporary.display()))?;
    std::fs::rename(&temporary, path).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests;
