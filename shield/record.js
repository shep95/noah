// Screen recording with an optional camera bubble, drawn into one picture
// here and written straight to the downloads folder. The WebM the browser
// writes carries the name of the program that wrote it and the time; those
// are blanked before the file is saved, so the recording says nothing about
// where it came from.
"use strict";

const byId = (id) => document.getElementById(id);
const stage = byId("stage");
const context = stage.getContext("2d");
let screenStream = null;
let cameraStream = null;
let micStream = null;
let recorder = null;
let chunks = [];
let lastBlob = null;
let timer = null;
let startedAt = 0;
let drawing = null;
const screenVideo = document.createElement("video");
const cameraVideo = document.createElement("video");
screenVideo.muted = true;
cameraVideo.muted = true;
let audioContext = null;

function settings() {
  return {
    camera: byId("camera-on").checked,
    mic: byId("mic-on").checked,
    system: byId("system-on").checked,
    countdown: byId("countdown").checked,
    corner: byId("corner").value,
    bubble: Number(byId("bubble-size").value),
    shape: byId("bubble-shape").value,
    bitrate: Number(byId("quality").value),
  };
}

function status(text) {
  byId("status").textContent = text;
}

function drawFrame() {
  const width = stage.width;
  const height = stage.height;
  if (screenVideo.readyState >= 2) {
    context.drawImage(screenVideo, 0, 0, width, height);
  } else {
    context.fillStyle = "#050706";
    context.fillRect(0, 0, width, height);
  }
  const options = settings();
  if (options.camera && cameraVideo.readyState >= 2 && cameraVideo.videoWidth) {
    const bubbleWidth = Math.round(width * options.bubble);
    const aspect = cameraVideo.videoWidth / cameraVideo.videoHeight;
    const bubbleHeight = options.shape === "circle" ? bubbleWidth : Math.round(bubbleWidth / aspect);
    const margin = Math.round(width * 0.02);
    const x = options.corner.endsWith("r") ? width - bubbleWidth - margin : margin;
    const y = options.corner.startsWith("b") ? height - bubbleHeight - margin : margin;
    const radius = options.shape === "circle" ? bubbleWidth / 2 : options.shape === "square" ? 0 : Math.round(bubbleWidth * 0.12);
    context.save();
    context.beginPath();
    context.roundRect(x, y, bubbleWidth, bubbleHeight, radius);
    context.closePath();
    context.shadowColor = "rgba(0,0,0,.45)";
    context.shadowBlur = Math.round(width * 0.02);
    context.fillStyle = "#000";
    context.fill();
    context.shadowBlur = 0;
    context.clip();
    // The camera fills the bubble the way object-fit: cover would.
    const sourceAspect = cameraVideo.videoWidth / cameraVideo.videoHeight;
    const targetAspect = bubbleWidth / bubbleHeight;
    let sx = 0, sy = 0, sw = cameraVideo.videoWidth, sh = cameraVideo.videoHeight;
    if (sourceAspect > targetAspect) { sw = sh * targetAspect; sx = (cameraVideo.videoWidth - sw) / 2; }
    else { sh = sw / targetAspect; sy = (cameraVideo.videoHeight - sh) / 2; }
    context.drawImage(cameraVideo, sx, sy, sw, sh, x, y, bubbleWidth, bubbleHeight);
    context.restore();
    context.save();
    context.beginPath();
    context.roundRect(x + 0.5, y + 0.5, bubbleWidth - 1, bubbleHeight - 1, radius);
    context.strokeStyle = "rgba(255,255,255,.35)";
    context.lineWidth = 1;
    context.stroke();
    context.restore();
  }
}

function loop() {
  drawFrame();
  drawing = requestAnimationFrame(loop);
}

function tick() {
  const seconds = Math.floor((Date.now() - startedAt) / 1000);
  const two = (n) => String(n).padStart(2, "0");
  byId("clock").textContent = `${two(Math.floor(seconds / 60))}:${two(seconds % 60)}`;
}

async function countdown() {
  const wrap = stage.parentElement;
  for (const n of [3, 2, 1]) {
    const label = document.createElement("div");
    label.className = "count";
    label.textContent = String(n);
    wrap.append(label);
    await new Promise((resolve) => setTimeout(resolve, 900));
    label.remove();
  }
}

async function start() {
  const options = settings();
  status("waiting for screen picker…");
  byId("result").textContent = "Nothing recorded yet.";
  byId("result-actions").hidden = true;
  if (!navigator.mediaDevices || !navigator.mediaDevices.getDisplayMedia) {
    status("screen recording not available in this window — try reopening the recorder");
    return;
  }
  try {
    screenStream = await navigator.mediaDevices.getDisplayMedia({
      video: { frameRate: { ideal: 30, max: 60 } },
      audio: options.system,
      selfBrowserSurface: "exclude",
      surfaceSwitching: "include",
    });
  } catch (error) {
    const name = error && error.name;
    if (name === "NotAllowedError" || name === "AbortError") {
      status("nothing chosen — press start and pick a screen, window or tab");
    } else {
      status("could not start: " + String(error && (error.message || error)));
    }
    return;
  }
  screenVideo.srcObject = screenStream;
  await screenVideo.play().catch(() => {});
  const track = screenStream.getVideoTracks()[0];
  const size = track.getSettings();
  stage.width = Math.min(size.width || 1280, 2560);
  stage.height = Math.round(stage.width * ((size.height || 720) / (size.width || 1280)));
  byId("stage-hint").hidden = true;
  track.addEventListener("ended", () => stop());

  if (options.camera) {
    try {
      cameraStream = await navigator.mediaDevices.getUserMedia({ video: { width: { ideal: 640 }, facingMode: "user" }, audio: false });
      cameraVideo.srcObject = cameraStream;
      await cameraVideo.play().catch(() => {});
    } catch (error) {
      status("no camera: " + String(error.message || error));
    }
  }
  if (options.mic) {
    try {
      micStream = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true }, video: false });
    } catch (error) {
      status("no microphone: " + String(error.message || error));
    }
  }

  if (!drawing) loop();
  if (options.countdown) await countdown();

  const output = stage.captureStream(30);
  // Microphone and the recorded sound become one track.
  const audioTracks = [...(micStream ? micStream.getAudioTracks() : []), ...(options.system ? screenStream.getAudioTracks() : [])];
  if (audioTracks.length === 1) {
    output.addTrack(audioTracks[0]);
  } else if (audioTracks.length > 1) {
    audioContext = new AudioContext();
    const destination = audioContext.createMediaStreamDestination();
    for (const audioTrack of audioTracks) {
      audioContext.createMediaStreamSource(new MediaStream([audioTrack])).connect(destination);
    }
    output.addTrack(destination.stream.getAudioTracks()[0]);
  }

  const mimeType = ["video/webm;codecs=vp9,opus", "video/webm;codecs=vp8,opus", "video/webm"].find((candidate) => MediaRecorder.isTypeSupported(candidate)) || "";
  chunks = [];
  recorder = new MediaRecorder(output, { mimeType, videoBitsPerSecond: options.bitrate, audioBitsPerSecond: 128000 });
  recorder.ondataavailable = (event) => { if (event.data && event.data.size) chunks.push(event.data); };
  recorder.onstop = finish;
  recorder.start(1000);
  startedAt = Date.now();
  tick();
  timer = setInterval(tick, 500);
  byId("clock").classList.add("live");
  byId("start").hidden = true;
  byId("stop").hidden = false;
  status("recording");
}

function stop() {
  if (recorder && recorder.state !== "inactive") recorder.stop();
  else cleanup();
}

function cleanup() {
  clearInterval(timer);
  timer = null;
  byId("clock").classList.remove("live");
  for (const stream of [screenStream, cameraStream, micStream]) {
    if (stream) for (const track of stream.getTracks()) track.stop();
  }
  screenStream = cameraStream = micStream = null;
  if (audioContext) { audioContext.close().catch(() => {}); audioContext = null; }
  if (drawing) { cancelAnimationFrame(drawing); drawing = null; }
  byId("start").hidden = false;
  byId("stop").hidden = true;
  byId("stage-hint").hidden = false;
}

async function finish() {
  cleanup();
  const raw = new Blob(chunks, { type: chunks[0] ? chunks[0].type : "video/webm" });
  chunks = [];
  status("finishing…");
  const clean = await stripMetadata(raw);
  lastBlob = clean;
  const saved = await save(clean);
  const seconds = Math.round((Date.now() - startedAt) / 1000);
  byId("result").textContent = saved
    ? `Saved: Downloads/noah-shield/recording ${stamp()}.webm · ${(clean.size / 1024 / 1024).toFixed(1)} MB · ${seconds} s. No name, date or program tag inside the file.`
    : "The file could not be saved; press save again.";
  byId("result-actions").hidden = false;
  status("");
}

function stamp() {
  const now = new Date();
  const two = (n) => String(n).padStart(2, "0");
  return `${now.getFullYear()}-${two(now.getMonth() + 1)}-${two(now.getDate())} ${two(now.getHours())}.${two(now.getMinutes())}`;
}

async function save(blob) {
  const url = URL.createObjectURL(blob);
  try {
    // chrome.downloads.download() returns the download id (a number >= 0) on
    // success and throws on failure. In older callback builds it returned
    // undefined; guard both. A download id of -1 signals failure in some paths.
    const id = await Shield.api.downloads.download({
      url,
      filename: `noah-shield/recording ${stamp()}.webm`,
      saveAs: false,
      conflictAction: "uniquify",
    });
    if (typeof id === "number" && id < 0) throw new Error("download id was " + id);
    return true;
  } catch (error) {
    // Surface a plain English reason — the user almost always sees "could not
    // save" when the downloads directory doesn't exist yet or Chrome blocked the
    // blob URL. Creating the directory first is not possible from the extension,
    // but the Downloads API creates the subdirectory automatically.
    const msg = String(error && (error.message || error));
    status("could not save — " + (msg.includes("interrupted") ? "check your Downloads folder is writable" : msg));
    return false;
  } finally {
    setTimeout(() => URL.revokeObjectURL(url), 90000);
  }
}

// ---- WebM metadata ----------------------------------------------------------------------
// EBML: each element is an id, a size, then a payload. The browser writes its
// name into Segment > Info > MuxingApp and WritingApp, and the time into
// DateUTC. Their bytes are blanked in place, so nothing else moves.
const ID_EBML = 0x1a45dfa3, ID_SEGMENT = 0x18538067, ID_INFO = 0x1549a966;
const ID_MUXING = 0x4d80, ID_WRITING = 0x5741, ID_DATE = 0x4461, ID_TITLE = 0x7ba9;

function readVint(bytes, offset, keepMarker) {
  const first = bytes[offset];
  if (first === undefined) return null;
  let length = 1;
  let mask = 0x80;
  while (length <= 8 && !(first & mask)) { mask >>= 1; length += 1; }
  if (length > 8) return null;
  let value = keepMarker ? first : first & (mask - 1);
  for (let index = 1; index < length; index++) value = value * 256 + bytes[offset + index];
  const unknown = !keepMarker && value === Math.pow(2, 7 * length) - 1;
  return { value, length, unknown };
}

function blank(bytes, start, end, fill) {
  for (let index = start; index < end; index++) bytes[index] = fill;
}

function scrub(bytes, start, end) {
  let offset = start;
  let touched = 0;
  while (offset < end) {
    const id = readVint(bytes, offset, true);
    if (!id) break;
    const size = readVint(bytes, offset + id.length, false);
    if (!size) break;
    const payload = offset + id.length + size.length;
    const next = size.unknown ? end : payload + size.value;
    if (id.value === ID_SEGMENT || id.value === ID_INFO) {
      touched += scrub(bytes, payload, Math.min(next, end));
    } else if (id.value === ID_MUXING || id.value === ID_WRITING || id.value === ID_TITLE) {
      blank(bytes, payload, Math.min(next, end), 0x20);
      touched += 1;
    } else if (id.value === ID_DATE) {
      blank(bytes, payload, Math.min(next, end), 0x00);
      touched += 1;
    } else if (id.value === ID_EBML) {
      // The header is fine as it is.
    }
    if (id.value === ID_INFO || (id.value !== ID_SEGMENT && offset > start + 4 * 1024 * 1024)) {
      // Past the Info element there is only media; stop reading.
      if (id.value === ID_INFO) return touched;
    }
    offset = next;
    if (size.unknown) break;
  }
  return touched;
}

async function stripMetadata(blob) {
  // Everything we change sits in the first megabytes; the rest is copied as is.
  const headLength = Math.min(blob.size, 8 * 1024 * 1024);
  const head = new Uint8Array(await blob.slice(0, headLength).arrayBuffer());
  try {
    scrub(head, 0, head.length);
  } catch {
    return blob;
  }
  return new Blob([head, blob.slice(headLength)], { type: blob.type });
}

byId("start").addEventListener("click", start);
byId("stop").addEventListener("click", stop);
byId("save-again").addEventListener("click", () => { if (lastBlob) save(lastBlob); });
for (const id of ["camera-on", "corner", "bubble-size", "bubble-shape"]) byId(id).addEventListener("change", () => { if (!drawing) drawFrame(); });
drawFrame();
window.addEventListener("beforeunload", () => { if (recorder && recorder.state !== "inactive") recorder.stop(); });
