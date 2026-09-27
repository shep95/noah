// Isolated-world bridge for profile.js: fetches the toggle from background
// and forwards changes so a popup switch flips the panel on live.
(() => {
  "use strict";
  if (window.__noahShieldProfileBridge) return;
  window.__noahShieldProfileBridge = true;
  const api = (typeof chrome !== "undefined" && chrome.runtime) ? chrome : (typeof browser !== "undefined" ? browser : null);
  if (!api || !api.runtime || !api.runtime.sendMessage) return;

  function send(config) {
    try { window.dispatchEvent(new CustomEvent("noah-profile-config", { detail: config })); } catch {}
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
