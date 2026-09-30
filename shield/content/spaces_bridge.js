// Ask the background for the Spaces settings and hand them to the MAIN
// world spaces.js. Also relays live updates so a toggle in the popup takes
// effect on this tab without a reload.
(() => {
  "use strict";
  if (window.__noahShieldSpacesBridge) return;
  window.__noahShieldSpacesBridge = true;
  const api = (typeof chrome !== "undefined" && chrome.runtime) ? chrome : (typeof browser !== "undefined" ? browser : null);
  if (!api || !api.runtime || !api.runtime.sendMessage) return;

  let cached = null;
  // The MAIN-world script announces a nonce at document_start, before any
  // page script can listen; every event to it carries that nonce, and one
  // without it is a page's forgery and is ignored.
  let nonce = null;
  const channel = (name) => name + ":" + nonce;
  window.addEventListener("noah-spaces-hello", (event) => {
    if (nonce !== null) return;
    nonce = String(event.detail || "");
    onPaired();
  });
  try { window.dispatchEvent(new CustomEvent("noah-spaces-bridge-ready")); } catch {}
  function onPaired() {
    if (cached) send(cached);
    // The audit link inside the Space panel: only the paired panel can ask.
    window.addEventListener(channel("noah-space-audit-request"), () => {
      api.runtime.sendMessage({ type: "inspect.arm.self" }, (reply) => {
        if (api.runtime.lastError || !reply || !reply.enabled) return;
        // Reload so the inspect panel catches the requests the page has
        // already made; the arm state is per tab in the background.
        try { location.reload(); } catch {}
      });
    });
  }
  function send(config) {
    cached = config;
    if (nonce === null) return;
    try { window.dispatchEvent(new CustomEvent(channel("noah-spaces-config"), { detail: config })); } catch {}
  }

  api.runtime.sendMessage({ type: "spaces.config" }, (reply) => {
    if (api.runtime.lastError || !reply) return;
    send(reply.spaces || { enabled: true });
  });

  if (api.runtime.onMessage && api.runtime.onMessage.addListener) {
    api.runtime.onMessage.addListener((message) => {
      if (!message || message.type !== "spaces.set") return;
      send(message.spaces || { enabled: true });
    });
  }

})();
