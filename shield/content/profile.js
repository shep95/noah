// An OSINT-style report on any X or Twitter profile page. Reads the same
// UserByScreenName / UserByRestId graphql responses the browser was already
// going to receive and pulls out what the page hides behind clicks: rest_id
// (numeric id that hints at signup era), age of account and post rate,
// verified and protected flags, withheld countries, bio URLs expanded,
// emails, phones and cross-platform handles found in the bio, profile image
// URL for reverse-image search, and shortcuts to Wayback Machine snapshots
// and HIBP breach checks for any email in the bio.
//
// Nothing about the profile leaves this tab except the two optional
// lookups (Wayback and HIBP) which the user opens in a new tab themselves.
(() => {
  "use strict";
  if (window.__noahShieldProfile) return;
  const scheme = location.protocol;
  if (scheme !== "http:" && scheme !== "https:") return;
  const host = location.hostname.toLowerCase();
  if (!/(^|\.)(twitter\.com|x\.com)$/.test(host)) return;
  window.__noahShieldProfile = true;

  let profile = null;
  let panel = null;
  let collapsed = true;
  try {
    const stored = sessionStorage.getItem("noah-profile-collapsed");
    if (stored !== null) collapsed = stored === "true";
  } catch {}
  let config = { enabled: true };
  // The bridge and this script pair up with a nonce before any page script
  // runs (both start at document_start), so a page cannot forge the events
  // that drive this script or listen in on them.
  const nonce = crypto.randomUUID();
  const channel = (name) => name + ":" + nonce;
  const hello = () => { try { window.dispatchEvent(new CustomEvent("noah-profile-hello", { detail: nonce })); } catch {} };
  window.addEventListener("noah-profile-bridge-ready", hello);
  hello();

  window.addEventListener(channel("noah-profile-config"), (event) => {
    const next = event.detail || {};
    config = { ...config, ...next };
    if (!config.enabled && panel) { panel.remove(); panel = null; }
    else if (config.enabled && profile) render();
  });

  // ---- walk graphql payload for the user record ---------------------
  function walk(node, want) {
    if (!node || typeof node !== "object") return null;
    if (node[want]) return node[want];
    if (Array.isArray(node)) {
      for (const item of node) {
        const found = walk(item, want);
        if (found) return found;
      }
      return null;
    }
    for (const key of Object.keys(node)) {
      const found = walk(node[key], want);
      if (found) return found;
    }
    return null;
  }
  function extract(json) {
    const user = walk(json, "user") || walk(json, "user_results");
    if (!user) return null;
    const inner = user.result || user;
    const legacy = inner.legacy || {};
    const core = inner.core || {};
    const professional = inner.professional || null;
    const verification = inner.verification || {};
    const screen_name = core.screen_name || legacy.screen_name;
    if (!screen_name) return null;
    return {
      rest_id: inner.rest_id || null,
      screen_name,
      name: core.name || legacy.name || screen_name,
      created: legacy.created_at || null,
      description: legacy.description || "",
      location: legacy.location || "",
      url_display: legacy.url || null,
      url_expanded: (legacy.entities && legacy.entities.url && legacy.entities.url.urls && legacy.entities.url.urls[0]) ? legacy.entities.url.urls[0].expanded_url : null,
      description_urls: (legacy.entities && legacy.entities.description && legacy.entities.description.urls) || [],
      followers: legacy.followers_count,
      following: legacy.friends_count,
      posts: legacy.statuses_count,
      likes: legacy.favourites_count,
      lists_in: legacy.listed_count,
      media: legacy.media_count,
      verified: Boolean(legacy.verified || verification.verified || (inner.is_blue_verified)),
      blue: Boolean(inner.is_blue_verified),
      protected: Boolean(legacy.protected),
      profile_image: legacy.profile_image_url_https ? legacy.profile_image_url_https.replace("_normal.", "_400x400.") : null,
      profile_banner: legacy.profile_banner_url || null,
      pinned: (legacy.pinned_tweet_ids_str && legacy.pinned_tweet_ids_str[0]) || null,
      withheld_countries: legacy.withheld_in_countries || [],
      withheld_scope: legacy.withheld_scope || null,
      profession: professional && professional.category && professional.category[0] ? professional.category[0].name : null,
      possibly_sensitive: Boolean(legacy.possibly_sensitive),
      default_profile: Boolean(legacy.default_profile),
      default_profile_image: Boolean(legacy.default_profile_image),
    };
  }
  function onData(json) {
    const found = extract(json);
    if (!found) return;
    profile = found;
    if (config.enabled) render();
  }

  // ---- fetch and XHR hooks ------------------------------------------
  const realFetch = window.fetch;
  if (typeof realFetch === "function") {
    window.fetch = function (...args) {
      const promise = realFetch.apply(this, args);
      try {
        const url = typeof args[0] === "string" ? args[0] : (args[0] && args[0].url) || "";
        if (isProfileUrl(url)) {
          promise.then((response) => {
            if (!response || !response.clone) return;
            response.clone().json().then(onData).catch(() => {});
          }).catch(() => {});
        }
      } catch {}
      return promise;
    };
  }
  const XHR = window.XMLHttpRequest;
  if (XHR && XHR.prototype) {
    const realOpen = XHR.prototype.open;
    const realSend = XHR.prototype.send;
    XHR.prototype.open = function (method, url, ...rest) {
      try { this.__noahProfileUrl = String(url || ""); } catch {}
      return realOpen.call(this, method, url, ...rest);
    };
    XHR.prototype.send = function (...args) {
      if (this.__noahProfileUrl && isProfileUrl(this.__noahProfileUrl)) {
        this.addEventListener("load", () => {
          try { onData(JSON.parse(this.responseText)); } catch {}
        });
      }
      return realSend.apply(this, args);
    };
  }
  function isProfileUrl(url) {
    return /\/UserByScreenName|\/UserByRestId|\/UserResultByScreenName|\/UserResults/i.test(url);
  }

  // ---- helpers for the report --------------------------------------
  function daysBetween(iso) {
    const time = Date.parse(iso);
    if (Number.isNaN(time)) return null;
    return Math.max(0, Math.floor((Date.now() - time) / 86400000));
  }
  function niceDate(iso) {
    const time = Date.parse(iso);
    if (Number.isNaN(time)) return "unknown";
    const date = new Date(time);
    return date.toISOString().slice(0, 10);
  }
  function signupEra(restId) {
    // Twitter's numeric user ids are roughly chronological; the ranges below
    // give a coarse era hint (matches the account-age you can also compute
    // from created_at, so the two lines should agree).
    if (!restId) return null;
    const n = Number(restId);
    if (!Number.isFinite(n)) return null;
    if (n < 1000000) return "2006–2008 (very early)";
    if (n < 15000000) return "2008–2009";
    if (n < 100000000) return "2009–2010";
    if (n < 500000000) return "2010–2012";
    if (n < 1500000000) return "2012–2013";
    if (n < 3000000000) return "2013–2015";
    if (n < 800000000000) return "2015–2018 (numeric range wrapped)";
    return "2018 or later";
  }
  function findInText(text) {
    const found = { emails: [], phones: [], handles: {} };
    if (!text) return found;
    const seen = new Set();
    const push = (list, value) => { if (!seen.has(value)) { seen.add(value); list.push(value); } };
    for (const m of text.matchAll(/\b[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[A-Za-z]{2,}\b/g)) push(found.emails, m[0]);
    for (const m of text.matchAll(/\+?\d{1,3}[\s.-]?\(?\d{3}\)?[\s.-]?\d{3}[\s.-]?\d{4}\b/g)) push(found.phones, m[0]);
    // Common social handle mentions people put in bios.
    const platforms = [
      ["instagram", /(?:^|\s|@)(?:ig|insta(?:gram)?)[\s:@/.]+([a-zA-Z0-9._]{2,32})/gi],
      ["tiktok", /(?:^|\s|@)tiktok[\s:@/.]+@?([a-zA-Z0-9._]{2,32})/gi],
      ["youtube", /(?:^|\s|@)(?:youtube|yt)[\s:@/.]+@?([a-zA-Z0-9._-]{2,32})/gi],
      ["twitch", /(?:^|\s|@)twitch[\s:@/.]+([a-zA-Z0-9_]{2,32})/gi],
      ["discord", /(?:^|\s|@)(?:discord|dc)[\s:#@/.]+([a-zA-Z0-9._#]{2,32})/gi],
      ["telegram", /(?:^|\s|@)(?:telegram|tg)[\s:@/.]+([a-zA-Z0-9_]{2,32})/gi],
      ["github", /(?:^|\s|@)(?:github|gh)[\s:@/.]+([a-zA-Z0-9-]{2,39})/gi],
      ["onlyfans", /(?:^|\s|@)(?:only\s?fans|of)[\s:@/.]+([a-zA-Z0-9._-]{2,32})/gi],
      ["kick", /(?:^|\s|@)kick[\s:@/.]+([a-zA-Z0-9_-]{2,32})/gi],
    ];
    for (const [platform, re] of platforms) {
      const list = [];
      for (const m of text.matchAll(re)) if (!list.includes(m[1].toLowerCase())) list.push(m[1].toLowerCase());
      if (list.length) found.handles[platform] = list;
    }
    return found;
  }
  function urlPlatform(url) {
    try {
      const host = new URL(url).hostname.replace(/^www\./, "");
      if (/instagram\.com$/.test(host)) return "instagram";
      if (/tiktok\.com$/.test(host)) return "tiktok";
      if (/(youtube\.com|youtu\.be)$/.test(host)) return "youtube";
      if (/twitch\.tv$/.test(host)) return "twitch";
      if (/facebook\.com$/.test(host)) return "facebook";
      if (/linkedin\.com$/.test(host)) return "linkedin";
      if (/github\.com$/.test(host)) return "github";
      if (/onlyfans\.com$/.test(host)) return "onlyfans";
      if (/kick\.com$/.test(host)) return "kick";
      if (/twitter\.com$|x\.com$/.test(host)) return "twitter";
      return null;
    } catch { return null; }
  }
  function handleFromUrl(url) {
    try {
      const parts = new URL(url).pathname.split("/").filter(Boolean);
      return parts[0] ? parts[0].replace(/^@/, "") : null;
    } catch { return null; }
  }

  // ---- style --------------------------------------------------------
  const STYLE_ID = "noah-shield-profile-style";
  function ensureStyle() {
    if (document.getElementById(STYLE_ID)) return;
    const style = document.createElement("style");
    style.id = STYLE_ID;
    style.textContent = `
      #noah-profile-panel { position: fixed; right: 14px; bottom: 14px; z-index: 2147483000; width: 340px; max-height: 78vh; display: flex; flex-direction: column; background: rgba(10, 14, 18, .96); color: #d8ddd6; border: 1px solid rgba(150, 190, 220, .18); border-radius: 12px; box-shadow: 0 12px 30px rgba(0,0,0,.5); font: 12px/1.5 -apple-system, "Segoe UI", system-ui, sans-serif; overflow: hidden; }
      #noah-profile-panel header { display: flex; align-items: center; gap: 8px; padding: 10px 12px; border-bottom: 1px solid rgba(150, 190, 220, .12); cursor: move; user-select: none; }
      #noah-profile-panel header b { font-family: Georgia, serif; font-weight: 400; font-size: 14px; color: #f1f4ef; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      #noah-profile-panel .noah-profile-pill { font-size: 10px; padding: 2px 7px; border-radius: 999px; border: 1px solid rgba(150, 190, 220, .25); color: #96becc; letter-spacing: .05em; text-transform: uppercase; }
      #noah-profile-panel button.noah-profile-toggle { all: unset; cursor: pointer; padding: 2px 8px; border-radius: 6px; color: #9aa298; font-size: 12px; }
      #noah-profile-panel button.noah-profile-toggle:hover { color: #f1f4ef; background: rgba(150, 190, 220, .08); }
      #noah-profile-panel .noah-profile-body { overflow-y: auto; padding: 8px 12px 12px; scrollbar-width: thin; scrollbar-color: rgba(150,190,220,.22) transparent; }
      #noah-profile-panel[data-collapsed="true"] { max-height: 46px; }
      #noah-profile-panel[data-collapsed="true"] .noah-profile-body { display: none; }
      #noah-profile-panel h3 { margin: 12px 0 4px; font: 400 10px var(--body, sans-serif); color: #96becc; letter-spacing: .09em; text-transform: uppercase; }
      #noah-profile-panel h3:first-child { margin-top: 0; }
      #noah-profile-panel .noah-kv { display: grid; grid-template-columns: 90px 1fr; gap: 2px 8px; font-size: 12px; }
      #noah-profile-panel .noah-kv .k { color: #9aa298; }
      #noah-profile-panel .noah-kv .v { color: #f1f4ef; word-break: break-word; }
      #noah-profile-panel .noah-kv .v.flag { color: #dcc896; }
      #noah-profile-panel .noah-kv .v.warn { color: #e69696; }
      #noah-profile-panel .noah-kv .v.ok { color: #a9cf9f; }
      #noah-profile-panel a { color: #96becc; text-decoration: none; }
      #noah-profile-panel a:hover { text-decoration: underline; color: #f1f4ef; }
      #noah-profile-panel .noah-chip { display: inline-block; padding: 2px 8px; margin: 2px 4px 2px 0; font-size: 11px; border-radius: 999px; background: rgba(150, 190, 220, .1); border: 1px solid rgba(150, 190, 220, .18); color: #d8ddd6; }
      #noah-profile-panel .noah-line { color: #d8ddd6; font-size: 12px; padding: 4px 0; border-bottom: 1px dashed rgba(150, 190, 220, .1); }
      #noah-profile-panel .noah-line:last-child { border-bottom: none; }
      #noah-profile-panel footer { padding: 6px 12px; font-size: 10px; color: #6f766e; border-top: 1px solid rgba(150, 190, 220, .1); }
    `;
    (document.head || document.documentElement).append(style);
  }

  // ---- render -------------------------------------------------------
  function kv(container, key, value, cls) {
    const k = document.createElement("span"); k.className = "k"; k.textContent = key;
    const v = document.createElement("span"); v.className = "v" + (cls ? " " + cls : "");
    if (value instanceof Node) v.append(value); else v.textContent = value == null ? "—" : String(value);
    container.append(k, v);
  }
  function link(text, href) {
    const a = document.createElement("a"); a.href = href; a.textContent = text;
    a.target = "_blank"; a.rel = "noopener noreferrer";
    return a;
  }

  function render() {
    ensureStyle();
    if (!panel) {
      panel = document.createElement("div");
      panel.id = "noah-profile-panel";
      document.documentElement.append(panel);
    }
    panel.replaceChildren();
    panel.dataset.collapsed = collapsed ? "true" : "false";

    const head = document.createElement("header");
    const title = document.createElement("b");
    title.textContent = "@" + profile.screen_name;
    const pill = document.createElement("span");
    pill.className = "noah-profile-pill";
    pill.textContent = "intel";
    const toggle = document.createElement("button");
    toggle.type = "button";
    toggle.className = "noah-profile-toggle";
    toggle.textContent = collapsed ? "show" : "hide";
    toggle.addEventListener("click", (event) => {
      event.stopPropagation();
      collapsed = !collapsed;
      panel.dataset.collapsed = collapsed ? "true" : "false";
      toggle.textContent = collapsed ? "show" : "hide";
      try { sessionStorage.setItem("noah-profile-collapsed", collapsed ? "true" : "false"); } catch {}
      render();
    });
    let dragStart = null;
    head.addEventListener("mousedown", (event) => {
      if (event.target === toggle) return;
      const rect = panel.getBoundingClientRect();
      dragStart = { x: event.clientX, y: event.clientY, left: rect.left, top: rect.top };
      event.preventDefault();
    });
    document.addEventListener("mousemove", (event) => {
      if (!dragStart) return;
      const left = Math.max(0, Math.min(window.innerWidth - 80, dragStart.left + (event.clientX - dragStart.x)));
      const top = Math.max(0, Math.min(window.innerHeight - 40, dragStart.top + (event.clientY - dragStart.y)));
      panel.style.left = left + "px";
      panel.style.top = top + "px";
      panel.style.right = "auto";
      panel.style.bottom = "auto";
    });
    document.addEventListener("mouseup", () => { dragStart = null; });
    head.append(title, pill, toggle);
    panel.append(head);
    if (collapsed) return;

    const body = document.createElement("div");
    body.className = "noah-profile-body";

    // Identity
    body.append(sectionHeading("identity"));
    const identity = document.createElement("div"); identity.className = "noah-kv";
    kv(identity, "display", profile.name);
    kv(identity, "handle", link("@" + profile.screen_name, "https://x.com/" + profile.screen_name));
    kv(identity, "rest id", profile.rest_id || "—");
    if (profile.rest_id) kv(identity, "signup era", signupEra(profile.rest_id));
    if (profile.verified) kv(identity, "verified", profile.blue ? "blue check (paid)" : "verified", "flag");
    if (profile.protected) kv(identity, "protected", "yes (posts hidden)", "warn");
    if (profile.default_profile_image) kv(identity, "avatar", "default egg — never uploaded a picture", "warn");
    if (profile.profession) kv(identity, "professional", profile.profession, "flag");
    if (profile.withheld_countries && profile.withheld_countries.length) kv(identity, "withheld in", profile.withheld_countries.join(", "), "warn");
    body.append(identity);

    // Timeline
    body.append(sectionHeading("timeline"));
    const timeline = document.createElement("div"); timeline.className = "noah-kv";
    const age = daysBetween(profile.created);
    kv(timeline, "created", (age != null ? age + " days ago · " : "") + niceDate(profile.created));
    if (age != null && age < 30) kv(timeline, "note", "brand new account (under a month)", "warn");
    if (age != null && age > 0 && profile.posts != null) {
      const perDay = (profile.posts / age).toFixed(2);
      kv(timeline, "posts/day", perDay + " (" + (profile.posts || 0).toLocaleString() + " total)", Number(perDay) > 50 ? "warn" : undefined);
    } else {
      kv(timeline, "posts", (profile.posts || 0).toLocaleString());
    }
    body.append(timeline);

    // Reach
    body.append(sectionHeading("reach"));
    const reach = document.createElement("div"); reach.className = "noah-kv";
    kv(reach, "followers", (profile.followers || 0).toLocaleString());
    kv(reach, "following", (profile.following || 0).toLocaleString());
    if (profile.followers > 0) {
      const ratio = (profile.following / Math.max(1, profile.followers)).toFixed(2);
      const isSpammy = profile.following > 500 && profile.following / Math.max(1, profile.followers) > 10;
      kv(reach, "follow ratio", ratio + " (following ÷ followers)", isSpammy ? "warn" : undefined);
    }
    kv(reach, "on lists", (profile.lists_in || 0).toLocaleString());
    if (profile.likes != null) kv(reach, "likes given", profile.likes.toLocaleString());
    if (profile.media != null) kv(reach, "media posts", profile.media.toLocaleString());
    body.append(reach);

    // Bio + leaks
    body.append(sectionHeading("bio and leaks"));
    if (profile.description) {
      const bio = document.createElement("div"); bio.className = "noah-line"; bio.textContent = profile.description;
      body.append(bio);
    }
    if (profile.location) {
      const loc = document.createElement("div"); loc.className = "noah-line";
      loc.append(document.createTextNode("location: "), link(profile.location, "https://www.google.com/maps/search/" + encodeURIComponent(profile.location)));
      body.append(loc);
    }
    if (profile.url_expanded) {
      const site = document.createElement("div"); site.className = "noah-line";
      site.append(document.createTextNode("website: "), link(profile.url_expanded, profile.url_expanded));
      body.append(site);
    }
    const bioLeaks = findInText(profile.description);
    for (const email of bioLeaks.emails) {
      const el = document.createElement("div"); el.className = "noah-line";
      el.append(document.createTextNode("email in bio: "), link(email, "mailto:" + email), document.createTextNode(" · "), link("check breaches (HIBP)", "https://haveibeenpwned.com/account/" + encodeURIComponent(email)));
      body.append(el);
    }
    for (const phone of bioLeaks.phones) {
      const el = document.createElement("div"); el.className = "noah-line";
      el.textContent = "phone in bio: " + phone;
      body.append(el);
    }
    // Cross-platform handles found in the bio text.
    if (Object.keys(bioLeaks.handles).length) {
      const wrap = document.createElement("div"); wrap.className = "noah-line";
      wrap.append(document.createTextNode("other platforms mentioned: "));
      for (const [platform, handles] of Object.entries(bioLeaks.handles)) {
        for (const handle of handles) {
          const chip = document.createElement("span"); chip.className = "noah-chip";
          chip.textContent = platform + " · " + handle;
          wrap.append(chip);
        }
      }
      body.append(wrap);
    }
    // Expanded bio URLs (X hides them behind t.co shorteners).
    for (const entry of profile.description_urls || []) {
      const url = entry.expanded_url || entry.url;
      if (!url) continue;
      const platform = urlPlatform(url);
      const el = document.createElement("div"); el.className = "noah-line";
      el.append(document.createTextNode("link: "), link(url, url));
      if (platform) {
        const chip = document.createElement("span"); chip.className = "noah-chip"; chip.textContent = platform;
        el.append(document.createTextNode(" "), chip);
        const handle = handleFromUrl(url);
        if (handle) el.append(document.createTextNode(" · @" + handle));
      }
      body.append(el);
    }

    // Assets that can be reverse-searched
    if (profile.profile_image && !profile.default_profile_image) {
      body.append(sectionHeading("assets"));
      const img = document.createElement("div"); img.className = "noah-line";
      img.append(
        document.createTextNode("profile picture: "),
        link("open", profile.profile_image),
        document.createTextNode(" · "),
        link("reverse-search on Google", "https://lens.google.com/uploadbyurl?url=" + encodeURIComponent(profile.profile_image)),
        document.createTextNode(" · "),
        link("Yandex", "https://yandex.com/images/search?rpt=imageview&url=" + encodeURIComponent(profile.profile_image)),
      );
      body.append(img);
      const note = document.createElement("div"); note.className = "noah-line";
      note.style.color = "#9aa298";
      note.textContent = "X strips most EXIF from uploaded avatars, but a reverse-image search can still turn up the picture on other sites, dating apps and old profiles.";
      body.append(note);
    }

    // Time-travel and cross-references
    body.append(sectionHeading("time travel"));
    const wm = document.createElement("div"); wm.className = "noah-line";
    wm.append(
      document.createTextNode("old versions of this profile: "),
      link("Wayback Machine", "https://web.archive.org/web/*/x.com/" + profile.screen_name),
      document.createTextNode(" · "),
      link("Nitter (mirror)", "https://nitter.net/" + profile.screen_name),
    );
    body.append(wm);
    if (profile.rest_id) {
      const rid = document.createElement("div"); rid.className = "noah-line";
      rid.append(
        document.createTextNode("look up rest_id " + profile.rest_id + ": "),
        link("Wayback for id-based URL", "https://web.archive.org/web/*/twitter.com/intent/user?user_id=" + profile.rest_id),
      );
      body.append(rid);
    }
    if (profile.pinned) {
      const pin = document.createElement("div"); pin.className = "noah-line";
      pin.append(
        document.createTextNode("pinned post: "),
        link("open", "https://x.com/" + profile.screen_name + "/status/" + profile.pinned),
      );
      body.append(pin);
    }

    panel.append(body);

    const foot = document.createElement("footer");
    foot.textContent = "Built from what your browser was already going to receive. External lookups (HIBP, Wayback, image search) only open when you click them.";
    panel.append(foot);
  }
  function sectionHeading(text) {
    const h = document.createElement("h3");
    h.textContent = text;
    return h;
  }
})();
