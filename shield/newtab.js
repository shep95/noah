// The new tab page: the picture the person chose, the time, a search box,
// and one line of what the shield did today. Nothing is fetched; the search
// goes to the browser's own engine.
"use strict";
const byId = (id) => document.getElementById(id);

function tick() {
  const now = new Date();
  const two = (n) => String(n).padStart(2, "0");
  byId("clock").textContent = `${now.getHours()}:${two(now.getMinutes())}`;
  byId("day").textContent = now.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
}
tick();
setInterval(tick, 10000);

(async () => {
  const [look, stored] = await Promise.all([Shield.loadLook(), Shield.api.storage.local.get("plainNewTab")]);
  Shield.applyLook(look);
  byId("plain").checked = Boolean(stored.plainNewTab);
  document.body.classList.toggle("plain", Boolean(stored.plainNewTab));
  const url = Shield.lookImageUrl(look);
  const picture = byId("picture");
  if (url) {
    const image = new Image();
    image.onload = () => { picture.style.backgroundImage = `url("${url}")`; picture.classList.add("in"); };
    image.src = url;
  }
  const state = await Shield.send({ type: "stats.today" });
  if (state && state.today) {
    const today = state.today;
    const parts = [];
    if (today.trackers) parts.push(`${today.trackers} ads and trackers stopped`);
    if (today.cookies || today.burned) parts.push(`${(today.cookies || 0) + (today.burned || 0)} cookies burned`);
    const scams = (today.phishing || 0) + (today.leaks || 0) + (today.keylog || 0) + (today.wallets || 0);
    if (scams) parts.push(`${scams} scams and leaks caught`);
    byId("numbers").textContent = parts.length ? "today: " + parts.join(" · ") : "";
  }
})();

byId("search").addEventListener("submit", (event) => {
  event.preventDefault();
  const text = byId("query").value.trim();
  if (!text) return;
  const looksLikeAddress = /^(https?:\/\/|[a-z0-9-]+(\.[a-z0-9-]+)+(\/|$))/i.test(text) && !/\s/.test(text);
  if (looksLikeAddress) {
    location.href = /^https?:\/\//i.test(text) ? text : "https://" + text;
    return;
  }
  if (Shield.api.search && Shield.api.search.query) {
    Shield.api.search.query({ text, disposition: "CURRENT_TAB" });
  } else {
    location.href = "https://duckduckgo.com/?q=" + encodeURIComponent(text);
  }
});
byId("change-look").addEventListener("click", (event) => { event.preventDefault(); Shield.api.runtime.openOptionsPage(); });
byId("shield-settings").addEventListener("click", (event) => { event.preventDefault(); Shield.api.runtime.openOptionsPage(); });
byId("plain").addEventListener("change", async (event) => {
  await Shield.api.storage.local.set({ plainNewTab: event.target.checked });
  document.body.classList.toggle("plain", event.target.checked);
});
