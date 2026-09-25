> [!IMPORTANT]
> Remove this line to confirm you've reviewed this PR before submitting.

# noah

**noah** is a fast, native code editor with a built-in AI pair-programmer called **shepherd**.
It is a fork of [Zed](https://github.com/zed-industries/zed), rebranded and extended by
**#houseofasher** / [asherin.com](https://asherin.com).

- Free to use. No paywall, no sign-up.
- Bring your own **Venice AI** API key — your key stays on your machine.
- **shepherd** reads your project's aesthetics, architecture, and code conventions, then
  matches your patterns.
- Custom editor wallpaper: the UI palette adapts to the image you choose.
- A real browser inside noah: shepherd can open pages, read them, click and fill forms, and you
  watch it live in the browser room (ctrl-alt-6) and can take over at any time.
- Every language: pick yours in Settings > General > Language; noah's interface and shepherd
  switch to it.
- Proof over claims: shepherd hands in evidence with each change (the commands that really ran,
  each claim tied to them, what it didn't verify), and a model from another family reviews it.
- Guard rails: prompt injection in web pages is withheld, secrets are redacted, irreversible
  commands always ask, new dependencies are checked against their registries, and every change
  is recorded in a tamper-evident provenance log.
- Mission control (ctrl-alt-7): every conversation, the decisions waiting on you, evidence,
  spend against your budget, and how shepherd's past work held up.
- Voice: speak to shepherd and hear its replies, through your own provider's API key.

Community: [Discord](https://discord.gg/M9hnebRwvk) · [asherin.com](https://asherin.com)

---

### Building noah

noah is a native desktop application built in Rust on the GPUI framework. It is **not** a web
app and is not deployed to a web host — it ships as a downloadable binary for macOS, Linux,
and Windows.

Follow the upstream Zed development docs for your platform (the toolchain is identical):

- [macOS](./docs/src/development/macos.md)
- [Linux](./docs/src/development/linux.md)
- [Windows](./docs/src/development/windows.md)

Quick start once the toolchain is set up:

```sh
# Run the editor (debug)
cargo run -p zed

# Release build
cargo build --release -p zed
```

> The Rust crate is still named `zed` internally (renaming 247 crates would gain nothing and
> break every internal path); the **product** you see — window title, About dialog, app
> bundle — is **noah**.

### Credit

noah stands on the shoulders of [Zed](https://zed.dev) by Zed Industries, licensed under
GPL-3.0 / Apache-2.0. See `LICENSE-GPL` and `LICENSE-APACHE`. This fork retains those licenses.

shepherd's browser is [agent-browser](https://github.com/vercel-labs/agent-browser) by Vercel
Labs, Apache-2.0. noah's installers include its binary unchanged, with its license alongside
(`script/fetch-agent-browser` pins and verifies the release).
