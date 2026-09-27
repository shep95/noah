// The tunnel: every request the browser makes goes to one chosen server and
// nowhere else. It is an encrypted proxy (HTTPS or SOCKS5 with remote DNS),
// which is what a browser extension can honestly do; it protects the browser,
// not the rest of the computer. What makes it not leak:
//   * the proxy list has no DIRECT fallback, so when the server is down the
//     requests fail instead of quietly going out on the plain connection;
//   * DNS is resolved by the server (CONNECT for HTTPS proxies, remote lookup
//     for SOCKS5), never on the machine;
//   * WebRTC is held to proxied routes, so it cannot announce the real address;
//   * only private and loopback addresses bypass it, and only if you say so.
"use strict";
(() => {

const Shield = (globalThis.Shield = globalThis.Shield || {});

Shield.BUILTIN_SERVERS = [
  {
    id: "tor-local",
    name: "Tor on this computer",
    kind: "socks5",
    host: "127.0.0.1",
    port: 9050,
    country: "",
    city: "",
    operator: "the Tor network, through the Tor client installed on this computer",
    source: "builtin",
    notes: "Free, worldwide and anonymous. Needs Tor (torproject.org) or the Tor Browser running; the exit country changes on its own.",
  },
];

const HOST_OK = /^(\[[0-9a-fA-F:.]+\]|[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*)$/;
const KINDS = new Set(["https", "http", "socks5"]);

Shield.validateServer = function validateServer(server) {
  if (!server || typeof server !== "object") return "not a server";
  if (!KINDS.has(server.kind)) return "kind must be https, socks5 or http";
  if (typeof server.host !== "string" || !HOST_OK.test(server.host) || server.host.length > 253) return "host is not a host name or address";
  const port = Number(server.port);
  if (!Number.isInteger(port) || port < 1 || port > 65535) return "port must be 1 to 65535";
  if (server.username && (typeof server.username !== "string" || server.username.length > 200)) return "user name too long";
  if (server.password && (typeof server.password !== "string" || server.password.length > 200)) return "password too long";
  if (server.kind === "http" && !Shield.isLocalHost(server.host)) {
    return "a plain http proxy across the internet shows your traffic to anyone on the path; use https or socks5";
  }
  return null;
};

Shield.allServers = async function allServers() {
  const [stored, feed] = await Promise.all([Shield.api.storage.local.get("myServers"), Shield.feedData("servers.json")]);
  const vetted = Array.isArray(feed && feed.servers)
    ? feed.servers.filter((server) => Shield.validateServer(server) === null).map((server) => ({ ...server, source: "vetted" }))
    : [];
  const mine = Array.isArray(stored.myServers) ? stored.myServers : [];
  return [...Shield.BUILTIN_SERVERS, ...vetted, ...mine];
};

Shield.addServer = async function addServer(server) {
  const problem = Shield.validateServer(server);
  if (problem) throw new Error(problem);
  const stored = await Shield.api.storage.local.get("myServers");
  const mine = Array.isArray(stored.myServers) ? stored.myServers : [];
  const entry = {
    id: "mine-" + crypto.randomUUID(),
    name: String(server.name || server.host).slice(0, 80),
    kind: server.kind,
    host: server.host,
    port: Number(server.port),
    username: server.username || "",
    password: server.password || "",
    country: String(server.country || "").slice(0, 2).toUpperCase(),
    city: String(server.city || "").slice(0, 60),
    operator: "you",
    source: "mine",
    notes: String(server.notes || "").slice(0, 300),
  };
  mine.push(entry);
  await Shield.api.storage.local.set({ myServers: mine });
  return entry;
};

Shield.removeServer = async function removeServer(id) {
  const stored = await Shield.api.storage.local.get("myServers");
  const mine = (Array.isArray(stored.myServers) ? stored.myServers : []).filter((server) => server.id !== id);
  await Shield.api.storage.local.set({ myServers: mine });
};

function pacTarget(server) {
  return server.kind === "https" ? `HTTPS ${server.host}:${server.port}` :
    server.kind === "socks5" ? `SOCKS5 ${server.host}:${server.port}` :
    `PROXY ${server.host}:${server.port}`;
}

// Site routes: a host (and its subdomains) through another server, or
// "direct" past the tunnel. Written into the PAC as a plain table.
function pacRoutes(routes, servers, killSwitch) {
  const table = {};
  for (const [site, serverId] of Object.entries(routes || {})) {
    if (serverId === "direct") {
      table[site] = "DIRECT";
      continue;
    }
    const server = servers.find((entry) => entry.id === serverId);
    if (server) table[site] = killSwitch ? pacTarget(server) : `${pacTarget(server)}; DIRECT`;
  }
  return table;
}

function pacFor(server, killSwitch, bypassLocal, routes = {}) {
  const target = pacTarget(server);
  const route = killSwitch ? target : `${target}; DIRECT`;
  // No dnsResolve() here: a lookup from inside the PAC would itself leak the
  // names you visit to the local resolver. Private ranges are matched on the
  // text of the host only.
  return `function FindProxyForURL(url, host) {
  host = String(host).toLowerCase();
  if (${bypassLocal ? "true" : "false"}) {
    if (isPlainHostName(host) || host === "localhost" || shExpMatch(host, "*.localhost") || shExpMatch(host, "*.local")) return "DIRECT";
    if (/^(127\\.|10\\.|192\\.168\\.|169\\.254\\.|0\\.)/.test(host)) return "DIRECT";
    if (/^172\\.(1[6-9]|2[0-9]|3[01])\\./.test(host)) return "DIRECT";
    if (host === "[::1]" || /^\\[f[cd][0-9a-f]{2}:/.test(host) || /^\\[fe80:/.test(host)) return "DIRECT";
  }
  var routes = ${JSON.stringify(routes)};
  var probe = host;
  while (probe) {
    if (Object.prototype.hasOwnProperty.call(routes, probe)) return routes[probe];
    var dot = probe.indexOf(".");
    if (dot < 0) break;
    probe = probe.substring(dot + 1);
  }
  return ${JSON.stringify(route)};
}`;
}

let activeServer = null;
let firefoxListener = null;
// Each apply gets a number; an exit check that finishes after a newer apply
// (the person disconnected while Tor was still not answering) must not write
// its stale verdict over the new state.
let generation = 0;

async function applyChromium(server, settings) {
  const proxy = Shield.api.proxy;
  if (!server) {
    await proxy.settings.clear({ scope: "regular" });
    return;
  }
  const servers = await Shield.allServers();
  const config = {
    mode: "pac_script",
    pacScript: { data: pacFor(server, settings.tunnel.killSwitch, settings.tunnel.bypassLocal, pacRoutes(settings.tunnel.siteRoutes, servers, settings.tunnel.killSwitch)), mandatory: true },
  };
  await proxy.settings.set({ value: config, scope: "regular" });
  const control = await proxy.settings.get({});
  if (control.levelOfControl !== "controlled_by_this_extension") {
    throw new Error("another extension or a policy controls the proxy (" + control.levelOfControl + ")");
  }
}

function proxyInfo(server) {
  const info = {
    type: server.kind === "socks5" ? "socks" : server.kind,
    host: server.host,
    port: server.port,
    proxyDNS: true,
    failoverTimeout: 5,
  };
  if (server.username) {
    info.username = server.username;
    info.password = server.password;
  }
  return info;
}

function firefoxHandler(details) {
  const server = activeServer;
  if (!server) return { type: "direct" };
  const host = Shield.hostOf(details.url);
  if (server.bypassLocal && Shield.isLocalHost(host)) return { type: "direct" };
  let probe = host;
  while (probe) {
    const routed = server.routes && server.routes[probe];
    if (routed === "direct") return { type: "direct" };
    if (routed) return server.killSwitch ? [proxyInfo(routed)] : [proxyInfo(routed), { type: "direct" }];
    const dot = probe.indexOf(".");
    if (dot < 0) break;
    probe = probe.slice(dot + 1);
  }
  const info = proxyInfo(server);
  return server.killSwitch ? [info] : [info, { type: "direct" }];
}

function withRoutes(server, settings, servers) {
  const routes = {};
  for (const [site, serverId] of Object.entries(settings.tunnel.siteRoutes || {})) {
    if (serverId === "direct") routes[site] = "direct";
    else {
      const target = servers.find((entry) => entry.id === serverId);
      if (target) routes[site] = target;
    }
  }
  return { ...server, killSwitch: settings.tunnel.killSwitch, bypassLocal: settings.tunnel.bypassLocal, routes };
}

function applyFirefox(server, settings, servers) {
  activeServer = server ? withRoutes(server, settings, servers) : null;
  if (!firefoxListener && server) {
    firefoxListener = firefoxHandler;
    Shield.api.proxy.onRequest.addListener(firefoxListener, { urls: ["<all_urls>"] });
  }
  if (firefoxListener && !server) {
    Shield.api.proxy.onRequest.removeListener(firefoxListener);
    firefoxListener = null;
  }
}

async function setWebRtc(guarded) {
  const policy = Shield.api.privacy && Shield.api.privacy.network && Shield.api.privacy.network.webRTCIPHandlingPolicy;
  if (!policy) return;
  try {
    if (guarded) await policy.set({ value: "disable_non_proxied_udp" });
    else await policy.clear({});
  } catch (error) {
    console.warn("shield: webrtc policy", error);
  }
}

Shield.tunnelStatus = async function tunnelStatus() {
  const stored = await Shield.api.storage.session.get("tunnelStatus");
  return stored.tunnelStatus || { state: "off" };
};

async function setStatus(status) {
  await Shield.api.storage.session.set({ tunnelStatus: { ...status, at: new Date().toISOString() } });
  return status;
}

// Applies the settings to the browser's proxy configuration, then asks the
// site where the traffic comes out. A failed check with the kill switch on
// leaves the tunnel up: nothing is sent until you turn it off yourself.
Shield.applyTunnel = async function applyTunnel(settings) {
  const servers = await Shield.allServers();
  const server = settings.tunnel.enabled ? servers.find((entry) => entry.id === settings.tunnel.serverId) : null;
  try {
    if (Shield.api.proxy && Shield.api.proxy.onRequest) applyFirefox(server, settings, servers);
    else await applyChromium(server, settings);
  } catch (error) {
    return setStatus({ state: "error", error: String(error.message || error), server });
  }
  activeServer = server ? withRoutes(server, settings, servers) : null;
  const mine = ++generation;
  await setWebRtc(Boolean(server) && settings.tunnel.webRtcGuard);
  if (!server) return setStatus({ state: "off" });
  await setStatus({ state: "checking", server });
  return Shield.checkExit(server, mine);
};

Shield.checkExit = async function checkExit(server, mine = generation) {
  const started = Date.now();
  const settle = (status) => (mine === generation ? setStatus(status) : status);
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 12000);
  try {
    const response = await fetch(Shield.WHOAMI_URL, { cache: "no-store", credentials: "omit", signal: controller.signal });
    if (!response.ok) throw new Error("exit check answered " + response.status);
    const exit = await response.json();
    return settle({
      state: "up",
      server,
      exit: { ip: String(exit.ip || ""), country: String(exit.country || ""), city: String(exit.city || "") },
      latencyMs: Date.now() - started,
      mismatch: Boolean(server.country && exit.country && server.country !== exit.country),
    });
  } catch (error) {
    return settle({
      state: "held",
      server,
      error: controller.signal.aborted ? "no answer through the tunnel in 12 seconds" : String(error.message || error),
    });
  } finally {
    clearTimeout(timer);
  }
};

// Each server in turn carries one exit check; the slowest and the dead ones
// sort last. The tunnel is put back the way it was afterwards.
Shield.speedTest = async function speedTest(settings, serverIds = null) {
  const servers = (await Shield.allServers()).filter((server) => !serverIds || serverIds.includes(server.id));
  const results = [];
  for (const server of servers) {
    const trial = Shield.deepMerge(settings, { tunnel: { enabled: true, serverId: server.id, killSwitch: true } });
    const status = await Shield.applyTunnel(trial);
    results.push({ id: server.id, name: server.name, country: server.country, state: status.state, latencyMs: status.state === "up" ? status.latencyMs : null, exit: status.exit || null, error: status.error || null });
  }
  await Shield.applyTunnel(settings);
  results.sort((left, right) => (left.latencyMs ?? Infinity) - (right.latencyMs ?? Infinity));
  await Shield.api.storage.local.set({ speedTest: { at: new Date().toISOString(), results } });
  return results;
};

// Chromium asks for proxy credentials through onAuthRequired; Firefox takes
// them from the proxy info itself.
Shield.proxyCredentials = function proxyCredentials(details) {
  if (!details.isProxy || !activeServer || !activeServer.username) return {};
  if (details.challenger && details.challenger.host !== activeServer.host) return {};
  return { authCredentials: { username: activeServer.username, password: activeServer.password || "" } };
};

Shield.activeServer = function () {
  return activeServer;
};
})();
