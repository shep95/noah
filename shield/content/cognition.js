/**
 * noah shield — cognitive sovereignty overlay
 *
 * Detects Robert Cialdini's six influence weapons in live page content
 * (urgency, scarcity, social proof, authority, reciprocity/confirm-shaming,
 * commitment/pre-ticked traps) and gives the user a transparent, dismissable
 * summary so they can decide with a clear head rather than a manipulated one.
 *
 * Runs at document_idle in the ISOLATED world so it cannot be detected or
 * tampered with by page scripts. Uses MutationObserver for SPAs and dynamic
 * content. Auto-unchecks pre-ticked marketing opt-in checkboxes.
 *
 * The UI is a small collapsible pill fixed to the bottom-right corner.
 * It appears only when ≥ 2 distinct manipulation vectors are active.
 */
(() => {
  "use strict";

  // ── Influence vector definitions ─────────────────────────────────────────
  // Each entry has:
  //   id       — short identifier used to deduplicate signals
  //   label    — human-readable name for the overlay
  //   desc     — one-line explanation
  //   patterns — array of RegExp tested against visible text
  //   check    — optional custom DOM inspector returning boolean

  const VECTORS = [
    {
      id: "urgency",
      label: "Urgency",
      desc: "Artificial time pressure is being used to rush your decision.",
      patterns: [
        /\blimited[\s-]?time\b/i,
        /\btoday only\b/i,
        /\bends?\s+(?:in|soon|tonight|at midnight)\b/i,
        /\b(?:hurry|act now|don't wait|last chance)\b/i,
        /\b(?:expires?|expiring)\s+(?:in|soon)\b/i,
        /\b(?:\d+\s+(?:hour|minute|second)s?\s+(?:left|remaining))\b/i,
        /\bcountdown\b/i,
      ],
    },
    {
      id: "scarcity",
      label: "Scarcity",
      desc: "Low-stock claims push FOMO — real scarcity rarely needs a banner.",
      patterns: [
        /\bonly\s+\d+\s+left\b/i,
        /\b\d+\s+(?:item|unit|seat|spot|ticket)s?\s+(?:left|remaining)\b/i,
        /\bselling\s+(?:fast|out)\b/i,
        /\balmost\s+(?:gone|sold out)\b/i,
        /\bhigh\s+demand\b/i,
        /\blimited\s+(?:stock|availability|supply|edition)\b/i,
        /\bout\s+of\s+stock\s+soon\b/i,
      ],
    },
    {
      id: "social_proof",
      label: "Social Proof",
      desc: "Crowd-following cues are meant to replace your own judgement.",
      patterns: [
        /\b\d+[,\d]*\s+(?:people|customers|users|others)\s+(?:bought|viewed|watching|looking at this)\b/i,
        /\bbest\s*seller\b/i,
        /\b#1\s+(?:choice|rated|selling)\b/i,
        /\b(?:trending|popular|top-rated)\b/i,
        /\bjoined\s+by\s+\d/i,
        /\b\d+\s+(?:reviews?|ratings?|five.star)\b/i,
      ],
    },
    {
      id: "authority",
      label: "Authority",
      desc: "Credential-dropping or endorsement claims signal deference pressure.",
      patterns: [
        /\b(?:as\s+seen\s+on|featured\s+in)\b/i,
        /\b(?:expert[- ]recommended|doctor[- ]approved|clinically\s+(?:tested|proven|validated))\b/i,
        /\b(?:award[- ]winning|industry[- ]leading|#1\s+trusted)\b/i,
        /\bscientifically\s+(?:proven|backed|validated)\b/i,
        /\bcertified\s+by\b/i,
      ],
    },
    {
      id: "confirm_shame",
      label: "Confirm-Shaming",
      desc: "The 'No' option is written to make you feel bad for declining.",
      patterns: [
        /\bno[,\s]+(?:i\s+)?(?:don't|hate|never|refuse|won't)\b/i,
        /\bno[,\s]+(?:i\s+)?(?:don't want|prefer not|am not interested)\s+(?:to\s+)?(?:save|learn|be|get|receive)\b/i,
        /\bi\s+(?:hate|don't like|don't care about)\s+(?:saving|deals|discounts|money|health|fun)\b/i,
        /\bno\s+thanks,\s+i\s+(?:like|love|enjoy|prefer)\b/i,
        /\bskip\s+(?:and\s+)?(?:miss|lose)\b/i,
      ],
    },
    {
      id: "pretick",
      label: "Pre-ticked Opt-in",
      desc: "A checkbox was pre-checked to sign you up without active consent.",
      patterns: [],  // handled by custom check below
      check: () => {
        const inputs = document.querySelectorAll('input[type="checkbox"]');
        for (const cb of inputs) {
          if (!cb.checked) continue;
          const label = getLabel(cb);
          if (!label) continue;
          if (/(?:newsletter|offer|promotion|marketing|update|news|deal|discount|partner|third.party)/i.test(label)) {
            return true;
          }
        }
        return false;
      },
    },
  ];

  // ── Helpers ──────────────────────────────────────────────────────────────

  const getLabel = (input) => {
    // <label for=id>, wrapping <label>, aria-label, or adjacent text
    if (input.id) {
      const lbl = document.querySelector(`label[for="${CSS.escape(input.id)}"]`);
      if (lbl) return lbl.textContent;
    }
    const parent = input.closest("label");
    if (parent) return parent.textContent;
    if (input.getAttribute("aria-label")) return input.getAttribute("aria-label");
    if (input.getAttribute("aria-labelledby")) {
      const ref = document.getElementById(input.getAttribute("aria-labelledby"));
      if (ref) return ref.textContent;
    }
    // Text node sibling
    const next = input.nextSibling;
    if (next && next.nodeType === Node.TEXT_NODE) return next.textContent;
    return "";
  };

  const visibleText = () => {
    // Sample text from meaningful elements without walking every node
    const tags = ["p", "span", "div", "h1", "h2", "h3", "h4", "li", "button", "a", "label", "strong", "em"];
    const parts = [];
    for (const tag of tags) {
      const els = document.querySelectorAll(tag);
      for (const el of els) {
        if (parts.length > 4000) break;
        const t = el.textContent.trim();
        if (t.length > 4 && t.length < 300) parts.push(t);
      }
      if (parts.length > 4000) break;
    }
    return parts.join(" ");
  };

  // ── Auto-uncheck pre-ticked marketing boxes ───────────────────────────────
  const autoUncheck = () => {
    const inputs = document.querySelectorAll('input[type="checkbox"]');
    let count = 0;
    for (const cb of inputs) {
      if (!cb.checked) continue;
      const label = getLabel(cb);
      if (!label) continue;
      if (/(?:newsletter|offer|promotion|marketing|update|news|deal|discount|partner|third.party)/i.test(label)) {
        cb.checked = false;
        cb.dispatchEvent(new Event("change", { bubbles: true }));
        count++;
      }
    }
    return count;
  };

  // ── Scan and score ────────────────────────────────────────────────────────
  const scan = () => {
    const text = visibleText();
    const active = [];
    for (const vec of VECTORS) {
      let hit = false;
      for (const pat of vec.patterns) {
        if (pat.test(text)) { hit = true; break; }
      }
      if (!hit && vec.check) hit = vec.check();
      if (hit) active.push(vec);
    }
    return active;
  };

  // ── UI ────────────────────────────────────────────────────────────────────
  let pill = null;
  let dismissed = false;
  let uncheckedCount = 0;

  const PILL_ID = "__noah_cognition_pill";
  const Z = 2147483647;

  const buildPill = (active) => {
    if (document.getElementById(PILL_ID)) return;

    const host = document.createElement("div");
    host.id = PILL_ID;
    host.setAttribute("data-noah", "1");

    // Shadow DOM keeps the pill isolated from page styles
    const shadow = host.attachShadow({ mode: "closed" });

    const style = document.createElement("style");
    style.textContent = `
      :host { all: initial; }
      #wrap {
        position: fixed;
        bottom: 18px;
        right: 18px;
        z-index: ${Z};
        font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
        font-size: 13px;
        line-height: 1.4;
        max-width: 300px;
        background: #0f0f12;
        color: #e8e8ec;
        border: 1px solid #2a2a35;
        border-radius: 12px;
        box-shadow: 0 4px 24px rgba(0,0,0,.6);
        overflow: hidden;
        transition: opacity .2s ease;
        user-select: none;
      }
      #header {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 10px 12px;
        cursor: pointer;
        background: #16161e;
      }
      #icon { font-size: 15px; flex-shrink: 0; }
      #title { flex: 1; font-weight: 600; font-size: 12px; color: #f0a500; letter-spacing: .03em; }
      #toggle { color: #555; font-size: 11px; }
      #body { padding: 0 12px 10px; display: none; }
      #body.open { display: block; }
      .vec {
        margin-top: 8px;
        border-left: 2px solid #f0a500;
        padding-left: 8px;
      }
      .vec-name { font-weight: 600; color: #f0a500; font-size: 11px; text-transform: uppercase; letter-spacing: .06em; }
      .vec-desc { color: #aaa; font-size: 11px; margin-top: 2px; }
      #uncheckNote { margin-top: 8px; color: #5eead4; font-size: 11px; }
      #dismiss {
        display: block;
        margin-top: 10px;
        width: 100%;
        padding: 5px 0;
        background: none;
        border: 1px solid #2a2a35;
        border-radius: 6px;
        color: #555;
        font-size: 11px;
        cursor: pointer;
        text-align: center;
      }
      #dismiss:hover { color: #888; border-color: #444; }
    `;

    const wrap = document.createElement("div");
    wrap.id = "wrap";

    const header = document.createElement("div");
    header.id = "header";
    header.innerHTML = `
      <span id="icon">🛡</span>
      <span id="title">Influence detected (${active.length})</span>
      <span id="toggle">▼</span>
    `;

    const body = document.createElement("div");
    body.id = "body";
    body.innerHTML = active.map((v) => `
      <div class="vec">
        <div class="vec-name">${v.label}</div>
        <div class="vec-desc">${v.desc}</div>
      </div>
    `).join("") + (uncheckedCount > 0
      ? `<div id="uncheckNote">↩ ${uncheckedCount} pre-ticked marketing checkbox${uncheckedCount > 1 ? "es" : ""} unchecked for you.</div>`
      : "") + `<button id="dismiss">Dismiss for this page</button>`;

    header.addEventListener("click", () => {
      const open = body.classList.toggle("open");
      wrap.querySelector("#toggle").textContent = open ? "▲" : "▼";
    });

    body.querySelector("#dismiss").addEventListener("click", () => {
      dismissed = true;
      host.remove();
    });

    wrap.appendChild(header);
    wrap.appendChild(body);
    shadow.appendChild(style);
    shadow.appendChild(wrap);
    document.documentElement.appendChild(host);
  };

  const removePill = () => {
    const el = document.getElementById(PILL_ID);
    if (el) el.remove();
    pill = null;
  };

  // ── Main loop ─────────────────────────────────────────────────────────────
  let scanTimer = null;
  let lastCount = 0;

  const runScan = () => {
    if (dismissed) return;
    uncheckedCount = autoUncheck();
    const active = scan();
    if (active.length >= 2) {
      if (active.length !== lastCount) {
        removePill();
        buildPill(active);
      }
      lastCount = active.length;
    } else {
      if (lastCount > 0) removePill();
      lastCount = 0;
    }
  };

  // Initial scan after DOM settles
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", () => setTimeout(runScan, 600));
  } else {
    setTimeout(runScan, 600);
  }

  // Re-scan on significant DOM mutations (SPA navigation, lazy-loaded content)
  let mutationDebounce = null;
  const observer = new MutationObserver(() => {
    if (dismissed) return;
    clearTimeout(mutationDebounce);
    mutationDebounce = setTimeout(runScan, 800);
  });
  observer.observe(document.body || document.documentElement, {
    childList: true,
    subtree: true,
    characterData: false,
    attributes: false,
  });
})();
