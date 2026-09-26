---
name: security-audit
description: Audits a codebase for security flaws the way a reviewer does, with the terminal and search tools. It maps the attack surface, hunts each weakness class with concrete searches, proves every finding, patches with tests, and reports what was checked and what was not. Use it when asked for a security audit, a pentest of the code, to find or patch vulnerabilities, or to harden an app.
---

# security audit

an audit is evidence work. nothing is a finding until you have read the code path and shown the bad input reaching it; nothing is fixed until a test that failed before passes after. work from the outside in, write down what you check, and report what you did not get to.

## 1. scope and threat model (ten minutes, in writing)

- what is this program, who runs it, where does it run (a user's desktop, a server, a browser), and what does it hold that matters (keys, files, money, other people's data)?
- draw the trust boundaries as a list: every place bytes come in from something you don't control. network listeners and clients, files the user opens, command-line arguments, environment, clipboard, urls and deep links, ipc, webviews, model and tool output, third-party packages, update channels.
- for each boundary name the worst plausible outcome: code execution, reading files outside the project, leaking a secret, tampering with an update, denial of service, privilege escalation.
- keep this list in `audit/scope.md` in the project (or `.noah/audit/` if the project must stay clean). everything below hangs off it.

## 2. inventory the attack surface with tools, not memory

use the terminal and the search tool. run these against the whole tree and read every hit that touches a boundary. adapt the patterns to the languages present.

- **process and shell**: `rg -n "Command::new|spawn\(|exec\(|system\(|subprocess|child_process|os\.popen|sh -c|cmd /c|powershell"` — look for user-influenced arguments, string-built command lines, missing quoting, `shell=True`.
- **file paths**: `rg -n "\.\./|join\(|PathBuf::from|open\(|read_to_string|write\(|create\(|remove_|rename\(|symlink"` — check for traversal (`..`, absolute paths, symlinks) on any path built from input; check permissions on files that hold secrets (should be 0600, created with create_new).
- **network**: `rg -n "bind\(|listen\(|reqwest|fetch\(|http::|TcpStream|UdpSocket|WebSocket|redirect"` — servers bound to 0.0.0.0 that should be loopback, missing authentication on local ports, requests to user-supplied urls (ssrf: loopback, link-local 169.254.0.0/16, metadata endpoints, file://), redirects followed across origins, tls verification turned off.
- **deserialization and parsing**: `rg -n "serde_json::from|from_str\(|yaml|toml::from|pickle|eval\(|Function\(|new Function|innerHTML|dangerouslySetInnerHTML|unsafe \{"` — untrusted input into parsers with size limits, recursion limits, or none; html sinks; `unsafe` blocks with raw pointers or transmute.
- **sql and queries**: `rg -n "format!\(.*(SELECT|INSERT|UPDATE|DELETE)|execute\(|query\(|raw\("` — string-built queries.
- **secrets**: `rg -n -i "api[_-]?key|secret|token|password|passwd|private[_-]?key|BEGIN (RSA|EC|OPENSSH)"` — anything hard-coded, logged, written to disk unencrypted, sent to a third party, or shown in error messages. also `git log -p -S"BEGIN RSA"` and `git log --all -p -S"api_key"` for secrets in history.
- **crypto and updates**: `rg -n "md5|sha1\(|rand::random|Math\.random|verify|signature|ed25519|hmac|update"` — weak hashes for anything security-relevant, non-cryptographic randomness for tokens, update downloads without signature and checksum checks, downgrade to http.
- **auth and permissions**: every handler or command that changes state: who may call it, and where is that checked? look for checks in the ui layer only.
- **prompt injection** (any program that feeds a model text it fetched): where does fetched text enter the prompt, is it fenced and screened, and can the model's output trigger actions without a person?
- **dependencies**: `cargo audit` (install with `cargo install cargo-audit` if missing), `npm audit`, `pip-audit`, `govulncheck`. read the advisories; a vulnerable function that is never called is a note, not a finding.
- **build and release**: scripts that curl-pipe to shell, unpinned actions or images, artifacts built without reproducibility, secrets in ci logs.

write each hit you consider into `audit/surface.md` with file:line and one line on why it does or does not matter.

## 3. prove every finding

- read the whole path from the boundary to the sink. many hits die here; say so in the notes.
- for the ones that survive, produce a proof that can be rerun: a unit test with the malicious input, a request against the program running locally, a file with a crafted name. never test against a system you do not own or were not asked to test; never send real secrets anywhere.
- rate it: **critical** (remote code execution, secret theft, update tampering), **high** (read or write outside the intended scope, auth bypass), **medium** (needs an unusual setup or a second bug), **low** (hardening, defense in depth).

## 4. patch like a maintainer

- the smallest change that closes the hole at the right layer, plus defense in depth where cheap (a loopback check and a redirect policy, not one or the other).
- a test that fails on the old code and passes on the new one, kept in the suite.
- never weaken or delete a test, never widen permissions to make something work, never turn off verification.
- run the project's own checks (build, lint, full tests) before calling anything fixed. if a fix would change behaviour users rely on, say so and ask.

## 5. report

write `audit/report.md` and answer in the conversation with the same shape:

- one paragraph: what was audited, from which commit, how long, with what tools.
- findings, worst first: severity, where (file:line), what an attacker can do, the proof (how to rerun it), the fix (commit or diff), and whether it is verified.
- what was checked and found sound, with the searches that were run, so the next person does not repeat them.
- what was not checked and why (out of time, out of scope, needs credentials or hardware).
- a short list of hardening that would help but was not done.

no finding is stated without its proof, and nothing is called fixed without the test that shows it. if the audit found nothing, say what was searched for and how; "no findings" without the list is not an audit.
