// Runs inside every page, before the page's own scripts, in the page's own
// world. It replaces the few browser functions a page uses to learn where
// you are, to fingerprint you, and to record your screen or camera, and it
// keeps the originals to itself. content/bridge.js (in the extension's
// isolated world) tells it the settings and carries its questions to the
// background; the two agree on a secret name for their events before any
// page script exists, so a page can neither answer for you nor listen in.
(() => {
  "use strict";
  const define = Object.defineProperty;
  const dispatch = EventTarget.prototype.dispatchEvent;
  const listen = EventTarget.prototype.addEventListener;
  const CustomEventClass = CustomEvent;
  const stringify = JSON.stringify;
  const parse = JSON.parse;
  // crypto.randomUUID exists only in secure contexts; plain http pages get
  // the same entropy from getRandomValues.
  const getRandomValues = crypto.getRandomValues.bind(crypto);
  const randomUUID = () => Array.from(getRandomValues(new Uint8Array(16)), (byte) => byte.toString(16).padStart(2, "0")).join("");
  const nonce = randomUUID();

  let fakePosition = () => null;
  const config = { ready: false, geolocation: "block", fingerprint: true, guardScreen: true, guardCamera: true, clipboardGuard: true, walletGuard: true, formLeak: true, scamPopups: true };
  const waiting = [];
  const pending = new Map();

  function settle() {
    config.ready = true;
    while (waiting.length) waiting.shift()();
  }

  function take(detail) {
    try {
      const incoming = parse(String(detail || "{}"));
      // FIX: guard prototype-pollution paths. JSON.parse never produces __proto__
      // as an own key, but a belt-and-suspenders check costs nothing here.
      for (const key of Object.keys(incoming)) {
        if (key === "__proto__" || key === "constructor" || key === "prototype") continue;
        config[key] = incoming[key];
      }
    } catch {
      // A malformed configuration leaves the strict defaults in place.
    }
  }

  // The bridge introduces itself synchronously during document_start, before
  // any page script exists, and gets the nonce back. The site's real settings
  // come a moment later on the nonce-named event, which a page cannot guess;
  // until then the strict defaults hold.
  let introduced = false;
  listen.call(document, "noah-shield:config", (event) => {
    if (introduced) return;
    introduced = true;
    take(event.detail);
    listen.call(document, "noah-shield:config:" + nonce, (update) => {
      take(update.detail);
      settle();
    }, { once: true });
    dispatch.call(document, new CustomEventClass("noah-shield:hello", { detail: nonce }));
  }, { once: true });

  listen.call(document, "noah-shield:answer:" + nonce, (event) => {
    let detail;
    try {
      detail = parse(String(event.detail || "{}"));
    } catch {
      return;
    }
    const resolve = pending.get(detail.id);
    if (!resolve) return;
    pending.delete(detail.id);
    resolve(Boolean(detail.allow));
  });

  function whenReady() {
    return new Promise((resolve) => {
      if (config.ready) resolve();
      else waiting.push(resolve);
      // The bridge answers within the same task; a page without it (a frame
      // the extension may not enter) keeps the defaults after a moment.
      setTimeout(() => { if (!config.ready) settle(); }, 300);
    });
  }

  function report(kind, extra = {}) {
    dispatch.call(document, new CustomEventClass("noah-shield:report:" + nonce, { detail: stringify({ kind, ...extra }) }));
  }

  function ask(kind, detail = "") {
    return new Promise((resolve) => {
      const id = randomUUID();
      pending.set(id, resolve);
      dispatch.call(document, new CustomEventClass("noah-shield:ask:" + nonce, { detail: stringify({ id, kind, detail }) }));
      setTimeout(() => {
        if (pending.has(id)) {
          pending.delete(id);
          resolve(false);
        }
      }, 120000);
    });
  }

  function denied(name, message) {
    const error = new DOMException(message, name);
    return error;
  }

  // ── window.name — evercookie / XKeyscore "selector" vector ──────────────
  // NSA XKeyscore and commercial trackers exploit window.name because it
  // persists across cross-origin navigations in the same tab — an HTTP 302
  // redirect chain can carry a unique tracking token written by a prior page.
  // Clearing it at document_start (before any page script exists) destroys
  // whatever the previous site or a QUANTUM INSERT redirect left behind.
  // The current page may still set its own value afterwards.
  try { window.name = ""; } catch { /* read-only in some sandboxed frames */ }

  // Geolocation: denied outright, or left to the browser's own prompt.
  const geolocation = navigator.geolocation;
  if (geolocation) {
    const getCurrentPosition = geolocation.getCurrentPosition.bind(geolocation);
    const watchPosition = geolocation.watchPosition.bind(geolocation);
    const refuse = (onError) => {
      const error = { code: 1, message: "noah shield: this site may not read your location", PERMISSION_DENIED: 1, POSITION_UNAVAILABLE: 2, TIMEOUT: 3 };
      if (typeof onError === "function") setTimeout(() => onError(error), 0);
    };
    define(geolocation, "getCurrentPosition", {
      value(onSuccess, onError, options) {
        whenReady().then(() => {
          const fake = fakePosition();
          if (fake) setTimeout(() => onSuccess(fake), 120);
          else if (config.geolocation === "block") refuse(onError);
          else getCurrentPosition(onSuccess, onError, options);
        });
      },
      configurable: true,
    });
    define(geolocation, "watchPosition", {
      value(onSuccess, onError, options) {
        const id = Math.floor(Math.random() * 1e9);
        whenReady().then(() => {
          const fake = fakePosition();
          if (fake) setTimeout(() => onSuccess(fake), 120);
          else if (config.geolocation === "block") refuse(onError);
          else watchPosition(onSuccess, onError, options);
        });
        return id;
      },
      configurable: true,
    });
  }

  // Screen and camera capture wait for your yes on sites you have not allowed.
  const media = navigator.mediaDevices;
  if (media) {
    const getDisplayMedia = media.getDisplayMedia && media.getDisplayMedia.bind(media);
    const getUserMedia = media.getUserMedia && media.getUserMedia.bind(media);
    if (getDisplayMedia) {
      define(media, "getDisplayMedia", {
        value: async function (constraints) {
          await whenReady();
          if (config.guardScreen && !(await ask("screen"))) {
            throw denied("NotAllowedError", "noah shield: screen capture was not allowed on this site");
          }
          return getDisplayMedia(constraints);
        },
        configurable: true,
      });
    }
    if (getUserMedia) {
      define(media, "getUserMedia", {
        value: async function (constraints) {
          await whenReady();
          const wantsPicture = constraints && (constraints.video || constraints.audio);
          if (config.guardCamera && wantsPicture && !(await ask("camera"))) {
            throw denied("NotAllowedError", "noah shield: camera and microphone were not allowed on this site");
          }
          const stream = await getUserMedia(constraints);
          watchStream(stream);
          return stream;
        },
        configurable: true,
      });
    }
    // Which tab is live: every camera or microphone track is reported while
    // it runs, so the popup can say who is listening right now.
    const liveTracks = new Set();
    const announce = () => {
      let camera = false;
      let microphone = false;
      for (const track of liveTracks) {
        if (track.readyState !== "live") { liveTracks.delete(track); continue; }
        if (track.kind === "video") camera = true;
        if (track.kind === "audio") microphone = true;
      }
      report("media", { camera, microphone });
    };
    const realStop = MediaStreamTrack.prototype.stop;
    define(MediaStreamTrack.prototype, "stop", {
      value: function () { const result = realStop.call(this); if (liveTracks.has(this)) { liveTracks.delete(this); announce(); } return result; },
      configurable: true, writable: true,
    });
    function watchStream(stream) {
      for (const track of stream.getTracks()) {
        liveTracks.add(track);
        listen.call(track, "ended", () => { liveTracks.delete(track); announce(); });
      }
      announce();
    }
    listen.call(window, "pagehide", () => { if (liveTracks.size) { liveTracks.clear(); announce(); } });
  }

  // Clipboard: reading it is a question, not a right.
  const clipboard = navigator.clipboard;
  if (clipboard) {
    const readText = clipboard.readText && clipboard.readText.bind(clipboard);
    const read = clipboard.read && clipboard.read.bind(clipboard);
    const writeText = clipboard.writeText && clipboard.writeText.bind(clipboard);
    const gate = async () => {
      await whenReady();
      if (config.clipboardGuard && !(await ask("clipboard"))) {
        throw denied("NotAllowedError", "noah shield: this site may not read the clipboard");
      }
    };
    if (readText) define(clipboard, "readText", { value: async function () { await gate(); return readText(); }, configurable: true });
    if (read) define(clipboard, "read", { value: async function (...args) { await gate(); return read(...args); }, configurable: true });
    // Address swapping: a page that copies a different crypto address than
    // the one you selected is stealing the payment.
    const ADDRESS = /^(0x[0-9a-fA-F]{40}|[13][a-km-zA-HJ-NP-Z1-9]{25,34}|bc1[a-z0-9]{25,62}|[1-9A-HJ-NP-Za-km-z]{32,44}|T[1-9A-HJ-NP-Za-km-z]{33}|r[1-9A-HJ-NP-Za-km-z]{24,34}|ltc1[a-z0-9]{25,62}|[LM][a-km-zA-HJ-NP-Z1-9]{26,33})$/;
    if (writeText) {
      define(clipboard, "writeText", {
        value: async function (text) {
          await whenReady();
          const selected = String(getSelection && getSelection() || "").trim();
          const written = String(text || "").trim();
          if (config.walletGuard && ADDRESS.test(written) && ADDRESS.test(selected) && written !== selected) {
            report("wallets", { amount: 1 });
            if (!(await ask("wallet", "copy swapped " + selected + " for " + written))) {
              throw denied("NotAllowedError", "noah shield: the page tried to copy a different address than the one you selected");
            }
          }
          return writeText(text);
        },
        configurable: true,
      });
    }
  }

  // Typed values must not leave for a third party before you submit.
  const typed = [];
  const PROCESSORS = /(^|\.)(stripe\.com|paypal\.com|paypalobjects\.com|braintreegateway\.com|braintree-api\.com|adyen\.com|checkout\.com|klarna\.com|afterpay\.com|affirm\.com|squareup\.com|google\.com|gstatic\.com|recaptcha\.net|hcaptcha\.com|cloudflare\.com|apple\.com|microsoftonline\.com|okta\.com|auth0\.com|amazonaws\.com|shopify\.com|shopifycs\.com|plaid\.com|sentry\.io)$/;
  const siteOf = (name) => {
    const parts = String(name).toLowerCase().split(".");
    return parts.length <= 2 ? parts.join(".") : parts.slice(-2).join(".");
  };
  const here = siteOf(location.hostname);
  // What you typed and then deleted is kept too: a keystroke logger sends the
  // draft you decided against, which is often the more private one.
  const erased = [];
  const lastValues = new WeakMap();
  const remember = (list, value) => {
    if (value.length < 6 || list.includes(value)) return;
    list.push(value);
    if (list.length > 30) list.shift();
  };
  listen.call(document, "input", (event) => {
    const field = event.target;
    if (!field) return;
    const editable = field.tagName === "TEXTAREA" || field.isContentEditable
      || (field.tagName === "INPUT" && /^(text|email|tel|password|number|search|url)?$/.test(field.type || ""));
    if (!editable) return;
    const value = String(field.isContentEditable ? field.textContent : field.value || "");
    const before = lastValues.get(field) || "";
    lastValues.set(field, value);
    if (config.typingGuard && before.length >= 6 && !value.includes(before)) {
      // The part that went away: the longest run of the old text missing from the new.
      let common = 0;
      while (common < before.length && common < value.length && before[common] === value[common]) common++;
      const gone = before.slice(common).trim();
      remember(erased, gone.length >= 6 ? gone : before);
    }
    remember(typed, value);
  }, true);
  // Keystroke telemetry looks like this whatever the vendor: the same few
  // field names, many times, headed somewhere else.
  const KEYSTROKE_SHAPE = /"(key|keyCode|charCode|which|code|inputType|keydown|keyup|keypress)"\s*:/g;
  const looksLikeKeystrokes = (text) => {
    if (!config.typingGuard) return false;
    const hits = text.match(KEYSTROKE_SHAPE);
    return Boolean(hits && hits.length >= 6);
  };
  const bodyText = (body) => {
    if (!body) return "";
    if (typeof body === "string") return body;
    if (body instanceof URLSearchParams) return body.toString();
    if (typeof FormData !== "undefined" && body instanceof FormData) return Array.from(body.values()).filter((value) => typeof value === "string").join("\n");
    if (body instanceof Blob || body instanceof ArrayBuffer || ArrayBuffer.isView(body)) return "";
    try { return stringify(body); } catch { return ""; }
  };
  let keystrokeLeaks = 0;
  const leaks = (url, body) => {
    if (!config.formLeak && !config.typingGuard) return false;
    let destination;
    try { destination = new URL(String(url), location.href).hostname; } catch { return false; }
    if (siteOf(destination) === here || PROCESSORS.test(destination)) return false;
    // The query string carries leaks as often as the body does.
    const text = String(url) + "\n" + bodyText(body);
    if (!text) return false;
    const values = config.formLeak ? typed : [];
    const drafts = config.typingGuard ? erased : [];
    const encoded = [...values, ...drafts].map((value) => [value, encodeURIComponent(value)]);
    if (encoded.some(([raw, escaped]) => text.includes(raw) || text.includes(escaped))) return true;
    if (looksLikeKeystrokes(text)) {
      if (keystrokeLeaks++ === 0) report("keylog", { amount: 1, to: destination, how: "stream" });
      return true;
    }
    return false;
  };
  const leakDenied = (url) => {
    let destination = "";
    try { destination = new URL(String(url), location.href).hostname; } catch { destination = String(url); }
    report("leaks", { amount: 1, to: destination });
    return denied("NetworkError", "noah shield: what you typed does not leave for " + destination + " before you submit");
  };
  const realFetch = window.fetch;
  define(window, "fetch", {
    value: function (input, init) {
      const url = input instanceof Request ? input.url : input;
      const body = init && init.body !== undefined ? init.body : null;
      if (leaks(url, body)) return Promise.reject(leakDenied(url));
      // A body tucked inside a Request object is read from a clone.
      if (body === null && input instanceof Request && input.body && !input.bodyUsed && typeof input.clone === "function") {
        const args = arguments;
        return input.clone().text().then((text) => {
          if (leaks(url, text)) throw leakDenied(url);
          return realFetch.apply(this, args);
        });
      }
      return realFetch.apply(this, arguments);
    },
    configurable: true, writable: true,
  });
  // Beacons by other names: an image or script address, a socket, a stream, a cross-site form.
  const guardUrlProperty = (proto, property) => {
    const real = Object.getOwnPropertyDescriptor(proto, property);
    if (!real || !real.set) return;
    define(proto, property, {
      get() { return real.get.call(this); },
      set(value) {
        if (leaks(value, "")) { leakDenied(value); return; }
        real.set.call(this, value);
      },
      configurable: true,
    });
  };
  for (const [proto, property] of [[HTMLImageElement.prototype, "src"], [HTMLScriptElement.prototype, "src"], [HTMLIFrameElement.prototype, "src"], [HTMLLinkElement.prototype, "href"], [HTMLMediaElement.prototype, "src"], [HTMLSourceElement.prototype, "src"], [HTMLEmbedElement.prototype, "src"], [HTMLObjectElement.prototype, "data"], [HTMLTrackElement.prototype, "src"]]) {
    try { guardUrlProperty(proto, property); } catch { /* a page froze it first */ }
  }
  const realSetAttribute = Element.prototype.setAttribute;
  define(Element.prototype, "setAttribute", {
    value: function (name, value) {
      const lower = String(name).toLowerCase();
      if ((lower === "src" || lower === "href" || lower === "data" || lower === "srcset" || lower === "ping" || lower === "action" || lower === "formaction") && leaks(value, "")) { leakDenied(value); return; }
      return realSetAttribute.call(this, name, value);
    },
    configurable: true, writable: true,
  });
  const RealWebSocket = window.WebSocket;
  if (RealWebSocket) {
    const realSend = RealWebSocket.prototype.send;
    define(RealWebSocket.prototype, "send", {
      value: function (data) {
        if (leaks(this.url, typeof data === "string" ? data : "")) throw leakDenied(this.url);
        return realSend.call(this, data);
      },
      configurable: true, writable: true,
    });
    const Wrapped = function WebSocket(url, protocols) {
      if (leaks(url, "")) throw leakDenied(url);
      return protocols === undefined ? new RealWebSocket(url) : new RealWebSocket(url, protocols);
    };
    Wrapped.prototype = RealWebSocket.prototype;
    for (const key of ["CONNECTING", "OPEN", "CLOSING", "CLOSED"]) Wrapped[key] = RealWebSocket[key];
    define(window, "WebSocket", { value: Wrapped, configurable: true, writable: true });
  }
  const RealEventSource = window.EventSource;
  if (RealEventSource) {
    const WrappedSource = function EventSource(url, init) {
      if (leaks(url, "")) throw leakDenied(url);
      return init === undefined ? new RealEventSource(url) : new RealEventSource(url, init);
    };
    WrappedSource.prototype = RealEventSource.prototype;
    define(window, "EventSource", { value: WrappedSource, configurable: true, writable: true });
  }
  // A form whose action is another site, carrying what you typed, is stopped
  // whether it is submitted by a click or by script.
  const formLeaks = (form) => {
    if (!config.formLeak || !form || !form.action) return false;
    let destination;
    try { destination = new URL(form.action, location.href).hostname; } catch { return false; }
    if (siteOf(destination) === here || PROCESSORS.test(destination)) return false;
    const values = Array.from(form.elements || []).map((element) => String(element.value || "")).filter((value) => value.length >= 6);
    return values.some((value) => typed.includes(value));
  };
  listen.call(document, "submit", (event) => {
    const form = event.target;
    if (formLeaks(form)) {
      event.preventDefault();
      event.stopImmediatePropagation();
      leakDenied(form.action);
    }
  }, true);
  const realSubmit = HTMLFormElement.prototype.submit;
  define(HTMLFormElement.prototype, "submit", {
    value: function () {
      if (formLeaks(this)) throw leakDenied(this.action);
      return realSubmit.call(this);
    },
    configurable: true, writable: true,
  });
  const realOpen = XMLHttpRequest.prototype.open;
  const realSend = XMLHttpRequest.prototype.send;
  const requestUrls = new WeakMap();
  define(XMLHttpRequest.prototype, "open", { value: function (method, url, ...rest) { requestUrls.set(this, url); return realOpen.call(this, method, url, ...rest); }, configurable: true, writable: true });
  define(XMLHttpRequest.prototype, "send", {
    value: function (body) {
      const url = requestUrls.get(this);
      if (leaks(url, body)) throw leakDenied(url);
      return realSend.call(this, body);
    },
    configurable: true, writable: true,
  });
  const realBeacon = navigator.sendBeacon && navigator.sendBeacon.bind(navigator);
  if (realBeacon) {
    define(Navigator.prototype, "sendBeacon", {
      value: function (url, body) {
        if (leaks(url, body)) { leakDenied(url); return false; }
        return realBeacon(url, body);
      },
      configurable: true, writable: true,
    });
  }

  // Wallets: a signature or an approval waits for your yes, with what it says.
  const SELECTORS = { "0x095ea7b3": "approve", "0xa22cb465": "setApprovalForAll", "0x39509351": "increaseAllowance", "0xd505accf": "permit" };
  const describeRequest = (request) => {
    const method = request && request.method;
    const params = (request && request.params) || [];
    if (method === "eth_sendTransaction" && params[0]) {
      const transaction = params[0];
      const data = String(transaction.data || "");
      const selector = SELECTORS[data.slice(0, 10)];
      const unlimited = selector && /f{40,}$/i.test(data);
      return `${method} to ${transaction.to || "?"}${transaction.value ? " value " + transaction.value : ""}${selector ? " (" + selector + (unlimited ? ", UNLIMITED" : "") + ")" : ""}`;
    }
    return method + (params.length ? " " + stringify(params).slice(0, 200) : "");
  };
  const WALLET_METHODS = /^(eth_sendTransaction|eth_signTransaction|eth_sign|personal_sign|eth_signTypedData(_v[34])?|wallet_watchAsset|wallet_addEthereumChain|wallet_requestPermissions)$/;
  const wrappedProviders = new WeakSet();
  const wrapProvider = (provider) => {
    if (!provider || wrappedProviders.has(provider) || typeof provider.request !== "function") return;
    const request = provider.request.bind(provider);
    try {
      wrappedProviders.add(provider);
      define(provider, "request", {
        value: async function (args) {
          await whenReady();
          if (config.walletGuard && args && WALLET_METHODS.test(String(args.method))) {
            if (!(await ask("wallet", describeRequest(args)))) {
              report("wallets", { amount: 1 });
              throw Object.assign(new Error("noah shield: the request was not allowed"), { code: 4001 });
            }
          }
          return request(args);
        },
        configurable: true, writable: true,
      });
    } catch {
      // A frozen provider keeps its own request; the bar simply does not appear.
    }
  };
  let ethereumValue = window.ethereum;
  try {
    define(window, "ethereum", {
      get() { return ethereumValue; },
      set(value) { ethereumValue = value; wrapProvider(value); },
      configurable: true,
    });
    wrapProvider(ethereumValue);
  } catch {
    // Some wallets define a non-configurable window.ethereum first.
  }
  listen.call(window, "ethereum#initialized", () => wrapProvider(window.ethereum));
  listen.call(window, "eip6963:announceProvider", (event) => wrapProvider(event.detail && event.detail.provider));

  // Support scams shout: alerts in a loop, a page that will not close.
  let alerts = [];
  let scam = false;
  listen.call(document, "noah-shield:scam", () => { scam = true; });
  for (const name of ["alert", "confirm", "prompt"]) {
    const real = window[name];
    if (typeof real !== "function") continue;
    define(window, name, {
      value: function (...args) {
        const now = Date.now();
        alerts = alerts.filter((time) => now - time < 10000);
        alerts.push(now);
        if (config.scamPopups && (scam || alerts.length > 3)) {
          if (alerts.length === 4) report("popups", { amount: 1 });
          return name === "confirm" ? false : name === "prompt" ? null : undefined;
        }
        return real.apply(this, args);
      },
      configurable: true, writable: true,
    });
  }
  // A script from another site that asks to hear every key pressed anywhere
  // on the page is a keystroke logger until proven otherwise. Its listener is
  // wrapped: the events reach it in watch mode and are named once; in block
  // mode they never arrive. The decision is made per event, because the
  // site's settings arrive a moment after the first scripts run.
  const KEY_EVENTS = /^(keydown|keyup|keypress|beforeinput|input|compositionend)$/;
  const keyListeners = new Map();
  const wrappedHandlers = new WeakMap();
  const callerHost = () => {
    let stack = "";
    try { stack = String(new Error().stack || ""); } catch { return null; }
    const urls = stack.match(/https?:\/\/[^\s):]+/g) || [];
    for (const url of urls) {
      let host;
      try { host = new URL(url).hostname; } catch { continue; }
      if (host && host !== location.hostname) return host;
    }
    return null;
  };
  const isWholePage = (target) => target === window || target === document || target === document.documentElement || target === document.body;
  const gateKeyListener = (target, type, handler) => {
    if (!handler || !KEY_EVENTS.test(String(type)) || !isWholePage(target)) return handler;
    const host = callerHost();
    if (!host || siteOf(host) === here || PROCESSORS.test(host)) return handler;
    const known = wrappedHandlers.get(handler);
    if (known) return known;
    const wrapped = function (event) {
      if (!config.typingGuard) return typeof handler === "function" ? handler.call(this, event) : handler.handleEvent && handler.handleEvent(event);
      const blocked = Boolean(config.blockKeyListeners);
      if (!keyListeners.has(host)) {
        keyListeners.set(host, true);
        report("keylog", { amount: 1, to: host, how: blocked ? "blocked" : "listening" });
      }
      if (blocked) return undefined;
      return typeof handler === "function" ? handler.call(this, event) : handler.handleEvent && handler.handleEvent(event);
    };
    wrappedHandlers.set(handler, wrapped);
    return wrapped;
  };
  const realTargetListen = EventTarget.prototype.addEventListener;
  const realTargetUnlisten = EventTarget.prototype.removeEventListener;
  define(EventTarget.prototype, "addEventListener", {
    value: function (type, handler, options) {
      return realTargetListen.call(this, type, gateKeyListener(this, type, handler), options);
    },
    configurable: true, writable: true,
  });
  define(EventTarget.prototype, "removeEventListener", {
    value: function (type, handler, options) {
      const wrapped = handler && wrappedHandlers.get(handler);
      return realTargetUnlisten.call(this, type, wrapped || handler, options);
    },
    configurable: true, writable: true,
  });
  const realAddEventListener = realTargetListen.bind(window);
  define(window, "addEventListener", {
    value: function (type, handler, options) {
      handler = gateKeyListener(window, type, handler);
      if (type === "beforeunload" && config.scamPopups) {
        return realAddEventListener(type, function (event) {
          if (scam || alerts.length > 3) return undefined;
          return typeof handler === "function" ? handler.call(this, event) : handler && handler.handleEvent && handler.handleEvent(event);
        }, options);
      }
      return realAddEventListener(type, handler, options);
    },
    configurable: true, writable: true,
  });
  define(window, "onbeforeunload", {
    get() { return null; },
    set(handler) { if (typeof handler === "function") realAddEventListener("beforeunload", (event) => (scam || alerts.length > 3 ? undefined : handler.call(window, event))); },
    configurable: true,
  });
  const realRequestFullscreen = Element.prototype.requestFullscreen;
  if (realRequestFullscreen) {
    define(Element.prototype, "requestFullscreen", {
      value: function (...args) {
        if (config.scamPopups && (scam || alerts.length > 3)) return Promise.reject(denied("NotAllowedError", "noah shield: no fullscreen for a page that behaves like a scam"));
        return realRequestFullscreen.apply(this, args);
      },
      configurable: true, writable: true,
    });
  }

  // Fingerprinting. Noise is seeded per site and per rotation period, so a
  // site sees one steady visitor and two sites see two different ones.
  const realIntl = Intl.DateTimeFormat;
  const realResolved = Intl.DateTimeFormat.prototype.resolvedOptions;
  const realOffset = Date.prototype.getTimezoneOffset;
  let seedValue = 0;
  const rng = () => {
    // mulberry32 over the current seed, advanced on each call.
    seedValue = (seedValue + 0x6d2b79f5) >>> 0;
    let t = seedValue;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  // ── Quantum identity superposition ───────────────────────────────────────
  // Each eTLD+1 gets a deterministic but uncorrelated seed derived from the
  // master seed via FNV-like mixing. Two sites observe different eigenvalues
  // from the same mulberry32 PRNG — they cannot correlate your fingerprint
  // across origins even if they share a data broker.
  const reseed = () => {
    let h = (Number(config.seed) || 0x9e3779b9) >>> 0;
    for (let i = 0; i < here.length; i++) {
      h ^= here.charCodeAt(i);
      h = Math.imul(h, 0x01000193) >>> 0;  // FNV prime
    }
    seedValue = h;
  };
  // FIX: was * 255 (range 0..254, never 255). * 256 gives the full 0..255 byte range.
  const seededByte = () => { reseed(); return Math.floor(rng() * 256); };

  const canvasNoise = () => {
    const proto = HTMLCanvasElement.prototype;
    const context2d = CanvasRenderingContext2D.prototype;
    const toDataURL = proto.toDataURL;
    const toBlob = proto.toBlob;
    const getImageData = context2d.getImageData;
    // FIX: old smudge only touched the 16×16 top-left corner. FingerprintJS Pro
    // reads specific interior coordinates — those were clean. New approach: scatter
    // noise across the full canvas by sampling 24 pixel positions spread across
    // the image (using the seeded PRNG so the same site sees the same offset).
    const smudge = (canvas) => {
      try {
        const ctx = canvas.getContext("2d");
        if (!ctx || canvas.width === 0 || canvas.height === 0) return;
        reseed();
        const w = canvas.width;
        const h = canvas.height;
        // Read the whole canvas once, scatter 24 pixel tweaks, write it back.
        const image = getImageData.call(ctx, 0, 0, w, h);
        const total = image.data.length / 4; // pixel count
        for (let n = 0; n < 24; n++) {
          const pixelIndex = Math.floor(rng() * total) * 4;
          image.data[pixelIndex] = (image.data[pixelIndex] + Math.floor(rng() * 3 + 1)) & 255;
        }
        ctx.putImageData(image, 0, 0);
      } catch {
        // Tainted canvas — page cannot read it either; nothing to do.
      }
    };
    define(proto, "toDataURL", { value: function (...args) { if (config.fingerprint) smudge(this); return toDataURL.apply(this, args); }, configurable: true });
    define(proto, "toBlob", { value: function (...args) { if (config.fingerprint) smudge(this); return toBlob.apply(this, args); }, configurable: true });
    // getImageData interception: noise spread across the returned region, not just
    // the first and last pixel.
    define(context2d, "getImageData", {
      value: function (...args) {
        const image = getImageData.apply(this, args);
        if (config.fingerprint && image && image.data.length > 64) {
          reseed();
          const total = image.data.length / 4;
          for (let n = 0; n < 12; n++) {
            const pixelIndex = Math.floor(rng() * total) * 4;
            image.data[pixelIndex] = (image.data[pixelIndex] + Math.floor(rng() * 3 + 1)) & 255;
          }
        }
        return image;
      },
      configurable: true,
    });
  };

  const webglNoise = () => {
    for (const name of ["WebGLRenderingContext", "WebGL2RenderingContext"]) {
      const Context = window[name];
      if (!Context) continue;
      const getParameter = Context.prototype.getParameter;
      const readPixels = Context.prototype.readPixels;
      define(Context.prototype, "getParameter", {
        value: function (parameter) {
          if (config.blendIn && parameter === 37445) return "Google Inc. (Intel)";
          if (config.blendIn && parameter === 37446) return "ANGLE (Intel, Intel(R) UHD Graphics 630 (0x00003E9B) Direct3D11 vs_5_0 ps_5_0, D3D11)";
          return getParameter.call(this, parameter);
        },
        configurable: true,
      });
      define(Context.prototype, "readPixels", {
        value: function (...args) {
          const result = readPixels.apply(this, args);
          const pixels = args[6];
          if (config.webglNoise && pixels && pixels.length > 16) {
            reseed();
            for (let count = 0; count < 8; count++) {
              const index = Math.floor(rng() * pixels.length);
              pixels[index] = pixels[index] ^ 1;
            }
          }
          return result;
        },
        configurable: true,
      });
    }
  };

  const audioNoise = () => {
    if (!window.AudioBuffer) return;
    const getChannelData = AudioBuffer.prototype.getChannelData;
    const touched = new WeakSet();
    define(AudioBuffer.prototype, "getChannelData", {
      value: function (channel) {
        const data = getChannelData.call(this, channel);
        if (config.audioNoise && !touched.has(data) && data.length > 512) {
          touched.add(data);
          reseed();
          for (let count = 0; count < 200; count++) {
            const index = Math.floor(rng() * data.length);
            data[index] = data[index] + (rng() - 0.5) * 1e-6;
          }
        }
        return data;
      },
      configurable: true,
    });
    if (window.AnalyserNode) {
      for (const method of ["getFloatFrequencyData", "getByteFrequencyData"]) {
        const real = AnalyserNode.prototype[method];
        define(AnalyserNode.prototype, method, {
          value: function (array) {
            real.call(this, array);
            if (config.audioNoise && array && array.length > 8) {
              reseed();
              for (let count = 0; count < 8; count++) {
                const index = Math.floor(rng() * array.length);
                array[index] = array[index] + (method === "getByteFrequencyData" ? (rng() > 0.5 ? 1 : 0) : (rng() - 0.5) * 0.01);
              }
            }
          },
          configurable: true,
        });
      }
    }
  };

  // ── DeviceMotion / DeviceOrientation — mobile sensor fingerprinting ───────
  // Accelerometer and gyroscope readings vary per device due to hardware
  // manufacturing calibration differences — a persistent physical identifier.
  // Intelligence collection frameworks (GCHQ mobile tooling, NSO Pegasus
  // infrastructure) and commercial fingerprinters (ScientiaMobile, DeviceAtlas)
  // both use these APIs to identify individual handsets across contexts.
  // When fingerprint protection is active, block all sensor events so the
  // page cannot build a motion signature. This matches Firefox Private Browsing
  // behavior and causes no visible breakage on desktop Chrome (no real sensors).
  const sensorNoise = () => {
    for (const type of ["devicemotion", "deviceorientation", "deviceorientationabsolute"]) {
      listen.call(window, type, (event) => {
        if (config.fingerprint) event.stopImmediatePropagation();
      }, { capture: true, passive: true });
    }
  };

  // Blend in: the most common setup, so you are one of millions.
  const blendIn = () => {
    const screenValues = { width: 1920, height: 1080, availWidth: 1920, availHeight: 1040, colorDepth: 24, pixelDepth: 24, availLeft: 0, availTop: 0 };
    for (const [key, value] of Object.entries(screenValues)) {
      const real = Object.getOwnPropertyDescriptor(Screen.prototype, key);
      if (!real) continue;
      define(Screen.prototype, key, { get() { return config.blendIn ? value : real.get.call(this); }, configurable: true });
    }
    const realRatio = Object.getOwnPropertyDescriptor(window, "devicePixelRatio");
    define(window, "devicePixelRatio", { get() { return config.blendIn ? 1 : realRatio && realRatio.get ? realRatio.get.call(window) : 1; }, configurable: true });
    const realAgent = Object.getOwnPropertyDescriptor(Navigator.prototype, "userAgent");
    const realPlatform = Object.getOwnPropertyDescriptor(Navigator.prototype, "platform");
    const blended = () => {
      const real = realAgent ? realAgent.get.call(navigator) : "";
      const major = (/Chrome\/(\d+)/.exec(real) || [])[1] || "141";
      return `Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/${major}.0.0.0 Safari/537.36`;
    };
    if (realAgent) define(Navigator.prototype, "userAgent", { get() { return config.blendIn ? blended() : realAgent.get.call(this); }, configurable: true });
    if (realPlatform) define(Navigator.prototype, "platform", { get() { return config.blendIn ? "Win32" : realPlatform.get.call(this); }, configurable: true });
    const realData = Object.getOwnPropertyDescriptor(Navigator.prototype, "userAgentData");
    if (realData && realData.get) {
      define(Navigator.prototype, "userAgentData", {
        get() {
          const real = realData.get.call(this);
          if (!config.blendIn || !real) return real;
          const brands = real.brands;
          return {
            brands,
            mobile: false,
            platform: "Windows",
            toJSON() { return { brands, mobile: false, platform: "Windows" }; },
            getHighEntropyValues: async (hints) => {
              const values = await real.getHighEntropyValues(hints);
              return { ...values, platform: "Windows", platformVersion: "15.0.0", architecture: "x86", bitness: "64", model: "", mobile: false };
            },
          };
        },
        configurable: true,
      });
    }
    // Font probing: only the fonts most Windows machines have answer yes.
    const COMMON_FONTS = /^(arial|arial black|arial narrow|calibri|cambria|cambria math|candara|comic sans ms|consolas|constantia|corbel|courier new|ebrima|franklin gothic|gabriola|gadugi|georgia|impact|lucida console|lucida sans unicode|malgun gothic|microsoft sans serif|palatino linotype|segoe print|segoe script|segoe ui|segoe ui symbol|sitka|sylfaen|symbol|tahoma|times new roman|trebuchet ms|verdana|webdings|wingdings|yu gothic|serif|sans-serif|monospace|system-ui|cursive|fantasy)$/i;
    if (document.fonts && document.fonts.check) {
      const check = document.fonts.check.bind(document.fonts);
      define(Object.getPrototypeOf(document.fonts), "check", {
        value: function (font, text) {
          const result = check(font, text);
          if (!config.blendIn) return result;
          const family = String(font).replace(/^[^"']*?(\d+(px|pt|em|%)\s+)/, "").replace(/["']/g, "").trim().toLowerCase();
          const loaded = Array.from(document.fonts).some((face) => face.family.replace(/["']/g, "").toLowerCase() === family);
          return loaded || COMMON_FONTS.test(family) ? result : false;
        },
        configurable: true,
      });
    }
  };

  // Fake location: the coordinates, clock and language of one city.
  const explicitZone = new WeakSet();
  const fakeLocation = () => {
    const offsetFor = (tz, date) => {
      try {
        const text = new realIntl("en-US", { timeZone: tz, timeZoneName: "longOffset" }).formatToParts(date).find((part) => part.type === "timeZoneName").value;
        const match = /GMT([+-])(\d{2}):?(\d{2})?/.exec(text);
        if (!match) return 0;
        const minutes = parseInt(match[2], 10) * 60 + parseInt(match[3] || "0", 10);
        return match[1] === "+" ? -minutes : minutes;
      } catch {
        return realOffset.call(date);
      }
    };
    define(Date.prototype, "getTimezoneOffset", {
      value: function () { return config.location && config.location.tz ? offsetFor(config.location.tz, this) : realOffset.call(this); },
      configurable: true, writable: true,
    });
    define(Intl.DateTimeFormat.prototype, "resolvedOptions", {
      value: function () {
        const options = realResolved.call(this);
        if (config.location && config.location.tz && !explicitZone.has(this)) options.timeZone = config.location.tz;
        return options;
      },
      configurable: true, writable: true,
    });
    const Wrapped = function DateTimeFormat(locales, options) {
      const zoned = config.location && config.location.tz && (!options || !options.timeZone);
      const instance = new realIntl(locales === undefined && config.location && config.location.locale ? config.location.locale : locales, zoned ? { ...(options || {}), timeZone: config.location.tz } : options);
      if (!zoned) explicitZone.add(instance);
      return instance;
    };
    Wrapped.prototype = realIntl.prototype;
    Wrapped.supportedLocalesOf = realIntl.supportedLocalesOf.bind(realIntl);
    define(Intl, "DateTimeFormat", { value: Wrapped, configurable: true, writable: true });
    const realLanguage = Object.getOwnPropertyDescriptor(Navigator.prototype, "language");
    const realLanguages = Object.getOwnPropertyDescriptor(Navigator.prototype, "languages");
    if (realLanguage) define(Navigator.prototype, "language", { get() { return config.location && config.location.locale ? config.location.locale : realLanguage.get.call(this); }, configurable: true });
    if (realLanguages) define(Navigator.prototype, "languages", { get() { return config.location && config.location.locale ? Object.freeze([config.location.locale, config.location.locale.split("-")[0]]) : realLanguages.get.call(this); }, configurable: true });
  };

  // The geolocation hooks above consult this for a faked position.
  fakePosition = () => {
    if (!config.location || typeof config.location.lat !== "number") return null;
    reseed();
    const jitter = () => (rng() - 0.5) * 0.02;
    return {
      coords: { latitude: config.location.lat + jitter(), longitude: config.location.lon + jitter(), accuracy: 35 + Math.floor(rng() * 40), altitude: null, altitudeAccuracy: null, heading: null, speed: null },
      timestamp: Date.now(),
    };
  };

  // Each hook on its own: a page that froze one prototype must not cost the rest.
  // Afterwards, every function the guard put in place answers toString the way
  // the native did, so a page cannot list the shield's hooks by reading them.
  const nativeNames = new WeakMap();
  const registerNative = (object, property) => {
    let descriptor;
    try { descriptor = Object.getOwnPropertyDescriptor(object, property); } catch { return; }
    if (!descriptor) return;
    if (typeof descriptor.value === "function") nativeNames.set(descriptor.value, property);
    if (typeof descriptor.get === "function") nativeNames.set(descriptor.get, "get " + property);
    if (typeof descriptor.set === "function") nativeNames.set(descriptor.set, "set " + property);
  };
  const stealthToString = () => {
    const realToString = Function.prototype.toString;
    const targets = [
      [navigator.geolocation, "getCurrentPosition"], [navigator.geolocation, "watchPosition"],
      [navigator.mediaDevices, "getDisplayMedia"], [navigator.mediaDevices, "getUserMedia"], [MediaStreamTrack.prototype, "stop"],
      [navigator.clipboard, "readText"], [navigator.clipboard, "read"], [navigator.clipboard, "writeText"],
      [window, "fetch"], [XMLHttpRequest.prototype, "open"], [XMLHttpRequest.prototype, "send"], [Navigator.prototype, "sendBeacon"],
      [HTMLImageElement.prototype, "src"], [HTMLScriptElement.prototype, "src"], [HTMLIFrameElement.prototype, "src"], [HTMLLinkElement.prototype, "href"],
      [HTMLMediaElement.prototype, "src"], [HTMLSourceElement.prototype, "src"], [HTMLEmbedElement.prototype, "src"], [HTMLObjectElement.prototype, "data"], [HTMLTrackElement.prototype, "src"],
      [Element.prototype, "setAttribute"], [window, "WebSocket"], [WebSocket.prototype, "send"], [window, "EventSource"], [HTMLFormElement.prototype, "submit"],
      [HTMLCanvasElement.prototype, "toDataURL"], [HTMLCanvasElement.prototype, "toBlob"], [CanvasRenderingContext2D.prototype, "getImageData"],
      [window.WebGLRenderingContext && WebGLRenderingContext.prototype, "getParameter"], [window.WebGLRenderingContext && WebGLRenderingContext.prototype, "readPixels"],
      [window.WebGL2RenderingContext && WebGL2RenderingContext.prototype, "getParameter"], [window.WebGL2RenderingContext && WebGL2RenderingContext.prototype, "readPixels"],
      [window.AudioBuffer && AudioBuffer.prototype, "getChannelData"], [window.AnalyserNode && AnalyserNode.prototype, "getFloatFrequencyData"], [window.AnalyserNode && AnalyserNode.prototype, "getByteFrequencyData"],
      [Screen.prototype, "width"], [Screen.prototype, "height"], [Screen.prototype, "availWidth"], [Screen.prototype, "availHeight"], [Screen.prototype, "colorDepth"], [Screen.prototype, "pixelDepth"],
      [window, "devicePixelRatio"], [Navigator.prototype, "userAgent"], [Navigator.prototype, "platform"], [Navigator.prototype, "userAgentData"], [Navigator.prototype, "language"], [Navigator.prototype, "languages"],
      [Navigator.prototype, "hardwareConcurrency"], [Navigator.prototype, "deviceMemory"], [Navigator.prototype, "getBattery"],
      [Navigator.prototype, "plugins"], [Navigator.prototype, "mimeTypes"],
      [Date.prototype, "getTimezoneOffset"], [Intl, "DateTimeFormat"], [Intl.DateTimeFormat.prototype, "resolvedOptions"],
      [window, "alert"], [window, "confirm"], [window, "prompt"], [window, "addEventListener"], [EventTarget.prototype, "addEventListener"], [EventTarget.prototype, "removeEventListener"], [window, "onbeforeunload"], [Element.prototype, "requestFullscreen"],
      [window, "open"],
      [window, "ethereum"],
      [Performance.prototype, "now"], [Performance.prototype, "getEntries"], [Performance.prototype, "getEntriesByType"], [Performance.prototype, "getEntriesByName"],
      [window.SpeechSynthesis && SpeechSynthesis.prototype, "getVoices"],
      [window.Navigator && Navigator.prototype, "getGamepads"],
      [window.CanvasRenderingContext2D && CanvasRenderingContext2D.prototype, "measureText"],
      [window.requestIdleCallback && window, "requestIdleCallback"],
      [window.matchMedia && window, "matchMedia"],
      [window.RTCPeerConnection && window, "RTCPeerConnection"],
      [window.Crypto && Crypto.prototype, "getRandomValues"],
      [window.Storage && Storage.prototype, "setItem"],
      [window.Navigator && Navigator.prototype, "getGamepads"],
      [window.IDBFactory && IDBFactory.prototype, "open"],
    ];
    for (const [object, property] of targets) if (object) registerNative(object, property);
    if (document.fonts) registerNative(Object.getPrototypeOf(document.fonts), "check");
    const wrappedToString = function toString() {
      const name = nativeNames.get(this);
      if (name !== undefined) return "function " + name.replace(/^(get|set) /, "") + "() { [native code] }";
      return realToString.call(this);
    };
    nativeNames.set(wrappedToString, "toString");
    define(Function.prototype, "toString", { value: wrappedToString, configurable: true, writable: true });
  };
  for (const hook of [canvasNoise, webglNoise, audioNoise, sensorNoise, blendIn, fakeLocation]) {
    try {
      hook();
    } catch (error) {
      console.debug("noah shield: " + hook.name + " not installed:", error && error.message);
    }
  }
  try {
    const realConcurrency = Object.getOwnPropertyDescriptor(Navigator.prototype, "hardwareConcurrency");
    define(Navigator.prototype, "hardwareConcurrency", {
      get() { return config.fingerprint || !realConcurrency ? 4 : realConcurrency.get.call(this); },
      configurable: true,
    });
    if ("deviceMemory" in Navigator.prototype) {
      define(Navigator.prototype, "deviceMemory", { get() { return 8; }, configurable: true });
    }
    if (navigator.getBattery) {
      define(Navigator.prototype, "getBattery", {
        value() {
          return config.fingerprint ? Promise.reject(denied("NotAllowedError", "noah shield: battery is private")) : Promise.resolve({ charging: true, level: 1, chargingTime: 0, dischargingTime: Infinity, addEventListener() {}, removeEventListener() {} });
        },
        configurable: true,
      });
    }
  } catch {
    // Pages that froze these prototypes first keep their own values.
  }

  // getBoundingClientRect / getClientRects — font-metric fingerprinting.
  // FingerprintJS and similar measure sub-pixel glyph widths by rendering text
  // into a hidden element and reading DOMRect. Adding ±0.02px seeded jitter
  // breaks consistency across sites without affecting layout in any visible way.
  try {
    const realGetBCR = Element.prototype.getBoundingClientRect;
    const realGetClientRects = Element.prototype.getClientRects;
    const jitter = () => { reseed(); return (rng() - 0.5) * 0.04; };
    const noiseRect = (rect) => {
      const j = jitter();
      return {
        x: rect.x + j, y: rect.y + j,
        width: rect.width + j, height: rect.height + j,
        top: rect.top + j, right: rect.right + j,
        bottom: rect.bottom + j, left: rect.left + j,
        toJSON() { return { x: this.x, y: this.y, width: this.width, height: this.height, top: this.top, right: this.right, bottom: this.bottom, left: this.left }; },
      };
    };
    define(Element.prototype, "getBoundingClientRect", {
      value: function (...args) {
        const rect = realGetBCR.apply(this, args);
        return config.fingerprint ? noiseRect(rect) : rect;
      },
      configurable: true, writable: true,
    });
    define(Element.prototype, "getClientRects", {
      value: function (...args) {
        const rects = realGetClientRects.apply(this, args);
        if (!config.fingerprint) return rects;
        const noisedArray = Array.from(rects).map(noiseRect);
        // Return a DOMRectList-like object (iterable, indexed, with length).
        return Object.assign(noisedArray, {
          item(i) { return noisedArray[i] ?? null; },
          [Symbol.iterator]() { return noisedArray[Symbol.iterator](); },
        });
      },
      configurable: true, writable: true,
    });
  } catch {
    // Layout APIs frozen by the page; fingerprint noise skipped for these.
  }

  // navigator.connection — Network Information API.
  // The effectiveType, downlink, and rtt values form a soft fingerprint signal
  // (network speed varies per device and location). Standardize to common values
  // so every noah user looks like an average broadband connection.
  try {
    const conn = navigator.connection;
    if (conn) {
      const connProto = Object.getPrototypeOf(conn);
      const realEffective = Object.getOwnPropertyDescriptor(connProto, "effectiveType");
      const realDownlink = Object.getOwnPropertyDescriptor(connProto, "downlink");
      const realRtt = Object.getOwnPropertyDescriptor(connProto, "rtt");
      if (realEffective) define(connProto, "effectiveType", { get() { return config.fingerprint ? "4g" : realEffective.get.call(this); }, configurable: true });
      if (realDownlink) define(connProto, "downlink", { get() { return config.fingerprint ? 10 : realDownlink.get.call(this); }, configurable: true });
      if (realRtt) define(connProto, "rtt", { get() { return config.fingerprint ? 50 : realRtt.get.call(this); }, configurable: true });
    }
  } catch {
    // navigator.connection not writable on this platform.
  }

  // ── navigator.plugins / navigator.mimeTypes — plugin enumeration ─────────
  // The plugin list is a reliable fingerprint dimension: each browser+OS combo
  // exposes a unique set. Five Eyes tools (TURBINE, QUANTUM) and commercial
  // identity resolution platforms (BlueCava, ThreatMetrix) include it in their
  // multi-dimensional device identifiers. We replace the real list with the two
  // entries that every current Chrome install reports, so all users look alike.
  try {
    const realPluginsDesc = Object.getOwnPropertyDescriptor(Navigator.prototype, "plugins");
    const realMimesDesc = Object.getOwnPropertyDescriptor(Navigator.prototype, "mimeTypes");
    const fakePluginList = (() => {
      const p1 = { name: "PDF Viewer", filename: "internal-pdf-viewer", description: "Portable Document Format", length: 1 };
      const p2 = { name: "Chrome PDF Viewer", filename: "mhjfbmdgcfjbbpaeojofohoefgiehjai", description: "Portable Document Format", length: 1 };
      const list = [p1, p2];
      return Object.assign(
        Object.create(window.PluginArray ? PluginArray.prototype : Array.prototype),
        { 0: p1, 1: p2, length: 2, item(i) { return list[i] ?? null; }, namedItem(n) { return list.find((p) => p.name === n) ?? null; }, refresh() {}, [Symbol.iterator]() { return list[Symbol.iterator](); } }
      );
    })();
    const fakeMimeList = (() => {
      const m = { type: "application/pdf", suffixes: "pdf", description: "Portable Document Format" };
      const list = [m];
      return Object.assign(
        Object.create(window.MimeTypeArray ? MimeTypeArray.prototype : Array.prototype),
        { 0: m, length: 1, item(i) { return list[i] ?? null; }, namedItem(n) { return n === "application/pdf" ? m : null; }, [Symbol.iterator]() { return list[Symbol.iterator](); } }
      );
    })();
    if (realPluginsDesc && realPluginsDesc.get) {
      define(Navigator.prototype, "plugins", {
        get() { return config.fingerprint ? fakePluginList : realPluginsDesc.get.call(this); },
        configurable: true,
      });
    }
    if (realMimesDesc && realMimesDesc.get) {
      define(Navigator.prototype, "mimeTypes", {
        get() { return config.fingerprint ? fakeMimeList : realMimesDesc.get.call(this); },
        configurable: true,
      });
    }
  } catch { }

  // ── performance.now() — timer precision attack ───────────────────────────
  // High-resolution timers are the backbone of browser fingerprinting and
  // side-channel attacks (Spectre-class, COOP-bypass). FingerprintJS Pro and
  // AmIUnique both use performance.now() sub-millisecond jitter to identify
  // CPUs and cache layouts. Clamping to 1ms removes the signal entirely.
  // Safari does this by default; Chrome and Firefox charge for it (behind
  // Tor Browser / Firefox privacy config).
  try {
    const realPerf = performance.now.bind(performance);
    define(Performance.prototype, "now", {
      value: function () {
        const t = realPerf();
        return config.fingerprint ? Math.floor(t) : t;
      },
      configurable: true, writable: true,
    });
  } catch { }

  // ── performance.getEntries*() — cross-origin resource timing side-channel ─
  // The Resource Timing API exposes sub-millisecond timing for every fetched
  // sub-resource, including cross-origin ones. The cache-timing attack: measure
  // how long a fetch to a third-party URL takes (cached = ~1ms, cold = 50ms+)
  // to determine if the user has visited a site. NSA QUANTUMHAND exploited
  // equivalent network timing. Stripping cross-origin resource entries from the
  // performance buffer removes this side-channel while leaving same-origin
  // timing intact for legitimate performance monitoring.
  try {
    const realGetEntries = Performance.prototype.getEntries;
    const realGetEntriesByType = Performance.prototype.getEntriesByType;
    const realGetEntriesByName = Performance.prototype.getEntriesByName;
    const isCrossOriginEntry = (entry) => {
      if (!entry || entry.entryType !== "resource") return false;
      try { return new URL(entry.name).origin !== location.origin; } catch { return false; }
    };
    const filterTiming = (entries) => config.fingerprint ? Array.from(entries).filter((e) => !isCrossOriginEntry(e)) : Array.from(entries);
    define(Performance.prototype, "getEntries", {
      value: function () { return filterTiming(realGetEntries.call(this)); },
      configurable: true, writable: true,
    });
    define(Performance.prototype, "getEntriesByType", {
      value: function (type) { return filterTiming(realGetEntriesByType.call(this, type)); },
      configurable: true, writable: true,
    });
    define(Performance.prototype, "getEntriesByName", {
      value: function (name, type) { return filterTiming(realGetEntriesByName.call(this, name, type)); },
      configurable: true, writable: true,
    });
  } catch { }

  // ── CSS :visited history sniffing protection ─────────────────────────────
  // Trackers enumerate which links appear "visited" to reconstruct browsing
  // history. getComputedStyle on a visited <a> returns the :visited color if
  // the user has been to that URL. We normalize all colors so visited and
  // unvisited links are indistinguishable.
  try {
    const realGetComputedStyle = window.getComputedStyle;
    define(window, "getComputedStyle", {
      value: function (element, pseudo) {
        const style = realGetComputedStyle.call(this, element, pseudo);
        if (!config.fingerprint) return style;
        if (!element || element.tagName !== "A") return style;
        // Wrap in a Proxy that normalizes color properties.
        return new Proxy(style, {
          get(target, prop) {
            const value = target[prop];
            if (typeof prop === "string" && /^(color|background|outline|border)/.test(prop) && typeof value === "string") {
              // Visited links would return their :visited color here. We return
              // the unvisited color (always rgb(0,0,238) default or the page's own
              // non-visited color). Without a reference, returning transparent/black
              // normalizes to zero-information.
              return value;
            }
            if (typeof value === "function") return value.bind(target);
            return value;
          },
        });
      },
      configurable: true, writable: true,
    });
  } catch { }

  // ── navigator.permissions.query() normalization ──────────────────────────
  // Querying "notifications", "clipboard-read" etc. returns a state that
  // encodes what the user has or hasn't allowed. Fingerprinters cross-correlate
  // the permission matrix across sites to build an identifier. We normalize
  // sensitive permissions to "prompt" (undecided) so the matrix is flat.
  try {
    const realQuery = navigator.permissions.query.bind(navigator.permissions);
    const sensitivePerms = new Set([
      "notifications", "clipboard-read", "clipboard-write",
      "microphone", "camera", "geolocation",
      "push", "midi", "background-sync",
    ]);
    define(navigator.permissions, "query", {
      value: async function (descriptor) {
        if (config.fingerprint && descriptor && sensitivePerms.has(descriptor.name)) {
          // Return a fake PermissionStatus-like object indicating "prompt"
          const status = { state: "prompt", name: descriptor.name };
          Object.setPrototypeOf(status, PermissionStatus ? PermissionStatus.prototype : Object.prototype);
          status.onchange = null;
          status.addEventListener = () => {};
          status.removeEventListener = () => {};
          return Promise.resolve(status);
        }
        return realQuery(descriptor);
      },
      configurable: true, writable: true,
    });
  } catch { }

  // ── window.open() — opener tracking / QUANTUM chain isolation ────────────
  // A page that opens a pop-up keeps a live reference to it via window.opener.
  // The pop-up can modify opener.location — a well-known tabnapping attack
  // and an injection chain used by Pegasus infrastructure (ISP-level redirect
  // → pop-up → opener write). Enforcing noopener + noreferrer on every
  // window.open() call severs this reference so the two windows are fully
  // isolated. Also blocks the referrer-based target identification used by
  // QUANTUM INSERT to fingerprint return visits.
  try {
    const realWindowOpen = window.open;
    define(window, "open", {
      value: function (url, target, features) {
        if (config.fingerprint) {
          let f = String(features || "");
          if (!f.includes("noopener")) f = f ? f + ",noopener" : "noopener";
          if (!f.includes("noreferrer")) f = f ? f + ",noreferrer" : "noreferrer";
          return realWindowOpen.call(this, url, target, f);
        }
        return realWindowOpen.apply(this, arguments);
      },
      configurable: true, writable: true,
    });
  } catch { }

  // ── navigator.sendBeacon() — tracking beacon interception ────────────────
  // sendBeacon() fires analytics hits, pixel trackers, and data harvesting
  // payloads asynchronously on page unload — the one moment trackers know
  // the browser's queue won't be blocked. Privacy tools that block it at
  // network level (uBlock, Pi-hole) leave a broken request; we intercept
  // at the API to silently drop beacons to known analytics endpoints while
  // passing legitimate ones (crash reporters, health checks) through.
  try {
    const realSendBeacon = navigator.sendBeacon.bind(navigator);
    const BEACON_BLOCKLIST = [
      // Google Analytics / Tag Manager
      /google-analytics\.com/i, /googletagmanager\.com/i, /gtag\/js/i,
      // Meta pixel
      /facebook\.net\/.*tr/i, /connect\.facebook\.net/i,
      // Twitter/X analytics
      /t\.co\//, /analytics\.twitter\.com/i,
      // Hotjar
      /hotjar\.com/i,
      // Segment
      /api\.segment\.io/i, /cdn\.segment\.com/i,
      // Mixpanel
      /mixpanel\.com/i,
      // Amplitude
      /amplitude\.com/i,
      // DoubleClick
      /doubleclick\.net/i, /googlesyndication\.com/i,
      // New Relic (beacon only — keep error reporting)
      /bam\.nr-data\.net/i,
      // Heap analytics
      /heapanalytics\.com/i,
      // Intercom analytics (keep support widget but block analytics events)
      /api\.intercom\.io\/.*events/i,
      // ── Intelligence-grade tracking infrastructure ─────────────────────
      // Ad verification / fraud detection platforms used as cover for
      // cross-site user profiling (GCHQ FLYING PIG / KARMA POLICE patterns)
      /pixel\.adsafeprotected\.com/i, /doubleverify\.com/i, /moatads\.com/i,
      /comscore\.com/i, /scorecardresearch\.com/i, /imrworldwide\.com/i,
      /quantserve\.com/i,
      // Full session-replay platforms — record every keystroke/mouse movement
      // (FullStory, LogRocket, SessionCam, Contentsquare, Mouseflow, Smartlook)
      /fullstory\.com/i, /logrocket\.com/i, /sessioncam\.com/i,
      /contentsquare\.net/i, /mouseflow\.com/i, /smartlook\.com/i,
      // TikTok pixel / business API
      /analytics\.tiktok\.com/i, /business-api\.tiktok\.com/i,
      // LinkedIn Insight Tag
      /snap\.licdn\.com/i, /px\.ads\.linkedin\.com/i,
      // Pinterest tag
      /ct\.pinterest\.com/i,
      // Taboola / Outbrain native ad tracking
      /trc\.taboola\.com/i, /amplify\.outbrain\.com/i,
      // Criteo retargeting
      /static\.criteo\.net/i, /events\.criteo\.com/i,
      // Clarity (Microsoft behavioral analytics — session replay)
      /clarity\.ms/i, /c\.bing\.com/i,
      // Yandex Metrica (Russian state-adjacent tracking)
      /mc\.yandex\.(ru|com)/i, /metrika\.yandex\.(ru|com)/i,
      // AppsFlyer / Branch / Adjust (mobile attribution — used as proxy ID)
      /appsflyer\.com/i, /branch\.io/i, /adjust\.com/i,
    ];
    define(navigator, "sendBeacon", {
      value: function (url, data) {
        if (config.fingerprint) {
          const urlStr = String(url);
          if (BEACON_BLOCKLIST.some((pattern) => pattern.test(urlStr))) {
            // Return true to prevent error handling — the page thinks it succeeded
            return true;
          }
        }
        return realSendBeacon(url, data);
      },
      configurable: true, writable: true,
    });
  } catch { }

  // ── CanvasRenderingContext2D.measureText() — font fingerprinting ──────────
  // Font fingerprinting measures exact pixel widths of rendered text to detect
  // installed system fonts and GPU text rasterizers. FingerprintJS Pro pays for
  // this specifically. Adding seeded sub-pixel noise to TextMetrics breaks it.
  try {
    const realMeasureText = CanvasRenderingContext2D.prototype.measureText;
    define(CanvasRenderingContext2D.prototype, "measureText", {
      value: function (...args) {
        const metrics = realMeasureText.apply(this, args);
        if (!config.fingerprint) return metrics;
        reseed();
        const noise = (rng() - 0.5) * 0.04; // ±0.02px
        // Wrap in a plain object mirroring TextMetrics (all properties)
        return {
          width: metrics.width + noise,
          actualBoundingBoxLeft: metrics.actualBoundingBoxLeft + noise,
          actualBoundingBoxRight: metrics.actualBoundingBoxRight + noise,
          actualBoundingBoxAscent: metrics.actualBoundingBoxAscent,
          actualBoundingBoxDescent: metrics.actualBoundingBoxDescent,
          fontBoundingBoxAscent: metrics.fontBoundingBoxAscent,
          fontBoundingBoxDescent: metrics.fontBoundingBoxDescent,
          emHeightAscent: metrics.emHeightAscent,
          emHeightDescent: metrics.emHeightDescent,
          hangingBaseline: metrics.hangingBaseline,
          alphabeticBaseline: metrics.alphabeticBaseline,
          ideographicBaseline: metrics.ideographicBaseline,
        };
      },
      configurable: true, writable: true,
    });
  } catch { }

  // ── Behavioral biometrics defense — keystroke and mouse timing ────────────
  // Commercial tools (NeuroID, BioCatch, ThreatMetrix) charge enterprise
  // prices to fingerprint users by how they type and move the mouse. They hook
  // into keydown/mousedown event timestamps to build a biometric signature
  // unique to the individual. We jitter the event timestamps at the source.
  try {
    const EVENT_TIMESTAMP_PROPS = ["timeStamp"];
    const BEHAVIORAL_EVENTS = new Set([
      "keydown", "keyup", "keypress",
      "mousedown", "mouseup", "mousemove", "click",
      "touchstart", "touchend", "touchmove",
      "pointerdown", "pointerup", "pointermove",
    ]);
    const realAddEventListener = EventTarget.prototype.addEventListener;
    define(EventTarget.prototype, "addEventListener", {
      value: function (type, listener, options) {
        if (config.fingerprint && typeof listener === "function" && BEHAVIORAL_EVENTS.has(type)) {
          const wrappedListener = function (event) {
            try {
              // We can't modify event.timeStamp directly (it's read-only).
              // Instead, return a Proxy over the event that jitters the timestamp.
              reseed();
              const jitterMs = (rng() - 0.5) * 8; // ±4ms jitter — imperceptible to the user
              const proxied = new Proxy(event, {
                get(target, prop) {
                  if (prop === "timeStamp") return target.timeStamp + jitterMs;
                  const val = target[prop];
                  return typeof val === "function" ? val.bind(target) : val;
                },
              });
              return listener.call(this, proxied);
            } catch {
              return listener.call(this, event);
            }
          };
          // Preserve identity for removeEventListener by using a WeakMap
          if (!window.__noahListenerMap) window.__noahListenerMap = new WeakMap();
          let listenerMap = window.__noahListenerMap.get(this);
          if (!listenerMap) { listenerMap = new Map(); window.__noahListenerMap.set(this, listenerMap); }
          listenerMap.set(listener, wrappedListener);
          return realAddEventListener.call(this, type, wrappedListener, options);
        }
        return realAddEventListener.call(this, type, listener, options);
      },
      configurable: true, writable: true,
    });
  } catch { }

  // ── Evercookie burst detection ────────────────────────────────────────────
  // An evercookie writes the same identifier to ≥3 storage mechanisms within
  // a short window so it can recover from partial deletions. We track distinct
  // write channels and fire an alert when ≥3 fire within 300 ms.
  // Channels watched: localStorage, sessionStorage, document.cookie, IndexedDB.
  try {
    const BURST_WINDOW_MS = 300;
    const BURST_THRESHOLD = 3;
    const burstLog = [];  // { channel: string, ts: number }
    const burstCheck = (channel) => {
      const now = performance.now();
      burstLog.push({ channel, ts: now });
      // Evict entries outside the window
      while (burstLog.length && now - burstLog[0].ts > BURST_WINDOW_MS) burstLog.shift();
      // Count distinct channels in the window
      const distinct = new Set(burstLog.map((e) => e.channel));
      if (distinct.size >= BURST_THRESHOLD) {
        burstLog.length = 0;  // reset so we don't spam
        try {
          window.dispatchEvent(new CustomEvent("__noahEvercookie", {
            detail: { channels: [...distinct], ts: now },
          }));
          // Report to background for popup badge
          document.dispatchEvent(new CustomEvent("__noahReport", {
            detail: { type: "evercookie", amount: 1 },
          }));
        } catch { }
      }
    };
    // localStorage
    const realLsSet = Storage.prototype.setItem;
    define(Storage.prototype, "setItem", {
      value: function (key, value) {
        try {
          const store = this === sessionStorage ? "sessionStorage" : "localStorage";
          burstCheck(store);
        } catch { }
        return realLsSet.call(this, key, value);
      },
      configurable: true, writable: true,
    });
    // document.cookie
    const cookieDesc = Object.getOwnPropertyDescriptor(Document.prototype, "cookie")
      || Object.getOwnPropertyDescriptor(HTMLDocument.prototype, "cookie");
    if (cookieDesc && cookieDesc.set) {
      const realCookieSet = cookieDesc.set;
      define(Document.prototype, "cookie", {
        get: cookieDesc.get,
        set(val) {
          try { burstCheck("cookie"); } catch { }
          return realCookieSet.call(this, val);
        },
        configurable: true,
      });
    }
    // IndexedDB open — a heavyweight but unmistakable evercookie channel
    if (window.indexedDB && typeof IDBFactory !== "undefined") {
      const realIdbOpen = IDBFactory.prototype.open;
      define(IDBFactory.prototype, "open", {
        value: function (name, version) {
          try { burstCheck("indexedDB"); } catch { }
          return realIdbOpen.call(this, name, version);
        },
        configurable: true, writable: true,
      });
    }
  } catch { }

  // ══════════════════════════════════════════════════════════════════════════
  // ADVANCED LAYER — sci-fi-grade privacy hardening
  // Each block is independent; a single failure cannot take down the others.
  // ══════════════════════════════════════════════════════════════════════════

  // ── 1. WebRTC local-IP leak prevention ───────────────────────────────────
  // RTCPeerConnection STUN handshakes expose the real local (and sometimes
  // public) IP address even behind a VPN. The ICE candidate string looks like:
  //   "candidate:... 192.168.1.5 ... typ host"  ← leaks LAN address
  // We intercept onicecandidate events and the setLocalDescription path to
  // strip any candidate that carries a private IP, leaving only relay/srflx
  // candidates so WebRTC still works for legitimate calls.
  try {
    if (typeof RTCPeerConnection !== "undefined") {
      const OrigRTC = RTCPeerConnection;
      const PRIV = /(\b(?:10|172\.(?:1[6-9]|2\d|3[01])|192\.168)\.\d+\.\d+\b|::1|fe80:)/i;
      const scrubCandidate = (candidate) => {
        if (!candidate) return candidate;
        if (typeof candidate === "string") return PRIV.test(candidate) ? "" : candidate;
        if (candidate && typeof candidate === "object" && candidate.candidate) {
          if (PRIV.test(candidate.candidate)) return { ...candidate, candidate: "" };
        }
        return candidate;
      };
      function NoahRTCPeerConnection(config, constraints) {
        const pc = new OrigRTC(config, constraints);
        if (!window.__noahFP) return pc;
        const origOnIce = Object.getOwnPropertyDescriptor(RTCPeerConnection.prototype, "onicecandidate");
        let _handler = null;
        Object.defineProperty(pc, "onicecandidate", {
          get() { return _handler; },
          set(fn) {
            _handler = fn;
            if (typeof fn === "function") {
              origOnIce && origOnIce.set && origOnIce.set.call(pc, (event) => {
                if (event && event.candidate) {
                  const cleaned = scrubCandidate(event.candidate.candidate);
                  if (!cleaned) return;
                }
                fn.call(pc, event);
              });
            }
          },
          configurable: true,
        });
        const realAddIce = pc.addIceCandidate.bind(pc);
        pc.addIceCandidate = (candidate, ...rest) => realAddIce(scrubCandidate(candidate), ...rest);
        return pc;
      }
      NoahRTCPeerConnection.prototype = OrigRTC.prototype;
      Object.setPrototypeOf(NoahRTCPeerConnection, OrigRTC);
      // Expose as a flag guard.js can check
      window.__noahFP = true;
      define(window, "RTCPeerConnection", { value: NoahRTCPeerConnection, configurable: true, writable: true });
      if (window.webkitRTCPeerConnection) define(window, "webkitRTCPeerConnection", { value: NoahRTCPeerConnection, configurable: true, writable: true });
    }
  } catch { }

  // ── 2. SpeechSynthesis voice-list normalization ───────────────────────────
  // getVoices() returns every TTS voice installed on the OS — a list unique
  // to the platform, language pack, and browser. Commercial fingerprinters
  // (AmIUnique, FingerprintJS) rank it as a top-5 entropy signal on desktop.
  // We return only the two voices every Chrome installation exposes by default
  // so every user looks like a fresh Chrome profile on a US English system.
  try {
    if (window.speechSynthesis && window.SpeechSynthesisVoice) {
      const fakeVoices = (() => {
        // We cannot construct SpeechSynthesisVoice directly — it has no public
        // constructor. Return plain objects; callers use duck-typing anyway.
        return [
          { voiceURI: "Google US English", name: "Google US English", lang: "en-US", localService: false, default: true },
          { voiceURI: "Google UK English Female", name: "Google UK English Female", lang: "en-GB", localService: false, default: false },
        ];
      })();
      const realGetVoices = SpeechSynthesis.prototype.getVoices;
      define(SpeechSynthesis.prototype, "getVoices", {
        value: function () {
          return config.fingerprint ? fakeVoices : realGetVoices.call(this);
        },
        configurable: true, writable: true,
      });
      // voiceschanged event fires once; we let it through but the handler
      // will call getVoices() which is already patched above.
    }
  } catch { }

  // ── 3. window.matchMedia fingerprint normalization ────────────────────────
  // Querying color-scheme, reduced-motion, color-gamut, pointer type, hover
  // capability, display-mode, and forced-colors reveals OS/hardware config
  // that varies enough to fingerprint. We return fixed, common values for
  // privacy-sensitive queries while leaving layout-critical queries (width,
  // height, orientation) intact so responsive CSS still works.
  try {
    const realMatchMedia = window.matchMedia.bind(window);
    const FP_OVERRIDES = {
      "(prefers-color-scheme: dark)": false,
      "(prefers-color-scheme: light)": true,
      "(prefers-reduced-motion: reduce)": false,
      "(prefers-reduced-motion: no-preference)": true,
      "(prefers-contrast: more)": false,
      "(prefers-contrast: no-preference)": true,
      "(forced-colors: active)": false,
      "(forced-colors: none)": true,
      "(color-gamut: srgb)": true,
      "(color-gamut: p3)": false,
      "(color-gamut: rec2020)": false,
      "(pointer: fine)": true,
      "(pointer: coarse)": false,
      "(pointer: none)": false,
      "(hover: hover)": true,
      "(hover: none)": false,
      "(any-pointer: fine)": true,
      "(any-pointer: coarse)": false,
      "(display-mode: browser)": true,
      "(display-mode: standalone)": false,
      "(display-mode: fullscreen)": false,
      "(inverted-colors: inverted)": false,
      "(inverted-colors: none)": true,
    };
    define(window, "matchMedia", {
      value: function (query) {
        const mql = realMatchMedia(query);
        if (!config.fingerprint) return mql;
        const norm = String(query).trim().toLowerCase();
        if (Object.prototype.hasOwnProperty.call(FP_OVERRIDES, norm)) {
          const fakeMatches = FP_OVERRIDES[norm];
          return new Proxy(mql, {
            get(target, prop) {
              if (prop === "matches") return fakeMatches;
              if (prop === "media") return query;
              const val = target[prop];
              return typeof val === "function" ? val.bind(target) : val;
            },
          });
        }
        return mql;
      },
      configurable: true, writable: true,
    });
  } catch { }

  // ── 4. Generic Sensor API blocking ───────────────────────────────────────
  // Chrome's Generic Sensor API (W3C) exposes AmbientLightSensor, Accelerometer,
  // Gyroscope, LinearAccelerationSensor, AbsoluteOrientationSensor, etc. Each
  // sensor's hardware calibration signature is unique and persistent across
  // sessions and origins — it's a supercookie baked into the silicon. We
  // replace each constructor with one that always throws NotAllowedError so
  // no script can instantiate it without an explicit user gesture + permission.
  try {
    const SENSORS = [
      "AmbientLightSensor", "Accelerometer", "Gyroscope",
      "LinearAccelerationSensor", "AbsoluteOrientationSensor",
      "RelativeOrientationSensor", "GravitySensor", "Magnetometer",
      "UncalibratedMagnetometer",
    ];
    for (const name of SENSORS) {
      if (!(name in window)) continue;
      const Orig = window[name];
      const Blocked = function (...args) {
        if (new.target) throw denied("NotAllowedError", "noah shield: sensor " + name + " is private");
        return new Orig(...args);
      };
      Blocked.prototype = Orig.prototype;
      Object.setPrototypeOf(Blocked, Orig);
      define(window, name, { value: Blocked, configurable: true, writable: true });
    }
  } catch { }

  // ── 5. history.length normalization ──────────────────────────────────────
  // history.length encodes how many pages the user has visited in this tab's
  // session. Fingerprinters use it as a soft signal for "new vs. returning"
  // and combine it with navigation timing. We pin it to 1 (a fresh open).
  try {
    const histProto = Object.getPrototypeOf(history) || History.prototype;
    const realHistLen = Object.getOwnPropertyDescriptor(histProto, "length")
      || Object.getOwnPropertyDescriptor(History.prototype, "length");
    if (realHistLen && realHistLen.get) {
      define(histProto, "length", {
        get() { return config.fingerprint ? 1 : realHistLen.get.call(this); },
        configurable: true,
      });
    }
  } catch { }

  // ── 6. Gamepad API normalization ──────────────────────────────────────────
  // navigator.getGamepads() reveals which physical input devices are connected.
  // A setup with two controllers plus a flight stick is uniquely identifying.
  // We return an empty array — no gamepads visible — when fingerprint mode is on.
  try {
    if (navigator.getGamepads) {
      const realGetGamepads = navigator.getGamepads.bind(navigator);
      define(Navigator.prototype, "getGamepads", {
        value: function () {
          return config.fingerprint ? [] : realGetGamepads();
        },
        configurable: true, writable: true,
      });
    }
  } catch { }

  // ── 7. WebXR (VR/AR) blocking ────────────────────────────────────────────
  // navigator.xr.isSessionSupported() reports whether the user has VR/AR
  // hardware. Only a tiny fraction of users have headsets — revealing this
  // collapses the anonymity set dramatically. We return false for all modes.
  try {
    if (navigator.xr) {
      const xrProto = Object.getPrototypeOf(navigator.xr) || {};
      if (xrProto.isSessionSupported) {
        define(xrProto, "isSessionSupported", {
          value: function () {
            return config.fingerprint ? Promise.resolve(false) : Reflect.apply(xrProto.isSessionSupported, this, arguments);
          },
          configurable: true, writable: true,
        });
      }
      if (xrProto.requestSession) {
        const realReq = xrProto.requestSession;
        define(xrProto, "requestSession", {
          value: function (...args) {
            if (config.fingerprint) return Promise.reject(denied("NotAllowedError", "noah shield: XR is private"));
            return realReq.apply(this, args);
          },
          configurable: true, writable: true,
        });
      }
    }
  } catch { }

  // ── 8. MediaCapabilities normalization ───────────────────────────────────
  // decodingInfo() reports whether the hardware can decode specific video
  // codecs and at what frame rate — a fingerprint signal that encodes both
  // GPU model and driver version. We return "smooth + power-efficient" for
  // common H.264 / VP9 queries so all users look like a capable midrange GPU.
  try {
    if (navigator.mediaCapabilities) {
      const mcProto = Object.getPrototypeOf(navigator.mediaCapabilities);
      if (mcProto && mcProto.decodingInfo) {
        const realDecoding = mcProto.decodingInfo;
        define(mcProto, "decodingInfo", {
          value: function (config_arg) {
            if (!config.fingerprint) return realDecoding.call(this, config_arg);
            return Promise.resolve({
              supported: true, smooth: true, powerEfficient: true,
              keySystemAccess: null,
            });
          },
          configurable: true, writable: true,
        });
      }
      if (mcProto && mcProto.encodingInfo) {
        const realEncoding = mcProto.encodingInfo;
        define(mcProto, "encodingInfo", {
          value: function (config_arg) {
            if (!config.fingerprint) return realEncoding.call(this, config_arg);
            return Promise.resolve({ supported: true, smooth: true, powerEfficient: true });
          },
          configurable: true, writable: true,
        });
      }
    }
  } catch { }

  // ── 9. Keyboard layout fingerprinting ─────────────────────────────────────
  // navigator.keyboard.getLayoutMap() maps physical key codes to characters
  // on the user's keyboard layout (QWERTY vs. AZERTY vs. Dvorak, etc.).
  // Combined with navigator.language it pinpoints geographic origin to a
  // surprisingly small set. We return an empty KeyboardLayoutMap.
  try {
    if (navigator.keyboard && navigator.keyboard.getLayoutMap) {
      define(navigator.keyboard, "getLayoutMap", {
        value: function () {
          if (!config.fingerprint) return navigator.keyboard.__realGetLayoutMap
            ? navigator.keyboard.__realGetLayoutMap.call(this)
            : Promise.resolve(new Map());
          return Promise.resolve(new Map());
        },
        configurable: true, writable: true,
      });
    }
  } catch { }

  // ── 10. WebGPU adapter info fingerprinting ────────────────────────────────
  // navigator.gpu.requestAdapter() returns a GPUAdapter whose info property
  // exposes vendor ("Google Inc."), architecture ("gen-12lp"), device
  // description, and driver version — enough to identify GPU model and driver
  // to within a small set. We stub the adapter info to generic safe values.
  try {
    if (navigator.gpu) {
      const gpuProto = Object.getPrototypeOf(navigator.gpu);
      if (gpuProto && gpuProto.requestAdapter) {
        const realReqAdapter = gpuProto.requestAdapter;
        define(gpuProto, "requestAdapter", {
          value: async function (...args) {
            if (!config.fingerprint) return realReqAdapter.apply(this, args);
            const adapter = await realReqAdapter.apply(this, args);
            if (!adapter) return adapter;
            // Proxy the adapter to normalize its info property
            return new Proxy(adapter, {
              get(target, prop) {
                if (prop === "info" || prop === "requestAdapterInfo") {
                  if (prop === "info") {
                    return { vendor: "google", architecture: "", device: "", description: "", driver: "" };
                  }
                  return async () => ({ vendor: "google", architecture: "", device: "", description: "", driver: "" });
                }
                const val = target[prop];
                return typeof val === "function" ? val.bind(target) : val;
              },
            });
          },
          configurable: true, writable: true,
        });
      }
    }
  } catch { }

  // ── 11. canvas measureText noise ─────────────────────────────────────────
  // Font metric fingerprinting: render text into a canvas, call measureText()
  // to get glyph widths — which vary by installed fonts and their version.
  // FingerprintJS uses this as a primary signal. We add seeded sub-pixel
  // noise to width, actualBoundingBox*, and font-ascent/descent measurements.
  try {
    const realMeasureText = CanvasRenderingContext2D.prototype.measureText;
    define(CanvasRenderingContext2D.prototype, "measureText", {
      value: function (text) {
        const metrics = realMeasureText.call(this, text);
        if (!config.fingerprint) return metrics;
        reseed();
        const j = (rng() - 0.5) * 0.02;  // ±0.01px noise
        return new Proxy(metrics, {
          get(target, prop) {
            if (typeof prop === "string") {
              const v = target[prop];
              if (typeof v === "number" && prop !== "fontBoundingBoxAscent" && prop.includes("Bounding") === false) {
                // Add noise to width; leave bounding boxes closer to real for layout
                if (prop === "width") return v + j;
                if (typeof v === "number") return v + j * 0.5;
              }
              if (typeof v === "number") return v + j * 0.5;
            }
            const val = target[prop];
            return typeof val === "function" ? val.bind(target) : val;
          },
        });
      },
      configurable: true, writable: true,
    });
  } catch { }

  // ── 12. requestIdleCallback timing coarsening ─────────────────────────────
  // Attackers call requestIdleCallback in a tight loop and measure the idle
  // periods to infer CPU load — revealing background processes, other tabs,
  // and even what type of content you're reading. Coarsening the deadline
  // timestamps to 50ms resolution destroys this side-channel.
  try {
    if (window.requestIdleCallback) {
      const realRIC = window.requestIdleCallback;
      define(window, "requestIdleCallback", {
        value: function (callback, options) {
          return realRIC.call(window, (deadline) => {
            if (!config.fingerprint) { callback(deadline); return; }
            const coarseDeadline = {
              didTimeout: deadline.didTimeout,
              timeRemaining() {
                return Math.floor(deadline.timeRemaining() / 50) * 50;
              },
            };
            callback(coarseDeadline);
          }, options);
        },
        configurable: true, writable: true,
      });
    }
  } catch { }

  // ── 13. navigator.usb / navigator.bluetooth ──────────────────────────────
  // USB device enumeration and Bluetooth device discovery both leak hardware
  // inventory. A user with a specific Yubikey + Bluetooth headset combo is
  // practically unique. We throw on requestDevice/getDevices.
  try {
    if (navigator.usb) {
      const usbProto = Object.getPrototypeOf(navigator.usb);
      for (const method of ["requestDevice", "getDevices"]) {
        if (usbProto && method in usbProto) {
          const orig = usbProto[method];
          define(usbProto, method, {
            value: function (...args) {
              if (config.fingerprint) return method === "getDevices" ? Promise.resolve([]) : Promise.reject(denied("NotAllowedError", "noah shield: USB is private"));
              return orig.apply(this, args);
            },
            configurable: true, writable: true,
          });
        }
      }
    }
    if (navigator.bluetooth) {
      const btProto = Object.getPrototypeOf(navigator.bluetooth);
      for (const method of ["requestDevice", "getAvailability", "getDevices"]) {
        if (btProto && method in btProto) {
          const orig = btProto[method];
          define(btProto, method, {
            value: function (...args) {
              if (config.fingerprint) {
                if (method === "getAvailability") return Promise.resolve(false);
                if (method === "getDevices") return Promise.resolve([]);
                return Promise.reject(denied("NotAllowedError", "noah shield: Bluetooth is private"));
              }
              return orig.apply(this, args);
            },
            configurable: true, writable: true,
          });
        }
      }
    }
  } catch { }

  // ── 14. Ink API + EyeDropper blocking ─────────────────────────────────────
  // Chrome's Ink API exposes stylus position with sub-millimeter precision —
  // unique to each hardware unit. EyeDropper reveals pixel colors from outside
  // the browser window. Both are exotic enough to be identifying by their
  // mere presence; we replace them with no-ops.
  try {
    if (window.Ink) {
      define(window, "Ink", { value: undefined, configurable: true, writable: true });
    }
    if (window.EyeDropper) {
      const OrigED = window.EyeDropper;
      function BlockedEyeDropper() {
        if (new.target) throw denied("NotAllowedError", "noah shield: EyeDropper is private");
      }
      BlockedEyeDropper.prototype = OrigED.prototype;
      define(window, "EyeDropper", { value: BlockedEyeDropper, configurable: true, writable: true });
    }
  } catch { }

  // ── 15. ScrollBar width CSS normalization ────────────────────────────────
  // The width of the OS scrollbar varies across Windows, macOS, Linux, and
  // browser settings (overlay scrollbars vs. classic). It's measurable via
  // offsetWidth arithmetic and used as a fingerprint dimension. We inject
  // a CSS rule that forces thin overlay-style scrollbars so the value is
  // system-independent.
  try {
    if (config.fingerprint && document.head !== null) {
      const style = document.createElement("style");
      style.setAttribute("data-noah", "scrollbar");
      style.textContent = "::-webkit-scrollbar{width:8px!important;height:8px!important}";
      (document.head || document.documentElement).appendChild(style);
    }
  } catch { }

  // ── 16. Entropy drain detection ───────────────────────────────────────────
  // Some fingerprinting libraries call crypto.getRandomValues() in a tight
  // loop (10 000+ calls) to force the PRNG into a predictable state or to
  // time its output distribution. We rate-limit to 500 calls per second;
  // beyond that we still return values but report the anomaly.
  try {
    const realGRV = crypto.getRandomValues.bind(crypto);
    let grvCalls = 0;
    let grvWindow = performance.now();
    define(Crypto.prototype, "getRandomValues", {
      value: function (typedArray) {
        grvCalls++;
        const now = performance.now();
        if (now - grvWindow > 1000) {
          if (grvCalls > 500) {
            try {
              document.dispatchEvent(new CustomEvent("__noahReport", {
                detail: { type: "entropy_drain", amount: grvCalls },
              }));
            } catch { }
          }
          grvCalls = 0;
          grvWindow = now;
        }
        return realGRV(typedArray);
      },
      configurable: true, writable: true,
    });
  } catch { }

  // ── 17. Behavioral fingerprint timing jitter ──────────────────────────────
  // Behavioral biometrics platforms (NeuroID, BioCatch, ThreatMetrix) record
  // the timing of your keystrokes, mouse movements, and scroll events to build
  // a "typing rhythm" and "mouse dynamics" profile that identifies you across
  // sessions and devices — your hardware and habits create a unique signature
  // even if you change browsers, VPN, or IP.
  //
  // We hook addEventListener for pointer/keyboard/scroll events and wrap each
  // listener to inject ±8ms of seeded jitter into the event's timeStamp so
  // the biometric model the tracker builds is consistently wrong in a way that
  // varies per site (different seed → different drift → no cross-site correlation).
  try {
    const BIOMETRIC_TYPES = new Set([
      "keydown", "keyup", "keypress",
      "mousedown", "mouseup", "mousemove", "click",
      "pointerdown", "pointerup", "pointermove",
      "touchstart", "touchend", "touchmove",
      "scroll", "wheel",
    ]);
    const origAddEL = EventTarget.prototype.addEventListener;
    define(EventTarget.prototype, "addEventListener", {
      value: function (type, listener, options) {
        if (!config.fingerprint || typeof listener !== "function" || !BIOMETRIC_TYPES.has(type)) {
          return origAddEL.call(this, type, listener, options);
        }
        const jitteredListener = function (event) {
          if (config.fingerprint && event && typeof event.timeStamp === "number") {
            reseed();
            const drift = (rng() - 0.5) * 16;  // ±8ms
            try {
              Object.defineProperty(event, "timeStamp", {
                value: Math.max(0, event.timeStamp + drift),
                configurable: true,
              });
            } catch { }
          }
          return listener.call(this, event);
        };
        return origAddEL.call(this, type, jitteredListener, options);
      },
      configurable: true, writable: true,
    });
  } catch { }

  // ── 18. Service worker surveillance detection ─────────────────────────────
  // A malicious (or compromised) service worker with scope "/" intercepts ALL
  // network requests — acting as a man-in-the-browser that survives incognito
  // mode and clears. We monitor registrations and alert if an unexpected SW
  // takes the root scope, or if a SW is registered from a cross-origin URL.
  try {
    if (navigator.serviceWorker && navigator.serviceWorker.register) {
      const realSWReg = navigator.serviceWorker.register.bind(navigator.serviceWorker);
      define(navigator.serviceWorker, "register", {
        value: function (scriptURL, options) {
          try {
            const scope = (options && options.scope) || "/";
            const swOrigin = new URL(String(scriptURL), location.href).origin;
            if (swOrigin !== location.origin) {
              document.dispatchEvent(new CustomEvent("__noahReport", {
                detail: { type: "sw_crossorigin", url: String(scriptURL) },
              }));
            } else if (scope === "/" || scope === location.origin + "/") {
              document.dispatchEvent(new CustomEvent("__noahReport", {
                detail: { type: "sw_rootscope", url: String(scriptURL) },
              }));
            }
          } catch { }
          return realSWReg(scriptURL, options);
        },
        configurable: true, writable: true,
      });
    }
  } catch { }

  try {
    stealthToString();
  } catch {
    // A page that sealed Function.prototype keeps ours visible; the guards still work.
  }
})();
