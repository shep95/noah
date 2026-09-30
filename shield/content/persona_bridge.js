// Isolated-world bridge for persona.js: fetches the shepherd text and
// the on/off settings from background, hands them to the MAIN world, and
// relays live setting changes.
(() => {
  "use strict";
  if (window.__noahShieldPersonaBridge) return;
  window.__noahShieldPersonaBridge = true;
  const api = (typeof chrome !== "undefined" && chrome.runtime) ? chrome : (typeof browser !== "undefined" ? browser : null);
  if (!api || !api.runtime || !api.runtime.sendMessage) return;

  let cached = null;
  // The MAIN-world script announces a nonce at document_start, before any
  // page script can listen; every event to it carries that nonce, and one
  // without it is a page's forgery and is ignored.
  let nonce = null;
  const channel = (name) => name + ":" + nonce;
  window.addEventListener("noah-persona-hello", (event) => {
    if (nonce !== null) return;
    nonce = String(event.detail || "");
    onPaired();
  });
  try { window.dispatchEvent(new CustomEvent("noah-persona-bridge-ready")); } catch {}
  function onPaired() {
    if (cached) send(cached);
    else fetchAndSend();
  }
  function send(payload) {
    cached = payload;
    if (nonce === null) return;
    try { window.dispatchEvent(new CustomEvent(channel("noah-persona-config"), { detail: payload })); } catch {}
  }
  function fetchAndSend() {
    api.runtime.sendMessage({ type: "persona.config" }, (reply) => {
      if (api.runtime.lastError || !reply) return;
      send({ config: reply.persona || { enabled: true, autoSeed: true }, prompt: reply.prompt || null });
    });
  }
  fetchAndSend();

  if (api.runtime.onMessage && api.runtime.onMessage.addListener) {
    api.runtime.onMessage.addListener((message) => {
      if (!message) return;
      if (message.type === "persona.set") {
        send({ config: message.persona || { enabled: true, autoSeed: true }, prompt: message.prompt || null });
      } else if (message.type === "persona.reseed") {
        if (nonce !== null) try { window.dispatchEvent(new CustomEvent(channel("noah-persona-force-seed"))); } catch {}
      }
    });
  }
})();
