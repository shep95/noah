// Ask the background for the Spaces settings and hand them to the MAIN
// world spaces.js. Also relays live updates so a toggle in the popup takes
// effect on this tab without a reload.
(() => {
  "use strict";
  if (window.__noahShieldSpacesBridge) return;
  window.__noahShieldSpacesBridge = true;
  const api = (typeof chrome !== "undefined" && chrome.runtime) ? chrome : (typeof browser !== "undefined" ? browser : null);
  if (!api || !api.runtime || !api.runtime.sendMessage) return;

  function send(config) {
    try { window.dispatchEvent(new CustomEvent("noah-spaces-config", { detail: config })); } catch {}
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

  // The "audit this page" link inside the Space panel: arms inspect mode
  // for this tab, then dispatches the decision so inspect.js opens its
  // recording panel next to the Space panel. A reload catches everything
  // from the first request; without one, whatever comes after is captured.
  window.addEventListener("noah-space-audit-request", () => {
    api.runtime.sendMessage({ type: "inspect.arm.self" }, (reply) => {
      if (api.runtime.lastError || !reply || !reply.enabled) return;
      try { window.dispatchEvent(new CustomEvent("noah-inspect-decision", { detail: { enabled: true } })); } catch {}
    });
  });
})();
