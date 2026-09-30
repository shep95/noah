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

  let lastDecision = null;
  // The MAIN-world script announces a nonce at document_start, before any
  // page script can listen; every event to it carries that nonce, and one
  // without it is a page's forgery and is ignored.
  let nonce = null;
  const channel = (name) => name + ":" + nonce;
  window.addEventListener("noah-inspect-hello", (event) => {
    if (nonce !== null) return;
    nonce = String(event.detail || "");
    onPaired();
  });
  try { window.dispatchEvent(new CustomEvent("noah-inspect-bridge-ready")); } catch {}
  function onPaired() {
    if (lastDecision !== null) tell(lastDecision);
  }
  function tell(enabled) {
    lastDecision = Boolean(enabled);
    if (nonce === null) return;
    try {
      window.dispatchEvent(new CustomEvent(channel("noah-inspect-decision"), { detail: { enabled: Boolean(enabled) } }));
    } catch {}
  }
  function control(action) {
    if (nonce === null) return;
    try { window.dispatchEvent(new CustomEvent(channel("noah-inspect-control"), { detail: { action } })); } catch {}
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
        else control("off");
      } else if (message.type === "inspect.control") {
        control(message.action);
      }
    });
  }
})();
