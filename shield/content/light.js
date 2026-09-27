// Light: a dimmer and a warmer over every page, by scene ("in a dark room",
// "at a restaurant") or by hand. A page cannot turn a backlight down; a dark
// veil over what the screen shows is the next best thing, and the warmth
// takes the blue out of it at night.
(() => {
  "use strict";
  if (window.top !== window) return;
  const api = globalThis.chrome ?? globalThis.browser;
  let veil = null;
  let warmth = null;

  function ensure() {
    if (veil && veil.isConnected) return;
    veil = document.createElement("div");
    warmth = document.createElement("div");
    for (const layer of [veil, warmth]) {
      Object.assign(layer.style, { position: "fixed", inset: "0", pointerEvents: "none", zIndex: "2147483645", transition: "opacity .5s ease, background .5s ease" });
      layer.style.setProperty("mix-blend-mode", "multiply");
    }
    veil.style.background = "#000";
    veil.style.opacity = "0";
    warmth.style.background = "#ffb45c";
    warmth.style.opacity = "0";
    (document.documentElement).append(veil, warmth);
  }

  function apply(light) {
    const dim = Math.min(0.85, Math.max(0, Number(light && light.dim) || 0));
    const warm = Math.min(1, Math.max(0, Number(light && light.warmth) || 0));
    if (!dim && !warm) {
      if (veil) { veil.remove(); warmth.remove(); veil = warmth = null; }
      return;
    }
    ensure();
    veil.style.opacity = String(dim);
    // Multiplying by amber at full strength is far too orange; a quarter is a candle.
    warmth.style.opacity = String(warm * 0.32);
  }

  api.runtime.sendMessage({ type: "light.state" }, (light) => {
    void api.runtime.lastError;
    if (light && !light.error) apply(light);
  });
  api.runtime.onMessage.addListener((message, sender) => {
    if (!message || message.type !== "light.apply" || sender.id !== api.runtime.id) return false;
    apply(message.light);
    return false;
  });
})();
