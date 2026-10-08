/**
 * noah shield — link-prefetch / prerender leak prevention
 *
 * <link rel="prefetch">, <link rel="prerender">, <link rel="preconnect">,
 * and <link rel="dns-prefetch"> fire network requests based on where the
 * browser thinks you MIGHT navigate next. An adversary who controls the
 * destination (or an ISP watching in-flight DNS) learns your reading intent
 * before you click. XKeyscore and KARMA POLICE specifically watch DNS
 * queries; prefetch is a clean source of advance notice.
 *
 * This script removes such hints for third-party origins at document_start
 * and watches for dynamically injected ones via MutationObserver.
 * Same-origin prefetch (used legitimately for SPA routing) is left intact.
 *
 * Runs in the ISOLATED world — no MAIN-world access needed.
 */
(() => {
  "use strict";

  const HINT_RELS = new Set(["prefetch", "prerender", "preconnect", "dns-prefetch"]);

  const pageOrigin = (() => {
    try { return new URL(location.href).origin; } catch { return ""; }
  })();

  const isThirdParty = (href) => {
    if (!href) return false;
    try {
      const u = new URL(href, location.href);
      // blob: and data: are first-party enough
      if (u.protocol === "blob:" || u.protocol === "data:") return false;
      return u.origin !== pageOrigin;
    } catch { return false; }
  };

  const pruneLink = (el) => {
    if (!(el instanceof HTMLLinkElement)) return;
    const rel = (el.rel || "").toLowerCase().trim();
    if (!HINT_RELS.has(rel)) return;
    const href = el.getAttribute("href") || el.getAttribute("imagesrcset") || "";
    if (isThirdParty(href)) {
      // Blank the href before the browser acts on it; remove after a tick
      el.rel = "norel-noah-removed";
      el.href = "";
      try { el.remove(); } catch { }
    }
  };

  // Prune anything already in DOM
  document.querySelectorAll("link[rel]").forEach(pruneLink);

  // Watch for runtime injections (SPAs, ad scripts)
  const observer = new MutationObserver((mutations) => {
    for (const mut of mutations) {
      for (const node of mut.addedNodes) {
        if (node.nodeType !== Node.ELEMENT_NODE) continue;
        if (node instanceof HTMLLinkElement) {
          pruneLink(node);
        } else {
          node.querySelectorAll && node.querySelectorAll("link[rel]").forEach(pruneLink);
        }
      }
    }
  });

  observer.observe(document.documentElement, { childList: true, subtree: true });
})();
