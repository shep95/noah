"use strict";

const allItems = new Map(); // url → item
let activeFilter = 'all';

const list = document.getElementById('dl-list');
const empty = document.getElementById('dl-empty');
const filterBar = document.getElementById('filter-bar');

function typeIcon(type) {
  if (type === 'video') return '🎬';
  if (type === 'audio') return '🎵';
  if (type === 'image') return '🖼';
  return '📄';
}

function isHls(item) { return /\.m3u8(\?|$)/i.test(item.url) || item.mimeType === 'application/x-mpegURL'; }

function renderItem(item) {
  const div = document.createElement('div');
  div.className = 'dl-item';
  div.dataset.type = item.type;

  // Thumbnail
  const thumb = document.createElement('div');
  thumb.className = 'dl-thumb';
  if (item.type === 'image') {
    const img = document.createElement('img');
    img.src = item.url;
    img.alt = '';
    img.style.cssText = 'width:48px;height:48px;border-radius:4px;object-fit:cover;';
    img.onerror = () => { thumb.textContent = typeIcon('image'); };
    thumb.replaceChildren(img);
  } else {
    thumb.textContent = typeIcon(item.type);
  }

  // Info
  const info = document.createElement('div');
  info.className = 'dl-info';
  const name = document.createElement('div');
  name.className = 'dl-name';
  name.textContent = item.filename || item.url.split('/').pop().split('?')[0] || item.url;
  const meta = document.createElement('div');
  meta.className = 'dl-meta';
  const badge = document.createElement('span');
  badge.className = 'dl-type-badge ' + (isHls(item) ? 'hls' : item.type);
  badge.textContent = isHls(item) ? 'hls stream' : item.type;
  meta.append(badge);
  try {
    const hostname = new URL(item.url).hostname;
    if (item.url.length < 80) {
      const u = document.createElement('span');
      u.style.marginLeft = '6px';
      u.textContent = hostname;
      meta.append(u);
    }
  } catch {}
  info.append(name, meta);

  // Download button
  const btn = document.createElement('button');
  btn.className = 'ghost dl-btn';
  btn.textContent = isHls(item) ? 'get manifest' : 'download';
  btn.addEventListener('click', () => {
    chrome.downloads.download({
      url: item.url,
      filename: 'noah-shield/' + (item.filename || 'download'),
      saveAs: false,
      conflictAction: 'uniquify'
    });
  });

  div.append(thumb, info, btn);
  return div;
}

function updateCounts() {
  const counts = { all: 0, video: 0, audio: 0, image: 0, document: 0 };
  for (const item of allItems.values()) {
    counts.all++;
    if (counts[item.type] !== undefined) counts[item.type]++;
    else counts[item.type] = 1;
  }
  for (const [type, count] of Object.entries(counts)) {
    const el = document.getElementById('count-' + type);
    if (el) el.textContent = count;
  }
}

function render() {
  list.replaceChildren();
  const items = [...allItems.values()].filter(i => activeFilter === 'all' || i.type === activeFilter);
  if (!items.length) {
    empty.hidden = false;
    filterBar.hidden = allItems.size === 0;
    return;
  }
  empty.hidden = true;
  filterBar.hidden = false;
  for (const item of items) list.append(renderItem(item));
  updateCounts();
}

function addItems(items) {
  for (const item of (items || [])) allItems.set(item.url, item);
  render();
  updateCounts();
}

document.getElementById('scan-btn').addEventListener('click', async () => {
  const btn = document.getElementById('scan-btn');
  btn.textContent = 'scanning…';
  btn.disabled = true;
  try {
    const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
    if (!tab) return;
    // Ask background for network-intercepted items
    const net = await chrome.runtime.sendMessage({ type: 'downloader.assets', tabId: tab.id });
    addItems(net && net.assets);
    // Ask content script for DOM-scanned items
    try {
      const dom = await chrome.tabs.sendMessage(tab.id, { type: 'downloader.scan' });
      addItems(dom && dom.items);
    } catch {} // content script may not be injected on chrome:// pages
  } finally {
    btn.textContent = 'scan active tab';
    btn.disabled = false;
  }
});

document.getElementById('clear-btn').addEventListener('click', () => { allItems.clear(); render(); });

document.getElementById('filter-bar').addEventListener('click', e => {
  const btn = e.target.closest('.filter');
  if (!btn) return;
  activeFilter = btn.dataset.type;
  document.querySelectorAll('.filter').forEach(b => b.classList.toggle('active', b === btn));
  render();
});
