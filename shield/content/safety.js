// Account safety and anti-scam, in the isolated world of every page:
// lookalike domains, password reuse and password over http, breach checks,
// hidden autofill fields, hidden third-party frames, fake support pages.
(() => {
  "use strict";
  const api = globalThis.chrome ?? globalThis.browser;
  const send = (message) => new Promise((resolve) => {
    try {
      api.runtime.sendMessage(message, (response) => {
        void api.runtime.lastError;
        resolve(response || {});
      });
    } catch {
      resolve({});
    }
  });
  const isTop = window.top === window;
  const host = location.hostname;

  function waitForConfig() {
    return new Promise((resolve) => {
      if (window.__noahShieldSiteConfig) return resolve(window.__noahShieldSiteConfig);
      document.addEventListener("noah-shield:site-config", () => resolve(window.__noahShieldSiteConfig), { once: true });
      setTimeout(() => resolve(window.__noahShieldSiteConfig || null), 4000);
    });
  }

  // ---- a full-page notice the page cannot press ---------------------------
  let noticeHost = null;
  function notice({ title, lines, buttons, tone }) {
    if (noticeHost) noticeHost.remove();
    noticeHost = document.createElement("noah-shield-notice");
    const shadow = noticeHost.attachShadow({ mode: "closed" });
    const style = document.createElement("style");
    style.textContent = `
      :host { all: initial; position: fixed; inset: 0; z-index: 2147483647; display: grid; place-items: center; background: rgba(4,6,5,.86); backdrop-filter: blur(10px); font: 15px/1.5 Georgia, "Times New Roman", serif; color: #d8ddd6; }
      .card { width: min(560px, calc(100vw - 40px)); background: rgba(8,11,9,.96); border: 1px solid rgba(180,210,190,.12); border-radius: 18px; padding: 34px 36px; box-shadow: 0 30px 80px rgba(0,0,0,.5); }
      .eyebrow { font: 12px/1 -apple-system, "Segoe UI", system-ui, sans-serif; letter-spacing: .08em; text-transform: lowercase; color: #9aa298; margin-bottom: 14px; }
      h1 { font-size: 30px; font-weight: 400; line-height: 1.15; margin: 0 0 14px; color: #f1f4ef; }
      h1.warn { color: #e8b4a8; }
      p { margin: 0 0 10px; color: #b9beb7; }
      .actions { display: flex; flex-wrap: wrap; gap: 10px; margin-top: 22px; font-family: -apple-system, "Segoe UI", system-ui, sans-serif; }
      button { font: 14px -apple-system, "Segoe UI", system-ui, sans-serif; padding: 10px 16px; border-radius: 10px; border: 1px solid rgba(180,210,190,.16); background: rgba(10,16,12,.9); color: #d8ddd6; cursor: pointer; }
      button.primary { background: #5f8a58; border-color: #5f8a58; color: #f1f4ef; }
      button:hover { filter: brightness(1.15); }
      small { display: block; margin-top: 16px; font: 12px -apple-system, "Segoe UI", system-ui, sans-serif; color: #8f958d; }
    `;
    const card = document.createElement("div");
    card.className = "card";
    const eyebrow = document.createElement("div");
    eyebrow.className = "eyebrow";
    eyebrow.textContent = "noah shield";
    const heading = document.createElement("h1");
    heading.textContent = title;
    if (tone === "warn") heading.className = "warn";
    card.append(eyebrow, heading);
    for (const line of lines) {
      const paragraph = document.createElement("p");
      paragraph.textContent = line;
      card.append(paragraph);
    }
    const actions = document.createElement("div");
    actions.className = "actions";
    for (const { label, primary, onClick } of buttons) {
      const button = document.createElement("button");
      button.textContent = label;
      if (primary) button.className = "primary";
      button.addEventListener("click", () => {
        noticeHost.remove();
        noticeHost = null;
        onClick();
      });
      actions.append(button);
    }
    card.append(actions);
    const small = document.createElement("small");
    small.textContent = "This notice comes from the noah shield extension, not from the page. The page cannot press its buttons.";
    card.append(small);
    shadow.append(style, card);
    (document.documentElement || document).append(noticeHost);
    // Escape puts the notice away without choosing anything.
    const onEscape = (event) => {
      if (event.key !== "Escape" || !noticeHost) return;
      noticeHost.remove();
      noticeHost = null;
      window.removeEventListener("keydown", onEscape, true);
    };
    window.addEventListener("keydown", onEscape, true);
  }

  // ---- lookalike domains ---------------------------------------------------
  async function checkLookalike(config) {
    if (!isTop || !config.lookalike) return;
    const result = await send({ type: "lookalike.check", host });
    if (!result || !result.brand || result.allowed) return;
    notice({
      tone: "warn",
      title: `This is not ${result.brand}.`,
      lines: [
        `${host} looks like ${result.brand} (${result.reason}), but ${result.brand} lives at ${result.real}. Pages like this are made to take a password or a card number.`,
        "If you meant to visit " + result.real + ", go there directly.",
      ],
      buttons: [
        { label: "Leave this page", primary: true, onClick: () => { location.replace("https://" + result.real); } },
        { label: "I know this site, continue", onClick: () => send({ type: "lookalike.allow", site: result.site }) },
      ],
    });
    send({ type: "safety.event", kind: "phishing", detail: host });
  }

  // ---- passwords -------------------------------------------------------------
  let salt = null;
  // crypto.subtle is missing on plain http pages; content/hash.js fills in.
  async function digestHex(algorithm, text) {
    if (crypto.subtle) {
      const digest = await crypto.subtle.digest(algorithm, new TextEncoder().encode(text));
      return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
    }
    const fallback = globalThis.__noahShieldHash;
    return algorithm === "SHA-1" ? fallback.sha1(text) : fallback.sha256(text);
  }
  async function hashPassword(password) {
    if (!salt) salt = (await send({ type: "password.salt" })).salt || "";
    return (await digestHex("SHA-256", salt + "\n" + password)).slice(0, 32);
  }
  async function sha1Hex(text) {
    return (await digestHex("SHA-1", text)).toUpperCase();
  }

  const checkedPasswords = new Set();
  async function passwordLeaving(input, config) {
    const password = input.value;
    if (!password || password.length < 6 || checkedPasswords.has(password)) return;
    checkedPasswords.add(password);
    if (config.passwordHttp && location.protocol === "http:") {
      notice({
        tone: "warn",
        title: "This password would travel unencrypted.",
        lines: [`${host} is a plain http page, so anything typed here can be read on the way. A real login page uses https.`],
        buttons: [{ label: "Understood", primary: true, onClick: () => {} }],
      });
      send({ type: "safety.event", kind: "passwordHttp", detail: host });
    }
    if (config.passwordReuse) {
      const hash = await hashPassword(password);
      const seen = await send({ type: "password.seen", hash });
      if (seen && seen.reused && seen.reused.length) {
        notice({
          tone: "warn",
          title: "You have used this password on " + seen.reused[0] + ".",
          lines: [
            `You are typing it into ${host}. If you meant to sign in to ${seen.reused[0]}, this page may be pretending to be it.`,
            "If you simply reuse the password on both, that is a separate risk: a leak at one site opens the other.",
          ],
          buttons: [
            { label: "Leave this page", primary: true, onClick: () => { history.length > 1 ? history.back() : location.replace("about:blank"); } },
            { label: "This is fine", onClick: () => {} },
          ],
        });
        send({ type: "safety.event", kind: "reuse", detail: host });
      }
    }
    if (config.breachCheck) {
      const sha1 = await sha1Hex(password);
      const answer = await send({ type: "password.breach", prefix: sha1.slice(0, 5), suffix: sha1.slice(5) });
      if (answer && answer.count > 0) {
        notice({
          tone: "warn",
          title: `This password is in ${answer.count.toLocaleString()} known leaks.`,
          lines: ["It appears in breach data anyone can download, so it is on the lists attackers try first. Change it here and anywhere else you use it."],
          buttons: [{ label: "Understood", primary: true, onClick: () => {} }],
        });
        send({ type: "safety.event", kind: "breached", detail: host });
      }
    }
  }

  function watchPasswords(config) {
    if (!config.passwordReuse && !config.passwordHttp && !config.breachCheck) return;
    document.addEventListener("focusout", (event) => {
      const input = event.target;
      if (input && input.tagName === "INPUT" && input.type === "password") passwordLeaving(input, config);
    }, true);
    document.addEventListener("submit", (event) => {
      const form = event.target;
      if (!form || !form.querySelector) return;
      const input = form.querySelector('input[type="password"]');
      if (input) passwordLeaving(input, config);
    }, true);
    document.addEventListener("keydown", (event) => {
      if (event.key !== "Enter") return;
      const input = event.target;
      if (input && input.tagName === "INPUT" && input.type === "password") passwordLeaving(input, config);
    }, true);
  }

  // ---- hidden autofill fields ----------------------------------------------
  const AUTOFILL = /^(email|username|name|given-name|family-name|tel|cc-|street-address|address|postal-code|current-password|new-password|one-time-code)/;
  function invisible(element) {
    const style = getComputedStyle(element);
    if (style.display === "none" || style.visibility === "hidden" || parseFloat(style.opacity) < 0.05) return true;
    const box = element.getBoundingClientRect();
    if (box.width < 2 || box.height < 2) return true;
    if (box.right < -100 || box.bottom < -100) return true;
    return false;
  }
  let disarmed = 0;
  function disarmHiddenFields() {
    for (const input of document.querySelectorAll("input:not([data-noah-shield])")) {
      const kind = (input.getAttribute("autocomplete") || input.name || input.id || "").toLowerCase();
      if (!AUTOFILL.test(kind) || input.type === "hidden") continue;
      if (!invisible(input)) continue;
      input.setAttribute("autocomplete", "off");
      input.setAttribute("data-noah-shield", "hidden-field");
      disarmed++;
    }
    if (disarmed) {
      send({ type: "safety.event", kind: "hiddenFields", detail: host, amount: disarmed });
      disarmed = 0;
    }
  }

  // ---- hidden third-party frames -------------------------------------------
  const FRAME_ALLOW = /(^|\.)(google\.com|gstatic\.com|recaptcha\.net|hcaptcha\.com|cloudflare\.com|stripe\.com|paypal\.com|paypalobjects\.com|braintreegateway\.com|adyen\.com|checkout\.com|klarna\.com|apple\.com|youtube\.com|youtube-nocookie\.com|vimeo\.com|facebook\.com|accounts\.google\.com|login\.microsoftonline\.com|okta\.com|auth0\.com|doubleclick\.net)$/;
  function siteOf(name) {
    const parts = name.split(".");
    return parts.length <= 2 ? name : parts.slice(-2).join(".");
  }
  function checkFrames() {
    for (const frame of document.querySelectorAll("iframe:not([data-noah-shield])")) {
      const source = frame.getAttribute("src") || "";
      if (!/^https?:/.test(source)) continue;
      let frameHost;
      try {
        frameHost = new URL(source).hostname;
      } catch {
        continue;
      }
      if (siteOf(frameHost) === siteOf(host) || FRAME_ALLOW.test(frameHost)) continue;
      const style = getComputedStyle(frame);
      const box = frame.getBoundingClientRect();
      const hidden = style.display === "none" || style.visibility === "hidden" || parseFloat(style.opacity) < 0.05 || (box.width <= 2 && box.height <= 2) || box.right < -50;
      if (!hidden) continue;
      frame.setAttribute("data-noah-shield", "hidden-frame");
      frame.remove();
      send({ type: "safety.event", kind: "frames", detail: frameHost });
    }
  }

  // ---- fake support pages ----------------------------------------------------
  const SCAM = [
    /your (computer|pc|mac|device|windows|system) (is|has been) (infected|locked|blocked|compromised|hacked)/i,
    /call (microsoft|apple|windows|google|amazon|support|us|now)[^.]{0,60}\d{3}[-. )]\s?\d{3}[-. ]\d{4}/i,
    /toll[- ]free[^.]{0,40}\d{3}[-. )]\s?\d{3}[-. ]\d{4}/i,
    /do not (close|shut ?down|restart|turn off) (this|your|the)/i,
    /(virus|trojan|spyware|malware) (alert|warning|detected)/i,
    /windows (defender|security) (alert|warning|scan)/i,
    /(error|threat) (code|#)\s*[:#]?\s*[A-Z0-9x-]{4,}/i,
    /(your|the) (account|apple id|icloud|paypal|bank account) (has been|is|was) (suspended|locked|limited|compromised)[^.]{0,80}(call|phone|verify now)/i,
  ];
  let scamHandled = false;
  function checkScam(config) {
    if (scamHandled || !config.scamPopups || !isTop) return;
    const text = (document.body && document.body.innerText || "").slice(0, 60000);
    let hits = 0;
    for (const pattern of SCAM) if (pattern.test(text)) hits++;
    const urgency = document.fullscreenElement !== null || /call now|immediately|within \d+ (minutes|hours)|urgent/i.test(text);
    if (hits >= 2 || (hits >= 1 && urgency)) {
      scamHandled = true;
      if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
      document.dispatchEvent(new CustomEvent("noah-shield:scam"));
      notice({
        tone: "warn",
        title: "This page is a support scam.",
        lines: [
          "It says your device is infected or locked and asks you to call a number. No real company does that from a web page; the number leads to people who will ask for payment or remote access.",
          "Your device is fine. Close this tab and do not call.",
        ],
        buttons: [
          { label: "Close this tab", primary: true, onClick: () => send({ type: "scam.close" }) },
          { label: "Keep it open", onClick: () => {} },
        ],
      });
      send({ type: "safety.event", kind: "popups", detail: host });
    }
  }

  // ---- start -----------------------------------------------------------------
  waitForConfig().then((config) => {
    if (!config || config.trusted) return;
    checkLookalike(config);
    watchPasswords(config);
    const sweep = () => {
      if (config.hiddenFields) disarmHiddenFields();
      if (config.hiddenFrames) checkFrames();
      checkScam(config);
    };
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", sweep, { once: true });
    else sweep();
    setTimeout(sweep, 2500);
    let pending = null;
    new MutationObserver(() => {
      if (pending) return;
      pending = setTimeout(() => { pending = null; sweep(); }, 700);
    }).observe(document.documentElement, { childList: true, subtree: true });
  });
})();
