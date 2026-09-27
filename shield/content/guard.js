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
      for (const key of Object.keys(incoming)) config[key] = incoming[key];
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
  const reseed = () => { seedValue = (Number(config.seed) || 0x9e3779b9) >>> 0; };
  const seededByte = () => { reseed(); return Math.floor(rng() * 255); };

  const canvasNoise = () => {
    const proto = HTMLCanvasElement.prototype;
    const context2d = CanvasRenderingContext2D.prototype;
    const toDataURL = proto.toDataURL;
    const toBlob = proto.toBlob;
    const getImageData = context2d.getImageData;
    const smudge = (canvas) => {
      try {
        const context = canvas.getContext("2d");
        if (!context || canvas.width === 0 || canvas.height === 0) return;
        const salt = seededByte();
        const image = getImageData.call(context, 0, 0, Math.min(canvas.width, 16), Math.min(canvas.height, 16));
        for (let index = 0; index < image.data.length; index += 4) image.data[index] = (image.data[index] + salt) & 255;
        context.putImageData(image, 0, 0);
      } catch {
        // A tainted canvas cannot be read by the page either.
      }
    };
    define(proto, "toDataURL", { value: function (...args) { if (config.fingerprint) smudge(this); return toDataURL.apply(this, args); }, configurable: true });
    define(proto, "toBlob", { value: function (...args) { if (config.fingerprint) smudge(this); return toBlob.apply(this, args); }, configurable: true });
    define(context2d, "getImageData", {
      value: function (...args) {
        const image = getImageData.apply(this, args);
        if (config.fingerprint && image && image.data.length > 64) {
          const salt = seededByte();
          image.data[0] = (image.data[0] + salt) & 255;
          image.data[image.data.length - 4] = (image.data[image.data.length - 4] + salt) & 255;
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
      [Date.prototype, "getTimezoneOffset"], [Intl, "DateTimeFormat"], [Intl.DateTimeFormat.prototype, "resolvedOptions"],
      [window, "alert"], [window, "confirm"], [window, "prompt"], [window, "addEventListener"], [EventTarget.prototype, "addEventListener"], [EventTarget.prototype, "removeEventListener"], [window, "onbeforeunload"], [Element.prototype, "requestFullscreen"],
      [window, "ethereum"],
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
  for (const hook of [canvasNoise, webglNoise, audioNoise, blendIn, fakeLocation]) {
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
  try {
    stealthToString();
  } catch {
    // A page that sealed Function.prototype keeps ours visible; the guards still work.
  }
})();
