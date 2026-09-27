// noah on this computer, reached through the browser's native messaging:
// the screen's real brightness and a Tor the shield can start and stop. The
// browser starts noah's command-line program on demand and closes it after
// each answer, so every call here is one short exchange.
"use strict";
(() => {
const Shield = (globalThis.Shield = globalThis.Shield || {});

Shield.HOST_NAME = "com.asherin.noah_shield";

Shield.host = function host(message) {
  return new Promise((resolve) => {
    const runtime = Shield.api.runtime;
    if (!runtime.sendNativeMessage) {
      resolve({ error: "this browser cannot talk to programs on the computer" });
      return;
    }
    try {
      const outcome = runtime.sendNativeMessage(Shield.HOST_NAME, message, (response) => {
        const failure = globalThis.chrome && globalThis.chrome.runtime.lastError;
        if (failure) resolve({ error: plainHostError(failure.message) });
        else resolve(response || {});
      });
      if (outcome && typeof outcome.then === "function") {
        outcome.then((response) => resolve(response || {}), (error) => resolve({ error: plainHostError(String(error && error.message ? error.message : error)) }));
      }
    } catch (error) {
      resolve({ error: plainHostError(String(error && error.message ? error.message : error)) });
    }
  });
};

function plainHostError(text) {
  if (/not found|No such native application|Specified native messaging host not found/i.test(text)) return "noah is not installed on this computer";
  if (/Access to the specified native messaging host is forbidden|not allowed/i.test(text)) return "noah's copy on this computer is older than the shield; open noah once to refresh it";
  return text;
}

// Whether noah answers, remembered for the session so the popup does not
// start a program every time it opens.
Shield.hostInfo = async function hostInfo(fresh = false) {
  const session = await Shield.api.storage.session.get("hostInfo");
  if (!fresh && session.hostInfo && Date.now() - session.hostInfo.at < 10 * 60 * 1000) return session.hostInfo;
  const answer = await Shield.host({ type: "hello" });
  const info = answer.error
    ? { present: false, error: answer.error, at: Date.now() }
    : { present: true, brightness: Boolean(answer.brightness), tor: answer.tor || null, platform: answer.platform || "", version: answer.host || "", at: Date.now() };
  await Shield.api.storage.session.set({ hostInfo: info });
  return info;
};
})();
