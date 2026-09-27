// Every listener in an X or Twitter Space, in one panel: their @, their
// display name, and a click that opens their profile. Listeners in a Space
// are not anonymous; their handles ride in the same audio-space responses
// the browser already receives, but the desktop UI hides them behind clicks
// and the mobile app hides them harder. Surfacing that list makes it easy
// for a parent watching over a young person's account to see who is in a
// room, before deciding to close it.
//
// Runs in the page's own world so it can wrap fetch and XMLHttpRequest.
// Reads only responses the browser was already going to receive, and
// touches nothing that leaves the page.
(() => {
  "use strict";
  if (window.__noahShieldSpaces) return;
  const host = location.hostname.toLowerCase();
  if (!/(^|\.)(twitter\.com|x\.com)$/.test(host)) return;
  window.__noahShieldSpaces = true;

  let space = null;
  let panel = null;
  let collapsed = false;
  let config = { enabled: true, newAccountDays: 30, flagLowFollowers: true, lowFollowersUnder: 20 };
  window.addEventListener("noah-spaces-config", (event) => {
    const next = event.detail || {};
    config = { ...config, ...next };
    if (!config.enabled && panel) { panel.remove(); panel = null; }
    else if (config.enabled && space) render();
  });

  // ---- pluck data out of the audio-space graphql responses --------------------------
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

  function userOf(entry) {
    if (!entry) return null;
    const inner = entry.user_results ? entry.user_results.result : entry;
    const legacy = (inner && inner.legacy) || entry.legacy || {};
    const core = (inner && inner.core) || {};
    const screen_name = core.screen_name || legacy.screen_name || entry.twitter_screen_name || entry.screen_name;
    if (!screen_name) return null;
    const name = core.name || legacy.name || entry.display_name || entry.name || screen_name;
    const created = legacy.created_at || null;
    const followers = typeof legacy.followers_count === "number" ? legacy.followers_count : null;
    const avatar = legacy.profile_image_url_https || null;
    return { screen_name, name, created, followers, avatar };
  }

  function extract(json) {
    const audioSpace = walk(json, "audioSpace");
    if (!audioSpace) return null;
    const metadata = audioSpace.metadata || {};
    const participants = audioSpace.participants || {};
    const admins = Array.isArray(participants.admins) ? participants.admins.map(userOf).filter(Boolean) : [];
    const speakers = Array.isArray(participants.speakers) ? participants.speakers.map(userOf).filter(Boolean) : [];
    const listeners = Array.isArray(participants.listeners) ? participants.listeners.map(userOf).filter(Boolean) : [];
    const host = userOf(metadata.creator_results && metadata.creator_results.result) || admins[0] || null;
    return {
      id: metadata.rest_id || audioSpace.rest_id || null,
      title: metadata.title || "(no title)",
      state: metadata.state || "",
      host,
      admins,
      speakers,
      listeners,
    };
  }

  function onData(json) {
    const found = extract(json);
    if (!found) return;
    space = found;
    if (config.enabled) render();
  }

  // ---- hooks: fetch and XHR live in the page's world -------------------------------
  const realFetch = window.fetch;
  if (typeof realFetch === "function") {
    window.fetch = function (...args) {
      const promise = realFetch.apply(this, args);
      try {
        const url = typeof args[0] === "string" ? args[0] : (args[0] && args[0].url) || "";
        if (isSpaceUrl(url)) {
          promise
            .then((response) => {
              if (!response || !response.clone) return;
              response.clone().json().then(onData).catch(() => {});
            })
            .catch(() => {});
        }
      } catch {
        // A bad Request object should not break the page's fetch.
      }
      return promise;
    };
  }

  const XHR = window.XMLHttpRequest;
  if (XHR && XHR.prototype) {
    const realOpen = XHR.prototype.open;
    const realSend = XHR.prototype.send;
    XHR.prototype.open = function (method, url, ...rest) {
      try { this.__noahSpaceUrl = String(url || ""); } catch {}
      return realOpen.call(this, method, url, ...rest);
    };
    XHR.prototype.send = function (...args) {
      if (this.__noahSpaceUrl && isSpaceUrl(this.__noahSpaceUrl)) {
        this.addEventListener("load", () => {
          try { onData(JSON.parse(this.responseText)); } catch {}
        });
      }
      return realSend.apply(this, args);
    };
  }

  function isSpaceUrl(url) {
    return /\/AudioSpaceById|\/AudioSpaceParticipant|\/audio_space|\/api\/graphql\/[^/]+\/AudioSpace/i.test(url);
  }

  // ---- the panel ----------------------------------------------------------------
  const STYLE_ID = "noah-shield-space-style";
  function ensureStyle() {
    if (document.getElementById(STYLE_ID)) return;
    const style = document.createElement("style");
    style.id = STYLE_ID;
    style.textContent = `
      #noah-space-panel { position: fixed; right: 14px; bottom: 14px; z-index: 2147483000; width: 340px; max-height: 70vh; display: flex; flex-direction: column; background: rgba(10, 16, 12, .95); color: #d8ddd6; border: 1px solid rgba(180, 210, 190, .16); border-radius: 12px; box-shadow: 0 10px 30px rgba(0,0,0,.45); font: 12px/1.5 -apple-system, "Segoe UI", system-ui, sans-serif; overflow: hidden; }
      #noah-space-panel header { display: flex; align-items: center; gap: 8px; padding: 10px 12px; border-bottom: 1px solid rgba(180, 210, 190, .1); cursor: default; }
      #noah-space-panel header b { font-family: Georgia, serif; font-weight: 400; font-size: 14px; color: #f1f4ef; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      #noah-space-panel .noah-space-pill { font-size: 10px; padding: 2px 7px; border-radius: 999px; border: 1px solid rgba(180, 210, 190, .2); color: #a9cf9f; letter-spacing: .05em; text-transform: uppercase; }
      #noah-space-panel button.noah-space-toggle { all: unset; cursor: pointer; padding: 2px 8px; border-radius: 6px; color: #9aa298; font-size: 12px; }
      #noah-space-panel button.noah-space-toggle:hover { color: #f1f4ef; background: rgba(180, 210, 190, .08); }
      #noah-space-panel section { padding: 10px 12px; }
      #noah-space-panel section + section { border-top: 1px solid rgba(180, 210, 190, .08); }
      #noah-space-panel section h3 { margin: 0 0 6px; font: 400 11px var(--body, sans-serif); color: #9aa298; letter-spacing: .06em; text-transform: uppercase; }
      #noah-space-panel .noah-space-list { display: flex; flex-direction: column; gap: 4px; max-height: 44vh; overflow-y: auto; scrollbar-width: thin; scrollbar-color: rgba(180,210,190,.2) transparent; }
      #noah-space-panel .noah-space-row { display: flex; align-items: center; gap: 8px; padding: 4px 6px; border-radius: 6px; text-decoration: none; color: #d8ddd6; transition: background .16s; }
      #noah-space-panel .noah-space-row:hover { background: rgba(180, 210, 190, .06); }
      #noah-space-panel .noah-space-row img { width: 20px; height: 20px; border-radius: 999px; background: rgba(180,210,190,.08); object-fit: cover; }
      #noah-space-panel .noah-space-row .noah-space-name { color: #f1f4ef; font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex-shrink: 1; }
      #noah-space-panel .noah-space-row .noah-space-handle { color: #9aa298; font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      #noah-space-panel .noah-space-row.noah-space-flag { border-left: 2px solid #dcc896; padding-left: 4px; }
      #noah-space-panel .noah-space-row.noah-space-flag .noah-space-handle::after { content: " · new account"; color: #dcc896; }
      #noah-space-panel footer { padding: 8px 12px; font-size: 10px; color: #6f766e; border-top: 1px solid rgba(180, 210, 190, .08); }
      #noah-space-panel[data-collapsed="true"] { max-height: 46px; }
      #noah-space-panel[data-collapsed="true"] section, #noah-space-panel[data-collapsed="true"] footer { display: none; }
      @media (prefers-reduced-motion: reduce) { #noah-space-panel .noah-space-row { transition: none; } }
    `;
    (document.head || document.documentElement).append(style);
  }

  function isNew(created) {
    if (!created) return false;
    const time = Date.parse(created);
    if (Number.isNaN(time)) return false;
    const days = Math.max(1, Number(config.newAccountDays) || 30);
    return (Date.now() - time) < days * 24 * 60 * 60 * 1000;
  }
  function isFewFollowers(user) {
    if (!config.flagLowFollowers) return false;
    return typeof user.followers === "number" && user.followers < (Number(config.lowFollowersUnder) || 20);
  }

  function row(user, flag) {
    const anchor = document.createElement("a");
    anchor.className = "noah-space-row" + (flag ? " noah-space-flag" : "");
    anchor.href = "https://x.com/" + encodeURIComponent(user.screen_name);
    anchor.target = "_blank";
    anchor.rel = "noopener noreferrer";
    anchor.title = user.name + " · @" + user.screen_name + (user.followers != null ? " · " + user.followers.toLocaleString() + " followers" : "");
    const avatar = document.createElement("img");
    avatar.alt = "";
    avatar.referrerPolicy = "no-referrer";
    if (user.avatar) avatar.src = user.avatar;
    const name = document.createElement("span");
    name.className = "noah-space-name";
    name.textContent = user.name;
    const handle = document.createElement("span");
    handle.className = "noah-space-handle";
    handle.textContent = "@" + user.screen_name;
    anchor.append(avatar, name, handle);
    return anchor;
  }

  function render() {
    ensureStyle();
    if (!panel) {
      panel = document.createElement("div");
      panel.id = "noah-space-panel";
      document.documentElement.append(panel);
    }
    panel.replaceChildren();
    const head = document.createElement("header");
    const title = document.createElement("b");
    title.textContent = space.title || "space";
    const pill = document.createElement("span");
    pill.className = "noah-space-pill";
    pill.textContent = "shield";
    const toggle = document.createElement("button");
    toggle.type = "button";
    toggle.className = "noah-space-toggle";
    toggle.textContent = collapsed ? "show" : "hide";
    toggle.addEventListener("click", () => {
      collapsed = !collapsed;
      panel.dataset.collapsed = collapsed ? "true" : "false";
      toggle.textContent = collapsed ? "show" : "hide";
    });
    head.append(title, pill, toggle);
    panel.append(head);
    if (collapsed) return;

    const speakerList = [space.host, ...space.admins.filter((u) => !space.host || u.screen_name !== space.host.screen_name), ...space.speakers].filter(Boolean);
    const seen = new Set();
    const dedupedSpeakers = speakerList.filter((u) => (seen.has(u.screen_name) ? false : seen.add(u.screen_name)));
    if (dedupedSpeakers.length) {
      const section = document.createElement("section");
      const heading = document.createElement("h3");
      heading.textContent = "host and speakers · " + dedupedSpeakers.length;
      const list = document.createElement("div");
      list.className = "noah-space-list";
      for (const user of dedupedSpeakers) list.append(row(user, false));
      section.append(heading, list);
      panel.append(section);
    }

    const listeners = (space.listeners || []).filter((u) => !seen.has(u.screen_name));
    const section = document.createElement("section");
    const heading = document.createElement("h3");
    heading.textContent = listeners.length ? "listeners · " + listeners.length : "listeners";
    const list = document.createElement("div");
    list.className = "noah-space-list";
    if (listeners.length === 0) {
      const note = document.createElement("div");
      note.style.color = "#9aa298";
      note.style.fontSize = "12px";
      note.textContent = "The room has no listeners yet, or has not sent its listener list to the browser.";
      list.append(note);
    } else {
      for (const user of listeners) list.append(row(user, isNew(user.created) || isFewFollowers(user)));
    }
    section.append(heading, list);
    panel.append(section);

    const foot = document.createElement("footer");
    foot.textContent = "From data your browser was already going to receive. Nothing leaves this page. Click a name to open its profile.";
    panel.append(foot);

    // Auditors on their own platform (X, Twitter): a one-click into inspect
    // mode so the AudioSpaceById responses and everything else on the page
    // are shown in full for the security team.
    const auditRow = document.createElement("div");
    auditRow.style.padding = "8px 12px";
    auditRow.style.borderTop = "1px solid rgba(180, 210, 190, .08)";
    auditRow.style.display = "flex";
    auditRow.style.alignItems = "center";
    auditRow.style.gap = "8px";
    const auditNote = document.createElement("span");
    auditNote.style.flex = "1";
    auditNote.style.color = "#9aa298";
    auditNote.style.fontSize = "11px";
    auditNote.textContent = "Auditing your own platform?";
    const auditBtn = document.createElement("button");
    auditBtn.type = "button";
    auditBtn.style.all = "unset";
    auditBtn.style.cursor = "pointer";
    auditBtn.style.padding = "3px 10px";
    auditBtn.style.borderRadius = "6px";
    auditBtn.style.border = "1px solid rgba(180, 210, 190, .25)";
    auditBtn.style.color = "#a9cf9f";
    auditBtn.style.fontSize = "11px";
    auditBtn.textContent = "arm inspect + reload";
    auditBtn.addEventListener("click", (event) => {
      event.preventDefault();
      window.dispatchEvent(new CustomEvent("noah-space-audit-request"));
    });
    auditRow.append(auditNote, auditBtn);
    panel.append(auditRow);
  }
})();
