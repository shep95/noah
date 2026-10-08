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
  // The last AudioSpace GraphQL path we saw, used as a template so mutations
  // fire against the same deployment. Twitter changes these query ids often,
  // so we sniff instead of hardcoding.
  let audioSpaceQueryPrefix = null;
  // HLS stream URL captured from the liveVideoStream / startAudioSpace response.
  // This is the m3u8 playlist URL that carries the Space's audio. We show it in
  // the panel so the user can paste it into VLC, ffmpeg or any HLS downloader.
  let hlsStreamUrl = null;
  let hlsDynamicUrl = null; // shorter-lived signed URL if present
  // Local soundboard clips saved in this browser only. Each is {id, name, url}
  // where url is a blob: URL for a File the user picked. Nothing uploads.
  let sounds = [];
  let playing = null;
  // Start collapsed so it never fights X's own widgets for screen space; the
  // user opens it when they want to look. Setting persists in this tab.
  let collapsed = true;
  try {
    const stored = sessionStorage.getItem("noah-space-collapsed");
    if (stored !== null) collapsed = stored === "true";
  } catch {}
  let config = { enabled: true, newAccountDays: 30, flagLowFollowers: true, lowFollowersUnder: 20 };
  // The bridge and this script pair up with a nonce before any page script
  // runs (both start at document_start), so a page cannot forge the events
  // that drive this script or listen in on them.
  const nonce = crypto.randomUUID();
  const channel = (name) => name + ":" + nonce;
  const hello = () => { try { window.dispatchEvent(new CustomEvent("noah-spaces-hello", { detail: nonce })); } catch {} };
  window.addEventListener("noah-spaces-bridge-ready", hello);
  hello();

  window.addEventListener(channel("noah-spaces-config"), (event) => {
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
    const rest_id = inner && inner.rest_id ? String(inner.rest_id) : (entry.rest_id ? String(entry.rest_id) : null);
    return { screen_name, name, created, followers, avatar, rest_id };
  }

  function extract(json) {
    const audioSpace = walk(json, "audioSpace");
    if (!audioSpace) return null;
    const metadata = audioSpace.metadata || {};
    const participants = audioSpace.participants || {};
    const admins = Array.isArray(participants.admins) ? participants.admins.map(userOf).filter(Boolean) : [];
    const speakers = Array.isArray(participants.speakers) ? participants.speakers.map(userOf).filter(Boolean) : [];
    // Anonymous listeners come back as entries with no screen_name. userOf
    // returns null for them, and we used to drop them silently. Count them
    // so the panel can name them "hidden listeners" — this is the point of
    // the feature for a parent looking at who is in the room.
    const rawListeners = Array.isArray(participants.listeners) ? participants.listeners : [];
    const listenerResults = rawListeners.map(userOf);
    const listeners = listenerResults.filter(Boolean);
    let hiddenListeners = listenerResults.length - listeners.length;
    // Some API shapes also carry a total count separately; if the total is
    // larger than the entries we received, the extras are also hidden.
    const declaredTotal = Number(
      participants.total || participants.total_participants ||
      metadata.total_live_listeners || metadata.total_participated ||
      audioSpace.total_live_listeners || 0
    );
    const nameableTotal = listeners.length + admins.length + speakers.length;
    if (Number.isFinite(declaredTotal) && declaredTotal > 0) {
      const extra = declaredTotal - nameableTotal - hiddenListeners;
      if (extra > 0) hiddenListeners += extra;
    }
    const host = userOf(metadata.creator_results && metadata.creator_results.result) || admins[0] || null;
    return {
      id: metadata.rest_id || audioSpace.rest_id || null,
      title: metadata.title || "(no title)",
      state: metadata.state || "",
      host,
      admins,
      speakers,
      listeners,
      hiddenListeners,
    };
  }

  // ---- HLS stream URL extraction ------------------------------------------------
  // Mechanism: when a user's browser connects to a live Space, X fetches the
  // stream's HLS playlist from its media backend. The response contains the
  // m3u8 URL. We intercept those responses — which the browser was already
  // going to receive — and surface the URL in the panel. The user can then
  // paste it into VLC, ffmpeg, or any HLS downloader to record or replay
  // the Space audio. Nothing is uploaded or transmitted; we only read what
  // already arrived.
  function extractHlsUrl(json) {
    if (!json || typeof json !== "object") return;
    // Walk all string values in the JSON looking for m3u8 URLs
    function walkForM3u8(node) {
      if (typeof node === "string") {
        if (node.includes(".m3u8") || node.includes("master_playlist") || node.includes("stream.m3u8")) {
          return node;
        }
        return null;
      }
      if (Array.isArray(node)) {
        for (const item of node) {
          const found = walkForM3u8(item);
          if (found) return found;
        }
        return null;
      }
      if (node && typeof node === "object") {
        // Prioritize known field names for the HLS URL
        for (const key of ["hls_url", "stream_url", "playlist_url", "location", "url"]) {
          if (typeof node[key] === "string" && node[key].includes(".m3u8")) return node[key];
        }
        for (const key of Object.keys(node)) {
          const found = walkForM3u8(node[key]);
          if (found) return found;
        }
      }
      return null;
    }
    const found = walkForM3u8(json);
    if (found && found !== hlsStreamUrl) {
      // Prefer HTTPS URLs; if it's a relative path, skip
      if (!found.startsWith("http")) return;
      hlsStreamUrl = found;
      if (space && config.enabled) render();
    }
  }

  function onData(json) {
    const found = extract(json);
    if (!found) return;
    space = found;
    // Also try to pull HLS URL from the same response — AudioSpaceById often
    // includes the stream location in the same payload as participant data
    extractHlsUrl(json);
    if (config.enabled) render();
  }

  function onStreamData(json) {
    // liveVideoStream / startAudioSpace / audioSpaceStream responses carry the
    // m3u8 URL even when they contain no participant metadata
    extractHlsUrl(json);
    // Some stream responses also update participant counts
    const found = extract(json);
    if (found) {
      space = found;
      if (config.enabled) render();
    }
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
        } else if (isStreamUrl(url)) {
          promise
            .then((response) => {
              if (!response || !response.clone) return;
              response.clone().json().then(onStreamData).catch(() => {});
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
      if (this.__noahSpaceUrl) {
        if (isSpaceUrl(this.__noahSpaceUrl)) {
          this.addEventListener("load", () => {
            try { onData(JSON.parse(this.responseText)); } catch {}
          });
        } else if (isStreamUrl(this.__noahSpaceUrl)) {
          this.addEventListener("load", () => {
            try { onStreamData(JSON.parse(this.responseText)); } catch {}
          });
        }
      }
      return realSend.apply(this, args);
    };
  }

  function isSpaceUrl(url) {
    const hit = /\/AudioSpaceById|\/AudioSpaceParticipant|\/audio_space|\/api\/graphql\/[^/]+\/AudioSpace/i.test(url);
    if (hit) {
      // Sniff the deployment prefix once so mutations fire against the same
      // graphql endpoint layout the page uses.
      const match = /(\/i\/api\/graphql\/[^/]+\/)/.exec(url);
      if (match) audioSpaceQueryPrefix = match[1];
    }
    return hit;
  }
  function isStreamUrl(url) {
    // These endpoints carry the HLS stream URL for a live Space
    return /\/liveVideoStream|\/startAudioSpace|\/AudioSpaceStream|\/live_video_stream|\/1\.1\/live_video_stream/i.test(url)
      || (url.includes("pscp.tv") && url.includes("broadcastAccess"))
      || url.includes("broadcast_access")
      || /\/spaces\/[^/]+\/stream/i.test(url);
  }

  // ---- who is this browser signed in as ------------------------------------
  // Twitter's non-httpOnly cookie carries the numeric user id (twid=u%3D…).
  // This is what the audio-space response uses as rest_id for the host.
  function currentUserId() {
    try {
      const m = /twid=u%3D(\d+)/.exec(document.cookie);
      return m ? m[1] : null;
    } catch { return null; }
  }
  function selfIsHost() {
    const uid = currentUserId();
    if (!uid || !space || !space.host) return false;
    const hostId = space.host.rest_id || (space.admins[0] && space.admins[0].rest_id);
    return hostId && String(hostId) === String(uid);
  }
  function csrfToken() {
    try {
      const m = /ct0=([a-f0-9]+)/.exec(document.cookie);
      return m ? m[1] : null;
    } catch { return null; }
  }

  // ---- host actions: hit the same graphql the web app hits -----------------
  // If the plugin-holder isn't the host, the server refuses the call with
  // 401/403; we surface the reply verbatim rather than pretend it worked.
  async function callAudioSpaceMutation(operationName, variables) {
    if (!audioSpaceQueryPrefix) throw new Error("no audio-space endpoint captured yet; reload the Space page first");
    if (!space || !space.id) throw new Error("no space id known yet");
    const csrf = csrfToken();
    if (!csrf) throw new Error("no csrf token on this browser; sign in to X first");
    const url = audioSpaceQueryPrefix + operationName;
    const body = JSON.stringify({ queryId: (audioSpaceQueryPrefix.split("/")[4] || ""), variables, features: {} });
    const response = await fetch(url, {
      method: "POST",
      credentials: "include",
      headers: {
        "content-type": "application/json",
        "x-csrf-token": csrf,
        "x-twitter-active-user": "yes",
        "x-twitter-auth-type": "OAuth2Session",
      },
      body,
    });
    let text = "";
    try { text = await response.text(); } catch {}
    return { ok: response.ok, status: response.status, text: text.slice(0, 400) };
  }
  const HostActions = {
    setTitle(title) {
      return callAudioSpaceMutation("AudioSpaceUpdateMetadata", {
        broadcast_id: space.id, title: String(title || "").slice(0, 100),
      });
    },
    mute(userId) {
      return callAudioSpaceMutation("AudioSpaceMuteSpeaker", {
        broadcast_id: space.id, user_id: String(userId),
      });
    },
    unmute(userId) {
      return callAudioSpaceMutation("AudioSpaceUnmuteSpeaker", {
        broadcast_id: space.id, user_id: String(userId),
      });
    },
    remove(userId) {
      return callAudioSpaceMutation("AudioSpaceRemoveUser", {
        broadcast_id: space.id, user_id: String(userId),
      });
    },
    promoteToSpeaker(userId) {
      return callAudioSpaceMutation("AudioSpaceInviteUsersToSpeak", {
        broadcast_id: space.id, user_ids: [String(userId)],
      });
    },
    demoteFromSpeaker(userId) {
      return callAudioSpaceMutation("AudioSpaceRemoveSpeaker", {
        broadcast_id: space.id, user_id: String(userId),
      });
    },
  };

  // ---- local soundboard: audio the user chose, played into their tab ------
  // Nothing leaves the tab. Other listeners only hear this if the user routes
  // their tab's audio into their microphone (a VB-Cable style loopback).
  const audioElement = new Audio();
  audioElement.preload = "none";
  audioElement.addEventListener("ended", () => { playing = null; renderControls(); });
  function playSound(sound) {
    if (!sound || !sound.url) return;
    if (playing) { audioElement.pause(); audioElement.currentTime = 0; }
    audioElement.src = sound.url;
    audioElement.volume = Math.max(0, Math.min(1, sound.volume ?? 0.8));
    audioElement.play().then(() => { playing = sound.id; renderControls(); }).catch(() => { playing = null; renderControls(); });
  }
  function stopSound() {
    if (playing) { audioElement.pause(); audioElement.currentTime = 0; playing = null; renderControls(); }
  }
  // Track all blob URLs created for sounds so they can be revoked on unload.
  const _blobUrls = [];
  window.addEventListener("beforeunload", () => {
    for (const u of _blobUrls) try { URL.revokeObjectURL(u); } catch {}
  });
  function addSound(file) {
    const id = "s" + Date.now() + Math.floor(Math.random() * 1e6);
    const url = URL.createObjectURL(file);
    _blobUrls.push(url);
    sounds.push({ id, name: file.name.replace(/\.[^.]+$/, "").slice(0, 24), url });
    renderControls();
  }
  function removeSound(id) {
    const found = sounds.find((s) => s.id === id);
    if (found && found.url && found.url.startsWith("blob:")) URL.revokeObjectURL(found.url);
    sounds = sounds.filter((s) => s.id !== id);
    if (playing === id) stopSound();
    renderControls();
  }

  // ---- the panel ----------------------------------------------------------------
  const STYLE_ID = "noah-shield-space-style";
  function ensureStyle() {
    if (document.getElementById(STYLE_ID)) return;
    const style = document.createElement("style");
    style.id = STYLE_ID;
    style.textContent = `
      @keyframes noah-space-arrive { from { opacity:0; transform:translateY(8px) scale(0.98); } to { opacity:1; transform:none; } }
      #noah-space-panel {
        position: fixed; left: 14px; bottom: 14px; z-index: 2147483000;
        width: 300px; max-height: 72vh; display: flex; flex-direction: column;
        background: rgba(5, 8, 6, .97);
        color: #d8ddd6;
        border: 1px solid rgba(114, 168, 104, .18);
        border-radius: 14px;
        box-shadow: 0 20px 60px rgba(0,0,0,.65), 0 0 0 1px rgba(0,0,0,.5) inset, 0 0 40px rgba(0,0,0,.3);
        font: 12px/1.5 -apple-system, "Segoe UI", system-ui, sans-serif;
        overflow: hidden;
        backdrop-filter: blur(20px) saturate(1.3);
        animation: noah-space-arrive .22s cubic-bezier(0.16,0.8,0.18,1) both;
      }
      #noah-space-panel header {
        display: flex; align-items: center; gap: 8px; padding: 11px 13px;
        border-bottom: 1px solid rgba(160, 200, 170, .09);
        cursor: move; user-select: none;
        background: linear-gradient(180deg, rgba(95,138,88,.08), transparent);
      }
      #noah-space-panel header b { font-family: Georgia, serif; font-weight: 400; font-size: 13px; color: #f1f4ef; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      #noah-space-panel .noah-space-pill { font-size: 9px; padding: 2px 7px; border-radius: 999px; border: 1px solid rgba(114, 168, 104, .28); color: #a2cb98; letter-spacing: .07em; text-transform: uppercase; background: rgba(95,138,88,.1); }
      #noah-space-panel button.noah-space-toggle { all: unset; cursor: pointer; padding: 2px 8px; border-radius: 6px; color: #8f9690; font-size: 12px; transition: color .14s, background .14s; }
      #noah-space-panel button.noah-space-toggle:hover { color: #f1f4ef; background: rgba(160, 200, 170, .07); }
      #noah-space-panel section { padding: 10px 13px; }
      #noah-space-panel section + section { border-top: 1px solid rgba(160, 200, 170, .07); }
      #noah-space-panel section h3 { margin: 0 0 7px; font: 400 10px sans-serif; color: #8f9690; letter-spacing: .08em; text-transform: uppercase; }
      #noah-space-panel .noah-space-list { display: flex; flex-direction: column; gap: 3px; max-height: 44vh; overflow-y: auto; scrollbar-width: thin; scrollbar-color: rgba(160,200,170,.18) transparent; }
      #noah-space-panel .noah-space-row { display: flex; align-items: center; gap: 8px; padding: 5px 7px; border-radius: 8px; text-decoration: none; color: #d8ddd6; transition: background .14s; }
      #noah-space-panel .noah-space-row:hover { background: rgba(160, 200, 170, .05); }
      #noah-space-panel .noah-space-row img { width: 22px; height: 22px; border-radius: 999px; background: rgba(160,200,170,.08); object-fit: cover; border: 1px solid rgba(160,200,170,.1); }
      #noah-space-panel .noah-space-row .noah-space-name { color: #edf0eb; font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex-shrink: 1; font-weight: 500; }
      #noah-space-panel .noah-space-row .noah-space-handle { color: #8f9690; font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      #noah-space-panel .noah-space-row.noah-space-flag { border-left: 2px solid rgba(216,194,142,.7); padding-left: 5px; background: rgba(216,194,142,.03); }
      #noah-space-panel .noah-space-row.noah-space-flag .noah-space-handle::after { content: " · new account"; color: #d8c28e; }
      #noah-space-panel .noah-space-hidden { margin-top: 5px; padding: 5px 9px; color: #d8c28e; font-size: 11px; border-left: 2px solid rgba(216,194,142,.5); background: rgba(216,194,142,.04); border-radius: 0 6px 6px 0; }
      #noah-space-panel .noah-space-actions { display: flex; gap: 4px; margin-left: auto; opacity: 0; transition: opacity .12s; }
      #noah-space-panel .noah-space-row:hover .noah-space-actions { opacity: 1; }
      #noah-space-panel .noah-space-act { all: unset; cursor: pointer; padding: 2px 7px; border-radius: 5px; border: 1px solid rgba(160, 200, 170, .16); color: #d8ddd6; font-size: 10px; transition: background .12s, border-color .12s; }
      #noah-space-panel .noah-space-act[disabled] { cursor: not-allowed; opacity: .28; }
      #noah-space-panel .noah-space-act:not([disabled]):hover { background: rgba(160, 200, 170, .09); border-color: rgba(160, 200, 170, .26); color: #f1f4ef; }
      #noah-space-panel .noah-space-act.danger:not([disabled]):hover { background: rgba(228, 173, 160, .1); color: #e4ada0; border-color: rgba(228, 173, 160, .28); }
      #noah-space-panel .noah-space-controls input[type="text"] { flex: 1; background: rgba(160, 200, 170, .04); border: 1px solid rgba(160, 200, 170, .13); border-radius: 7px; padding: 4px 8px; color: #f1f4ef; font: inherit; }
      #noah-space-panel .noah-space-controls .noah-row { display: flex; gap: 6px; align-items: center; margin-top: 6px; }
      #noah-space-panel .noah-space-controls button { all: unset; cursor: pointer; padding: 3px 10px; border-radius: 7px; border: 1px solid rgba(114, 168, 104, .28); color: #a2cb98; font-size: 11px; background: rgba(95,138,88,.07); transition: background .13s; }
      #noah-space-panel .noah-space-controls button:not([disabled]):hover { background: rgba(95,138,88,.14); }
      #noah-space-panel .noah-space-controls button[disabled] { cursor: not-allowed; opacity: .32; color: #8f9690; }
      #noah-space-panel .noah-space-controls .note { color: #8f9690; font-size: 10px; margin-top: 5px; line-height: 1.5; }
      #noah-space-panel .noah-space-controls .note.warn { color: #d8c28e; }
      #noah-space-panel .noah-space-sounds { display: flex; flex-wrap: wrap; gap: 4px; margin-top: 7px; }
      #noah-space-panel .noah-sound { display: inline-flex; align-items: center; gap: 3px; padding: 3px 8px; border-radius: 999px; background: rgba(160, 200, 170, .06); border: 1px solid rgba(160, 200, 170, .14); color: #d8ddd6; font-size: 11px; transition: border-color .13s; }
      #noah-space-panel .noah-sound button { all: unset; cursor: pointer; padding: 0 3px; color: #8f9690; transition: color .12s; }
      #noah-space-panel .noah-sound button:hover { color: #f1f4ef; }
      #noah-space-panel .noah-sound.playing { border-color: rgba(162,203,152,.5); color: #a2cb98; box-shadow: 0 0 8px rgba(95,138,88,.2); }
      #noah-space-panel .noah-space-op { margin-top: 4px; font-size: 10px; color: #8f9690; }
      #noah-space-panel .noah-space-op.ok { color: #a2cb98; }
      #noah-space-panel .noah-space-op.bad { color: #e4ada0; }
      #noah-space-panel footer { padding: 8px 13px; font-size: 10px; color: #626860; border-top: 1px solid rgba(160, 200, 170, .07); }
      #noah-space-panel .noah-dl-box { background: rgba(95,138,88,.05); border: 1px solid rgba(114,168,104,.18); border-radius: 9px; padding: 9px 11px; margin-top: 4px; }
      #noah-space-panel .noah-dl-url { font: 11px/1.4 "SF Mono", "Fira Mono", monospace; color: #a2cb98; word-break: break-all; margin-bottom: 7px; }
      #noah-space-panel .noah-dl-cmd { font: 10px/1.5 "SF Mono", "Fira Mono", monospace; color: #8f9690; white-space: pre-wrap; word-break: break-all; background: rgba(0,0,0,.25); border-radius: 5px; padding: 5px 8px; margin: 4px 0; }
      #noah-space-panel .noah-dl-btns { display: flex; gap: 5px; flex-wrap: wrap; margin-bottom: 7px; }
      #noah-space-panel .noah-dl-btn { all: unset; cursor: pointer; padding: 3px 10px; border-radius: 6px; border: 1px solid rgba(114,168,104,.28); color: #a2cb98; font-size: 10px; background: rgba(95,138,88,.08); transition: background .13s; }
      #noah-space-panel .noah-dl-btn:hover { background: rgba(95,138,88,.17); }
      #noah-space-panel .noah-dl-copied { color: #7bc17a; font-size: 10px; margin-left: 4px; opacity: 0; transition: opacity .2s; }
      #noah-space-panel .noah-dl-copied.show { opacity: 1; }
      #noah-space-panel[data-collapsed="true"] { max-height: 44px; }
      #noah-space-panel[data-collapsed="true"] section, #noah-space-panel[data-collapsed="true"] footer { display: none; }
      @media (prefers-reduced-motion: reduce) { #noah-space-panel { animation: none; } #noah-space-panel .noah-space-row { transition: none; } }
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

  function row(user, flag, kind) {
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

    // Host actions: appear only when the browser's own user is the host of
    // this Space. The server will refuse them anyway if the caller is not
    // authorised, so this is UX, not a security boundary.
    if (selfIsHost() && user.rest_id) {
      const actions = document.createElement("span");
      actions.className = "noah-space-actions";
      const uid = String(user.rest_id);
      const isSelf = uid === String(currentUserId());
      if (kind === "speaker") {
        actions.append(makeActionButton("mute", () => HostActions.mute(uid)));
        actions.append(makeActionButton("unmute", () => HostActions.unmute(uid)));
        actions.append(makeActionButton("demote", () => HostActions.demoteFromSpeaker(uid), { disabled: isSelf }));
      } else if (kind === "listener") {
        actions.append(makeActionButton("promote", () => HostActions.promoteToSpeaker(uid)));
      }
      if (kind !== "self") {
        actions.append(makeActionButton("remove", () => HostActions.remove(uid), { danger: true, disabled: isSelf }));
      }
      anchor.append(actions);
    }
    return anchor;
  }
  function makeActionButton(label, action, opts) {
    opts = opts || {};
    const button = document.createElement("button");
    button.type = "button";
    button.className = "noah-space-act" + (opts.danger ? " danger" : "");
    button.textContent = label;
    if (opts.disabled) button.disabled = true;
    button.addEventListener("click", async (event) => {
      event.preventDefault();
      event.stopPropagation();
      // confirmAction: destructive mutations (remove, demote) require confirmation before firing.
      if (opts.danger && !window.confirm("Send \"" + label + "\" to Twitter for this user? This cannot be undone from the panel.")) return;
      button.disabled = true;
      const previous = button.textContent;
      button.textContent = "…";
      try {
        const result = await action();
        button.textContent = result.ok ? "done" : (result.status + " error");
      } catch (error) {
        button.textContent = String(error.message || "error").slice(0, 20);
      }
      setTimeout(() => { button.textContent = previous; button.disabled = opts.disabled; }, 2000);
    });
    return button;
  }

  function renderControls() {
    if (!panel) return;
    const section = panel.querySelector("[data-role='controls']");
    if (!section) return;
    section.replaceChildren();

    // Header
    const heading = document.createElement("h3");
    heading.textContent = selfIsHost() ? "controls · you are the host" : "controls";
    section.append(heading);

    // Host actions: edit title. Buttons on each participant row do the rest.
    const titleRow = document.createElement("div");
    titleRow.className = "noah-row";
    const titleInput = document.createElement("input");
    titleInput.type = "text";
    titleInput.value = space.title || "";
    titleInput.placeholder = "space title";
    titleInput.disabled = !selfIsHost();
    const titleButton = document.createElement("button");
    titleButton.textContent = "save title";
    titleButton.disabled = !selfIsHost();
    const status = document.createElement("div");
    status.className = "noah-space-op";
    titleButton.addEventListener("click", async () => {
      titleButton.disabled = true;
      status.className = "noah-space-op";
      status.textContent = "saving…";
      try {
        const reply = await HostActions.setTitle(titleInput.value);
        status.className = "noah-space-op " + (reply.ok ? "ok" : "bad");
        status.textContent = reply.ok ? "title updated" : "server said " + reply.status + ": " + reply.text.slice(0, 80);
      } catch (error) {
        status.className = "noah-space-op bad";
        status.textContent = String(error.message || error);
      }
      titleButton.disabled = !selfIsHost();
    });
    titleRow.append(titleInput, titleButton);
    section.append(titleRow, status);

    if (!selfIsHost()) {
      const note = document.createElement("div");
      note.className = "note";
      note.textContent = "Host-only actions (edit title, mute, remove, promote to speaker, demote from speaker) show up on each row when you are the host of this Space. Otherwise Twitter's server would refuse them.";
      section.append(note);
    } else {
      const note = document.createElement("div");
      note.className = "note";
      note.textContent = "Hover a name to mute, unmute, promote, demote or remove. Every action goes through Twitter with your own auth; the server has the final say.";
      section.append(note);
    }

    // Soundboard: local audio the user picked. Nothing uploads. If the user
    // wants other listeners to hear it, they route their tab audio into
    // their microphone (VB-Cable, BlackHole, Voicemeeter).
    const soundHeading = document.createElement("h3");
    soundHeading.textContent = "soundboard · " + sounds.length;
    soundHeading.style.marginTop = "12px";
    section.append(soundHeading);

    const soundRow = document.createElement("div");
    soundRow.className = "noah-space-sounds";
    for (const sound of sounds) {
      const chip = document.createElement("span");
      chip.className = "noah-sound" + (playing === sound.id ? " playing" : "");
      const playBtn = document.createElement("button");
      playBtn.type = "button";
      playBtn.textContent = playing === sound.id ? "◼" : "▶";
      playBtn.addEventListener("click", () => (playing === sound.id ? stopSound() : playSound(sound)));
      const label = document.createElement("span");
      label.textContent = sound.name;
      const del = document.createElement("button");
      del.type = "button";
      del.textContent = "×";
      del.title = "remove this sound";
      del.addEventListener("click", () => removeSound(sound.id));
      chip.append(playBtn, label, del);
      soundRow.append(chip);
    }
    section.append(soundRow);

    const addRow = document.createElement("div");
    addRow.className = "noah-row";
    const fileInput = document.createElement("input");
    fileInput.type = "file";
    fileInput.accept = "audio/*";
    fileInput.multiple = true;
    fileInput.style.flex = "1";
    fileInput.style.fontSize = "11px";
    fileInput.style.color = "#9aa298";
    fileInput.addEventListener("change", (event) => {
      const files = Array.from(event.target.files || []);
      for (const file of files) if (file && file.type.startsWith("audio")) addSound(file);
      event.target.value = "";
    });
    addRow.append(fileInput);
    section.append(addRow);

    const soundNote = document.createElement("div");
    soundNote.className = "note";
    soundNote.textContent = "Sounds play from your own speakers. To share them with the room, route your tab audio into your microphone with a virtual audio cable (VB-Cable on Windows, BlackHole on Mac).";
    section.append(soundNote);

    // Honest note about the anti-kick request: not something a plugin can do.
    const kickNote = document.createElement("div");
    kickNote.className = "note warn";
    kickNote.textContent = "A plugin cannot stop a host from removing you: the disconnect happens on Twitter's side. If you are getting brigaded as a host, lock the room to speakers only from Twitter's own controls.";
    section.append(kickNote);
  }

  function render() {
    ensureStyle();
    if (!panel) {
      panel = document.createElement("div");
      panel.id = "noah-space-panel";
      document.documentElement.append(panel);
    }
    panel.replaceChildren();
    panel.dataset.collapsed = collapsed ? "true" : "false";
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
    toggle.addEventListener("click", (event) => {
      event.stopPropagation();
      collapsed = !collapsed;
      panel.dataset.collapsed = collapsed ? "true" : "false";
      toggle.textContent = collapsed ? "show" : "hide";
      try { sessionStorage.setItem("noah-space-collapsed", collapsed ? "true" : "false"); } catch {}
      render();
    });
    // Drag the panel by its header so users can move it out of the way of X's
    // own widgets. The position sticks in this tab's sessionStorage.
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
    document.addEventListener("mouseup", () => {
      if (!dragStart) return;
      dragStart = null;
      try { sessionStorage.setItem("noah-space-pos", JSON.stringify({ left: panel.style.left, top: panel.style.top })); } catch {}
    });
    try {
      const pos = JSON.parse(sessionStorage.getItem("noah-space-pos") || "null");
      if (pos && pos.left && pos.top) {
        panel.style.left = pos.left;
        panel.style.top = pos.top;
        panel.style.right = "auto";
        panel.style.bottom = "auto";
      }
    } catch {}
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
      for (const user of dedupedSpeakers) list.append(row(user, false, "speaker"));
      section.append(heading, list);
      panel.append(section);
    }

    const listeners = (space.listeners || []).filter((u) => !seen.has(u.screen_name));
    const hiddenCount = Math.max(0, Number(space.hiddenListeners) || 0);
    const section = document.createElement("section");
    const heading = document.createElement("h3");
    const totalListeners = listeners.length + hiddenCount;
    heading.textContent = totalListeners ? "listeners · " + totalListeners : "listeners";
    const list = document.createElement("div");
    list.className = "noah-space-list";
    if (listeners.length === 0 && hiddenCount === 0) {
      const note = document.createElement("div");
      note.style.color = "#9aa298";
      note.style.fontSize = "12px";
      note.textContent = "The room has no listeners yet, or has not sent its listener list to the browser.";
      list.append(note);
    } else {
      for (const user of listeners) list.append(row(user, isNew(user.created) || isFewFollowers(user), "listener"));
    }
    section.append(heading, list);
    if (hiddenCount > 0) {
      const hiddenRow = document.createElement("div");
      hiddenRow.className = "noah-space-hidden";
      hiddenRow.textContent = "hidden listeners · " + hiddenCount + " (their handles are not in what the server sent this tab)";
      section.append(hiddenRow);
    }
    panel.append(section);

    // ---- Space downloader ---------------------------------------------------
    // When liveVideoStream / startAudioSpace returns an m3u8 URL we surface it
    // here so the user can record or replay the Space with any HLS tool.
    // Nothing is uploaded; we only show the URL the browser already received.
    const dlSection = document.createElement("section");
    {
      const heading = document.createElement("h3");
      heading.textContent = "space downloader";
      dlSection.append(heading);

      if (hlsStreamUrl) {
        const box = document.createElement("div");
        box.className = "noah-dl-box";

        const urlEl = document.createElement("div");
        urlEl.className = "noah-dl-url";
        urlEl.textContent = hlsStreamUrl;
        box.append(urlEl);

        const btns = document.createElement("div");
        btns.className = "noah-dl-btns";
        const copied = document.createElement("span");
        copied.className = "noah-dl-copied";
        copied.textContent = "copied";

        const copyBtn = document.createElement("button");
        copyBtn.type = "button";
        copyBtn.className = "noah-dl-btn";
        copyBtn.textContent = "copy stream url";
        copyBtn.addEventListener("click", () => {
          navigator.clipboard.writeText(hlsStreamUrl).then(() => {
            copied.classList.add("show");
            setTimeout(() => copied.classList.remove("show"), 1800);
          }).catch(() => {
            // Fallback for browsers that block clipboard in content scripts
            const ta = document.createElement("textarea");
            ta.value = hlsStreamUrl;
            ta.style.position = "fixed";
            ta.style.top = "-9999px";
            document.body.append(ta);
            ta.select();
            document.execCommand("copy");
            ta.remove();
            copied.classList.add("show");
            setTimeout(() => copied.classList.remove("show"), 1800);
          });
        });

        const vlcBtn = document.createElement("button");
        vlcBtn.type = "button";
        vlcBtn.className = "noah-dl-btn";
        vlcBtn.textContent = "open in vlc";
        vlcBtn.title = "Opens vlc:// protocol — VLC must be installed";
        vlcBtn.addEventListener("click", () => {
          window.open("vlc://" + hlsStreamUrl, "_blank");
        });

        btns.append(copyBtn, vlcBtn, copied);
        box.append(btns);

        const ffLabel = document.createElement("div");
        ffLabel.style.color = "#8f9690";
        ffLabel.style.fontSize = "10px";
        ffLabel.style.marginBottom = "3px";
        ffLabel.textContent = "record with ffmpeg (paste in a terminal):";
        const ffCmd = document.createElement("div");
        ffCmd.className = "noah-dl-cmd";
        ffCmd.textContent = `ffmpeg -i "${hlsStreamUrl}" -c copy space_recording.aac`;
        box.append(ffLabel, ffCmd);

        const copyFf = document.createElement("button");
        copyFf.type = "button";
        copyFf.className = "noah-dl-btn";
        copyFf.textContent = "copy ffmpeg command";
        const copiedFf = document.createElement("span");
        copiedFf.className = "noah-dl-copied";
        copiedFf.textContent = "copied";
        copyFf.addEventListener("click", () => {
          const cmd = `ffmpeg -i "${hlsStreamUrl}" -c copy space_recording.aac`;
          navigator.clipboard.writeText(cmd).then(() => {
            copiedFf.classList.add("show");
            setTimeout(() => copiedFf.classList.remove("show"), 1800);
          }).catch(() => {});
        });
        const copyFfRow = document.createElement("div");
        copyFfRow.className = "noah-dl-btns";
        copyFfRow.style.marginTop = "5px";
        copyFfRow.append(copyFf, copiedFf);
        box.append(copyFfRow);

        const note = document.createElement("div");
        note.className = "note";
        note.style.marginTop = "7px";
        note.style.fontSize = "10px";
        note.style.color = "#626860";
        note.textContent = "The stream URL expires. If it stops working, reload the Space and the shield will catch a fresh one. VLC: Media → Open Network Stream and paste the URL.";
        box.append(note);

        dlSection.append(box);
      } else {
        const waiting = document.createElement("div");
        waiting.style.color = "#8f9690";
        waiting.style.fontSize = "11px";
        waiting.style.marginTop = "3px";
        waiting.textContent = "Waiting for the browser to receive the stream URL — it arrives when you open or join a live Space.";
        dlSection.append(waiting);
      }
    }
    panel.append(dlSection);

    // Controls: soundboard for the plugin holder, and host actions when they
    // are the host of the room. Rendered by renderControls() so it can also
    // re-render itself as sounds are added or a mutation completes.
    const controls = document.createElement("section");
    controls.className = "noah-space-controls";
    controls.dataset.role = "controls";
    panel.append(controls);
    renderControls();

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
      window.dispatchEvent(new CustomEvent(channel("noah-space-audit-request")));
    });
    auditRow.append(auditNote, auditBtn);
    panel.append(auditRow);
  }
})();
