// Inspect mode: everything the page reads or sends, uncensored, for a team
// stress-testing its own site. Runs in the page's own world (so wrapping
// fetch, XMLHttpRequest, WebSocket, sendBeacon and form submission catches
// what the app itself sees). Off by default. The popup arms it per tab and
// warns that turning it on records credentials and secrets in plain text.
//
// Everything intercepted stays in a ring buffer in the page and never leaves
// the browser on its own. The panel has a copy-out button for handing the
// transcript to the security team.
(() => {
  "use strict";
  if (window.__noahShieldInspect) return;
  const scheme = location.protocol;
  if (scheme !== "http:" && scheme !== "https:") return;
  window.__noahShieldInspect = true;

  const MAX_EVENTS = 4000;
  const MAX_BODY = 200000;
  const events = [];
  const buffer = [];
  let enabled = null; // null: waiting for the bridge, true: on, false: off
  let panel = null;
  let collapsed = false;
  let paused = false;
  let counter = 0;

  function push(event) {
    event.id = ++counter;
    event.at = Date.now();
    if (enabled === null) {
      buffer.push(event);
      if (buffer.length > MAX_EVENTS) buffer.shift();
      return;
    }
    if (!enabled || paused) return;
    events.push(event);
    if (events.length > MAX_EVENTS) events.shift();
    window.dispatchEvent(new CustomEvent("noah-inspect-out", { detail: event }));
    renderRow(event);
  }

  window.addEventListener("noah-inspect-decision", (event) => {
    enabled = Boolean(event.detail && event.detail.enabled);
    if (enabled) {
      // Flush anything the hooks caught while we waited.
      const pending = buffer.splice(0);
      for (const item of pending) events.push(item);
      if (events.length > MAX_EVENTS) events.splice(0, events.length - MAX_EVENTS);
      renderPanel();
    } else {
      buffer.length = 0;
    }
  });
  window.addEventListener("noah-inspect-control", (event) => {
    const detail = event.detail || {};
    if (detail.action === "off") {
      enabled = false;
      buffer.length = 0;
      events.length = 0;
      if (panel) { panel.remove(); panel = null; }
    } else if (detail.action === "clear") {
      events.length = 0;
      if (panel) renderPanel();
    }
  });

  // ---- helpers ----------------------------------------------------------
  function textOf(value, headers) {
    try {
      if (value == null) return null;
      if (typeof value === "string") return value.slice(0, MAX_BODY);
      if (value instanceof URLSearchParams) return value.toString().slice(0, MAX_BODY);
      if (value instanceof FormData) {
        const parts = [];
        for (const [name, entry] of value.entries()) {
          if (entry instanceof File) parts.push(`${name}=<file ${entry.name} ${entry.size} bytes>`);
          else parts.push(`${name}=${String(entry)}`);
        }
        return parts.join("&").slice(0, MAX_BODY);
      }
      if (value instanceof Blob) return `<blob ${value.type || "unknown"} ${value.size} bytes>`;
      if (value instanceof ArrayBuffer) return `<binary ${value.byteLength} bytes>`;
      if (ArrayBuffer.isView(value)) return `<binary ${value.byteLength} bytes>`;
      if (typeof value === "object") {
        try { return JSON.stringify(value).slice(0, MAX_BODY); } catch { return String(value).slice(0, MAX_BODY); }
      }
      return String(value).slice(0, MAX_BODY);
    } catch (error) {
      return "<unreadable: " + String(error && error.message ? error.message : error) + ">";
    }
  }
  function headersOf(headers) {
    const out = {};
    if (!headers) return out;
    if (headers instanceof Headers) {
      for (const [name, value] of headers.entries()) out[name] = value;
    } else if (Array.isArray(headers)) {
      for (const [name, value] of headers) out[name] = value;
    } else if (typeof headers === "object") {
      for (const key of Object.keys(headers)) out[key] = headers[key];
    }
    return out;
  }
  function findSecrets(...texts) {
    const hits = [];
    const seen = new Set();
    const patterns = [
      { kind: "jwt", re: /\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b/g },
      { kind: "aws-access-key", re: /\bAKIA[0-9A-Z]{16}\b/g },
      { kind: "aws-secret", re: /\b(?<![A-Za-z0-9\/+])[A-Za-z0-9\/+]{40}\b(?![A-Za-z0-9\/+])/g },
      { kind: "github-token", re: /\b(gh[pousr]_[A-Za-z0-9]{36,})\b/g },
      { kind: "google-api", re: /\bAIza[0-9A-Za-z\-_]{35}\b/g },
      { kind: "slack-token", re: /\bxox[abpors]-[A-Za-z0-9-]{10,}\b/g },
      { kind: "stripe-secret", re: /\b(sk|rk)_(live|test)_[A-Za-z0-9]{16,}\b/g },
      { kind: "openai-key", re: /\bsk-[A-Za-z0-9_-]{20,}\b/g },
      { kind: "private-key-block", re: /-----BEGIN [A-Z ]+PRIVATE KEY-----/g },
      // Social platforms carry their own session shapes; a security team
      // auditing their own site wants these called out just as loudly.
      { kind: "twitter-bearer", re: /\bAAAAAAAAAAAAAAAAAAAAA[A-Za-z0-9%]{40,}\b/g },
      { kind: "twitter-guest-token", re: /guest_token["'=:\s]+(\d{15,25})/g },
      { kind: "twitter-csrf", re: /(?:x-csrf-token|ct0)["'=:\s]+([a-f0-9]{32,})/gi },
      { kind: "twitter-auth-cookie", re: /(?:auth_token|kdt|twid)["'=:\s]+([A-Za-z0-9%_.-]{20,})/gi },
      { kind: "facebook-token", re: /\bEAA[A-Za-z0-9]{50,}\b/g },
      { kind: "facebook-cookie", re: /(?:c_user|xs|fr|datr|sb)=([^;\s"']{10,})/gi },
      { kind: "session-cookie", re: /(?:sessionid|PHPSESSID|JSESSIONID|connect\.sid|next-auth\.session-token)=([^;\s"']{16,})/gi },
      { kind: "bearer-header", re: /\bBearer\s+([A-Za-z0-9._~+/=-]{16,})/gi },
      { kind: "instagram-cookie", re: /(?:sessionid|ds_user_id|csrftoken|ig_did)=([^;\s"']{10,})/gi },
      { kind: "tiktok-cookie", re: /(?:tt_webid|msToken|ttwid)=([^;\s"']{10,})/gi },
      { kind: "email", re: /\b[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[A-Z|a-z]{2,}\b/g },
      { kind: "credit-card", re: /\b(?:4\d{12}(?:\d{3})?|5[1-5]\d{14}|3[47]\d{13}|6(?:011|5\d{2})\d{12})\b/g },
      { kind: "us-ssn", re: /\b(?!000|666|9)\d{3}-(?!00)\d{2}-(?!0000)\d{4}\b/g },
      { kind: "phone", re: /\+?\d{1,3}[\s-]?\(?\d{3}\)?[\s-]?\d{3}[\s-]?\d{4}\b/g },
    ];
    for (const text of texts) {
      if (!text) continue;
      const value = typeof text === "string" ? text : String(text);
      for (const { kind, re } of patterns) {
        re.lastIndex = 0;
        let m;
        while ((m = re.exec(value))) {
          const key = kind + ":" + m[0];
          if (seen.has(key)) continue;
          seen.add(key);
          hits.push({ kind, snippet: m[0].slice(0, 80) });
        }
      }
    }
    return hits;
  }
  // GraphQL is how X, Twitter, Instagram and many other platforms carry
  // their user data. The operation name in the URL or body tells you what
  // the request actually does, which matters when auditing a page's API.
  function graphqlOp(url, body) {
    try {
      const match = /\/graphql\/[^/]+\/([A-Za-z0-9_-]+)/.exec(url || "");
      if (match) return match[1];
    } catch {}
    if (body && typeof body === "string") {
      try {
        const parsed = JSON.parse(body);
        if (parsed && parsed.operationName) return String(parsed.operationName);
        if (Array.isArray(parsed) && parsed[0] && parsed[0].operationName) return String(parsed[0].operationName);
      } catch {}
      const m = /"operationName"\s*:\s*"([^"]+)"/.exec(body);
      if (m) return m[1];
    }
    return null;
  }
  function currentCookies() {
    try {
      return document.cookie ? document.cookie : "";
    } catch { return ""; }
  }
  function decodeJwt(text) {
    if (!text) return null;
    const match = /eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+/.exec(text);
    if (!match) return null;
    const parts = match[0].split(".");
    const decode = (part) => {
      try {
        const padded = part.replace(/-/g, "+").replace(/_/g, "/") + "===".slice((part.length + 3) % 4);
        return JSON.parse(atob(padded));
      } catch { return null; }
    };
    return { header: decode(parts[0]), payload: decode(parts[1]) };
  }

  // ---- fetch ----------------------------------------------------------
  const realFetch = window.fetch;
  if (typeof realFetch === "function") {
    window.fetch = async function (input, init) {
      const started = performance.now();
      const request = input instanceof Request ? input : null;
      const method = ((init && init.method) || (request && request.method) || "GET").toUpperCase();
      const url = typeof input === "string" ? input : (request && request.url) || String(input);
      const requestHeaders = headersOf((init && init.headers) || (request && request.headers));
      let requestBody = null;
      if (init && init.body != null) requestBody = textOf(init.body);
      else if (request && request.body && method !== "GET" && method !== "HEAD") {
        try { requestBody = await request.clone().text(); requestBody = requestBody.slice(0, MAX_BODY); } catch {}
      }
      let response, error;
      try { response = await realFetch.call(this, input, init); }
      catch (thrown) { error = thrown; }
      const elapsed = Math.round(performance.now() - started);
      const event = {
        kind: "fetch",
        url,
        method,
        requestHeaders,
        requestBody,
        elapsedMs: elapsed,
      };
      if (error) {
        event.error = String(error && error.message ? error.message : error);
      } else if (response) {
        event.status = response.status;
        event.statusText = response.statusText;
        event.responseHeaders = headersOf(response.headers);
        event.responseUrl = response.url;
        try {
          const clone = response.clone();
          const type = (response.headers.get("content-type") || "").toLowerCase();
          // Streaming responses (SSE, chunked JSON) never finish — race the
          // read against a five-second timer so a live feed never stalls
          // this event, and put a note on the row instead.
          if (/^text\/event-stream/.test(type)) {
            event.responseBody = "<streaming response (server-sent events); frames captured separately>";
          } else if (/^(text\/|application\/(json|xml|xhtml|javascript|x-www-form-urlencoded|graphql)|application\/[^;]*\+json)/.test(type) || type === "") {
            const timeout = new Promise((_, reject) => setTimeout(() => reject(new Error("read-timeout")), 5000));
            try {
              const body = await Promise.race([clone.text(), timeout]);
              event.responseBody = body.slice(0, MAX_BODY);
              if (body.length > MAX_BODY) event.responseTruncated = body.length;
            } catch (readError) {
              event.responseBody = String(readError && readError.message === "read-timeout"
                ? "<response body still streaming after 5s>"
                : "<could not read response body>");
            }
          } else {
            event.responseBody = `<binary ${type || "unknown"}>`;
          }
        } catch {}
      }
      const jwt = decodeJwt((requestHeaders.authorization || requestHeaders.Authorization || "") + "\n" + (requestBody || ""));
      if (jwt) event.jwt = jwt;
      const op = graphqlOp(url, requestBody);
      if (op) event.graphql = op;
      event.cookies = currentCookies();
      event.secrets = findSecrets(requestBody, event.responseBody, event.cookies, JSON.stringify(requestHeaders), JSON.stringify(event.responseHeaders));
      push(event);
      if (error) throw error;
      return response;
    };
  }

  // ---- XMLHttpRequest -----------------------------------------------
  const XHR = window.XMLHttpRequest;
  if (XHR && XHR.prototype) {
    const realOpen = XHR.prototype.open;
    const realSend = XHR.prototype.send;
    const realSetHeader = XHR.prototype.setRequestHeader;
    XHR.prototype.open = function (method, url, ...rest) {
      this.__noahInspect = { method: String(method).toUpperCase(), url: String(url), headers: {}, started: performance.now() };
      return realOpen.call(this, method, url, ...rest);
    };
    XHR.prototype.setRequestHeader = function (name, value) {
      if (this.__noahInspect) this.__noahInspect.headers[name] = value;
      return realSetHeader.call(this, name, value);
    };
    XHR.prototype.send = function (body) {
      const noah = this.__noahInspect;
      if (noah) {
        noah.body = textOf(body);
        this.addEventListener("loadend", () => {
          let responseHeaders = {};
          try {
            const raw = this.getAllResponseHeaders() || "";
            for (const line of raw.split(/\r?\n/)) {
              const index = line.indexOf(":");
              if (index > 0) responseHeaders[line.slice(0, index).trim()] = line.slice(index + 1).trim();
            }
          } catch {}
          let responseBody = null;
          try {
            const type = (responseHeaders["content-type"] || "").toLowerCase();
            if (this.responseType === "" || this.responseType === "text") {
              responseBody = (this.responseText || "").slice(0, MAX_BODY);
            } else if (this.responseType === "json") {
              try { responseBody = JSON.stringify(this.response).slice(0, MAX_BODY); } catch {}
            } else {
              responseBody = `<binary ${type || this.responseType || "unknown"}>`;
            }
          } catch {}
          const event = {
            kind: "xhr",
            method: noah.method,
            url: noah.url,
            requestHeaders: noah.headers,
            requestBody: noah.body,
            status: this.status,
            statusText: this.statusText,
            responseHeaders,
            responseBody,
            elapsedMs: Math.round(performance.now() - noah.started),
          };
          const jwt = decodeJwt((noah.headers.authorization || noah.headers.Authorization || "") + "\n" + (noah.body || ""));
          if (jwt) event.jwt = jwt;
          const op = graphqlOp(noah.url, noah.body);
          if (op) event.graphql = op;
          event.cookies = currentCookies();
          event.secrets = findSecrets(noah.body, responseBody, event.cookies, JSON.stringify(noah.headers), JSON.stringify(responseHeaders));
          push(event);
        });
      }
      return realSend.call(this, body);
    };
  }

  // ---- WebSocket ----------------------------------------------------
  const RealSocket = window.WebSocket;
  if (RealSocket) {
    function Wrapped(url, protocols) {
      const socket = protocols === undefined ? new RealSocket(url) : new RealSocket(url, protocols);
      const started = performance.now();
      push({ kind: "ws-open", url: String(url), protocols: protocols || null });
      const realSend = socket.send.bind(socket);
      socket.send = function (data) {
        push({ kind: "ws-send", url: String(url), body: textOf(data) });
        return realSend(data);
      };
      socket.addEventListener("message", (message) => {
        push({ kind: "ws-recv", url: String(url), body: textOf(message.data) });
      });
      socket.addEventListener("close", (event) => {
        push({ kind: "ws-close", url: String(url), code: event.code, reason: event.reason, elapsedMs: Math.round(performance.now() - started) });
      });
      socket.addEventListener("error", () => {
        push({ kind: "ws-error", url: String(url) });
      });
      return socket;
    }
    Wrapped.prototype = RealSocket.prototype;
    Wrapped.CONNECTING = RealSocket.CONNECTING;
    Wrapped.OPEN = RealSocket.OPEN;
    Wrapped.CLOSING = RealSocket.CLOSING;
    Wrapped.CLOSED = RealSocket.CLOSED;
    window.WebSocket = Wrapped;
  }

  // ---- EventSource (Server-Sent Events) ----------------------------
  // Live feeds on social platforms often come as EventSource streams
  // (notifications, timelines, chats). The response body isn't reachable
  // through fetch/XHR hooks, so we wrap the constructor and its onmessage.
  const RealES = window.EventSource;
  if (RealES) {
    function WrappedES(url, config) {
      const source = config === undefined ? new RealES(url) : new RealES(url, config);
      const started = performance.now();
      push({ kind: "sse-open", url: String(url), withCredentials: Boolean(config && config.withCredentials) });
      const dispatch = source.addEventListener.bind(source);
      dispatch("message", (event) => {
        push({ kind: "sse-recv", url: String(url), lastEventId: event.lastEventId || null, body: textOf(event.data), secrets: findSecrets(textOf(event.data)) });
      });
      dispatch("error", () => { push({ kind: "sse-error", url: String(url), readyState: source.readyState, elapsedMs: Math.round(performance.now() - started) }); });
      return source;
    }
    WrappedES.prototype = RealES.prototype;
    WrappedES.CONNECTING = RealES.CONNECTING;
    WrappedES.OPEN = RealES.OPEN;
    WrappedES.CLOSED = RealES.CLOSED;
    window.EventSource = WrappedES;
  }

  // ---- sendBeacon --------------------------------------------------
  if (navigator.sendBeacon) {
    const realBeacon = navigator.sendBeacon.bind(navigator);
    navigator.sendBeacon = function (url, data) {
      push({ kind: "beacon", url: String(url), body: textOf(data), secrets: findSecrets(textOf(data)) });
      return realBeacon(url, data);
    };
  }

  // ---- form submission ---------------------------------------------
  document.addEventListener("submit", (event) => {
    try {
      const form = event.target;
      if (!(form instanceof HTMLFormElement)) return;
      const fields = [];
      for (const [name, value] of new FormData(form).entries()) {
        fields.push({ name, value: typeof value === "string" ? value : `<file ${value.name || ""} ${value.size} bytes>` });
      }
      push({
        kind: "form",
        url: form.action || location.href,
        method: (form.method || "GET").toUpperCase(),
        enctype: form.enctype || null,
        fields,
        secrets: findSecrets(fields.map((f) => `${f.name}=${f.value}`).join("\n")),
      });
    } catch {}
  }, true);

  // ---- errors and CSP violations -----------------------------------
  window.addEventListener("error", (event) => {
    push({ kind: "error", message: event.message || String(event.error), source: event.filename || null, line: event.lineno || null, column: event.colno || null });
  });
  window.addEventListener("unhandledrejection", (event) => {
    push({ kind: "unhandled", reason: textOf(event.reason) });
  });
  document.addEventListener("securitypolicyviolation", (event) => {
    push({ kind: "csp", directive: event.effectiveDirective || event.violatedDirective, blockedURI: event.blockedURI, sample: event.sample || "" });
  });

  // ---- postMessage ------------------------------------------------
  const realPost = window.postMessage.bind(window);
  window.postMessage = function (...args) {
    const [message, targetOrArgs] = args;
    const to = targetOrArgs && typeof targetOrArgs === "object" && !Array.isArray(targetOrArgs) && "targetOrigin" in targetOrArgs
      ? targetOrArgs.targetOrigin
      : (typeof targetOrArgs === "string" ? targetOrArgs : "*");
    push({ kind: "postMessage.send", to: String(to || "*"), body: textOf(message) });
    return realPost(...args);
  };
  window.addEventListener("message", (event) => {
    push({ kind: "postMessage.recv", from: event.origin || "?", body: textOf(event.data) });
  });

  // ---- snapshot helpers ------------------------------------------
  function snapshotStorage() {
    const cookies = document.cookie.split(/;\s*/).filter(Boolean).map((entry) => {
      const eq = entry.indexOf("=");
      return { name: entry.slice(0, eq), value: decodeURIComponent(entry.slice(eq + 1)) };
    });
    const readStore = (store) => {
      const out = [];
      try { for (let i = 0; i < store.length; i++) { const key = store.key(i); out.push({ key, value: store.getItem(key) }); } } catch {}
      return out;
    };
    push({ kind: "snapshot", cookies, localStorage: readStore(window.localStorage), sessionStorage: readStore(window.sessionStorage) });
  }

  // ---- panel ------------------------------------------------------
  function ensurePanel() {
    if (panel) return;
    panel = document.createElement("div");
    panel.id = "noah-inspect-panel";
    panel.innerHTML = `
      <style>
        #noah-inspect-panel { position: fixed; right: 12px; bottom: 12px; z-index: 2147483647; width: 460px; max-height: 78vh; display: flex; flex-direction: column; background: rgba(8, 12, 10, .96); color: #d8ddd6; border: 1px solid rgba(180, 210, 190, .16); border-radius: 12px; box-shadow: 0 12px 34px rgba(0,0,0,.5); font: 12px/1.5 -apple-system, "Segoe UI", system-ui, sans-serif; overflow: hidden; }
        #noah-inspect-panel header { display: flex; align-items: center; gap: 8px; padding: 9px 11px; border-bottom: 1px solid rgba(180, 210, 190, .1); }
        #noah-inspect-panel header b { font-family: Georgia, serif; font-weight: 400; font-size: 14px; color: #f1f4ef; flex: 1; }
        #noah-inspect-panel .noah-inspect-warn { padding: 8px 11px; background: rgba(220, 200, 150, .08); color: #dcc896; font-size: 11px; border-bottom: 1px solid rgba(220, 200, 150, .18); }
        #noah-inspect-panel .noah-inspect-tools { display: flex; gap: 6px; padding: 8px 11px; border-bottom: 1px solid rgba(180, 210, 190, .08); align-items: center; }
        #noah-inspect-panel .noah-inspect-tools input { flex: 1; background: rgba(180, 210, 190, .05); color: #d8ddd6; border: 1px solid rgba(180, 210, 190, .12); border-radius: 6px; padding: 3px 8px; font: inherit; }
        #noah-inspect-panel button { all: unset; cursor: pointer; padding: 3px 8px; border-radius: 6px; color: #a9cf9f; border: 1px solid rgba(180, 210, 190, .18); font-size: 11px; }
        #noah-inspect-panel button:hover { background: rgba(180, 210, 190, .06); color: #f1f4ef; }
        #noah-inspect-panel .noah-inspect-list { flex: 1; overflow-y: auto; scrollbar-width: thin; scrollbar-color: rgba(180,210,190,.2) transparent; padding: 4px 0; }
        #noah-inspect-panel .noah-inspect-row { padding: 6px 11px; border-bottom: 1px solid rgba(180, 210, 190, .04); }
        #noah-inspect-panel .noah-inspect-row .noah-inspect-head { display: flex; gap: 8px; cursor: pointer; align-items: baseline; }
        #noah-inspect-panel .noah-inspect-row .noah-inspect-kind { color: #9aa298; font-size: 10px; letter-spacing: .06em; text-transform: uppercase; width: 60px; flex-shrink: 0; }
        #noah-inspect-panel .noah-inspect-row .noah-inspect-status { font-size: 10px; color: #9aa298; }
        #noah-inspect-panel .noah-inspect-row .noah-inspect-status.ok { color: #a9cf9f; }
        #noah-inspect-panel .noah-inspect-row .noah-inspect-status.bad { color: #e69696; }
        #noah-inspect-panel .noah-inspect-row .noah-inspect-url { color: #d8ddd6; word-break: break-all; font-size: 11px; flex: 1; }
        #noah-inspect-panel .noah-inspect-row .noah-inspect-secret { color: #e69696; font-size: 10px; margin-left: 6px; }
        #noah-inspect-panel .noah-inspect-row .noah-inspect-op { color: #a9cf9f; font-size: 10px; margin-left: 4px; font-family: ui-monospace, SFMono-Regular, monospace; }
        #noah-inspect-panel .noah-inspect-body { display: none; margin-top: 6px; padding: 6px 8px; background: rgba(180, 210, 190, .03); border-radius: 6px; font-family: ui-monospace, SFMono-Regular, monospace; font-size: 11px; color: #b9beb7; white-space: pre-wrap; word-break: break-all; max-height: 260px; overflow-y: auto; }
        #noah-inspect-panel .noah-inspect-row.open .noah-inspect-body { display: block; }
        #noah-inspect-panel .noah-inspect-empty { padding: 12px 11px; color: #9aa298; font-size: 12px; border-bottom: 1px solid rgba(180, 210, 190, .04); }
        #noah-inspect-panel .noah-inspect-empty a { color: #a9cf9f; text-decoration: underline; }
        #noah-inspect-panel[data-has-events="true"] .noah-inspect-empty { display: none; }
        #noah-inspect-panel footer { padding: 6px 11px; font-size: 10px; color: #6f766e; border-top: 1px solid rgba(180, 210, 190, .08); }
        #noah-inspect-panel[data-collapsed="true"] .noah-inspect-list, #noah-inspect-panel[data-collapsed="true"] .noah-inspect-tools, #noah-inspect-panel[data-collapsed="true"] .noah-inspect-warn, #noah-inspect-panel[data-collapsed="true"] footer { display: none; }
      </style>
      <header>
        <b>inspect · <span class="noah-inspect-count">0</span></b>
        <button class="noah-inspect-pause">pause</button>
        <button class="noah-inspect-snap">snapshot</button>
        <button class="noah-inspect-copy">copy all</button>
        <button class="noah-inspect-clear">clear</button>
        <button class="noah-inspect-toggle">hide</button>
      </header>
      <div class="noah-inspect-warn">recording this page's requests, forms, storage, WebSocket frames, live streams and postMessage traffic in full, tokens and secrets included. only turn this on for a site you own or a platform you are authorised to audit.</div>
      <div class="noah-inspect-tools">
        <input placeholder="filter (url or body substring)" class="noah-inspect-filter">
        <button class="noah-inspect-reload" title="Reload so the panel catches the requests the page already made">reload</button>
      </div>
      <div class="noah-inspect-list"></div>
      <div class="noah-inspect-empty">Nothing recorded yet on this page. Anything the page fetches, posts, streams or reads next lands here in full. If the page has already finished loading, <a href="javascript:void(0)" class="noah-inspect-reload-link">reload</a> to catch what it already sent.</div>
      <footer>Nothing leaves the browser. Copy the transcript into your bug tracker.</footer>
    `;
    document.documentElement.append(panel);
    panel.querySelector(".noah-inspect-copy").addEventListener("click", copyAll);
    panel.querySelector(".noah-inspect-clear").addEventListener("click", () => { events.length = 0; renderPanel(); });
    panel.querySelector(".noah-inspect-pause").addEventListener("click", (event) => {
      paused = !paused;
      event.target.textContent = paused ? "resume" : "pause";
    });
    panel.querySelector(".noah-inspect-snap").addEventListener("click", snapshotStorage);
    panel.querySelector(".noah-inspect-toggle").addEventListener("click", (event) => {
      collapsed = !collapsed;
      panel.dataset.collapsed = collapsed ? "true" : "false";
      event.target.textContent = collapsed ? "show" : "hide";
    });
    panel.querySelector(".noah-inspect-filter").addEventListener("input", renderList);
    const reloadNow = () => { try { location.reload(); } catch {} };
    panel.querySelector(".noah-inspect-reload").addEventListener("click", reloadNow);
    panel.querySelector(".noah-inspect-reload-link").addEventListener("click", reloadNow);
  }

  function renderPanel() {
    ensurePanel();
    renderList();
  }
  function renderList() {
    if (!panel) return;
    const list = panel.querySelector(".noah-inspect-list");
    list.replaceChildren();
    const filter = panel.querySelector(".noah-inspect-filter").value.trim().toLowerCase();
    for (const event of events) {
      if (filter) {
        const haystack = (event.url || "") + " " + (event.requestBody || "") + " " + (event.responseBody || "") + " " + (event.body || "");
        if (!haystack.toLowerCase().includes(filter)) continue;
      }
      list.append(rowFor(event));
    }
    panel.querySelector(".noah-inspect-count").textContent = String(events.length);
    panel.dataset.hasEvents = events.length ? "true" : "false";
  }
  function renderRow(event) {
    if (!panel) return;
    const list = panel.querySelector(".noah-inspect-list");
    const filter = panel.querySelector(".noah-inspect-filter").value.trim().toLowerCase();
    if (filter) {
      const haystack = (event.url || "") + " " + (event.requestBody || "") + " " + (event.responseBody || "") + " " + (event.body || "");
      if (!haystack.toLowerCase().includes(filter)) return;
    }
    list.append(rowFor(event));
    list.scrollTop = list.scrollHeight;
    panel.querySelector(".noah-inspect-count").textContent = String(events.length);
    panel.dataset.hasEvents = "true";
  }
  function rowFor(event) {
    const row = document.createElement("div");
    row.className = "noah-inspect-row";
    const head = document.createElement("div");
    head.className = "noah-inspect-head";
    const kind = document.createElement("span");
    kind.className = "noah-inspect-kind";
    kind.textContent = event.method ? event.method : event.kind;
    const url = document.createElement("span");
    url.className = "noah-inspect-url";
    url.textContent = event.url || event.message || event.from || event.directive || event.kind;
    head.append(kind, url);
    if (event.graphql) {
      const op = document.createElement("span");
      op.className = "noah-inspect-op";
      op.textContent = "· " + event.graphql;
      head.append(op);
    }
    if (event.status != null) {
      const status = document.createElement("span");
      status.className = "noah-inspect-status " + (event.status >= 500 ? "bad" : event.status >= 400 ? "bad" : event.status ? "ok" : "");
      status.textContent = event.status + " · " + event.elapsedMs + "ms";
      head.append(status);
    }
    if (event.secrets && event.secrets.length) {
      const secret = document.createElement("span");
      secret.className = "noah-inspect-secret";
      secret.textContent = "🔑 " + event.secrets.map((s) => s.kind).join(", ");
      head.append(secret);
    }
    row.append(head);
    const body = document.createElement("div");
    body.className = "noah-inspect-body";
    body.textContent = formatBody(event);
    row.append(body);
    head.addEventListener("click", () => row.classList.toggle("open"));
    return row;
  }
  function formatBody(event) {
    const lines = [];
    lines.push(new Date(event.at).toISOString());
    if (event.kind) lines.push("kind: " + event.kind);
    if (event.url) lines.push("url: " + event.url);
    if (event.method) lines.push("method: " + event.method);
    if (event.graphql) lines.push("graphql operation: " + event.graphql);
    if (event.status != null) lines.push("status: " + event.status + " " + (event.statusText || ""));
    if (event.elapsedMs != null) lines.push("elapsed: " + event.elapsedMs + "ms");
    if (event.requestHeaders && Object.keys(event.requestHeaders).length) {
      lines.push("\nrequest headers:");
      for (const [name, value] of Object.entries(event.requestHeaders)) lines.push("  " + name + ": " + value);
    }
    if (event.requestBody) lines.push("\nrequest body:\n" + event.requestBody);
    if (event.responseHeaders && Object.keys(event.responseHeaders).length) {
      lines.push("\nresponse headers:");
      for (const [name, value] of Object.entries(event.responseHeaders)) lines.push("  " + name + ": " + value);
    }
    if (event.responseBody) lines.push("\nresponse body:\n" + event.responseBody);
    if (event.body) lines.push("\nbody:\n" + event.body);
    if (event.fields) {
      lines.push("\nform fields:");
      for (const field of event.fields) lines.push("  " + field.name + ": " + field.value);
    }
    if (event.jwt) {
      lines.push("\njwt header: " + JSON.stringify(event.jwt.header));
      lines.push("jwt payload: " + JSON.stringify(event.jwt.payload));
    }
    if (event.secrets && event.secrets.length) {
      lines.push("\nsecrets found:");
      for (const secret of event.secrets) lines.push("  " + secret.kind + ": " + secret.snippet);
    }
    if (event.cookies) lines.push("\ncookies: " + JSON.stringify(event.cookies));
    if (event.localStorage) lines.push("localStorage: " + JSON.stringify(event.localStorage));
    if (event.sessionStorage) lines.push("sessionStorage: " + JSON.stringify(event.sessionStorage));
    if (event.error) lines.push("\nerror: " + event.error);
    if (event.reason) lines.push("\nreason: " + event.reason);
    if (event.message) lines.push("\nmessage: " + event.message);
    if (event.responseTruncated) lines.push("\n(response truncated at " + MAX_BODY + " bytes, real length " + event.responseTruncated + ")");
    return lines.join("\n");
  }
  function copyAll() {
    const transcript = { site: location.href, exportedAt: new Date().toISOString(), events };
    const text = JSON.stringify(transcript, null, 2);
    navigator.clipboard.writeText(text).then(() => {
      const button = panel.querySelector(".noah-inspect-copy");
      const previous = button.textContent;
      button.textContent = "copied " + events.length;
      setTimeout(() => (button.textContent = previous), 1500);
    }).catch((error) => {
      const button = panel.querySelector(".noah-inspect-copy");
      button.textContent = "copy failed: " + String(error.message || error).slice(0, 40);
    });
  }
})();
