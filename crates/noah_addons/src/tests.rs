use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::json;

use super::*;
use crate::tool_api::{
    ListAddonsInput, RunAddonInput, SaveAddonInput, SaveRequest, render_list, render_refused,
    render_review, render_run_output, render_saved,
};

fn candidate_score_code(max_years: u32) -> String {
    format!(
        r#"export function run({{ candidates }}) {{
  const MAX_YEARS = {max_years};
  return candidates
    .map((c) => {{
      if (c.skills_required <= 0) throw new Error(`${{c.name}}: skills_required must be > 0`);
      const skills = (c.skills_matched / c.skills_required) * 40;
      const exp = (Math.min(c.years, MAX_YEARS) / MAX_YEARS) * 30;
      const interview = (c.interview / 10) * 30;
      return {{ name: c.name, score: +(skills + exp + interview).toFixed(1),
               breakdown: {{ skills: +skills.toFixed(1), experience: +exp.toFixed(1), interview: +interview.toFixed(1) }} }};
    }})
    .sort((a, b) => b.score - a.score);
}}
"#
    )
}

fn candidate_score_tests(max_years: u32) -> JsonValue {
    json!([
        {"name": "perfect", "input": {"candidates": [{"name":"a","skills_matched":9,"skills_required":9,"years":max_years,"interview":10}]},
         "expect": [{"name":"a","score":100,"breakdown":{"skills":40,"experience":30,"interview":30}}]},
        {"name": "zero required", "input": {"candidates": [{"name":"b","skills_matched":1,"skills_required":0,"years":1,"interview":5}]},
         "expect_error": "skills_required must be > 0"}
    ])
}

fn candidate_score_schema() -> JsonValue {
    json!({
        "type": "object",
        "required": ["candidates"],
        "properties": {
            "candidates": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["name", "skills_matched", "skills_required", "years", "interview"]
                }
            }
        }
    })
}

fn candidate_score_draft(max_years: u32) -> Draft {
    Draft {
        name: "candidate-score".into(),
        use_when: "ranking or scoring job candidates from skills, experience and interview results"
            .into(),
        input_schema: candidate_score_schema(),
        code: candidate_score_code(max_years),
        tests: candidate_score_tests(max_years),
        limits: Limits::default(),
    }
}

fn limits(cpu_ms: u64, memory_mb: u64) -> Limits {
    Limits { cpu_ms, memory_mb }
}

fn run_code(code: &str, input: JsonValue) -> Result<Execution, ExecutionError> {
    execute(code, &input, Limits::default())
}

fn thrown_message(result: Result<Execution, ExecutionError>) -> String {
    match result {
        Err(ExecutionError::Thrown(message)) => message,
        other => panic!("expected the script to throw, got {other:?}"),
    }
}

fn source_from_draft(draft: &Draft) -> AddonSource {
    let manifest = Manifest {
        name: draft.name.clone(),
        version: 1,
        use_when: draft.use_when.clone(),
        entry: DEFAULT_ENTRY.into(),
        input_schema: draft.input_schema.clone(),
        limits: draft.limits,
    };
    AddonSource::from_texts(
        manifest.to_toml().expect("manifest serializes"),
        draft.code.clone(),
        serde_json::to_string(&draft.tests).expect("tests serialize"),
    )
    .expect("valid add-on")
}

fn write_addon(root: &Path, folder: &str, manifest: &str) {
    let directory = root.join(folder);
    std::fs::create_dir_all(&directory).expect("create add-on folder");
    std::fs::write(directory.join(MANIFEST_FILE_NAME), manifest).expect("write manifest");
    std::fs::write(
        directory.join(DEFAULT_ENTRY),
        "export function run(input) { return input; }\n",
    )
    .expect("write code");
    std::fs::write(
        directory.join(TESTS_FILE_NAME),
        r#"[{"input": 1, "expect": 1}]"#,
    )
    .expect("write tests");
}

fn simple_manifest(name: &str, use_when: &str) -> String {
    format!("name = \"{name}\"\nversion = 1\nuse_when = \"{use_when}\"\n")
}

// Sandbox

#[test]
fn runs_a_module_and_returns_json() {
    let execution = run_code(
        "export function run({ a, b }) { return { sum: a + b, items: [a, b] }; }",
        json!({"a": 2, "b": 3}),
    )
    .expect("runs");
    assert_eq!(execution.output, json!({"sum": 5, "items": [2, 3]}));
    assert!(execution.logs.is_empty());
}

#[test]
fn awaits_an_async_run() {
    let execution = run_code(
        "export async function run(input) { const value = await Promise.resolve(input * 2); return value + 1; }",
        json!(20),
    )
    .expect("runs");
    assert_eq!(execution.output, json!(41));
}

#[test]
fn infinite_loop_is_stopped_at_the_time_limit() {
    let started = Instant::now();
    let result = execute(
        "export function run() { while (true) {} }",
        &json!(null),
        limits(200, 64),
    );
    assert_eq!(result, Err(ExecutionError::TimedOut { cpu_ms: 200 }));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "took {:?}",
        started.elapsed()
    );
}

#[test]
fn infinite_loop_that_catches_errors_is_still_stopped() {
    let result = execute(
        "export function run() { for (;;) { try { while (true) {} } catch (error) {} } }",
        &json!(null),
        limits(200, 64),
    );
    assert_eq!(result, Err(ExecutionError::TimedOut { cpu_ms: 200 }));
}

#[test]
fn infinite_loop_at_module_top_level_is_stopped() {
    let result = execute(
        "while (true) {}\nexport function run() { return 1; }",
        &json!(null),
        limits(200, 64),
    );
    assert_eq!(result, Err(ExecutionError::TimedOut { cpu_ms: 200 }));
}

#[test]
fn growing_allocation_hits_the_memory_limit() {
    let result = execute(
        "export function run() { const chunks = []; while (true) chunks.push(new Array(100000).fill(1.5)); }",
        &json!(null),
        limits(10_000, 16),
    );
    assert_eq!(result, Err(ExecutionError::OutOfMemory { memory_mb: 16 }));
}

#[test]
fn single_huge_allocation_hits_the_memory_limit() {
    let result = execute(
        "export function run() { return 'x'.repeat(500 * 1024 * 1024).length; }",
        &json!(null),
        limits(10_000, 16),
    );
    assert_eq!(result, Err(ExecutionError::OutOfMemory { memory_mb: 16 }));

    let result = execute(
        "export function run() { return new ArrayBuffer(200 * 1024 * 1024).byteLength; }",
        &json!(null),
        limits(10_000, 16),
    );
    assert_eq!(result, Err(ExecutionError::OutOfMemory { memory_mb: 16 }));
}

#[test]
fn deep_recursion_is_a_stack_overflow_not_a_crash() {
    let message = thrown_message(run_code(
        "function down(n) { return down(n + 1) + 1; }\nexport function run() { return down(0); }",
        json!(null),
    ));
    assert!(
        message.contains("Maximum call stack size exceeded"),
        "{message}"
    );
}

#[test]
fn has_no_host_access_globals() {
    let execution = run_code(
        r#"export function run() {
  const names = ["require", "fetch", "os", "std", "process", "Deno", "Bun", "XMLHttpRequest",
    "WebSocket", "setTimeout", "setInterval", "importScripts", "module", "exports", "scriptArgs", "print"];
  return Object.fromEntries(names.map((name) => [name, typeof globalThis[name]]));
}"#,
        json!(null),
    )
    .expect("runs");
    let object = execution.output.as_object().expect("an object");
    for (name, kind) in object {
        assert_eq!(
            kind, "undefined",
            "`{name}` should not exist in the sandbox"
        );
    }
}

#[test]
fn calling_require_or_fetch_throws() {
    let message = thrown_message(run_code(
        "export function run() { return require('fs'); }",
        json!(null),
    ));
    assert!(message.contains("require"), "{message}");
    let message = thrown_message(run_code(
        "export function run() { return fetch('https://example.com'); }",
        json!(null),
    ));
    assert!(message.contains("fetch"), "{message}");
}

#[test]
fn std_and_os_modules_cannot_be_imported() {
    for module in ["std", "os", "fs", "./other.js"] {
        let result = run_code(
            &format!(
                "import * as imported from \"{module}\";\nexport function run() {{ return typeof imported; }}"
            ),
            json!(null),
        );
        match result {
            Err(ExecutionError::Invalid(message)) | Err(ExecutionError::Thrown(message)) => {
                assert!(
                    message.contains("could not load module"),
                    "{module}: {message}"
                );
            }
            other => panic!("importing {module} should fail, got {other:?}"),
        }
    }
    let message = thrown_message(run_code(
        "export async function run() { const os = await import('os'); return typeof os; }",
        json!(null),
    ));
    assert!(message.contains("could not load module"), "{message}");
}

#[test]
fn thrown_errors_are_reported() {
    let message = thrown_message(run_code(
        "export function run() { throw new Error('amount must be positive'); }",
        json!(null),
    ));
    assert_eq!(message, "amount must be positive");
    let message = thrown_message(run_code(
        "export function run() { null.field; }",
        json!(null),
    ));
    assert!(message.starts_with("TypeError"), "{message}");
    let message = thrown_message(run_code(
        "export async function run() { throw new RangeError('too big'); }",
        json!(null),
    ));
    assert_eq!(message, "RangeError: too big");
}

#[test]
fn a_module_without_run_is_rejected() {
    let result = run_code("export function main() { return 1; }", json!(null));
    assert!(
        matches!(&result, Err(ExecutionError::Invalid(message)) if message.contains("export a function named `run`")),
        "{result:?}"
    );
    let result = run_code("export function run( {", json!(null));
    assert!(
        matches!(&result, Err(ExecutionError::Invalid(message)) if message.contains("main.js doesn't load")),
        "{result:?}"
    );
}

#[test]
fn non_json_results_are_rejected() {
    let result = run_code("export function run() { return undefined; }", json!(null));
    assert!(
        matches!(result, Err(ExecutionError::Invalid(_))),
        "{result:?}"
    );
    let result = run_code(
        "export function run() { return new Promise(() => {}); }",
        json!(null),
    );
    assert!(
        matches!(&result, Err(ExecutionError::Invalid(message)) if message.contains("never settles")),
        "{result:?}"
    );
}

#[test]
fn output_size_is_capped() {
    let result = run_code(
        "export function run() { return 'x'.repeat(2 * 1024 * 1024); }",
        json!(null),
    );
    assert!(
        matches!(result, Err(ExecutionError::OutputTooLarge { bytes }) if bytes > MAX_OUTPUT_BYTES),
        "{result:?}"
    );
}

#[test]
fn input_size_is_capped() {
    let input = json!("x".repeat(MAX_INPUT_BYTES + 1));
    let result = run_code("export function run(input) { return 1; }", input);
    assert!(
        matches!(result, Err(ExecutionError::InputTooLarge { .. })),
        "{result:?}"
    );
}

#[test]
fn console_output_is_collected_and_capped() {
    let execution = run_code(
        "export function run() { console.log('total', 3, { a: 1 }); console.warn('careful'); return 1; }",
        json!(null),
    )
    .expect("runs");
    assert_eq!(execution.logs, vec!["total 3 {\"a\":1}", "warn: careful"]);

    let execution = run_code(
        "export function run() { for (let i = 0; i < 10000; i++) console.log('line ' + i); return 1; }",
        json!(null),
    )
    .expect("runs");
    let total: usize = execution.logs.iter().map(String::len).sum();
    assert!(total < 20 * 1024, "logs are {total} bytes");
    assert!(
        execution
            .logs
            .last()
            .is_some_and(|line| line.ends_with("[logs truncated]"))
    );
}

#[test]
fn each_call_starts_from_a_fresh_runtime() {
    let code = "globalThis.count = (globalThis.count || 0) + 1;\nexport function run() { return globalThis.count; }";
    assert_eq!(run_code(code, json!(null)).expect("runs").output, json!(1));
    assert_eq!(run_code(code, json!(null)).expect("runs").output, json!(1));
}

// Manifest

const SPEC_MANIFEST: &str = r#"
name = "candidate-score"
version = 2
use_when = "ranking or scoring job candidates from skills, experience and interview results"
entry = "main.js"

[input]
type = "object"
required = ["candidates"]
properties.candidates = { type = "array", items = { type = "object", required = ["name","skills_matched","skills_required","years","interview"] } }

[permissions]
network = []
secrets = []

[limits]
cpu_ms = 2000
memory_mb = 64
"#;

#[test]
fn parses_the_spec_manifest() {
    let manifest = Manifest::parse(SPEC_MANIFEST).expect("valid manifest");
    assert_eq!(manifest.name, "candidate-score");
    assert_eq!(manifest.version, 2);
    assert_eq!(manifest.entry, "main.js");
    assert_eq!(manifest.limits, limits(2000, 64));
    assert_eq!(manifest.input_schema, candidate_score_schema());
}

#[test]
fn manifest_defaults() {
    let manifest = Manifest::parse(&simple_manifest("tip", "splitting a restaurant bill"))
        .expect("valid manifest");
    assert_eq!(manifest.entry, DEFAULT_ENTRY);
    assert_eq!(manifest.limits, Limits::default());
    assert_eq!(manifest.input_schema, json!({}));
}

#[test]
fn manifest_round_trips_through_toml() {
    let manifest = Manifest::parse(SPEC_MANIFEST).expect("valid manifest");
    let written = manifest.to_toml().expect("serializes");
    assert_eq!(Manifest::parse(&written).expect("reparses"), manifest);
}

#[test]
fn malformed_manifests_are_rejected() {
    let cases: &[(&str, &str)] = &[
        ("name = ", "invalid"),
        ("version = 1\nuse_when = \"x\"", "name"),
        ("name = \"a\"\nuse_when = \"x\"", "version"),
        ("name = \"a\"\nversion = 1", "use_when"),
        (
            "name = \"a\"\nversion = \"one\"\nuse_when = \"x\"",
            "invalid",
        ),
        (
            "name = \"a\"\nversion = 1\nuse_when = \"x\"\ncolour = \"red\"",
            "colour",
        ),
        (
            "name = \"Bad Name\"\nversion = 1\nuse_when = \"x\"",
            "valid add-on name",
        ),
        (
            "name = \"../escape\"\nversion = 1\nuse_when = \"x\"",
            "valid add-on name",
        ),
        (
            "name = \"trailing-\"\nversion = 1\nuse_when = \"x\"",
            "valid add-on name",
        ),
        (
            "name = \"a\"\nversion = 0\nuse_when = \"x\"",
            "version must be 1",
        ),
        (
            "name = \"a\"\nversion = 1\nuse_when = \"  \"",
            "use_when is empty",
        ),
        (
            "name = \"a\"\nversion = 1\nuse_when = \"x\"\nentry = \"../main.js\"",
            "entry",
        ),
        (
            "name = \"a\"\nversion = 1\nuse_when = \"x\"\nentry = \"main.py\"",
            "entry",
        ),
        (
            "name = \"a\"\nversion = 1\nuse_when = \"x\"\n[limits]\ncpu_ms = 60000",
            "cpu_ms",
        ),
        (
            "name = \"a\"\nversion = 1\nuse_when = \"x\"\n[limits]\nmemory_mb = 4096",
            "memory_mb",
        ),
        (
            "name = \"a\"\nversion = 1\nuse_when = \"x\"\n[limits]\nthreads = 4",
            "threads",
        ),
        (
            "name = \"a\"\nversion = 1\nuse_when = \"x\"\n[input]\ntype = 5",
            "input schema is invalid",
        ),
        (
            "name = \"a\"\nversion = 1\nuse_when = \"x\"\n[permissions]\nfilesystem = [\"/\"]",
            "filesystem",
        ),
    ];
    for (text, expected) in cases {
        let error = match Manifest::parse(text) {
            Ok(manifest) => panic!("accepted {text:?} as {manifest:?}"),
            Err(error) => format!("{error:#}"),
        };
        assert!(
            error.contains(expected),
            "parsing {text:?}: expected an error mentioning {expected:?}, got {error:?}"
        );
    }
}

#[test]
fn network_and_secret_permissions_are_refused_with_a_reason() {
    let error = Manifest::parse(
        "name = \"rates\"\nversion = 1\nuse_when = \"x\"\n[permissions]\nnetwork = [\"api.exchangerate.host\"]",
    )
    .expect_err("network is refused")
    .to_string();
    assert!(error.contains("network access"), "{error}");
    assert!(error.contains("api.exchangerate.host"), "{error}");

    let error = Manifest::parse(
        "name = \"rates\"\nversion = 1\nuse_when = \"x\"\n[permissions]\nsecrets = [\"API_KEY\"]",
    )
    .expect_err("secrets are refused")
    .to_string();
    assert!(error.contains("secrets"), "{error}");
}

// tests.json

#[test]
fn parses_test_cases() {
    let cases = parse_tests(
        r#"[{"name": "n", "input": {"a": 1}, "expect": null}, {"input": 2, "expect_error": "bad"}]"#,
    )
    .expect("valid tests");
    assert_eq!(
        cases,
        vec![
            TestCase {
                name: Some("n".into()),
                input: json!({"a": 1}),
                expectation: Expectation::Output(JsonValue::Null),
            },
            TestCase {
                name: None,
                input: json!(2),
                expectation: Expectation::Error("bad".into()),
            },
        ]
    );
}

#[test]
fn malformed_tests_are_rejected() {
    let cases: &[(&str, &str)] = &[
        ("{", "isn't valid JSON"),
        ("{}", "array"),
        ("[]", "at least one"),
        ("[1]", "must be an object"),
        (r#"[{"expect": 1}]"#, "no `input`"),
        (r#"[{"input": 1}]"#, "needs `expect` or `expect_error`"),
        (
            r#"[{"input": 1, "expect": 1, "expect_error": "x"}]"#,
            "both",
        ),
        (r#"[{"input": 1, "expect_error": ""}]"#, "non-empty"),
        (
            r#"[{"input": 1, "expected": 1}]"#,
            "unknown field `expected`",
        ),
    ];
    for (text, expected) in cases {
        let error = parse_tests(text).expect_err(text).to_string();
        assert!(error.contains(expected), "{text}: {error}");
    }
    let too_many =
        serde_json::to_string(&vec![json!({"input": 1, "expect": 1}); MAX_TEST_CASES + 1])
            .expect("serializes");
    assert!(parse_tests(&too_many).is_err());
}

#[test]
fn json_comparison_is_by_value() {
    assert!(json_matches(&json!(1), &json!(1.0)));
    assert!(json_matches(
        &json!({"a": 1, "b": [2]}),
        &json!({"b": [2.0], "a": 1})
    ));
    assert!(!json_matches(&json!({"a": 1}), &json!({"a": 1, "b": 2})));
    assert!(!json_matches(&json!([1, 2]), &json!([1])));
    assert!(!json_matches(&json!(0.3), &json!(0.30000000000000004)));
    assert!(!json_matches(&json!("1"), &json!(1)));
}

#[test]
fn passing_tests_pass() {
    let source = source_from_draft(&candidate_score_draft(8));
    let report = run_tests(&source);
    assert!(report.all_passed(), "{}", report.describe());
    assert_eq!(report.summary(), "tests: 2 cases, 2 passed");
}

#[test]
fn failing_tests_fail_with_details() {
    let mut draft = candidate_score_draft(8);
    draft.tests = json!([
        {"name": "wrong score", "input": {"candidates": [{"name":"a","skills_matched":9,"skills_required":9,"years":8,"interview":10}]},
         "expect": [{"name":"a","score":99,"breakdown":{"skills":40,"experience":30,"interview":30}}]},
        {"name": "no error", "input": {"candidates": []}, "expect_error": "empty"},
        {"name": "wrong error", "input": {"candidates": [{"name":"b","skills_matched":1,"skills_required":0,"years":1,"interview":5}]},
         "expect_error": "something else"},
        {"name": "schema", "input": {"people": []}, "expect": []},
        {"name": "ok", "input": {"candidates": []}, "expect": []}
    ]);
    let report = run_tests(&source_from_draft(&draft));
    assert_eq!(report.total(), 5);
    assert_eq!(report.passed(), 1);
    assert!(!report.all_passed());
    let details: Vec<&str> = report
        .cases
        .iter()
        .filter_map(|case| case.detail.as_deref())
        .collect();
    assert!(details[0].contains("expected [{"), "{}", details[0]);
    assert!(
        details[1].contains("expected an error containing \"empty\""),
        "{}",
        details[1]
    );
    assert!(
        details[2].contains("skills_required must be > 0"),
        "{}",
        details[2]
    );
    assert!(details[3].contains("input schema"), "{}", details[3]);
    assert!(report.describe().contains("#1 wrong score failed"));
}

#[test]
fn a_timeout_fails_a_test_instead_of_hanging() {
    let mut draft = candidate_score_draft(8);
    draft.code = "export function run() { while (true) {} }".into();
    draft.limits = limits(100, 64);
    let report = run_tests(&source_from_draft(&draft));
    assert_eq!(report.passed(), 0);
    assert!(
        report.cases[0]
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("timed out")),
        "{report:?}"
    );
}

#[test]
fn run_requires_passing_tests_and_valid_input() {
    let source = source_from_draft(&candidate_score_draft(8));
    let execution = run(
        &source,
        &json!({"candidates": [
            {"name":"ana","skills_matched":7,"skills_required":9,"years":4,"interview":8.5},
            {"name":"raj","skills_matched":9,"skills_required":9,"years":1,"interview":6}
        ]}),
    )
    .expect("runs");
    let names: Vec<&str> = execution
        .output
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|candidate| candidate["name"].as_str())
        .collect();
    assert_eq!(names, vec!["ana", "raj"]);

    let error = run(&source, &json!({"people": []})).expect_err("schema rejects it");
    assert!(
        matches!(&error, RunError::InvalidInput(message) if message.contains("candidates")),
        "{error}"
    );

    let mut broken = candidate_score_draft(8);
    broken.tests = json!([{"input": {"candidates": []}, "expect": ["something"]}]);
    let error = run(&source_from_draft(&broken), &json!({"candidates": []}))
        .expect_err("failing tests block the run");
    assert!(matches!(error, RunError::TestsFailing(_)), "{error}");
    assert!(error.to_string().contains("won't run it"));
}

// Versioning

fn roots_in(directory: &Path) -> AddonRoots {
    AddonRoots::new(directory.join("global"), Some(&directory.join("project")))
}

#[test]
fn saving_keeps_every_version() {
    let temporary = tempfile::tempdir().expect("temp dir");
    let roots = roots_in(temporary.path());

    let first = prepare_save(candidate_score_draft(10), Scope::Global, &roots).expect("prepares");
    assert_eq!(first.source.manifest.version, 1);
    assert!(first.report.all_passed(), "{}", first.report.describe());
    assert_eq!(first.previous_version, None);
    assert!(
        !first.directory.exists(),
        "preparing must not write anything"
    );
    let installed = install(&first).expect("installs");
    assert_eq!(installed.directory, roots.global.join("asherin.candidate-score"));
    for file in [MANIFEST_FILE_NAME, DEFAULT_ENTRY, TESTS_FILE_NAME] {
        assert!(installed.directory.join(file).is_file(), "{file} missing");
    }

    let second = prepare_save(candidate_score_draft(8), Scope::Global, &roots).expect("prepares");
    assert_eq!(second.source.manifest.version, 2);
    assert_eq!(second.previous_version, Some(1));
    let diff = second.code_diff().expect("has a diff");
    assert!(diff.contains("-  const MAX_YEARS = 10;"), "{diff}");
    assert!(diff.contains("+  const MAX_YEARS = 8;"), "{diff}");
    assert_eq!(second.changed_tests(), vec!["#1 perfect"]);
    let installed = install(&second).expect("installs");

    assert_eq!(installed.earlier_versions(), vec![1]);
    let current = installed.load_source().expect("loads");
    assert_eq!(current.manifest.version, 2);
    assert!(current.code.contains("MAX_YEARS = 8"));
    let old = installed.load_version(1).expect("v1 kept");
    assert_eq!(old.manifest.version, 1);
    assert!(old.code.contains("MAX_YEARS = 10"));
    assert!(installed.load_version(7).is_err());

    let restore = prepare_restore(&installed, 1, &roots).expect("prepares a restore");
    assert_eq!(restore.source.manifest.version, 3);
    assert_eq!(restore.restored_from, Some(1));
    assert!(restore.source.code.contains("MAX_YEARS = 10"));
    let installed = install(&restore).expect("restores");
    assert_eq!(installed.earlier_versions(), vec![1, 2]);
    assert_eq!(installed.version_list(), "v1, v2, v3");
    assert!(
        installed
            .load_version(2)
            .expect("v2 kept")
            .code
            .contains("MAX_YEARS = 8")
    );
    assert!(prepare_restore(&installed, 3, &roots).is_err());
}

#[test]
fn unchanged_saves_are_detected() {
    let temporary = tempfile::tempdir().expect("temp dir");
    let roots = roots_in(temporary.path());
    install(&prepare_save(candidate_score_draft(8), Scope::Global, &roots).expect("prepares"))
        .expect("installs");
    let again = prepare_save(candidate_score_draft(8), Scope::Global, &roots).expect("prepares");
    assert!(again.is_unchanged());
    let changed = prepare_save(candidate_score_draft(9), Scope::Global, &roots).expect("prepares");
    assert!(!changed.is_unchanged());
}

#[test]
fn failing_tests_are_never_saved() {
    let temporary = tempfile::tempdir().expect("temp dir");
    let roots = roots_in(temporary.path());
    let mut draft = candidate_score_draft(8);
    draft.tests = json!([{"input": {"candidates": []}, "expect": [1]}]);
    let prepared = prepare_save(draft, Scope::Global, &roots).expect("prepares");
    assert!(!prepared.report.all_passed());
    let error = install(&prepared).expect_err("refused").to_string();
    assert!(error.contains("tests must all pass"), "{error}");
    assert!(!prepared.directory.exists());
}

#[test]
fn a_save_prepared_before_another_one_is_refused() {
    let temporary = tempfile::tempdir().expect("temp dir");
    let roots = roots_in(temporary.path());
    let first = prepare_save(candidate_score_draft(8), Scope::Global, &roots).expect("prepares");
    let second = prepare_save(candidate_score_draft(9), Scope::Global, &roots).expect("prepares");
    install(&first).expect("installs");
    let error = install(&second).expect_err("stale").to_string();
    assert!(
        error.contains("changed while this save was waiting"),
        "{error}"
    );
}

#[test]
fn project_scope_needs_a_project() {
    let temporary = tempfile::tempdir().expect("temp dir");
    let roots = AddonRoots::new(temporary.path().join("global"), None);
    let error = prepare_save(candidate_score_draft(8), Scope::Project, &roots)
        .expect_err("no project")
        .to_string();
    assert!(error.contains("no project folder"), "{error}");
}

#[test]
fn invalid_drafts_are_rejected_before_testing() {
    let temporary = tempfile::tempdir().expect("temp dir");
    let roots = roots_in(temporary.path());
    let mut draft = candidate_score_draft(8);
    draft.name = "Candidate Score".into();
    assert!(prepare_save(draft, Scope::Global, &roots).is_err());
    let mut draft = candidate_score_draft(8);
    draft.input_schema = json!({"type": "object", "default": null});
    assert!(prepare_save(draft, Scope::Global, &roots).is_err());
    let mut draft = candidate_score_draft(8);
    draft.tests = json!([]);
    assert!(prepare_save(draft, Scope::Global, &roots).is_err());
    let mut draft = candidate_score_draft(8);
    draft.limits = limits(60_000, 64);
    assert!(prepare_save(draft, Scope::Global, &roots).is_err());
}

// Discovery

#[test]
fn discovers_global_and_project_addons_with_project_winning() {
    let temporary = tempfile::tempdir().expect("temp dir");
    let roots = roots_in(temporary.path());
    let project = roots.project.clone().expect("project root");
    write_addon(
        &roots.global,
        "tip",
        &simple_manifest("tip", "splitting a restaurant bill"),
    );
    write_addon(
        &roots.global,
        "shared",
        &simple_manifest("shared", "the global one"),
    );
    write_addon(
        &project,
        "shared",
        &simple_manifest("shared", "the project one"),
    );
    write_addon(
        &project,
        "vat",
        &simple_manifest("vat", "adding VAT to prices"),
    );

    let registry = discover(&roots);
    let names: Vec<(&str, Scope, bool)> = registry
        .addons
        .iter()
        .map(|addon| {
            (
                addon.manifest.name.as_str(),
                addon.scope,
                addon.overrides_global,
            )
        })
        .collect();
    assert_eq!(
        names,
        vec![
            ("shared", Scope::Project, true),
            ("tip", Scope::Global, false),
            ("vat", Scope::Project, false),
        ]
    );
    assert_eq!(
        registry.get("shared").expect("found").manifest.use_when,
        "the project one"
    );
    assert!(registry.problems.is_empty(), "{:?}", registry.problems);

    let without_project = discover(&AddonRoots::new(roots.global.clone(), None));
    assert_eq!(
        without_project
            .get("shared")
            .expect("found")
            .manifest
            .use_when,
        "the global one"
    );
    assert!(without_project.get("vat").is_none());
}

#[test]
fn discovery_reports_broken_folders_and_skips_hidden_ones() {
    let temporary = tempfile::tempdir().expect("temp dir");
    let roots = roots_in(temporary.path());
    write_addon(&roots.global, "good", &simple_manifest("good", "things"));
    write_addon(
        &roots.global,
        "renamed",
        &simple_manifest("other", "things"),
    );
    write_addon(&roots.global, "broken", "name = [");
    write_addon(
        &roots.global,
        ".hidden",
        &simple_manifest("hidden", "things"),
    );
    std::fs::create_dir_all(roots.global.join("empty")).expect("create folder");
    std::fs::write(roots.global.join("stray.txt"), "not an add-on").expect("write file");

    let registry = discover(&roots);
    assert_eq!(registry.addons.len(), 1);
    assert_eq!(registry.addons[0].manifest.name, "good");
    let mut problems: Vec<String> = registry
        .problems
        .iter()
        .map(|problem| {
            format!(
                "{}: {}",
                problem
                    .directory
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default(),
                problem.message
            )
        })
        .collect();
    problems.sort();
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(
        problems[0].starts_with("broken: addon.toml is invalid"),
        "{}",
        problems[0]
    );
    assert!(problems[1].starts_with("empty: reading"), "{}", problems[1]);
    assert!(
        problems[2].contains("folder is `renamed`"),
        "{}",
        problems[2]
    );
}

#[test]
fn discovery_of_missing_folders_is_empty() {
    let temporary = tempfile::tempdir().expect("temp dir");
    let registry = discover(&roots_in(&temporary.path().join("nowhere")));
    assert!(registry.addons.is_empty());
    assert!(registry.problems.is_empty());
}

#[test]
fn search_ranks_by_shared_words() {
    let temporary = tempfile::tempdir().expect("temp dir");
    let roots = roots_in(temporary.path());
    write_addon(
        &roots.global,
        "candidate-score",
        &simple_manifest("candidate-score", "ranking job candidates by skills"),
    );
    write_addon(
        &roots.global,
        "position-size",
        &simple_manifest("position-size", "position size from account and risk"),
    );
    let registry = discover(&roots);
    let found: Vec<&str> = registry
        .search("rank these job candidates")
        .iter()
        .map(|addon| addon.manifest.name.as_str())
        .collect();
    assert_eq!(found, vec!["candidate-score"]);
    assert!(registry.search("weather tomorrow").is_empty());
}

#[test]
fn a_hand_edited_project_addon_runs_after_its_tests_pass() {
    let temporary = tempfile::tempdir().expect("temp dir");
    let roots = roots_in(temporary.path());
    let project = roots.project.clone().expect("project root");
    write_addon(&project, "echo", &simple_manifest("echo", "echoing"));
    let registry = discover(&roots);
    let addon = registry.get("echo").expect("found");
    let source = addon.load_source().expect("loads");
    assert_eq!(
        run(&source, &json!({"x": 1})).expect("runs").output,
        json!({"x": 1})
    );
}

// Tool shapes

#[test]
fn save_tool_input_accepts_a_model_call() {
    let call = json!({
        "name": "candidate-score",
        "use_when": "ranking job candidates",
        "input_schema": candidate_score_schema(),
        "code": candidate_score_code(8),
        "tests": candidate_score_tests(8),
        "scope": "project",
        "cpu_ms": 500
    });
    let input: SaveAddonInput = serde_json::from_value(call).expect("deserializes");
    match input.into_request().expect("valid request") {
        SaveRequest::Draft { draft, scope } => {
            assert_eq!(scope, Some(Scope::Project));
            assert_eq!(draft.limits, limits(500, DEFAULT_MEMORY_MB));
            assert_eq!(draft.input_schema, candidate_score_schema());
            assert_eq!(draft.tests, candidate_score_tests(8));
        }
        other => panic!("expected a draft, got {other:?}"),
    }
}

#[test]
fn save_tool_input_accepts_stringified_json() {
    let call = json!({
        "name": "candidate-score",
        "use_when": "ranking job candidates",
        "input_schema": candidate_score_schema().to_string(),
        "code": candidate_score_code(8),
        "tests": candidate_score_tests(8).to_string()
    });
    let input: SaveAddonInput = serde_json::from_value(call).expect("deserializes");
    let SaveRequest::Draft { draft, scope } = input.into_request().expect("valid") else {
        panic!("expected a draft");
    };
    assert_eq!(scope, None);
    assert_eq!(draft.input_schema, candidate_score_schema());
    assert_eq!(draft.tests, candidate_score_tests(8));
}

#[test]
fn save_tool_input_restore_and_errors() {
    let input: SaveAddonInput =
        serde_json::from_value(json!({"name": "candidate-score", "restore_version": 1}))
            .expect("deserializes");
    assert_eq!(
        input.into_request().expect("valid"),
        SaveRequest::Restore {
            name: "candidate-score".into(),
            version: 1
        }
    );
    let input: SaveAddonInput =
        serde_json::from_value(json!({"name": "candidate-score", "code": "x"}))
            .expect("deserializes");
    let error = input.into_request().expect_err("incomplete").to_string();
    assert_eq!(error, "missing use_when, input_schema, tests");
    let input: SaveAddonInput = serde_json::from_value(
        json!({"name": "candidate-score", "restore_version": 1, "code": "x"}),
    )
    .expect("deserializes");
    assert!(input.into_request().is_err());
    let input: SaveAddonInput = serde_json::from_value(json!({
        "name": "a", "use_when": "x", "input_schema": "{not json", "code": "x", "tests": []
    }))
    .expect("deserializes");
    assert!(input.into_request().is_err());
}

#[test]
fn tool_schemas_describe_the_tools() {
    for schema in [
        serde_json::to_value(schemars::schema_for!(SaveAddonInput)).expect("schema"),
        serde_json::to_value(schemars::schema_for!(RunAddonInput)).expect("schema"),
        serde_json::to_value(schemars::schema_for!(ListAddonsInput)).expect("schema"),
    ] {
        assert!(
            schema["description"]
                .as_str()
                .is_some_and(|text| text.len() > 40),
            "{schema}"
        );
        assert!(schema["properties"].is_object(), "{schema}");
    }
    let schema = serde_json::to_value(schemars::schema_for!(SaveAddonInput)).expect("schema");
    assert_eq!(schema["required"], json!(["name"]));
    let schema = serde_json::to_value(schemars::schema_for!(RunAddonInput)).expect("schema");
    assert_eq!(schema["required"], json!(["name", "input"]));
}

#[test]
fn run_tool_input_unwraps_stringified_objects_only() {
    let input: RunAddonInput =
        serde_json::from_value(json!({"name": "a", "input": "{\"x\": 1}"})).expect("deserializes");
    assert_eq!(input.input_value(), json!({"x": 1}));
    let input: RunAddonInput =
        serde_json::from_value(json!({"name": "a", "input": "42"})).expect("deserializes");
    assert_eq!(input.input_value(), json!("42"));
    let input: RunAddonInput =
        serde_json::from_value(json!({"name": "a", "input": {"x": 1}})).expect("deserializes");
    assert_eq!(input.input_value(), json!({"x": 1}));
}

#[test]
fn tool_outputs_read_well() {
    let temporary = tempfile::tempdir().expect("temp dir");
    let roots = roots_in(temporary.path());

    assert!(render_list(&discover(&roots), None).starts_with("no add-ons are saved yet"));

    let first = prepare_save(candidate_score_draft(10), Scope::Global, &roots).expect("prepares");
    let review = render_review(&first);
    assert!(
        review.contains("**asherin.candidate-score** v1 (global)"),
        "{review}"
    );
    assert!(review.contains("permissions: none"), "{review}");
    assert!(review.contains("tests: 2 cases, 2 passed"), "{review}");
    assert!(review.contains("```js\nexport function run"), "{review}");
    let installed = install(&first).expect("installs");
    assert!(render_saved(&first).starts_with("saved asherin.candidate-score v1 (global)"));

    let second = prepare_save(candidate_score_draft(8), Scope::Global, &roots).expect("prepares");
    let review = render_review(&second);
    assert!(review.contains("changes to main.js since v1"), "{review}");
    assert!(review.contains("```diff"), "{review}");
    assert!(review.contains("changed tests: #1 perfect"), "{review}");
    assert!(render_saved(&second).starts_with("updated to asherin.candidate-score v2"));

    let execution = run(
        &installed.load_source().expect("loads"),
        &json!({"candidates": []}),
    )
    .expect("runs");
    let output = render_run_output(&installed, &execution);
    assert!(
        output.starts_with("ran asherin.candidate-score v1 (global, tested)"),
        "{output}"
    );
    assert!(output.contains("```json\n[]\n```"), "{output}");

    let list = render_list(&discover(&roots), None);
    assert!(
        list.contains("- asherin.candidate-score v1 (global): use when ranking"),
        "{list}"
    );
    let list = render_list(&discover(&roots), Some("weather"));
    assert!(list.starts_with("no saved add-on matches that"), "{list}");

    let mut failing = candidate_score_draft(8);
    failing.tests = json!([{"input": {"candidates": []}, "expect": [1]}]);
    let prepared = prepare_save(failing, Scope::Global, &roots).expect("prepares");
    let refused = render_refused(&prepared);
    assert!(refused.starts_with("not saved:"), "{refused}");
    assert!(refused.contains("#1 failed"), "{refused}");
}
