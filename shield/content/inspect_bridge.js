// The isolated-world bridge for inspect mode. The MAIN-world inspect.js
// hooks fetch, XHR, WebSocket and forms, but it can't reach chrome.runtime.
// This script asks the background whether this tab is armed and forwards
// the decision back to the page world through a CustomEvent.
(() => {
  "use strict";
  if (window.__noahShieldInspectBridge) return;
  window.__noahShieldInspectBridge = true;
  const api = (typeof chrome !== "undefined" && chrome.runtime) ? chrome : (typeof browser !== "undefined" ? browser : null);
  if (!api || !api.runtime || !api.runtime.sendMessage) return;

  function tell(enabled) {
    try {
      window.dispatchEvent(new CustomEvent("noah-inspect-decision", { detail: { enabled: Boolean(enabled) } }));
    } catch {}
  }

  try {
    api.runtime.sendMessage({ type: "inspect.enabled" }, (reply) => {
      if (api.runtime.lastError) { tell(false); return; }
      tell(reply && reply.enabled);
    });
  } catch { tell(false); }

  if (api.runtime.onMessage && api.runtime.onMessage.addListener) {
    api.runtime.onMessage.addListener((message) => {
      if (!message) return;
      if (message.type === "inspect.set") {
        if (message.enabled) tell(true);
        else {
          try { window.dispatchEvent(new CustomEvent("noah-inspect-control", { detail: { action: "off" } })); } catch {}
        }
      } else if (message.type === "inspect.control") {
        try { window.dispatchEvent(new CustomEvent("noah-inspect-control", { detail: { action: message.action } })); } catch {}
      }
    });
  }
})();
