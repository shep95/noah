"use strict";

const _found = new Map(); // url → item

function guessType(url) {
  const u = url.split('?')[0].toLowerCase();
  if (/\.(mp4|webm|mkv|mov|avi|m3u8|mpd|ts|m4v)$/.test(u)) return 'video';
  if (/\.(mp3|wav|ogg|flac|aac|m4a|opus|weba)$/.test(u)) return 'audio';
  if (/\.(jpg|jpeg|png|gif|webp|svg|avif|bmp|ico)$/.test(u)) return 'image';
  if (/\.(pdf|docx?|xlsx?|pptx?|zip|rar|7z)$/.test(u)) return 'document';
  return null;
}

function guessMime(url) {
  const u = url.split('?')[0].toLowerCase();
  if (/\.mp4$/.test(u)) return 'video/mp4';
  if (/\.webm$/.test(u)) return 'video/webm';
  if (/\.mp3$/.test(u)) return 'audio/mpeg';
  if (/\.png$/.test(u)) return 'image/png';
  if (/\.jpg$|\.jpeg$/.test(u)) return 'image/jpeg';
  if (/\.gif$/.test(u)) return 'image/gif';
  if (/\.webp$/.test(u)) return 'image/webp';
  if (/\.pdf$/.test(u)) return 'application/pdf';
  if (/\.m3u8$/.test(u)) return 'application/x-mpegURL';
  return '';
}

function guessFilename(url) {
  try {
    const path = new URL(url).pathname;
    const name = path.split('/').filter(Boolean).pop() || 'download';
    return decodeURIComponent(name).slice(0, 120);
  } catch { return 'download'; }
}

function addItem(url) {
  if (!url || url.startsWith('data:') || url.startsWith('blob:') || _found.has(url)) return;
  if (!/^https?:/.test(url)) return;
  const type = guessType(url);
  if (!type) return;
  _found.set(url, { url, type, mimeType: guessMime(url), filename: guessFilename(url) });
}

function scanDom() {
  document.querySelectorAll('video[src],video>source[src]').forEach(el => addItem(el.src || el.getAttribute('src')));
  document.querySelectorAll('audio[src],audio>source[src]').forEach(el => addItem(el.src || el.getAttribute('src')));
  document.querySelectorAll('img[src]').forEach(el => { if (!el.src.startsWith('data:')) addItem(el.src); });
  document.querySelectorAll('a[href]').forEach(el => addItem(el.href));
  document.querySelectorAll('link[rel=preload][href]').forEach(el => addItem(el.href));
  // OG tags
  document.querySelectorAll('meta[property="og:video"],meta[property="og:image"],meta[property="og:audio"]').forEach(el => addItem(el.content));
  // data-src attributes (lazy loading)
  document.querySelectorAll('[data-src],[data-video-src],[data-lazy-src]').forEach(el => {
    addItem(el.getAttribute('data-src') || el.getAttribute('data-video-src') || el.getAttribute('data-lazy-src'));
  });
  flush();
}

function flush() {
  if (_found.size === 0) return;
  chrome.runtime.sendMessage({ type: 'downloader.found', items: [..._found.values()] }).catch(() => {});
}

// Initial scan
scanDom();

// Watch for dynamic additions
const obs = new MutationObserver(() => scanDom());
obs.observe(document.body || document.documentElement, { childList: true, subtree: true, attributes: true, attributeFilter: ['src', 'href', 'data-src'] });

// Also report on request
chrome.runtime.onMessage.addListener((msg, sender, reply) => {
  if (msg.type === 'downloader.scan') { scanDom(); reply({ items: [..._found.values()] }); return true; }
});
