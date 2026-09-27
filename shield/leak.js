"use strict";

const byId = (id) => document.getElementById(id);

async function exitAddress() {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 10000);
  try {
    const response = await fetch(Shield.WHOAMI_URL, { cache: "no-store", credentials: "omit", signal: controller.signal });
    if (!response.ok) throw new Error("answered " + response.status);
    return await response.json();
  } finally {
    clearTimeout(timer);
  }
}

// Every candidate WebRTC offers: with the guard on and the tunnel up, only
// relay or proxied candidates should appear; a local or public address here
// would be a leak.
function webRtcAddresses() {
  return new Promise((resolve) => {
    const found = new Set();
    let connection;
    try {
      connection = new RTCPeerConnection({ iceServers: [{ urls: "stun:stun.l.google.com:19302" }] });
    } catch (error) {
      resolve({ addresses: [], error: String(error.message || error) });
      return;
    }
    connection.createDataChannel("probe");
    connection.onicecandidate = (event) => {
      if (!event.candidate) return;
      const match = /candidate:\S+ \d+ \S+ \d+ (\S+) \d+ typ (\S+)/.exec(event.candidate.candidate);
      if (match) found.add(match[1] + " (" + match[2] + ")");
    };
    connection.createOffer().then((offer) => connection.setLocalDescription(offer)).catch(() => {});
    setTimeout(() => {
      connection.close();
      resolve({ addresses: Array.from(found) });
    }, 4000);
  });
}

async function run() {
  byId("verdict").textContent = "…";
  const state = await Shield.send({ type: "state" });
  const tunnel = state && state.tunnel ? state.tunnel : { state: "off" };
  byId("tunnel").textContent = tunnel.state === "up" ? `up through ${tunnel.server.name}${tunnel.exit ? ", exit " + tunnel.exit.ip + " " + tunnel.exit.country : ""}` : tunnel.state === "off" ? "off" : tunnel.state + (tunnel.error ? ": " + tunnel.error : "");
  let exit = null;
  try {
    exit = await exitAddress();
    byId("exit").textContent = `${exit.ip}${exit.country ? " in " + [exit.city, exit.country].filter(Boolean).join(", ") : ""}`;
  } catch (error) {
    byId("exit").textContent = "no answer (" + String(error.message || error) + ")";
  }
  const rtc = await webRtcAddresses();
  byId("webrtc").textContent = rtc.addresses.length ? rtc.addresses.join(", ") : rtc.error ? "unavailable: " + rtc.error : "none offered";
  byId("locale").textContent = `${navigator.language}, ${Intl.DateTimeFormat().resolvedOptions().timeZone}`;

  const notes = [];
  let verdict = "";
  const publicRtc = rtc.addresses.filter((entry) => /typ (srflx|prflx|host)/.test(entry) && !/\.local|^(10\.|192\.168\.|172\.(1[6-9]|2\d|3[01])\.|127\.|fe80|f[cd][0-9a-f]{2}:)/.test(entry) && !/^[0-9a-f]{8}-/.test(entry));
  if (tunnel.state === "up") {
    if (exit && tunnel.exit && exit.ip !== tunnel.exit.ip) {
      verdict = "The exit changed since the tunnel came up.";
      notes.push("The site now sees " + exit.ip + " where the tunnel check saw " + tunnel.exit.ip + ". Re-check the tunnel.");
    } else if (publicRtc.length) {
      verdict = "WebRTC offers an address the tunnel does not cover.";
      notes.push("Turn the WebRTC guard on in the tunnel section, or check whether another extension controls the WebRTC policy.");
    } else {
      verdict = "No leak found.";
      notes.push("Traffic leaves through the tunnel's exit and WebRTC offers nothing outside it.");
    }
  } else {
    verdict = exit ? "The tunnel is off: sites see " + exit.ip + "." : "The tunnel is off and the exit check found no network.";
    if (publicRtc.length) notes.push("WebRTC also offers " + publicRtc.join(", ") + ".");
  }
  byId("verdict").textContent = verdict;
  byId("notes").textContent = notes.join(" ");
}

byId("again").addEventListener("click", run);
run();
