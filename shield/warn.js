"use strict";

const params = new URLSearchParams(location.search);
const kind = params.get("kind");
const title = document.getElementById("title");
const lines = document.getElementById("lines");
const actions = document.getElementById("actions");
const extra = document.getElementById("extra");

function paragraph(text) {
  const element = document.createElement("p");
  element.textContent = text;
  lines.append(element);
  return element;
}
function button(label, primary, onClick) {
  const element = document.createElement("button");
  element.textContent = label;
  if (primary) element.className = "primary";
  element.addEventListener("click", onClick);
  actions.append(element);
}
async function closeThisTab() {
  const tab = await Shield.api.tabs.getCurrent();
  if (tab) await Shield.api.tabs.remove(tab.id);
}

async function lookalike() {
  const url = params.get("url") || "";
  const host = Shield.hostOf(url);
  const brand = params.get("brand") || "";
  const real = params.get("real") || "";
  const reason = params.get("reason") || "";
  title.textContent = `This is not ${brand}.`;
  title.className = "warn";
  paragraph(`${host} looks like ${brand} (${reason}), but ${brand} lives at ${real}. Pages like this exist to take a password or a card number.`);
  const code = document.createElement("code");
  code.textContent = url;
  const where = paragraph("You were going to: ");
  where.append(code);
  button("Go to the real " + real, true, () => { location.replace("https://" + real); });
  button("Back to safety", false, () => closeThisTab());
  button("I know this site, continue", false, async () => {
    await Shield.send({ type: "lookalike.allow", site: Shield.siteOf(host) });
    location.replace(url);
  });
}

async function download() {
  const id = parseInt(params.get("id"), 10);
  const info = await Shield.send({ type: "download.info", id });
  title.textContent = "Keep this download?";
  title.className = "warn";
  if (!info || info.error) {
    paragraph("The download is gone already.");
    button("Close", true, () => closeThisTab());
    return;
  }
  const code = document.createElement("code");
  code.textContent = info.name;
  paragraph("File: ").append(code);
  paragraph("From: " + (info.source || info.url));
  const list = document.createElement("ul");
  for (const reason of info.reasons) {
    const item = document.createElement("li");
    item.textContent = reason;
    list.append(item);
  }
  lines.append(list);
  if (info.virusTotal) {
    if (info.virusTotal.stats) {
      const stats = info.virusTotal.stats;
      paragraph(`VirusTotal: ${stats.malicious || 0} engines call the address malicious, ${stats.suspicious || 0} suspicious, ${stats.harmless || 0} harmless.`);
    } else if (info.virusTotal.pending) {
      paragraph("VirusTotal is still analysing the address.");
    } else if (info.virusTotal.error) {
      paragraph("VirusTotal check failed: " + info.virusTotal.error);
    }
  } else {
    extra.textContent = "Add a VirusTotal key in the shield's settings and downloads like this are checked against its engines too.";
  }
  button("Delete it", true, async () => {
    await Shield.send({ type: "download.decide", id, keep: false });
    closeThisTab();
  });
  button("Keep it, I asked for this file", false, async () => {
    await Shield.send({ type: "download.decide", id, keep: true });
    closeThisTab();
  });
}

if (kind === "lookalike") lookalike();
else if (kind === "download") download();
else {
  title.textContent = "Nothing to show.";
  button("Close", true, () => closeThisTab());
}
