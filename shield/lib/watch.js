// The watch: what could be recording the screen from inside the browser.
// Pages are handled by content/guard.js (screen and camera capture must be
// allowed per site); this file audits the other installed extensions, which
// is where frame-by-frame screenshot malware usually lives.
"use strict";
(() => {

const Shield = (globalThis.Shield = globalThis.Shield || {});

const CAPTURE_PERMISSIONS = {
  desktopCapture: "can record the whole screen",
  tabCapture: "can record the tab you are watching",
  pageCapture: "can save every page as a file",
  debugger: "can attach the debugger and take screenshots of any tab",
  nativeMessaging: "can talk to a program installed on this computer",
  clipboardRead: "can read what you copy",
  proxy: "can route your traffic through its own server",
  webRequestBlocking: "can rewrite every request",
  history: "can read your whole browsing history",
  management: "can disable other extensions, including this one",
  downloads: "can download files on its own",
  privacy: "can change the browser's privacy settings",
  webNavigation: "sees every page you open",
  cookies: "can read the cookies that keep you signed in",
};

function coversEverything(hostPermissions) {
  return (hostPermissions || []).some((pattern) => pattern === "<all_urls>" || /^\*:\/\/\*\/|^https?:\/\/\*\//.test(pattern));
}

// Two signals together make a screen recorder: permission to see or capture,
// and the reach to do it everywhere. Sideloaded extensions count double: a
// store copy at least passed a review.
function assess(extension) {
  const reasons = [];
  let score = 0;
  const permissions = extension.permissions || [];
  const everywhere = coversEverything(extension.hostPermissions);
  for (const permission of permissions) {
    if (CAPTURE_PERMISSIONS[permission]) {
      reasons.push(CAPTURE_PERMISSIONS[permission]);
      score += permission === "desktopCapture" || permission === "tabCapture" || permission === "debugger" ? 3 : 1;
    }
  }
  if (everywhere) {
    reasons.push("runs on every site");
    score += permissions.includes("scripting") || permissions.includes("webRequest") ? 2 : 1;
  }
  if (extension.installType === "development" || extension.installType === "sideload") {
    reasons.push("was not installed from a store (" + extension.installType + ")");
    score += 2;
  }
  if (extension.installType === "admin") reasons.push("was installed by a policy on this computer");
  let level = "quiet";
  if (score >= 6) level = "watch";
  else if (score >= 3) level = "note";
  return { level, score, reasons };
}

Shield.auditExtensions = async function auditExtensions() {
  const management = Shield.api.management;
  if (!management || !management.getAll) return { supported: false, extensions: [] };
  const self = Shield.api.runtime.id;
  let all = [];
  try {
    all = await management.getAll();
  } catch (error) {
    return { supported: false, error: String(error.message || error), extensions: [] };
  }
  const extensions = all
    .filter((extension) => extension.id !== self && extension.type === "extension")
    .map((extension) => ({
      id: extension.id,
      name: extension.name,
      version: extension.version,
      enabled: extension.enabled,
      installType: extension.installType,
      mayDisable: extension.mayDisable,
      homepage: extension.homepageUrl || "",
      ...assess(extension),
    }))
    .sort((left, right) => right.score - left.score || left.name.localeCompare(right.name));
  return { supported: true, extensions };
};

Shield.setExtensionEnabled = async function setExtensionEnabled(id, enabled) {
  if (id === Shield.api.runtime.id) throw new Error("that is the shield itself");
  console.warn("[noah-shield] setExtensionEnabled called on external extension:", id, "— ensure this is intentional");
  await Shield.api.management.setEnabled(id, enabled);
};

// The list of sites allowed to capture, kept short and exact.
Shield.captureAllowed = function captureAllowed(settings, site) {
  return settings.capture.allowedSites.includes(site);
};
})();
