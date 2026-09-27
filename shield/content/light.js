// Light: the shield turns the screen's real brightness through noah, never
// the page. The one thing left on the page is the optional warm tint, a
// candle-coloured layer that takes the blue out at night, and only when the
// person asked for it.
(() => {
  "use strict";
  if (window.top !== window) return;
  const api = globalThis.chrome ?? globalThis.browser;
  let warmth = null;

  function apply(light) {
    const warm = Math.min(1, Math.max(0, Number(light && light.warmth) || 0));
    if (!warm) {
      if (warmth) { warmth.remove(); warmth = null; }
      return;
    }
    if (!warmth || !warmth.isConnected) {
      warmth = document.createElement("div");
      Object.assign(warmth.style, { position: "fixed", inset: "0", pointerEvents: "none", zIndex: "2147483645", background: "#ffb45c", opacity: "0", transition: "opacity .5s ease" });
      warmth.style.setProperty("mix-blend-mode", "multiply");
      document.documentElement.append(warmth);
    }
    // Multiplying by amber at full strength is far too orange; a third is a candle.
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
