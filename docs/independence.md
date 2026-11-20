# Moving noah off Zed's infrastructure

noah is a fork of Zed: 258 crates, of which 10 are noah's own (`noah_*`, `asherin_*`). This plan removes noah's dependence on Zed in three stages, cheapest and most valuable first.

## Stage 1: stop calling Zed's servers

These run on every install today and send noah users' requests to Zed Industries:

| what | where | what to do |
|---|---|---|
| extension store (`api.zed.dev/extensions`) | `extension_host` (`build_zed_api_url`) | host a noah registry, or ship a curated set inside the app; until then, install from file only |
| account, cloud models, collaboration (`zed.dev`, `cloud.zed.dev`, `collab.zed.dev`) | `client`, `cloud_api_client`, `language_models_cloud`, `channel`; `server_url` in `assets/settings/default.json` | remove sign-in and cloud models from the build; keep bring-your-own-key and local models |
| telemetry (`/telemetry/events`) | `client/src/telemetry.rs` | remove the uploader entirely, so nothing can be turned on by mistake |
| remote-development server binary (`zed-remote-server`) | `auto_update::download_remote_server_release` | build and sign a `noah-remote-server` with the same release key as the app |
| agent registry (`cdn.agentclientprotocol.com`) | `project::agent_registry_store` | fetch only when the person opens the external-agents page |
| crash reports | `zed/src/reliability` | already fixed at build time; confirm the endpoint is noah's |

How to tell it's done: a fresh install, opened with no network, logs no request to any `zed.dev` or `agentclientprotocol.com` host, and nothing the person uses stops working.

## Stage 2: put noah's features behind clean boundaries

Most of noah's own features live inside Zed's crates (`workspace/src/room_rail.rs`, `agent`, `agent_ui`). Move each one into a `noah_*` crate that depends on a small interface instead of Zed internals:

1. rooms and rail → `noah_rooms`
2. shepherd's tools, brain, trust and evidence → `noah_agent`
3. device room, capture, shield → already `noah_device`, `noah_capture`, `noah_trust`

How to tell it's done: each `noah_*` crate builds and tests on its own with a mock host.

## Stage 3: decide on the shell

GPUI (Zed's interface framework) is fast but runs only on desktop systems, and Zed changes it without notice. There are two choices:

- **Keep GPUI.** noah stays native and fast, and pulls in GPUI updates when it chooses. There is no phone version.
- **Rewrite the shell** (for example Tauri with a web interface, keeping the Rust core from stage 2). This makes phones possible and frees noah from Zed's codebase, but the editor itself has to be rebuilt or replaced (for example with Monaco or CodeMirror). It is a large project: estimate it after stage 2, when the size of the core is known.

Recommendation: do stages 1 and 2 now. Decide stage 3 once stage 2 shows how much of noah is really Zed.
