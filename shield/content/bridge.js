// The isolated-world half of the page guard: fetches this site's settings,
// hands them to content/guard.js in the page world, and turns its questions
// ("may this page record the screen?") into a bar you answer. The bar lives
// in a closed shadow root, so page scripts cannot press its buttons.
(() => {
  "use strict";

  // The shield's own elements carry the session's tag; in stealth mode it is a
  // name no page can look for.
  function tagName(kind) {
    const tag = (window.__noahShieldSiteConfig && window.__noahShieldSiteConfig.tag) || "noah-shield";
    return tag + "-" + kind;
  }
  const api = globalThis.chrome ?? globalThis.browser;
  let nonce = null;
  let siteConfig = null;

  const WORDING = {
    screen: [" wants to record your screen.", "noah shield holds the request until you decide. Allowing still opens the browser's own picker."],
    camera: [" wants your camera or microphone.", "noah shield holds the request until you decide. The browser's own permission prompt follows."],
    clipboard: [" wants to read your clipboard.", "Whatever you copied last, a password or an address included, would be handed to this page."],
    wallet: [" is asking your wallet to sign or approve something.", "Read what it asks for below. Unlimited approvals let a contract move your tokens later without asking again."],
  };

  document.addEventListener("noah-shield:hello", (event) => {
    if (nonce === null) nonce = String(event.detail || "");
    if (!nonce) return;
    if (siteConfig) sendUpdate();
    document.addEventListener("noah-shield:ask:" + nonce, (asked) => {
      let detail;
      try {
        detail = JSON.parse(String(asked.detail || "{}"));
      } catch {
        return;
      }
      const kind = WORDING[detail.kind] ? detail.kind : "camera";
      handleAsk(String(detail.id || ""), kind, String(detail.detail || ""));
    });
    // What the guard stopped on its own (a leak, an alert storm) is counted.
    document.addEventListener("noah-shield:report:" + nonce, (reported) => {
      let detail;
      try {
        detail = JSON.parse(String(reported.detail || "{}"));
      } catch {
        return;
      }
      if (detail.kind === "media") {
        api.runtime.sendMessage({ type: "media.state", camera: Boolean(detail.camera), microphone: Boolean(detail.microphone) }, () => void api.runtime.lastError);
        return;
      }
      api.runtime.sendMessage({ type: "safety.event", kind: String(detail.kind || "other"), amount: Number(detail.amount) || 1 }, () => void api.runtime.lastError);
      if (detail.kind === "leaks") toast(`noah shield stopped this page from sending what you typed to ${detail.to || "another site"}.`);
      if (detail.kind === "keylog") {
        toast(detail.how === "stream"
          ? `noah shield stopped ${detail.to || "another site"} from receiving your keystrokes from this page.`
          : detail.how === "blocked"
            ? `noah shield kept ${detail.to || "a script from another site"} from listening to every key you press here.`
            : `${detail.to || "a script from another site"} listens to every key you press on this page. What you type stays here; turn on "block" under settings to refuse it the listener.`);
      }
    });
  }, { once: true });

  // The introduction is synchronous with document_start on purpose: the guard
  // takes the first one, before any page script runs, and answers with the
  // nonce. The site's real settings follow on the nonce-named event.
  const defaults = { geolocation: "block", fingerprint: true, guardScreen: true, guardCamera: true, clipboardGuard: true, walletGuard: true, formLeak: true, typingGuard: true, blockKeyListeners: false, scamPopups: true };
  document.dispatchEvent(new CustomEvent("noah-shield:config", { detail: JSON.stringify(defaults) }));

  function sendUpdate() {
    document.dispatchEvent(new CustomEvent("noah-shield:config:" + nonce, { detail: JSON.stringify(siteConfig) }));
  }

  api.runtime.sendMessage({ type: "guard.config" }, (config) => {
    siteConfig = api.runtime.lastError || !config || config.error ? defaults : config;
    if (nonce) sendUpdate();
    // The other content scripts wait for this so they never run on a quiet or trusted site.
    window.__noahShieldSiteConfig = siteConfig;
    document.dispatchEvent(new CustomEvent("noah-shield:site-config"));
  });

  function answer(id, allow) {
    document.dispatchEvent(new CustomEvent("noah-shield:answer:" + nonce, { detail: JSON.stringify({ id, allow }) }));
  }

  function handleAsk(id, kind, detail) {
    api.runtime.sendMessage({ type: "capture.ask", kind }, (response) => {
      if (api.runtime.lastError || !response) {
        answer(id, false);
        return;
      }
      if (response.decision === "allow") {
        answer(id, true);
        return;
      }
      showBar(id, kind, response.site || location.hostname, detail);
    });
  }

  const STYLE = `
    :host { all: initial; position: fixed; top: 0; left: 0; right: 0; z-index: 2147483647; font: 14px/1.4 -apple-system, "Segoe UI", system-ui, sans-serif; }
    .bar { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; padding: 12px 18px; background: #0b0e0c; color: #d8ddd6; border-bottom: 1px solid rgba(180,210,190,.12); box-shadow: 0 6px 24px rgba(0,0,0,.35); }
    .bar b { font-weight: 600; color: #fff; }
    .bar span { flex: 1 1 auto; min-width: 200px; }
    button { font: inherit; padding: 6px 12px; border-radius: 8px; border: 1px solid rgba(180,210,190,.16); background: #131916; color: #d8ddd6; cursor: pointer; }
    button:hover { background: #1c2320; }
    button.stop { border-color: #6b2a2a; background: #2a1414; }
    small { display: block; color: #9aa298; }
    code { display: block; margin-top: 4px; font: 12px ui-monospace, SFMono-Regular, Menlo, monospace; color: #c9cfc7; word-break: break-all; }
    .toast { position: fixed; left: 18px; bottom: 18px; padding: 10px 14px; border-radius: 10px; background: #0b0e0c; color: #d8ddd6; border: 1px solid rgba(180,210,190,.12); font: 13px -apple-system, "Segoe UI", system-ui, sans-serif; box-shadow: 0 12px 40px rgba(0,0,0,.45); }

      @keyframes ns-arrive { from { opacity: 0; } to { opacity: 1; } }
      :host { animation: ns-arrive .22s ease-out; }
      @media (prefers-reduced-motion: reduce) { :host, * { animation: none !important; transition: none !important; } }
  `;

  let host = null;
  const barQueue = [];
  let barActive = false;
  function drainBarQueue() {
    if (barActive || !barQueue.length) return;
    barActive = true;
    const { id, kind, site, detail } = barQueue.shift();
    _showBarNow(id, kind, site, detail);
  }
  function showBar(id, kind, site, detail) {
    barQueue.push({ id, kind, site, detail });
    setTimeout(drainBarQueue, barActive ? 500 : 0);
  }
  function showBarFinished() {
    barActive = false;
    drainBarQueue();
  }
  function _showBarNow(id, kind, site, detail) {
    if (host) host.remove();
    host = document.createElement(tagName("bar"));
    const shadow = host.attachShadow({ mode: "closed" });
    const style = document.createElement("style");
    style.textContent = STYLE;
    const bar = document.createElement("div");
    bar.className = "bar";
    bar.setAttribute("role", "alertdialog");
    bar.setAttribute("aria-modal", "true");
    bar.setAttribute("aria-label", "noah shield permission request");
    const text = document.createElement("span");
    const strong = document.createElement("b");
    strong.textContent = site;
    const [headline, explanation] = WORDING[kind];
    text.append(strong, document.createTextNode(headline));
    const small = document.createElement("small");
    small.textContent = explanation;
    text.append(small);
    if (detail) {
      const code = document.createElement("code");
      code.textContent = detail.slice(0, 400);
      text.append(code);
    }
    const once = document.createElement("button");
    once.textContent = "Allow once";
    once.setAttribute("aria-label", "Allow " + kind + " once");
    const always = document.createElement("button");
    always.textContent = "Always allow " + site;
    always.setAttribute("aria-label", "Always allow " + kind + " from " + site);
    const stop = document.createElement("button");
    stop.className = "stop";
    stop.textContent = "Stop it";
    stop.setAttribute("aria-label", "Block " + kind + " from " + site);
    const decide = (allow, forever) => {
      api.runtime.sendMessage({ type: "capture.decide", kind, allow, always: forever }, () => {
        void api.runtime.lastError;
        answer(id, allow);
        host.remove();
        host = null;
        showBarFinished();
      });
    };
    once.addEventListener("click", () => decide(true, false));
    always.addEventListener("click", () => decide(true, true));
    stop.addEventListener("click", () => decide(false, false));
    bar.append(text, once, always, stop);
    shadow.append(style, bar);
    (document.documentElement || document).append(host);
    // FIX: double querySelector call (unnecessary second layout read) and no guard for the
    // case where decide() was called within the 50ms window (host set to null, bar removed).
    setTimeout(() => { const btn = shadow.querySelector("button"); if (host && btn) btn.focus(); }, 50);
  }

  let toastHost = null;
  function toast(message) {
    if (toastHost) toastHost.remove();
    toastHost = document.createElement(tagName("toast"));
    const shadow = toastHost.attachShadow({ mode: "closed" });
    const style = document.createElement("style");
    style.textContent = STYLE;
    const box = document.createElement("div");
    box.className = "toast";
    box.textContent = message;
    shadow.append(style, box);
    (document.documentElement || document).append(toastHost);
    setTimeout(() => { if (toastHost) { toastHost.remove(); toastHost = null; } }, 6000);
  }
})();
