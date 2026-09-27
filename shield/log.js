// The log page: what the shield did, filtered by kind, site or a word.
"use strict";

const byId = (id) => document.getElementById(id);
const params = new URLSearchParams(location.search);

const KIND_WORDS = {
  notice: ["notices", ""],
  vpn: ["vpn", ""],
  capture: ["screenshots and recordings", ""],
  light: ["light", ""],
  search: ["search results cleaned", "good"],
  phishing: ["look-alike sites", "bad"],
  passwordHttp: ["password without https", "warn"],
  reuse: ["reused passwords", "warn"],
  breached: ["breached passwords", "bad"],
  hiddenFields: ["hidden form fields", ""],
  frames: ["hidden frames", ""],
  popups: ["scam pop-ups", "bad"],
  leaks: ["form leaks", "bad"],
  keylog: ["keystroke loggers", "bad"],
  wallets: ["wallet reaches", "warn"],
  clipboard: ["clipboard reads", "warn"],
  fees: ["hidden fees", "warn"],
  darkPatterns: ["dark patterns", "warn"],
  traps: ["subscription traps", "warn"],
  reviewsChecked: ["reviews checked", ""],
  banners: ["cookie banners", ""],
  overlays: ["overlays", ""],
  autoplay: ["autoplay", ""],
  timers: ["fake countdowns", ""],
  pastes: ["pastes", ""],
  uploads: ["uploads", ""],
  cookies: ["cookies", ""],
  rules: ["ad and tracker rules", ""],
  other: ["other", ""],
};

function two(number) {
  return String(number).padStart(2, "0");
}

function when(at) {
  const date = new Date(at);
  const today = new Date();
  const sameDay = date.toDateString() === today.toDateString();
  const time = `${two(date.getHours())}:${two(date.getMinutes())}:${two(date.getSeconds())}`;
  return sameDay ? time : `${date.getFullYear()}-${two(date.getMonth() + 1)}-${two(date.getDate())} ${time}`;
}

let entries = [];

async function load() {
  const result = await Shield.send({ type: "log.list", kind: byId("kind").value, site: byId("site").value.trim().toLowerCase(), limit: 3000 });
  entries = (result && result.entries) || [];
  render();
}

function render() {
  const word = byId("find").value.trim().toLowerCase();
  const shown = word ? entries.filter((entry) => `${entry.text} ${entry.detail} ${entry.site}`.toLowerCase().includes(word)) : entries;
  const rows = byId("rows");
  rows.replaceChildren();
  for (const entry of shown) {
    const row = document.createElement("tr");
    const time = document.createElement("td");
    time.textContent = when(entry.at);
    const kind = document.createElement("td");
    kind.className = "kind";
    const pill = document.createElement("span");
    const [label, tone] = KIND_WORDS[entry.kind] || [entry.kind, ""];
    pill.textContent = label;
    if (tone) pill.className = tone;
    kind.append(pill);
    const site = document.createElement("td");
    site.textContent = entry.site || "";
    const text = document.createElement("td");
    text.textContent = entry.text;
    if (entry.detail) {
      const detail = document.createElement("span");
      detail.className = "detail";
      detail.textContent = entry.detail;
      text.append(detail);
    }
    row.append(time, kind, site, text);
    rows.append(row);
  }
  byId("empty").hidden = shown.length > 0;
  byId("count").textContent = shown.length ? `${shown.length} entr${shown.length === 1 ? "y" : "ies"}` : "";
}

(function fillKinds() {
  const select = byId("kind");
  for (const [value, [label]] of Object.entries(KIND_WORDS)) {
    const option = document.createElement("option");
    option.value = value;
    option.textContent = label;
    select.append(option);
  }
  if (params.get("kind")) select.value = params.get("kind");
  if (params.get("site")) byId("site").value = params.get("site");
})();

byId("kind").addEventListener("change", load);
byId("site").addEventListener("input", () => { clearTimeout(byId("site").timer); byId("site").timer = setTimeout(load, 300); });
byId("find").addEventListener("input", render);
byId("refresh").addEventListener("click", load);
byId("clear").addEventListener("click", async () => {
  if (!confirm("Clear the whole log on this device?")) return;
  await Shield.send({ type: "log.clear" });
  await load();
});
byId("export").addEventListener("click", () => {
  const lines = entries.map((entry) => `${new Date(entry.at).toISOString()}\t${entry.kind}\t${entry.site}\t${entry.text}${entry.detail ? "\t" + entry.detail : ""}`);
  const blob = new Blob([lines.join("\n") + "\n"], { type: "text/plain" });
  const anchor = document.createElement("a");
  anchor.href = URL.createObjectURL(blob);
  anchor.download = `noah-shield-log ${new Date().toISOString().slice(0, 10)}.txt`;
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(anchor.href), 5000);
});

function tick() {
  const now = new Date();
  byId("clock").textContent = `${two(now.getHours())}:${two(now.getMinutes())}`;
}
tick();
setInterval(tick, 20000);
Shield.api.storage.onChanged.addListener((changes, area) => {
  if (area === "local" && changes.log) load();
});
load();
