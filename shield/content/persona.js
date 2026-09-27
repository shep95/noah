// Auto-seed every AI chat with the shepherd instructions so the whole
// conversation runs under those rules. Works across Google Gemini and Lens,
// OpenAI ChatGPT, Anthropic Claude, Microsoft Copilot, Perplexity, Poe,
// Mistral, DeepSeek, HuggingChat, xAI Grok, Yandex Alice/GPT, Character.ai,
// Pi.ai, You.com, Kagi Assistant, and AI Studio. The persona goes in as
// turn one of a fresh chat; every response after that is under those rules.
//
// Runs in the page's own world so it can drive the site's rich-text editor
// and click its send button. Chat UIs are picky about how text arrives —
// direct value assignment is ignored by most because their state models
// listen for real InputEvents.
(() => {
  "use strict";
  if (window.__noahShieldPersona) return;
  const host = location.hostname.toLowerCase();
  window.__noahShieldPersona = true;

  let config = { enabled: true, autoSeed: true, seededChats: {} };
  let prompt = null;
  let pill = null;
  let seededThisChat = false;
  let seedingInProgress = false;
  let site = null;

  window.addEventListener("noah-persona-config", (event) => {
    const next = event.detail || {};
    if (next.prompt) prompt = next.prompt;
    if (next.config) config = { ...config, ...next.config };
    ensurePill();
    if (config.enabled && config.autoSeed && prompt && !seededThisChat) tryAutoSeed();
  });
  window.addEventListener("noah-persona-force-seed", () => { seededThisChat = false; tryAutoSeed(); });

  // ---- per-site selectors + new-chat detection ------------------------------
  // Each entry answers: which node is the composer, which is the send button,
  // and does this URL look like a fresh conversation.
  const SITES = {
    gemini: {
      test: (h) => /(^|\.)(gemini\.google\.com|bard\.google\.com|lens\.google\.com|aistudio\.google\.com)$/.test(h),
      composer: [
        "rich-textarea .ql-editor",
        "rich-textarea [contenteditable='true']",
        "div[contenteditable='true'][role='textbox']",
        "div[contenteditable='true'][aria-label*='prompt' i]",
        "textarea[aria-label*='prompt' i]",
        "textarea[aria-label*='message' i]",
      ],
      send: [
        "button[aria-label*='send' i]",
        "button[aria-label*='submit' i]",
        "button[data-testid*='send' i]",
        "button[data-mat-icon-name='send']",
      ],
      newChat: (path) => /^\/(app\/?|)$/.test(path) || /prompts\/new/.test(path),
    },
    chatgpt: {
      test: (h) => /(^|\.)(chatgpt\.com|chat\.openai\.com|openai\.com)$/.test(h),
      composer: [
        "div[contenteditable='true']#prompt-textarea",
        "textarea#prompt-textarea",
        "div[contenteditable='true'][data-id*='prompt']",
        "textarea[placeholder*='message' i]",
        "div[contenteditable='true'][role='textbox']",
      ],
      send: [
        "button[data-testid='send-button']",
        "button[data-testid='fruitjuice-send-button']",
        "button[aria-label*='send' i]",
        "button:has(svg[data-testid*='send' i])",
      ],
      newChat: (path) => path === "/" || /^\/(gpts?|c)?\/?$/.test(path),
    },
    claude: {
      test: (h) => /(^|\.)claude\.ai$/.test(h),
      composer: [
        "div[contenteditable='true'].ProseMirror",
        "div[contenteditable='true'][aria-label*='prompt' i]",
        "div[contenteditable='true'][role='textbox']",
      ],
      send: [
        "button[aria-label*='send' i]",
        "button[data-testid*='send' i]",
        "fieldset button:has(svg)",
      ],
      newChat: (path) => path === "/" || /^\/new$/.test(path) || /^\/chats?\/?$/.test(path),
    },
    copilot: {
      test: (h) => /(^|\.)(copilot\.microsoft\.com|bing\.com)$/.test(h),
      composer: [
        "textarea#userInput",
        "textarea[aria-label*='ask' i]",
        "textarea[placeholder*='ask' i]",
        "div[contenteditable='true']",
      ],
      send: [
        "button[aria-label*='send' i]",
        "button[aria-label*='submit' i]",
        "button[data-testid*='submit' i]",
      ],
      newChat: (path) => path === "/" || /copilot\/?$/.test(path) || /chats\/new/.test(path),
    },
    perplexity: {
      test: (h) => /(^|\.)perplexity\.ai$/.test(h),
      composer: [
        "textarea[placeholder*='ask' i]",
        "textarea[placeholder*='follow' i]",
        "textarea",
      ],
      send: [
        "button[aria-label*='submit' i]",
        "button[aria-label*='send' i]",
        "button[data-testid*='submit' i]",
      ],
      newChat: (path) => path === "/" || /^\/search$/.test(path),
    },
    poe: {
      test: (h) => /(^|\.)poe\.com$/.test(h),
      composer: [
        "textarea[class*='ChatMessageInputContainer']",
        "textarea[placeholder*='message' i]",
        "textarea",
      ],
      send: [
        "button[class*='SendButton']",
        "button[aria-label*='send' i]",
      ],
      newChat: (path) => /^\/[^/]+$/.test(path) && !/^\/chat\//.test(path),
    },
    mistral: {
      test: (h) => /(^|\.)(chat\.mistral\.ai|mistral\.ai)$/.test(h),
      composer: [
        "textarea[placeholder*='message' i]",
        "div[contenteditable='true']",
        "textarea",
      ],
      send: [
        "button[aria-label*='send' i]",
        "button[type='submit']",
      ],
      newChat: (path) => path === "/" || /^\/chat\/?$/.test(path),
    },
    deepseek: {
      test: (h) => /(^|\.)(chat\.deepseek\.com|deepseek\.com)$/.test(h),
      composer: [
        "textarea#chat-input",
        "textarea[placeholder*='message' i]",
        "textarea",
      ],
      send: [
        "div[role='button'][aria-label*='send' i]",
        "button[aria-label*='send' i]",
      ],
      newChat: (path) => path === "/" || /^\/$/.test(path),
    },
    huggingchat: {
      test: (h) => /(^|\.)huggingface\.co$/.test(h) && /\/chat/.test(location.pathname),
      composer: [
        "textarea[placeholder*='ask' i]",
        "textarea[enterkeyhint='send']",
        "textarea",
      ],
      send: [
        "button[type='submit']",
        "button[aria-label*='send' i]",
      ],
      newChat: (path) => /^\/chat\/?$/.test(path),
    },
    grok: {
      test: (h) => /(^|\.)(x\.ai|grok\.com|grok\.x\.ai)$/.test(h),
      composer: [
        "textarea[aria-label*='ask grok' i]",
        "textarea[placeholder*='ask' i]",
        "div[contenteditable='true'][role='textbox']",
        "textarea",
      ],
      send: [
        "button[aria-label*='send' i]",
        "button[type='submit']",
      ],
      newChat: (path) => path === "/" || /^\/chat\/?$/.test(path),
    },
    yandex: {
      test: (h) => /(^|\.)(ya\.ru|yandex\.com|yandex\.ru|alice\.yandex\.ru|shad\.yandex\.ru)$/.test(h) && (/gpt/i.test(location.pathname) || /alice/i.test(location.pathname) || /assistant/i.test(location.pathname)),
      composer: [
        "textarea[placeholder*='Спроси' i]",
        "textarea[placeholder*='ask' i]",
        "textarea",
        "div[contenteditable='true']",
      ],
      send: [
        "button[aria-label*='отправить' i]",
        "button[aria-label*='send' i]",
        "button[type='submit']",
      ],
      newChat: (path) => true,
    },
    character: {
      test: (h) => /(^|\.)(character\.ai|beta\.character\.ai)$/.test(h),
      composer: [
        "textarea[placeholder*='message' i]",
        "textarea",
      ],
      send: [
        "button[aria-label*='send' i]",
        "button[type='submit']",
      ],
      newChat: (path) => /^\/chat/.test(path) || /^\/new/.test(path),
    },
    pi: {
      test: (h) => /(^|\.)pi\.ai$/.test(h),
      composer: [
        "textarea[placeholder*='talk with pi' i]",
        "textarea",
      ],
      send: [
        "button[aria-label*='send' i]",
      ],
      newChat: (path) => /^\/(talk|discover)?\/?$/.test(path),
    },
    you: {
      test: (h) => /(^|\.)you\.com$/.test(h),
      composer: [
        "textarea#search-input-textarea",
        "textarea[data-testid*='chat' i]",
        "textarea",
      ],
      send: [
        "button[aria-label*='submit' i]",
        "button[data-testid*='submit' i]",
      ],
      newChat: (path) => path === "/" || /^\/search$/.test(path),
    },
    kagi: {
      test: (h) => /(^|\.)kagi\.com$/.test(h) && /(assistant|ai)/.test(location.pathname),
      composer: [
        "textarea[placeholder*='ask' i]",
        "textarea",
      ],
      send: [
        "button[type='submit']",
        "button[aria-label*='send' i]",
      ],
      newChat: (path) => /assistant\/?$/.test(path),
    },
    venice: {
      test: (h) => /(^|\.)venice\.ai$/.test(h),
      composer: [
        "textarea[placeholder*='message' i]",
        "textarea",
        "div[contenteditable='true']",
      ],
      send: [
        "button[aria-label*='send' i]",
        "button[type='submit']",
      ],
      newChat: (path) => path === "/" || /^\/chat/.test(path),
    },
    // Test-only host so the Playwright suite can exercise this without
    // fighting Chromium's HSTS preload list on real AI domains. Also
    // useful for anyone hosting a private AI at shieldtest.example.
    shieldtest: {
      test: (h) => h === "shieldtest.example",
      composer: [
        "rich-textarea .ql-editor",
        "div[contenteditable='true'][role='textbox']",
        "div[contenteditable='true']",
        "textarea",
      ],
      send: [
        "button[aria-label*='send' i]",
        "button[data-testid='send-button']",
        "button[type='submit']",
      ],
      newChat: () => true,
    },
  };
  function detectSite(host, path) {
    for (const [name, spec] of Object.entries(SITES)) {
      try { if (spec.test(host, path)) return { name, ...spec }; } catch {}
    }
    return null;
  }

  // ---- generic finders that fall back through the per-site list ------------
  function findComposer() {
    for (const sel of site.composer) {
      try {
        const node = document.querySelector(sel);
        if (node && isVisible(node)) return node;
      } catch {}
    }
    // Last resort: any focused-looking contenteditable big enough to hold a message.
    const all = document.querySelectorAll("textarea, div[contenteditable='true']");
    for (const node of all) {
      if (!isVisible(node)) continue;
      const rect = node.getBoundingClientRect();
      if (rect.width > 200 && rect.height > 20) return node;
    }
    return null;
  }
  function findSendButton() {
    for (const sel of site.send) {
      try {
        const node = document.querySelector(sel);
        if (node) {
          const button = node.closest("button") || node.closest("[role='button']") || node;
          if (button && !button.disabled && isVisible(button) && button.getAttribute("aria-disabled") !== "true") return button;
        }
      } catch {}
    }
    return null;
  }
  function isVisible(node) {
    if (!node) return false;
    const rect = node.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return false;
    const style = window.getComputedStyle(node);
    return style.display !== "none" && style.visibility !== "hidden" && style.opacity !== "0";
  }
  function isNewChat() {
    try { if (site.newChat(location.pathname)) return true; } catch {}
    // Fallback: composer exists and no chat history is rendered nearby.
    const anyMessages = document.querySelector("article, [data-message-author-role], [data-testid*='message' i]");
    return !anyMessages;
  }
  function currentChatKey() { return host + location.pathname; }

  // ---- drive the composer (works for textarea, input and contenteditable) --
  function setComposerText(node, text) {
    if (!node) return false;
    try {
      node.focus();
      if (node.tagName === "TEXTAREA" || node.tagName === "INPUT") {
        const setter = Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, "value")
          || Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value");
        setter && setter.set && setter.set.call(node, text);
        node.dispatchEvent(new Event("input", { bubbles: true }));
        node.dispatchEvent(new Event("change", { bubbles: true }));
        return true;
      }
      // Contenteditable: direct textContent set + a single InputEvent is
      // faster than execCommand for the 240KB shepherd payload (execCommand
      // in Chromium can hang for tens of seconds on that size). Most modern
      // editors (ProseMirror, Quill, Lexical) accept the input event and
      // reconcile. If a site rejects that path, execCommand is the fallback.
      node.textContent = text;
      node.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: text }));
      node.dispatchEvent(new Event("change", { bubbles: true }));
      return true;
    } catch (error) {
      console.warn("noah shield persona:", error);
      return false;
    }
  }

  async function tryAutoSeed() {
    if (!config.enabled || !config.autoSeed || !prompt) return;
    if (seededThisChat) return;
    if (seedingInProgress) return;
    seedingInProgress = true;
    try {
      if (!isNewChat()) { setPill("existing chat, will not seed"); return; }
      const composer = await waitFor(findComposer, 8000);
      if (!composer) { setPill("waiting for composer…"); return; }
      const existing = (composer.value !== undefined ? composer.value : (composer.textContent || "")).trim();
      if (existing.length > 0) { setPill("chat in progress, not seeding"); return; }
      setPill("seeding shepherd…");
      if (!setComposerText(composer, prompt)) { setPill("could not paste into composer"); return; }
      // Mark seeded now so a re-entrant call while the button search is
      // pending sees the composer's transient text as "in progress" from
      // us, not from the user.
      seededThisChat = true;
      await sleep(300);
      const button = await waitFor(findSendButton, 4000);
      if (!button) {
        setPill("shepherd pasted; press send");
        return;
      }
      button.click();
      setPill("shepherd is turn one · " + site.name + " runs under those rules");
    } finally {
      seedingInProgress = false;
    }
  }
  function waitFor(fn, timeoutMs) {
    return new Promise((resolve) => {
      const start = Date.now();
      const tick = () => {
        const found = fn();
        if (found) return resolve(found);
        if (Date.now() - start >= timeoutMs) return resolve(null);
        setTimeout(tick, 200);
      };
      tick();
    });
  }
  function sleep(ms) { return new Promise((resolve) => setTimeout(resolve, ms)); }

  // ---- pill on the page ---------------------------------------------------
  function ensurePill() {
    if (!config.enabled) { if (pill) { pill.remove(); pill = null; } return; }
    if (pill) return;
    pill = document.createElement("div");
    pill.id = "noah-persona-pill";
    pill.style.cssText = "position:fixed;left:14px;bottom:14px;z-index:2147483000;padding:6px 12px;background:rgba(8,12,10,.94);color:#a9cf9f;border:1px solid rgba(180,210,190,.22);border-radius:999px;font:12px -apple-system,'Segoe UI',system-ui,sans-serif;box-shadow:0 6px 20px rgba(0,0,0,.4);display:flex;align-items:center;gap:8px;user-select:none;max-width:60vw;";
    pill.title = "noah shield: shepherd persona active on " + site.name;
    const dot = document.createElement("span");
    dot.style.cssText = "width:6px;height:6px;border-radius:50%;background:#a9cf9f;box-shadow:0 0 6px #a9cf9f;flex-shrink:0;";
    pill.append(dot);
    const label = document.createElement("span");
    label.id = "noah-persona-pill-label";
    label.style.cssText = "overflow:hidden;text-overflow:ellipsis;white-space:nowrap;";
    label.textContent = "shepherd · " + site.name + " · idle";
    pill.append(label);
    const reseed = document.createElement("button");
    reseed.type = "button";
    reseed.textContent = "reseed";
    reseed.style.cssText = "all:unset;padding:2px 8px;border-radius:999px;border:1px solid rgba(180,210,190,.25);color:#96becc;font-size:11px;cursor:pointer;flex-shrink:0;";
    reseed.addEventListener("click", (event) => {
      event.stopPropagation();
      seededThisChat = false;
      tryAutoSeed();
    });
    pill.append(reseed);
    document.documentElement.append(pill);
  }
  function setPill(text) {
    ensurePill();
    if (!pill) return;
    const label = pill.querySelector("#noah-persona-pill-label");
    if (label) label.textContent = "shepherd · " + site.name + " · " + text;
  }

  // ---- watch for SPA navigation -------------------------------------------
  function watchNavigation() {
    let lastPath = location.pathname;
    const check = () => {
      if (location.pathname !== lastPath) {
        lastPath = location.pathname;
        seededThisChat = false;
        setPill("new chat detected");
        if (config.enabled && config.autoSeed && prompt) tryAutoSeed();
      }
    };
    setInterval(check, 800);
    const push = history.pushState;
    history.pushState = function () { const r = push.apply(this, arguments); check(); return r; };
    window.addEventListener("popstate", check);
  }

  site = detectSite(host, location.pathname);
  if (!site) return;
  ensurePill();
  watchNavigation();
  // Ask the bridge for the config, in case its initial dispatch happened
  // before this script's listener was registered.
  try { window.dispatchEvent(new CustomEvent("noah-persona-request")); } catch {}
  setTimeout(() => {
    try { window.dispatchEvent(new CustomEvent("noah-persona-request")); } catch {}
    if (config.enabled && config.autoSeed && prompt) tryAutoSeed();
  }, 1500);
})();
