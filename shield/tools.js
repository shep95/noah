"use strict";

const byId = (id) => document.getElementById(id);
const params = new URLSearchParams(location.search);

function cell(content, className = "") {
  const td = document.createElement("td");
  if (content instanceof Node) td.append(content);
  else td.textContent = String(content);
  if (className) td.className = className;
  return td;
}
function button(label, onClick) {
  const element = document.createElement("button");
  element.className = "ghost";
  element.textContent = label;
  element.addEventListener("click", onClick);
  return element;
}
function link(url, label) {
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.target = "_blank";
  anchor.rel = "noopener noreferrer";
  anchor.textContent = label || url;
  return anchor;
}
async function activeTab() {
  const tabs = await Shield.api.tabs.query({ active: true, lastFocusedWindow: true });
  const tab = tabs.find((candidate) => !candidate.url.startsWith(Shield.api.runtime.getURL(""))) || tabs[0];
  if (tab && !tab.url.startsWith(Shield.api.runtime.getURL(""))) return tab;
  const all = await Shield.api.tabs.query({});
  return all.filter((candidate) => /^https?:/.test(candidate.url)).sort((left, right) => (right.lastAccessed || 0) - (left.lastAccessed || 0))[0] || null;
}

// ---- week --------------------------------------------------------------------------
Shield.send({ type: "report.weekly" }).then((report) => {
  byId("week").textContent = report && report.text ? report.text : "no report";
});

// ---- links ------------------------------------------------------------------------------
async function unshorten(url) {
  const out = byId("unshorten-out");
  out.textContent = "following…";
  const result = await Shield.send({ type: "tools.unshorten", url });
  out.replaceChildren();
  if (result.error) {
    out.textContent = result.error;
    return;
  }
  const lines = [
    `Final address: ${result.final}`,
    result.redirected ? `Redirected ${result.hops.length - 1 || 1} time(s).` : "No redirect: the address is what it says.",
    result.shortener ? "The start was a known link shortener." : "",
    result.lookalike ? `Careful: the destination looks like ${result.lookalike.brand} (${result.lookalike.reason}) but is ${Shield.hostOf(result.final)}.` : "",
  ].filter(Boolean);
  for (const text of lines) {
    const paragraph = document.createElement("p");
    paragraph.textContent = text;
    if (/Careful/.test(text)) paragraph.className = "bad";
    out.append(paragraph);
  }
  out.append(link(result.final, "open it, knowing where it goes"));
}
byId("unshorten-go").addEventListener("click", () => unshorten(byId("unshorten-input").value.trim()));
if (params.get("unshorten")) {
  byId("unshorten-input").value = params.get("unshorten");
  unshorten(params.get("unshorten"));
}
byId("hops-go").addEventListener("click", async () => {
  const tab = await activeTab();
  const list = byId("hops-out");
  list.replaceChildren();
  if (!tab) return;
  const { hops } = await Shield.send({ type: "tools.hops", tabId: tab.id });
  if (!hops || !hops.length) {
    const item = document.createElement("li");
    item.textContent = "No hops recorded for that tab yet (navigate to something, then look again).";
    list.append(item);
    return;
  }
  for (const hop of hops) {
    const item = document.createElement("li");
    item.textContent = hop;
    list.append(item);
  }
});

// ---- QR ----------------------------------------------------------------------------------
function decodeImage(image) {
  const canvas = byId("qr-canvas");
  const scale = Math.min(1, 1200 / Math.max(image.width, image.height));
  canvas.width = Math.round(image.width * scale);
  canvas.height = Math.round(image.height * scale);
  const context = canvas.getContext("2d");
  context.drawImage(image, 0, 0, canvas.width, canvas.height);
  const data = context.getImageData(0, 0, canvas.width, canvas.height);
  return globalThis.jsQR(data.data, data.width, data.height, { inversionAttempts: "attemptBoth" });
}
async function showQr(code) {
  const out = byId("qr-out");
  out.replaceChildren();
  if (!code) {
    out.textContent = "No QR code found in that picture.";
    return;
  }
  const text = code.data;
  const first = document.createElement("p");
  first.textContent = "It says: " + text;
  out.append(first);
  if (/^https?:\/\//i.test(text)) {
    const result = await Shield.send({ type: "tools.unshorten", url: text });
    const second = document.createElement("p");
    if (result.error) second.textContent = "Could not follow it: " + result.error;
    else second.textContent = `It leads to ${result.final}${result.lookalike ? ". Careful: that looks like " + result.lookalike.brand + " but is not." : "."}`;
    if (result.lookalike) second.className = "bad";
    out.append(second, link(result.final || text, "open it"));
  } else if (/^(WIFI:|BEGIN:VCARD|mailto:|tel:|sms:|bitcoin:|ethereum:)/i.test(text)) {
    const second = document.createElement("p");
    second.textContent = "Not a web link: a " + (text.split(":")[0].replace("BEGIN", "contact card")) + ". Treat payment and Wi-Fi codes from strangers with suspicion.";
    out.append(second);
  }
}
byId("qr-file").addEventListener("change", (event) => {
  const file = event.target.files && event.target.files[0];
  if (!file) return;
  const image = new Image();
  image.onload = () => showQr(decodeImage(image));
  image.src = URL.createObjectURL(file);
});
byId("qr-shot").addEventListener("click", async () => {
  const tab = await activeTab();
  if (!tab) return;
  try {
    const dataUrl = await Shield.api.tabs.captureVisibleTab(tab.windowId, { format: "png" });
    const image = new Image();
    image.onload = () => showQr(decodeImage(image));
    image.src = dataUrl;
  } catch (error) {
    byId("qr-out").textContent = "Could not capture that tab: " + String(error.message || error);
  }
});

// ---- policy ------------------------------------------------------------------------------------
byId("policy-go").addEventListener("click", async () => {
  const tab = await activeTab();
  const out = byId("policy-out");
  out.replaceChildren();
  if (!tab) return;
  const result = await Shield.send({ type: "tools.policy", tabId: tab.id });
  if (result.error) {
    out.textContent = result.error;
    return;
  }
  byId("policy-meta").textContent = `${tab.title ? tab.title.slice(0, 60) + ": " : ""}grade ${result.grade}, about ${result.minutes} minutes of reading, ${result.wordCount} words.`;
  if (!result.lines.length) {
    out.textContent = "None of the usual red flags. Either a short, plain policy, or one that avoids the usual words.";
    return;
  }
  for (const line of result.lines) {
    const paragraph = document.createElement("p");
    paragraph.className = line.good ? "muted" : "";
    const strong = document.createElement("b");
    strong.textContent = line.line + " ";
    const quote = document.createElement("span");
    quote.className = "muted";
    quote.textContent = "“…" + line.quote + "…”";
    paragraph.append(strong, quote);
    out.append(paragraph);
  }
});

// ---- letters and brokers --------------------------------------------------------------------------
byId("letter-go").addEventListener("click", async () => {
  const result = await Shield.send({ type: "tools.letter", name: byId("letter-name").value, email: byId("letter-email").value, company: byId("letter-company").value, law: byId("letter-law").value });
  byId("letter-out").value = result.text || "";
});
byId("letter-copy").addEventListener("click", async () => {
  try { await navigator.clipboard.writeText(byId("letter-out").value); byId("letter-copy").textContent = "copied"; } catch { byId("letter-out").select(); }
});
Shield.send({ type: "tools.letter" }).then((result) => {
  const list = byId("brokers");
  for (const broker of result.brokers || []) {
    const item = document.createElement("li");
    item.append(link(broker.optOut, broker.name), document.createTextNode(" · " + broker.optOut.replace(/^https?:\/\//, "").slice(0, 60)));
    list.append(item);
  }
});

// ---- purchases -----------------------------------------------------------------------------------------
async function renderPurchases() {
  const data = await Shield.send({ type: "shop.watchlist" });
  const watchlist = byId("watchlist");
  watchlist.replaceChildren();
  const emptyRow = (table, message) => { const row = document.createElement("tr"); row.append(cell(message, "muted")); table.append(row); };
  if (!data.watchlist.length) emptyRow(watchlist, "Nothing watched yet. On a product page, press “Watch for a drop” on the shield's card.");
  for (const entry of data.watchlist) {
    const row = document.createElement("tr");
    row.append(cell(link(entry.url, entry.title || entry.url)), cell(entry.price ? entry.price + " " + entry.currency : "?"), cell(entry.dropped ? `dropped ${entry.dropped.from} → ${entry.dropped.to}` : entry.lastChecked ? "checked " + new Date(entry.lastChecked).toLocaleDateString() : "not checked yet"));
    const actions = cell("", "actions");
    actions.append(button("stop", async () => { await Shield.send({ type: "shop.unwatch", url: entry.url }); renderPurchases(); }));
    row.append(actions);
    watchlist.append(row);
  }
  const receipts = byId("receipts");
  receipts.replaceChildren();
  if (!data.receipts.length) emptyRow(receipts, "No receipts yet. Order confirmation pages are saved as you see them.");
  for (const receipt of data.receipts) {
    const row = document.createElement("tr");
    row.append(cell(new Date(receipt.at).toLocaleDateString()), cell(receipt.site), cell(receipt.orderNumber || "—"), cell(receipt.total ? receipt.total + " " + receipt.currency : "?"));
    const actions = cell("", "actions");
    const months = document.createElement("input");
    months.type = "number";
    months.min = "1";
    months.max = "120";
    months.value = receipt.warrantyMonths || "";
    months.placeholder = "warranty months";
    months.style.width = "120px";
    actions.append(months, document.createTextNode(" "), button("set", async () => { await Shield.send({ type: "shop.warranty", url: receipt.url, months: Number(months.value) }); renderPurchases(); }), document.createTextNode(" "), link(receipt.url, "open"));
    row.append(actions);
    receipts.append(row);
  }
  const spending = byId("spending");
  spending.replaceChildren();
  for (const [month, stores] of Object.entries(data.spending || {}).sort().reverse()) {
    for (const [store, total] of Object.entries(stores)) {
      const row = document.createElement("tr");
      row.append(cell(month), cell(store), cell(total.toFixed(2)));
      spending.append(row);
    }
  }
  const reminders = byId("reminders");
  reminders.replaceChildren();
  if (!data.reminders.length) emptyRow(reminders, "No reminders pending.");
  for (const reminder of data.reminders) {
    const row = document.createElement("tr");
    row.append(cell(new Date(reminder.at).toLocaleString()), cell(reminder.kind), cell(reminder.label), cell(link(reminder.url, "open")));
    reminders.append(row);
  }
}
renderPurchases();

// ---- vault -----------------------------------------------------------------------------------------------
byId("vault-open").addEventListener("click", async () => {
  const result = await Shield.send({ type: "vault.open", passphrase: byId("vault-pass").value });
  if (result.error) byId("vault-status").textContent = result.error;
  else {
    byId("vault-text").value = (result.notes || []).join("\n");
    byId("vault-status").textContent = `${(result.notes || []).length} notes`;
  }
});
byId("vault-save").addEventListener("click", async () => {
  const notes = byId("vault-text").value.split("\n").filter(Boolean);
  const result = await Shield.send({ type: "vault.save", passphrase: byId("vault-pass").value, notes });
  byId("vault-status").textContent = result.error || `saved ${notes.length} notes, encrypted`;
});
