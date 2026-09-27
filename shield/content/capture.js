// Screenshots of the page, driven from the popup: what is on screen, the
// whole page stitched together, or an area you drag out. Before the picture
// is taken, private details (emails, phone and card numbers, keys, filled-in
// personal fields) can be blurred in the page itself, so they are never in
// the file. Everything happens here and in the worker; nothing is uploaded.
(() => {
  "use strict";
  if (window.top !== window) return;
  // Injected again into a page that was open before the shield was updated;
  // one copy is enough.
  if (window.__noahShieldCapture) return;
  window.__noahShieldCapture = true;
  const api = globalThis.chrome ?? globalThis.browser;
  const send = (message) => new Promise((resolve) => {
    try { api.runtime.sendMessage(message, (response) => { void api.runtime.lastError; resolve(response || {}); }); } catch { resolve({}); }
  });
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const dpr = () => window.devicePixelRatio || 1;

  // ---- private details --------------------------------------------------------
  const PATTERNS = [
    /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g,
    /\b(?:\+?\d{1,3}[ .-]?)?(?:\(\d{2,4}\)[ .-]?)?\d{3}[ .-]?\d{3,4}[ .-]?\d{3,4}\b/g,
    /\b(?:\d[ -]?){13,19}\b/g,
    /\b[A-Z]{2}\d{2}(?:[ ]?[A-Z0-9]{4}){3,7}\b/g,
    /\b\d{3}-\d{2}-\d{4}\b/g,
    /\b(?:sk|pk|rk|ghp|gho|xox[abpr]|AKIA|AIza)[A-Za-z0-9_-]{12,}\b/g,
    /\b[A-Fa-f0-9]{32,}\b/g,
    /\b0x[a-fA-F0-9]{40}\b/g,
    /\b\d{1,5}\s+[A-Z][A-Za-z]+(?:\s+[A-Z][A-Za-z]+)*\s+(?:Street|St\.?|Avenue|Ave\.?|Road|Rd\.?|Lane|Ln\.?|Drive|Dr\.?|Boulevard|Blvd\.?|Way|Court|Ct\.?|Place|Pl\.?)\b/g,
  ];
  const FIELD = /^(email|tel|password)$/;
  const AUTOFILL = /(cc-|street-address|postal-code|address-line|tel|email|bday|name|organization|username)/i;
  const blurred = [];

  function blurRange(range) {
    const span = document.createElement("span");
    span.style.setProperty("filter", "blur(7px)", "important");
    span.style.setProperty("border-radius", "3px", "important");
    try {
      range.surroundContents(span);
      blurred.push(span);
    } catch {
      // The match crossed an element boundary; blur the whole text's parent instead.
      const parent = range.startContainer.parentElement;
      if (parent) blurElement(parent);
    }
  }

  function blurElement(element) {
    if (element.dataset.nsBlurred) return;
    element.dataset.nsBlurred = element.style.getPropertyValue("filter") || "none";
    element.style.setProperty("filter", "blur(8px)", "important");
    blurred.push(element);
  }

  function blurPrivate() {
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, {
      acceptNode: (node) => {
        const parent = node.parentElement;
        if (!parent || /^(SCRIPT|STYLE|NOSCRIPT|TEXTAREA)$/.test(parent.tagName)) return NodeFilter.FILTER_REJECT;
        return node.nodeValue.trim().length >= 6 ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP;
      },
    });
    const nodes = [];
    let node;
    while ((node = walker.nextNode())) nodes.push(node);
    for (const text of nodes) {
      const value = text.nodeValue;
      const hits = [];
      for (const pattern of PATTERNS) {
        pattern.lastIndex = 0;
        let match;
        while ((match = pattern.exec(value))) {
          if (match[0].replace(/\D/g, "").length >= 7 || /@|[A-Za-z]/.test(match[0])) hits.push([match.index, match.index + match[0].length]);
          if (!pattern.global) break;
        }
      }
      if (!hits.length) continue;
      // Later matches first, so earlier offsets stay valid after each wrap.
      hits.sort((left, right) => right[0] - left[0]);
      let lastStart = Infinity;
      for (const [start, end] of hits) {
        if (end > lastStart) continue;
        lastStart = start;
        const range = document.createRange();
        try {
          range.setStart(text, start);
          range.setEnd(text, Math.min(end, text.nodeValue.length));
          blurRange(range);
        } catch { /* the node changed under us */ }
      }
    }
    for (const field of document.querySelectorAll("input, textarea")) {
      const kind = (field.getAttribute("autocomplete") || field.name || field.id || "").toLowerCase();
      if (field.value && (FIELD.test(field.type) || AUTOFILL.test(kind))) blurElement(field);
    }
    for (const element of document.querySelectorAll("[data-sensitive], .sensitive, [aria-label*='account number' i], [aria-label*='balance' i]")) blurElement(element);
    return blurred.length;
  }

  function unblur() {
    for (const element of blurred.splice(0)) {
      if (element.tagName === "SPAN" && !element.dataset.nsBlurred) {
        const parent = element.parentNode;
        if (!parent) continue;
        while (element.firstChild) parent.insertBefore(element.firstChild, element);
        parent.removeChild(element);
        parent.normalize();
      } else {
        const before = element.dataset.nsBlurred;
        if (before === "none") element.style.removeProperty("filter");
        else element.style.setProperty("filter", before);
        delete element.dataset.nsBlurred;
      }
    }
  }

  // ---- the shots --------------------------------------------------------------
  function loadImage(dataUrl) {
    return new Promise((resolve, reject) => {
      const image = new Image();
      image.onload = () => resolve(image);
      image.onerror = () => reject(new Error("the capture could not be read"));
      image.src = dataUrl;
    });
  }

  async function shot() {
    const response = await send({ type: "capture.shot" });
    if (!response.dataUrl) throw new Error(response.error || "the browser refused the capture");
    return response.dataUrl;
  }

  function stamp() {
    const now = new Date();
    const two = (n) => String(n).padStart(2, "0");
    return `${now.getFullYear()}-${two(now.getMonth() + 1)}-${two(now.getDate())} ${two(now.getHours())}.${two(now.getMinutes())}.${two(now.getSeconds())}`;
  }

  async function save(canvasOrDataUrl, label) {
    const dataUrl = typeof canvasOrDataUrl === "string" ? canvasOrDataUrl : canvasOrDataUrl.toDataURL("image/png");
    const filename = `noah-shield/${label} ${stamp()}.png`;
    // Very large pages go over the message size; those download from here.
    if (dataUrl.length > 48 * 1024 * 1024) {
      const anchor = document.createElement("a");
      anchor.href = dataUrl;
      anchor.download = filename.replace("noah-shield/", "");
      anchor.click();
      return { ok: true, local: true };
    }
    return send({ type: "capture.save", dataUrl, filename });
  }

  async function visible() {
    return save(await shot(), "screenshot");
  }

  // Hidden while the page is scrolled through, so a sticky header is in the
  // picture once instead of on every strip.
  function freezeSticky() {
    const frozen = [];
    for (const element of document.querySelectorAll("body *")) {
      const style = getComputedStyle(element);
      if (style.position === "fixed" || style.position === "sticky") {
        frozen.push([element, element.style.getPropertyValue("visibility"), element.style.getPropertyPriority("visibility")]);
        element.style.setProperty("visibility", "hidden", "important");
      }
    }
    return () => { for (const [element, value, priority] of frozen) { if (value) element.style.setProperty("visibility", value, priority); else element.style.removeProperty("visibility"); } };
  }

  async function fullPage() {
    const scrollingElement = document.scrollingElement || document.documentElement;
    const startX = window.scrollX;
    const startY = window.scrollY;
    const viewHeight = window.innerHeight;
    const viewWidth = window.innerWidth;
    const ratio = dpr();
    // A canvas taller than about 16k device pixels fails silently in every
    // browser, so a very long page is cut there rather than lost.
    const totalHeight = Math.max(viewHeight, Math.min(scrollingElement.scrollHeight, Math.floor(16000 / ratio)));
    const canvas = document.createElement("canvas");
    canvas.width = Math.round(viewWidth * ratio);
    canvas.height = Math.round(totalHeight * ratio);
    const context = canvas.getContext("2d");
    const html = document.documentElement;
    const smooth = html.style.getPropertyValue("scroll-behavior");
    html.style.setProperty("scroll-behavior", "auto", "important");
    let thaw = () => {};
    try {
      let y = 0;
      let first = true;
      while (y < totalHeight) {
        window.scrollTo(startX, y);
        await sleep(first ? 250 : 120);
        const actualY = window.scrollY;
        const image = await loadImage(await shot());
        const drawHeight = Math.min(viewHeight, totalHeight - actualY);
        context.drawImage(image, 0, 0, image.width, Math.round(drawHeight * (image.height / viewHeight)), 0, Math.round(actualY * ratio), canvas.width, Math.round(drawHeight * ratio));
        if (first) { thaw = freezeSticky(); first = false; }
        if (actualY + viewHeight >= totalHeight) break;
        y = actualY + viewHeight;
        // captureVisibleTab allows about two shots a second.
        await sleep(520);
      }
    } finally {
      thaw();
      window.scrollTo(startX, startY);
      if (smooth) html.style.setProperty("scroll-behavior", smooth); else html.style.removeProperty("scroll-behavior");
    }
    return save(canvas, "full page");
  }

  function pickArea() {
    return new Promise((resolve) => {
      const host = document.createElement("div");
      const shadow = host.attachShadow({ mode: "closed" });
      const style = document.createElement("style");
      style.textContent = `
        :host { all: initial; position: fixed; inset: 0; z-index: 2147483647; cursor: crosshair; }
        .veil { position: absolute; inset: 0; background: rgba(0,0,0,.28); }
        .box { position: absolute; border: 1px solid #a9cf9f; background: rgba(169,207,159,.08); box-shadow: 0 0 0 9999px rgba(0,0,0,.28); display: none; }
        .hint { position: absolute; top: 18px; left: 50%; transform: translateX(-50%); padding: 8px 14px; border-radius: 10px; background: #0b0e0c; color: #d8ddd6; font: 13px -apple-system, "Segoe UI", system-ui, sans-serif; border: 1px solid rgba(180,210,190,.14); }
      `;
      const veil = document.createElement("div"); veil.className = "veil";
      const box = document.createElement("div"); box.className = "box";
      const hint = document.createElement("div"); hint.className = "hint"; hint.textContent = "drag over what you want in the picture · esc cancels";
      shadow.append(style, veil, box, hint);
      document.documentElement.append(host);
      let start = null;
      const finish = (rect) => { host.remove(); window.removeEventListener("keydown", onKey, true); resolve(rect); };
      const onKey = (event) => { if (event.key === "Escape") { event.preventDefault(); finish(null); } };
      window.addEventListener("keydown", onKey, true);
      host.addEventListener("mousedown", (event) => {
        start = [event.clientX, event.clientY];
        veil.style.display = "none";
        box.style.display = "block";
        hint.style.display = "none";
        event.preventDefault();
      });
      host.addEventListener("mousemove", (event) => {
        if (!start) return;
        const left = Math.min(start[0], event.clientX), top = Math.min(start[1], event.clientY);
        const width = Math.abs(event.clientX - start[0]), height = Math.abs(event.clientY - start[1]);
        Object.assign(box.style, { left: left + "px", top: top + "px", width: width + "px", height: height + "px" });
      });
      host.addEventListener("mouseup", (event) => {
        if (!start) return;
        const rect = { left: Math.min(start[0], event.clientX), top: Math.min(start[1], event.clientY), width: Math.abs(event.clientX - start[0]), height: Math.abs(event.clientY - start[1]) };
        finish(rect.width >= 4 && rect.height >= 4 ? rect : null);
      });
    });
  }

  async function area() {
    const rect = await pickArea();
    if (!rect) return { cancelled: true };
    // Two frames so the overlay is gone before the browser takes the picture.
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    await sleep(60);
    const image = await loadImage(await shot());
    const ratio = image.width / window.innerWidth;
    const canvas = document.createElement("canvas");
    canvas.width = Math.round(rect.width * ratio);
    canvas.height = Math.round(rect.height * ratio);
    canvas.getContext("2d").drawImage(image, Math.round(rect.left * ratio), Math.round(rect.top * ratio), canvas.width, canvas.height, 0, 0, canvas.width, canvas.height);
    return save(canvas, "screenshot");
  }

  api.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (!message || sender.id !== api.runtime.id) return false;
    if (message.type === "capture.ping") {
      sendResponse({ ready: true });
      return false;
    }
    if (message.type !== "capture.run") return false;
    (async () => {
      let hidden = 0;
      try {
        if (message.blur) hidden = blurPrivate();
        const result = message.mode === "full" ? await fullPage() : message.mode === "area" ? await area() : await visible();
        sendResponse({ ...result, blurred: hidden });
      } catch (error) {
        sendResponse({ error: String(error && error.message ? error.message : error) });
      } finally {
        unblur();
      }
    })();
    return true;
  });
})();
