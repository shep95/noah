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
  function send(payload) {
    cached = payload;
    try { window.dispatchEvent(new CustomEvent("noah-persona-config", { detail: payload })); } catch {}
  }
  function fetchAndSend() {
    api.runtime.sendMessage({ type: "persona.config" }, (reply) => {
      if (api.runtime.lastError || !reply) return;
      send({ config: reply.persona || { enabled: true, autoSeed: true }, prompt: reply.prompt || null });
    });
  }
  fetchAndSend();
  // Persona.js runs at document_idle so it may miss the first dispatch;
  // it fires a request event on load and we replay the cached config.
  window.addEventListener("noah-persona-request", () => {
    if (cached) send(cached);
    else fetchAndSend();
  });

  if (api.runtime.onMessage && api.runtime.onMessage.addListener) {
    api.runtime.onMessage.addListener((message) => {
      if (!message) return;
      if (message.type === "persona.set") {
        send({ config: message.persona || { enabled: true, autoSeed: true }, prompt: message.prompt || null });
      } else if (message.type === "persona.reseed") {
        try { window.dispatchEvent(new CustomEvent("noah-persona-force-seed")); } catch {}
      }
    });
  }
})();
