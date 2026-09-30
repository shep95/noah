// Before something private leaves your hands: a warning when a card number,
// a government id, a secret key or a seed phrase is pasted into a page that
// has no business with it (AI chats included), and photos stripped of their
// location and camera metadata before they upload.
(() => {
  "use strict";

  // The shield's own elements carry the session's tag; in stealth mode it is a
  // name no page can look for.
  function tagName(kind) {
    const tag = (window.__noahShieldSiteConfig && window.__noahShieldSiteConfig.tag) || "noah-shield";
    return tag + "-" + kind;
  }
  const api = globalThis.chrome ?? globalThis.browser;
  const send = (message) => new Promise((resolve) => {
    try { api.runtime.sendMessage(message, (response) => { void api.runtime.lastError; resolve(response || {}); }); } catch { resolve({}); }
  });

  function waitForConfig() {
    return new Promise((resolve) => {
      if (window.__noahShieldSiteConfig) return resolve(window.__noahShieldSiteConfig);
      document.addEventListener("noah-shield:site-config", () => resolve(window.__noahShieldSiteConfig), { once: true });
      setTimeout(() => resolve(window.__noahShieldSiteConfig || null), 4000);
    });
  }

  // ---- what counts as private ----------------------------------------------------------------
  function luhn(digits) {
    let sum = 0;
    let double = false;
    for (let index = digits.length - 1; index >= 0; index--) {
      let value = Number(digits[index]);
      if (double) { value *= 2; if (value > 9) value -= 9; }
      sum += value;
      double = !double;
    }
    return sum % 10 === 0;
  }
  const AI_CHAT = /(^|\.)(chatgpt\.com|openai\.com|claude\.ai|gemini\.google\.com|copilot\.microsoft\.com|perplexity\.ai|poe\.com|character\.ai|meta\.ai|grok\.com|x\.ai|deepseek\.com|chat\.mistral\.ai|huggingface\.co|you\.com|pi\.ai|kimi\.moonshot\.cn|chat\.qwen\.ai)$/i;
  const PAYMENT = /(^|\.)(stripe\.com|paypal\.com|checkout\.com|adyen\.com|klarna\.com|affirm\.com|shopify\.com|amazon\.(?:com|co\.uk|co\.jp|com\.au|com\.br|com\.mx|com\.tr|de|fr|it|es|ca|in|nl|se|pl|sg|ae|sa|eg)|apple\.com|google\.com)$/i;
  function classify(textValue) {
    const found = [];
    const digitsOnly = textValue.replace(/[\s-]/g, "");
    const cards = digitsOnly.match(/(?<!\d)\d{13,19}(?!\d)/g) || [];
    if (cards.some((card) => luhn(card) && /^(4|5[1-5]|2[2-7]|3[47]|6(011|5)|35)/.test(card))) found.push("a card number");
    if (/\b\d{3}-\d{2}-\d{4}\b/.test(textValue)) found.push("a social security number");
    if (/\b[A-Z]{2}\d{2}[A-Z0-9]{11,30}\b/.test(digitsOnly.toUpperCase()) && /\b(DE|GB|FR|NL|ES|IT|BE|CH|AT|IE|PT|SE|NO|DK|FI|PL)\d{2}/i.test(textValue)) found.push("an IBAN");
    if (/\b(sk|rk)-(live|test|proj)?[-_]?[A-Za-z0-9]{16,}|\bAKIA[0-9A-Z]{16}\b|\bgh[pousr]_[A-Za-z0-9]{30,}\b|\bxox[baprs]-[A-Za-z0-9-]{10,}\b|-----BEGIN [A-Z ]*PRIVATE KEY-----|\bAIza[0-9A-Za-z_-]{35}\b|\beyJ[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{10,}\b/.test(textValue)) found.push("an API key or token");
    const words = textValue.trim().toLowerCase().split(/\s+/);
    if ((words.length === 12 || words.length === 24) && words.every((word) => /^[a-z]{3,8}$/.test(word))) found.push("what looks like a wallet seed phrase");
    if (/\b(password|passwd|pwd)\s*[:=]\s*\S{6,}/i.test(textValue)) found.push("a password");
    if (/\b[A-PR-WY][1-9]\d\s?\d{4}[1-9]\b/.test(textValue) && /passport/i.test(textValue)) found.push("a passport number");
    return found;
  }

  // ---- the paste bar ------------------------------------------------------------------------------
  let bar = null;
  function askPaste(found, onAllow) {
    if (bar) bar.remove();
    bar = document.createElement(tagName("bar"));
    const shadow = bar.attachShadow({ mode: "closed" });
    const style = document.createElement("style");
    style.textContent = `
      :host { all: initial; position: fixed; top: 0; left: 0; right: 0; z-index: 2147483647; font: 14px/1.4 -apple-system, "Segoe UI", system-ui, sans-serif; }
      .bar { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; padding: 12px 18px; background: #0b0e0c; color: #d8ddd6; border-bottom: 1px solid rgba(180,210,190,.12); box-shadow: 0 6px 24px rgba(0,0,0,.35); }
      .bar span { flex: 1 1 auto; min-width: 200px; } b { color: #fff; } small { display: block; color: #9aa298; }

      @keyframes ns-arrive { from { opacity: 0; } to { opacity: 1; } }
      :host { animation: ns-arrive .22s ease-out; }
      @media (prefers-reduced-motion: reduce) { :host, * { animation: none !important; transition: none !important; } }
      button { font: inherit; padding: 6px 12px; border-radius: 8px; border: 1px solid rgba(180,210,190,.16); background: #131916; color: #d8ddd6; cursor: pointer; }
      button.stop { border-color: #6b2a2a; background: #2a1414; }
    `;
    const box = document.createElement("div");
    box.className = "bar";
    const textNode = document.createElement("span");
    const strong = document.createElement("b");
    strong.textContent = "You are pasting " + found.join(" and ") + " into " + location.hostname + ".";
    const small = document.createElement("small");
    small.textContent = AI_CHAT.test(location.hostname)
      ? "Chat services keep what you send and may train on it. Take the private part out first."
      : "This site is not a payment processor or a bank you trusted. Pasting anyway is your call.";
    textNode.append(strong, small);
    const allow = document.createElement("button");
    allow.textContent = "Paste anyway";
    const stop = document.createElement("button");
    stop.className = "stop";
    stop.textContent = "Don't paste";
    allow.addEventListener("click", () => { bar.remove(); bar = null; onAllow(); });
    stop.addEventListener("click", () => { bar.remove(); bar = null; send({ type: "safety.event", kind: "pastes", amount: 1 }); });
    box.append(textNode, allow, stop);
    shadow.append(style, box);
    (document.documentElement || document).append(bar);
  }

  function insert(target, value) {
    if (!target) return;
    if (target.isContentEditable) {
      document.execCommand("insertText", false, value);
      return;
    }
    if ("value" in target) {
      const start = target.selectionStart ?? target.value.length;
      const end = target.selectionEnd ?? target.value.length;
      const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(target), "value");
      const next = target.value.slice(0, start) + value + target.value.slice(end);
      if (setter && setter.set) setter.set.call(target, next); else target.value = next;
      target.dispatchEvent(new Event("input", { bubbles: true }));
      target.selectionStart = target.selectionEnd = start + value.length;
    }
  }

  function watchPastes(config) {
    document.addEventListener("paste", (event) => {
      const value = event.clipboardData && event.clipboardData.getData("text/plain");
      if (!value || value.length > 20000) return;
      const found = classify(value);
      if (!found.length) return;
      const site = location.hostname;
      if (PAYMENT.test(site) && !AI_CHAT.test(site)) return;
      if (config.trusted && !AI_CHAT.test(site)) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      const target = event.target;
      askPaste(found, () => insert(target, value));
    }, true);
  }

  // ---- uploads: EXIF, XMP and location out of photos ------------------------------------------------
  function stripJpeg(bytes) {
    if (bytes[0] !== 0xff || bytes[1] !== 0xd8) return null;
    const out = [bytes.subarray(0, 2)];
    let offset = 2;
    let removed = false;
    while (offset + 4 <= bytes.length) {
      if (bytes[offset] !== 0xff) break;
      const marker = bytes[offset + 1];
      if (marker === 0xda) { out.push(bytes.subarray(offset)); break; }
      const length = (bytes[offset + 2] << 8) | bytes[offset + 3];
      const segment = bytes.subarray(offset, offset + 2 + length);
      // APP1 (EXIF and XMP), APP2 ICC stays, APP13 (Photoshop IPTC), comments: out.
      if (marker === 0xe1 || marker === 0xed || marker === 0xfe || (marker >= 0xe3 && marker <= 0xef && marker !== 0xe2)) removed = true;
      else out.push(segment);
      offset += 2 + length;
    }
    if (!removed) return null;
    const total = out.reduce((sum, part) => sum + part.length, 0);
    const result = new Uint8Array(total);
    let position = 0;
    for (const part of out) { result.set(part, position); position += part.length; }
    return result;
  }
  function stripPng(bytes) {
    const signature = [137, 80, 78, 71, 13, 10, 26, 10];
    if (!signature.every((value, index) => bytes[index] === value)) return null;
    const out = [bytes.subarray(0, 8)];
    let offset = 8;
    let removed = false;
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    while (offset + 8 <= bytes.length) {
      const length = view.getUint32(offset);
      const type = String.fromCharCode(bytes[offset + 4], bytes[offset + 5], bytes[offset + 6], bytes[offset + 7]);
      const chunk = bytes.subarray(offset, offset + 12 + length);
      if (["tEXt", "zTXt", "iTXt", "eXIf", "tIME"].includes(type)) removed = true;
      else out.push(chunk);
      offset += 12 + length;
      if (type === "IEND") break;
    }
    if (!removed) return null;
    const total = out.reduce((sum, part) => sum + part.length, 0);
    const result = new Uint8Array(total);
    let position = 0;
    for (const part of out) { result.set(part, position); position += part.length; }
    return result;
  }
  function stripWebp(bytes) {
    if (String.fromCharCode(...bytes.subarray(0, 4)) !== "RIFF" || String.fromCharCode(...bytes.subarray(8, 12)) !== "WEBP") return null;
    const out = [];
    let offset = 12;
    let removed = false;
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    while (offset + 8 <= bytes.length) {
      const type = String.fromCharCode(...bytes.subarray(offset, offset + 4));
      const length = view.getUint32(offset + 4, true);
      const padded = length + (length % 2);
      const chunk = bytes.subarray(offset, offset + 8 + padded);
      if (type === "EXIF" || type === "XMP ") removed = true;
      else out.push(chunk);
      offset += 8 + padded;
    }
    if (!removed) return null;
    const body = out.reduce((sum, part) => sum + part.length, 0);
    const result = new Uint8Array(12 + body);
    result.set(bytes.subarray(0, 12), 0);
    new DataView(result.buffer).setUint32(4, 4 + body, true);
    let position = 12;
    for (const part of out) { result.set(part, position); position += part.length; }
    return result;
  }

  async function cleanFile(file) {
    if (!/^image\/(jpeg|png|webp)$/.test(file.type) || file.size > 40 * 1024 * 1024) return null;
    const bytes = new Uint8Array(await file.arrayBuffer());
    const cleaned = file.type === "image/jpeg" ? stripJpeg(bytes) : file.type === "image/png" ? stripPng(bytes) : stripWebp(bytes);
    if (!cleaned) return null;
    return new File([cleaned], file.name, { type: file.type, lastModified: Date.now() });
  }

  function watchUploads() {
    document.addEventListener("change", async (event) => {
      const input = event.target;
      if (!input || input.tagName !== "INPUT" || input.type !== "file" || !input.files || !input.files.length || input.dataset.noahCleaned) return;
      const transfer = new DataTransfer();
      let cleanedCount = 0;
      for (const file of Array.from(input.files)) {
        const cleaned = await cleanFile(file);
        transfer.items.add(cleaned || file);
        if (cleaned) cleanedCount++;
      }
      if (!cleanedCount) return;
      input.dataset.noahCleaned = "1";
      event.stopImmediatePropagation();
      input.files = transfer.files;
      input.dispatchEvent(new Event("change", { bubbles: true }));
      setTimeout(() => delete input.dataset.noahCleaned, 500);
      send({ type: "safety.event", kind: "uploads", amount: cleanedCount });
    }, true);
  }

  window.__noahShieldStrip = { stripJpeg, stripPng, stripWebp, classify };

  waitForConfig().then((config) => {
    if (!config) return;
    if (config.pasteGuard !== false) watchPastes(config);
    if (config.uploadStrip !== false && !config.trusted) watchUploads();
  });
})();
