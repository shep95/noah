// Signed data feeds from noah.asherin.com: the vetted tunnel servers, the
// coupon list, and the shield's own latest version. Each feed is a JSON file
// with an ed25519 signature beside it (<name>.sig), made by
// script/build-shield with noah's release key. A feed whose signature does
// not verify, or that is past its own expiry, is refused and the last good
// copy stays in use, so the website alone cannot feed the shield anything.
"use strict";
(() => {

const Shield = (globalThis.Shield = globalThis.Shield || {});

Shield.FEEDS = ["servers.json", "coupons.json", "latest.json"];
Shield.FEED_MAX_AGE_DAYS = 45;

let verifyKeyPromise = null;
function verifyKey() {
  if (!verifyKeyPromise) {
    verifyKeyPromise = crypto.subtle
      .importKey("raw", Shield.fromHex(Shield.PUBLIC_KEY_HEX), { name: "Ed25519" }, false, ["verify"])
      .catch((error) => {
        verifyKeyPromise = null;
        throw new Error("this browser cannot verify ed25519 signatures: " + error.message);
      });
  }
  return verifyKeyPromise;
}

// The signed text is "noah-shield\n<name>\n" followed by the file's bytes,
// so a signature for one feed can never be replayed as another.
async function verifyFeed(name, bytes, signatureText) {
  const key = await verifyKey();
  const prefix = new TextEncoder().encode("noah-shield\n" + name + "\n");
  const message = new Uint8Array(prefix.length + bytes.length);
  message.set(prefix, 0);
  message.set(bytes, prefix.length);
  return crypto.subtle.verify({ name: "Ed25519" }, key, Shield.fromBase64(signatureText), message);
}

async function fetchText(url) {
  const response = await fetch(url, { cache: "no-store", credentials: "omit", redirect: "error" });
  if (!response.ok) throw new Error(url + " answered " + response.status);
  return response;
}

Shield.fetchSignedFeed = async function fetchSignedFeed(name) {
  const [body, signature] = await Promise.all([
    fetchText(Shield.FEED_BASE + name).then((response) => response.arrayBuffer()),
    fetchText(Shield.FEED_BASE + name + ".sig").then((response) => response.text()),
  ]);
  const bytes = new Uint8Array(body);
  const verified = await verifyFeed(name, bytes, signature);
  if (!verified) throw new Error(name + ": the signature does not match noah's release key");
  const data = JSON.parse(new TextDecoder().decode(bytes));
  if (typeof data !== "object" || data === null) throw new Error(name + ": not an object");
  const generated = Date.parse(data.generated || "");
  if (Number.isNaN(generated)) throw new Error(name + ": no generated date");
  if (data.expires && Date.parse(data.expires) < Date.now()) {
    throw new Error(name + ": this copy expired on " + data.expires);
  }
  if (Date.now() - generated > Shield.FEED_MAX_AGE_DAYS * 86400000) {
    throw new Error(name + ": older than " + Shield.FEED_MAX_AGE_DAYS + " days, refusing a stale feed");
  }
  return data;
};

Shield.loadFeeds = async function loadFeeds() {
  const stored = await Shield.api.storage.local.get("feeds");
  return stored.feeds || {};
};

// Refreshes every feed, keeping whatever was cached when one fails, and
// records the failure so the options page can show it.
Shield.refreshFeeds = async function refreshFeeds() {
  const feeds = await Shield.loadFeeds();
  for (const name of Shield.FEEDS) {
    const entry = feeds[name] || {};
    try {
      const fresh = await Shield.fetchSignedFeed(name);
      // An older signed copy is still refused: a replayed feed cannot roll the shield back.
      if (entry.data && Date.parse(fresh.generated) < Date.parse(entry.data.generated)) throw new Error(name + ": older than the copy already held");
      entry.data = fresh;
      entry.fetched = new Date().toISOString();
      entry.error = null;
    } catch (error) {
      entry.error = String(error && error.message ? error.message : error);
      entry.failedAt = new Date().toISOString();
    }
    feeds[name] = entry;
  }
  await Shield.api.storage.local.set({ feeds });
  return feeds;
};

Shield.feedData = async function feedData(name) {
  const feeds = await Shield.loadFeeds();
  return feeds[name] && feeds[name].data ? feeds[name].data : null;
};

// The newest published shield, compared with the one running. Firefox and
// store builds update themselves; a copy loaded by hand only learns here.
Shield.updateStatus = async function updateStatus() {
  const latest = await Shield.feedData("latest.json");
  const running = Shield.api.runtime.getManifest().version;
  if (!latest || !latest.version) return { running, latest: null, newer: false };
  return {
    running,
    latest: latest.version,
    newer: Shield.compareVersions(latest.version, running) > 0,
    page: typeof latest.page === "string" && latest.page.startsWith(Shield.SITE + "/") ? latest.page : Shield.SITE + "/shield",
    notes: latest.notes || "",
  };
};
})();
