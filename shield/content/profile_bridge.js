// Isolated-world bridge for profile.js: fetches the toggle from background
// and forwards changes so a popup switch flips the panel on live.
(() => {
  "use strict";
  if (window.__noahShieldProfileBridge) return;
  window.__noahShieldProfileBridge = true;
  const api = (typeof chrome !== "undefined" && chrome.runtime) ? chrome : (typeof browser !== "undefined" ? browser : null);
  if (!api || !api.runtime || !api.runtime.sendMessage) return;

  let cached = null;
  // The MAIN-world script announces a nonce at document_start, before any
  // page script can listen; every event to it carries that nonce, and one
  // without it is a page's forgery and is ignored.
  let nonce = null;
  const channel = (name) => name + ":" + nonce;
  window.addEventListener("noah-profile-hello", (event) => {
    if (nonce !== null) return;
    nonce = String(event.detail || "");
    onPaired();
  });
  try { window.dispatchEvent(new CustomEvent("noah-profile-bridge-ready")); } catch {}
  function onPaired() {
    if (cached) send(cached);
  }
  function send(config) {
    cached = config;
    if (nonce === null) return;
    try { window.dispatchEvent(new CustomEvent(channel("noah-profile-config"), { detail: config })); } catch {}
  }

  api.runtime.sendMessage({ type: "profile.config" }, (reply) => {
    if (api.runtime.lastError || !reply) return;
    send(reply.profile || { enabled: true });
  });

  if (api.runtime.onMessage && api.runtime.onMessage.addListener) {
    api.runtime.onMessage.addListener((message) => {
      if (!message || message.type !== "profile.set") return;
      send(message.profile || { enabled: true });
    });
  }
})();
