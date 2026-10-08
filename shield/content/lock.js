// Tab lock: on the sites you chose, a few idle minutes put a curtain over
// the page until the PIN is typed. It keeps eyes off a screen left alone;
// it is not a vault, and it says so.
(() => {
  "use strict";

  // The shield's own elements carry the session's tag; in stealth mode it is a
  // name no page can look for.
  function tagName(kind) {
    const tag = (window.__noahShieldSiteConfig && window.__noahShieldSiteConfig.tag) || "noah-shield";
    return tag + "-" + kind;
  }
  if (window.top !== window) return;
  const api = globalThis.chrome ?? globalThis.browser;
  const send = (message) => new Promise((resolve) => {
    try { api.runtime.sendMessage(message, (response) => { void api.runtime.lastError; resolve(response || {}); }); } catch { resolve({}); }
  });

  function waitForConfig() {
    return new Promise((resolve) => {
      if (window.__noahShieldSiteConfig) return resolve(window.__noahShieldSiteConfig);
      document.addEventListener("noah-shield:site-config", () => resolve(window.__noahShieldSiteConfig), { once: true });
      setTimeout(() => resolve(window.__noahShieldSiteConfig || null), 4000);
    });
  }

  let curtain = null;
  let lastActivity = Date.now();
  function showCurtain() {
    if (curtain) return;
    curtain = document.createElement(tagName("notice"));
    const shadow = curtain.attachShadow({ mode: "closed" });
    const style = document.createElement("style");
    style.textContent = `
      :host { all: initial; position: fixed; inset: 0; z-index: 2147483647; display: grid; place-items: center; background: #070909; font: 15px/1.5 Georgia, "Times New Roman", serif; color: #d8ddd6; }
      .card { width: min(380px, calc(100vw - 40px)); text-align: center; }
      h1 { font-size: 26px; font-weight: 400; margin: 0 0 10px; color: #f1f4ef; }
      p { margin: 0 0 18px; color: #9aa298; font: 13px -apple-system, "Segoe UI", system-ui, sans-serif; }
      input { font: 22px ui-monospace, SFMono-Regular, Menlo, monospace; letter-spacing: .3em; text-align: center; width: 180px; padding: 8px; border-radius: 10px; border: 1px solid rgba(180,210,190,.16); background: #0b0e0c; color: #f1f4ef; }
      .bad { color: #e8b4a8; }

      @keyframes ns-arrive { from { opacity: 0; } to { opacity: 1; } }
      :host { animation: ns-arrive .22s ease-out; }
      @media (prefers-reduced-motion: reduce) { :host, * { animation: none !important; transition: none !important; } }
    `;
    const card = document.createElement("div");
    card.className = "card";
    const heading = document.createElement("h1");
    heading.textContent = "locked";
    const note = document.createElement("p");
    note.textContent = location.hostname + " was left alone. Type the shield's PIN to lift the curtain.";
    const input = document.createElement("input");
    input.type = "password";
    input.inputMode = "numeric";
    input.autocomplete = "off";
    input.maxLength = 12;
    async function hashPin(pin) {
      const enc = new TextEncoder();
      const buf = await crypto.subtle.digest("SHA-256", enc.encode(pin));
      return Array.from(new Uint8Array(buf)).map((b) => b.toString(16).padStart(2, "0")).join("");
    }
    input.addEventListener("keydown", async (event) => {
      if (event.key !== "Enter") return;
      const answer = await send({ type: "lock.check", pinHash: await hashPin(input.value) });
      if (answer && answer.ok) {
        curtain.remove();
        curtain = null;
        lastActivity = Date.now();
      } else {
        note.textContent = "That is not it.";
        note.className = "bad";
        input.value = "";
      }
    });
    card.append(heading, note, input);
    shadow.append(style, card);
    (document.documentElement || document).append(curtain);
    setTimeout(() => input.focus(), 50);
  }

  waitForConfig().then((config) => {
    if (!config || !config.lockMinutes) return;
    const minutes = Number(config.lockMinutes);
    const touch = () => { if (!curtain) lastActivity = Date.now(); };
    for (const type of ["mousemove", "keydown", "scroll", "touchstart", "pointerdown"]) window.addEventListener(type, touch, { capture: true, passive: true });
    setInterval(() => {
      if (!curtain && Date.now() - lastActivity > minutes * 60000) showCurtain();
    }, 5000);
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "visible" && Date.now() - lastActivity > minutes * 60000 && !curtain) showCurtain();
    });
  });
})();
