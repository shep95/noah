use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use rquickjs::{Array, CatchResultExt as _, CaughtError, Context, Ctx, Module, Runtime, Value};

use crate::Limits;

pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_LOG_BYTES: usize = 16 * 1024;
const JS_STACK_BYTES: usize = 1024 * 1024;
const THREAD_STACK_BYTES: usize = 8 * 1024 * 1024;
// A few native builtins (for example `indexOf` on a huge sparse array) loop in
// C without reaching QuickJS's interrupt check. The caller stops waiting a
// little after the script's own deadline so such a script can't hang a turn.
const WATCHDOG_GRACE: Duration = Duration::from_secs(2);

// Gives add-ons a `console` whose output is collected (and capped) instead of
// written anywhere. Evaluates to the array of collected lines.
const PRELUDE: &str = r#"(() => {
  const lines = [];
  let size = 0;
  const limit = LIMIT;
  const show = (value) => {
    if (typeof value === "string") return value;
    try {
      const text = JSON.stringify(value);
      return text === undefined ? String(value) : text;
    } catch (error) {
      return String(value);
    }
  };
  const writer = (prefix) => (...values) => {
    if (size >= limit) return;
    let line = prefix + values.map(show).join(" ");
    if (size + line.length > limit) line = line.slice(0, limit - size) + " [logs truncated]";
    size += line.length;
    lines.push(line);
  };
  globalThis.console = {
    log: writer(""),
    info: writer(""),
    debug: writer(""),
    warn: writer("warn: "),
    error: writer("error: "),
  };
  return lines;
})()"#;

/// The result of one successful call to an add-on's `run`.
#[derive(Debug, Clone, PartialEq)]
pub struct Execution {
    pub output: serde_json::Value,
    pub logs: Vec<String>,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionError {
    /// The script threw, or `run` returned a promise that rejected.
    Thrown(String),
    TimedOut {
        cpu_ms: u64,
    },
    OutOfMemory {
        memory_mb: u64,
    },
    InputTooLarge {
        bytes: usize,
    },
    OutputTooLarge {
        bytes: usize,
    },
    /// The script couldn't be loaded, doesn't export `run`, or returned
    /// something that isn't JSON.
    Invalid(String),
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Thrown(message) => write!(formatter, "{message}"),
            Self::TimedOut { cpu_ms } => {
                write!(
                    formatter,
                    "timed out: the add-on ran longer than its {cpu_ms} ms limit"
                )
            }
            Self::OutOfMemory { memory_mb } => write!(
                formatter,
                "out of memory: the add-on used more than its {memory_mb} MB limit"
            ),
            Self::InputTooLarge { bytes } => write!(
                formatter,
                "the input is {bytes} bytes; add-on input is limited to {MAX_INPUT_BYTES} bytes"
            ),
            Self::OutputTooLarge { bytes } => write!(
                formatter,
                "the output is {bytes} bytes; add-on output is limited to {MAX_OUTPUT_BYTES} bytes"
            ),
            Self::Invalid(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for ExecutionError {}

/// Runs `code` (an ES module exporting `run`) with `input` in a fresh QuickJS
/// runtime.
///
/// The runtime is created without QuickJS's `std` and `os` modules and
/// without a module loader, and nothing from the host is exposed except a
/// `console` that collects lines. So the script has no filesystem, network,
/// process or clock-sleeping access; it can only compute. Time, memory, stack
/// and output size are capped.
pub fn execute(
    code: &str,
    input: &serde_json::Value,
    limits: Limits,
) -> Result<Execution, ExecutionError> {
    let input_json = serde_json::to_string(input)
        .map_err(|error| ExecutionError::Invalid(format!("the input isn't valid JSON: {error}")))?;
    if input_json.len() > MAX_INPUT_BYTES {
        return Err(ExecutionError::InputTooLarge {
            bytes: input_json.len(),
        });
    }
    let started = Instant::now();
    let (sender, receiver) = mpsc::sync_channel(1);
    let code = code.to_string();
    std::thread::Builder::new()
        .name("noah-addon".into())
        .stack_size(THREAD_STACK_BYTES)
        .spawn(move || {
            let result = execute_on_current_thread(&code, &input_json, limits);
            if sender.send(result).is_err() {
                // The caller already gave up after the watchdog deadline.
            }
        })
        .map_err(|error| {
            ExecutionError::Invalid(format!("couldn't start the add-on sandbox: {error}"))
        })?;
    let wait = Duration::from_millis(limits.cpu_ms) + WATCHDOG_GRACE;
    match receiver.recv_timeout(wait) {
        Ok(result) => result.map(|execution| Execution {
            elapsed: started.elapsed(),
            ..execution
        }),
        Err(RecvTimeoutError::Timeout) => Err(ExecutionError::TimedOut {
            cpu_ms: limits.cpu_ms,
        }),
        Err(RecvTimeoutError::Disconnected) => Err(ExecutionError::Invalid(
            "the add-on sandbox stopped unexpectedly".into(),
        )),
    }
}

enum Failure {
    Script(String),
    Invalid(String),
    OutputTooLarge(usize),
}

fn execute_on_current_thread(
    code: &str,
    input_json: &str,
    limits: Limits,
) -> Result<Execution, ExecutionError> {
    let setup_error = |error: rquickjs::Error| {
        ExecutionError::Invalid(format!("the sandbox failed to start: {error}"))
    };
    let runtime = Runtime::new().map_err(setup_error)?;
    let memory_bytes =
        usize::try_from(limits.memory_mb.saturating_mul(1024 * 1024)).unwrap_or(usize::MAX);
    runtime.set_memory_limit(memory_bytes);
    runtime.set_max_stack_size(JS_STACK_BYTES);
    let deadline = Instant::now() + Duration::from_millis(limits.cpu_ms);
    let timed_out = Arc::new(AtomicBool::new(false));
    runtime.set_interrupt_handler(Some(Box::new({
        let timed_out = timed_out.clone();
        move || {
            if Instant::now() >= deadline {
                timed_out.store(true, Ordering::Relaxed);
                true
            } else {
                false
            }
        }
    })));
    let context = Context::full(&runtime).map_err(setup_error)?;
    let result = context.with(|ctx| run_in_context(&ctx, code, input_json));
    let failure = match result {
        Ok((output, logs)) => {
            return Ok(Execution {
                output,
                logs,
                elapsed: Duration::ZERO,
            });
        }
        Err(failure) => failure,
    };
    if timed_out.load(Ordering::Relaxed) {
        return Err(ExecutionError::TimedOut {
            cpu_ms: limits.cpu_ms,
        });
    }
    Err(match failure {
        Failure::Script(message) if is_out_of_memory(&message) => ExecutionError::OutOfMemory {
            memory_mb: limits.memory_mb,
        },
        Failure::Invalid(message) if is_out_of_memory(&message) => ExecutionError::OutOfMemory {
            memory_mb: limits.memory_mb,
        },
        Failure::Script(message) => ExecutionError::Thrown(message),
        Failure::Invalid(message) => ExecutionError::Invalid(message),
        Failure::OutputTooLarge(bytes) => ExecutionError::OutputTooLarge { bytes },
    })
}

fn is_out_of_memory(message: &str) -> bool {
    let message = message.to_lowercase();
    message.contains("out of memory") || message.contains("allocation failed")
}

fn run_in_context(
    ctx: &Ctx<'_>,
    code: &str,
    input_json: &str,
) -> Result<(serde_json::Value, Vec<String>), Failure> {
    let prelude = PRELUDE.replace("LIMIT", &MAX_LOG_BYTES.to_string());
    let logs: Array = ctx
        .eval(prelude)
        .catch(ctx)
        .map_err(|error| Failure::Invalid(describe(error)))?;

    let module = Module::declare(ctx.clone(), "main.js", code)
        .catch(ctx)
        .map_err(|error| Failure::Invalid(format!("main.js doesn't load: {}", describe(error))))?;
    let (module, loaded) = module
        .eval()
        .catch(ctx)
        .map_err(|error| Failure::Script(describe(error)))?;
    loaded
        .finish::<()>()
        .catch(ctx)
        .map_err(|error| Failure::Script(describe(error)))?;

    let run: Value = module
        .get("run")
        .catch(ctx)
        .map_err(|error| Failure::Invalid(describe(error)))?;
    let Some(run) = run.as_function() else {
        return Err(Failure::Invalid(
            "main.js must export a function named `run`: `export function run(input) { ... }`"
                .into(),
        ));
    };
    let input = ctx
        .json_parse(input_json)
        .catch(ctx)
        .map_err(|error| Failure::Invalid(describe(error)))?;
    let mut result: Value = run
        .call((input,))
        .catch(ctx)
        .map_err(|error| Failure::Script(describe(error)))?;
    if let Some(promise) = result.as_promise() {
        result = promise
            .finish::<Value>()
            .catch(ctx)
            .map_err(|error| match error {
                CaughtError::Error(rquickjs::Error::WouldBlock) => Failure::Invalid(
                    "`run` returned a promise that never settles; add-ons can't wait on anything"
                        .into(),
                ),
                error => Failure::Script(describe(error)),
            })?;
    }
    let json = ctx
        .json_stringify(result)
        .catch(ctx)
        .map_err(|error| Failure::Script(describe(error)))?
        .ok_or_else(|| {
            Failure::Invalid("`run` returned undefined or a function; return a JSON value".into())
        })?
        .to_string()
        .map_err(|error| Failure::Invalid(error.to_string()))?;
    if json.len() > MAX_OUTPUT_BYTES {
        return Err(Failure::OutputTooLarge(json.len()));
    }
    let output = serde_json::from_str(&json)
        .map_err(|error| Failure::Invalid(format!("`run` returned invalid JSON: {error}")))?;

    let mut lines = Vec::new();
    for line in logs.iter::<String>() {
        lines.push(line.map_err(|error| Failure::Invalid(error.to_string()))?);
    }
    Ok((output, lines))
}

fn describe(error: CaughtError<'_>) -> String {
    match error {
        CaughtError::Exception(exception) => {
            let name: Option<String> = exception.get("name").ok();
            let message = exception.message().unwrap_or_default();
            match name {
                Some(name) if !name.is_empty() && name != "Error" => format!("{name}: {message}"),
                _ => message,
            }
        }
        CaughtError::Value(value) => match value.as_string().map(|string| string.to_string()) {
            Some(Ok(text)) => text,
            _ => format!("the add-on threw a non-error value ({})", value.type_name()),
        },
        CaughtError::Error(error) => error.to_string(),
    }
}
