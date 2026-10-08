// A frequency under whatever is playing: a pure tone from 20 Hz up, and below
// that a binaural beat (two carriers a few hertz apart, one per ear), because
// no speaker can play 2 Hz and the ear hears the difference instead. Runs in
// a page with an audio context: Chrome's offscreen document, or the tools
// page on Firefox.
(() => {
  "use strict";
  const Shield = (globalThis.Shield = globalThis.Shield || {});

  Shield.TONE_PRESETS = [
    { hz: 2, name: "2 Hz · deep rest (binaural)" },
    { hz: 4, name: "4 Hz · drowsy (binaural)" },
    { hz: 7.83, name: "7.83 Hz · Schumann (binaural)" },
    { hz: 10, name: "10 Hz · calm focus (binaural)" },
    { hz: 40, name: "40 Hz · gamma" },
    { hz: 174, name: "174 Hz" },
    { hz: 285, name: "285 Hz" },
    { hz: 396, name: "396 Hz" },
    { hz: 417, name: "417 Hz" },
    { hz: 432, name: "432 Hz" },
    { hz: 528, name: "528 Hz" },
    { hz: 639, name: "639 Hz" },
    { hz: 741, name: "741 Hz" },
    { hz: 852, name: "852 Hz" },
    { hz: 963, name: "963 Hz" },
  ];
  Shield.TONE_MIN = 2;
  Shield.TONE_MAX = 963;
  const BINAURAL_BELOW = 20;
  const CARRIER = 200;

  let context = null;
  let nodes = null;
  let current = { playing: false, hz: 432, volume: 0.15 };

  function ensureContext() {
    if (!context) context = new (globalThis.AudioContext || globalThis.webkitAudioContext)();
    return context;
  }

  function stopNodes(fadeSeconds = 0.25) {
    if (!nodes) return;
    const { gain, oscillators } = nodes;
    const now = context.currentTime;
    gain.gain.cancelScheduledValues(now);
    gain.gain.setTargetAtTime(0, now, fadeSeconds / 4);
    for (const oscillator of oscillators) oscillator.stop(now + fadeSeconds);
    nodes = null;
  }

  Shield.tonePlay = async function tonePlay(hz, volume) {
    hz = Math.min(Shield.TONE_MAX, Math.max(Shield.TONE_MIN, Number(hz) || 432));
    volume = Math.min(1, Math.max(0, Number(volume) || 0.15));
    const audio = ensureContext();
    if (audio.state === "suspended") await audio.resume();
    stopNodes(0.15);
    const gain = audio.createGain();
    gain.gain.value = 0;
    gain.connect(audio.destination);
    const oscillators = [];
    if (hz < BINAURAL_BELOW) {
      const merger = audio.createChannelMerger(2);
      merger.connect(gain);
      for (const [index, frequency] of [[0, CARRIER - hz / 2], [1, CARRIER + hz / 2]]) {
        const oscillator = audio.createOscillator();
        oscillator.type = "sine";
        oscillator.frequency.value = frequency;
        oscillator.connect(merger, 0, index);
        oscillator.start();
        oscillators.push(oscillator);
      }
    } else {
      const oscillator = audio.createOscillator();
      oscillator.type = "sine";
      oscillator.frequency.value = hz;
      oscillator.connect(gain);
      oscillator.start();
      oscillators.push(oscillator);
    }
    // Quiet by default and eased in; a sine at full scale is unpleasant.
    gain.gain.setTargetAtTime(volume * 0.5, audio.currentTime, 0.4);
    nodes = { gain, oscillators };
    current = { playing: true, hz, volume, binaural: hz < BINAURAL_BELOW };
    return current;
  };

  Shield.toneSetVolume = function toneSetVolume(volume) {
    volume = Math.min(1, Math.max(0, Number(volume) || 0));
    current.volume = volume;
    if (nodes && context) nodes.gain.gain.setTargetAtTime(volume * 0.5, context.currentTime, 0.1);
    return current;
  };

  Shield.toneStop = function toneStop() {
    stopNodes();
    if (context) { context.close().catch(() => {}); context = null; }
    current = { ...current, playing: false };
    return current;
  };

  Shield.toneState = function toneState() {
    return current;
  };

  // Messages from the worker, from the one page that hosts the sound: the
  // offscreen document where the browser has one, otherwise the tools page.
  const hosts = typeof location !== "undefined" && (location.pathname.endsWith("/offscreen.html") || !(globalThis.chrome && globalThis.chrome.offscreen));
  if (hosts && Shield.api && Shield.api.runtime && Shield.api.runtime.onMessage) {
    Shield.api.runtime.onMessage.addListener((message, sender, sendResponse) => {
      if (!message || message.target !== "tone-host") return false;
      if (sender.id !== Shield.api.runtime.id) return false;
      (async () => {
        if (message.action === "play") sendResponse(await Shield.tonePlay(message.hz, message.volume));
        else if (message.action === "volume") sendResponse(Shield.toneSetVolume(message.volume));
        else if (message.action === "stop") sendResponse(Shield.toneStop());
        else sendResponse(Shield.toneState());
      })().catch((error) => sendResponse({ error: String(error && error.message ? error.message : error) }));
      return true;
    });
  }
})();
