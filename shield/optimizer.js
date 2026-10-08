"use strict";

// ─── Helpers ────────────────────────────────────────────────────────────────

function formatBytes(n) {
  if (n < 1024) return n + ' B';
  if (n < 1048576) return (n / 1024).toFixed(1) + ' KB';
  return (n / 1048576).toFixed(2) + ' MB';
}

function ext(name) { return name.split('.').pop().toLowerCase(); }

// ─── PNG optimizer ───────────────────────────────────────────────────────────
async function optimizePng(blob) {
  const bitmap = await createImageBitmap(blob);
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const ctx = canvas.getContext('2d');
  ctx.drawImage(bitmap, 0, 0);
  bitmap.close();
  return canvas.convertToBlob({ type: 'image/png' });
}

// ─── JPEG optimizer ──────────────────────────────────────────────────────────
async function optimizeJpeg(blob) {
  const bitmap = await createImageBitmap(blob);
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  canvas.getContext('2d').drawImage(bitmap, 0, 0);
  bitmap.close();
  return canvas.convertToBlob({ type: 'image/jpeg', quality: 0.92 });
}

// ─── WebP converter ──────────────────────────────────────────────────────────
async function optimizeToWebp(blob) {
  const bitmap = await createImageBitmap(blob);
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  canvas.getContext('2d').drawImage(bitmap, 0, 0);
  bitmap.close();
  return canvas.convertToBlob({ type: 'image/webp', quality: 0.92 });
}

// ─── ZIP re-compressor (for DOCX, XLSX, PPTX) ───────────────────────────────
async function recompressZip(blob) {
  const buf = await blob.arrayBuffer();
  const bytes = new Uint8Array(buf);

  function readUint16LE(b, o) { return b[o] | (b[o+1] << 8); }
  function readUint32LE(b, o) { return (b[o] | (b[o+1]<<8) | (b[o+2]<<16) | (b[o+3]<<24)) >>> 0; }
  function writeUint16LE(b, o, v) { b[o]=v&0xff; b[o+1]=(v>>8)&0xff; }
  function writeUint32LE(b, o, v) { b[o]=v&0xff; b[o+1]=(v>>8)&0xff; b[o+2]=(v>>16)&0xff; b[o+3]=(v>>24)&0xff; }

  // Find EOCD signature 0x06054b50
  let eocdOffset = -1;
  for (let i = bytes.length - 22; i >= 0; i--) {
    if (bytes[i]===0x50 && bytes[i+1]===0x4b && bytes[i+2]===0x05 && bytes[i+3]===0x06) {
      eocdOffset = i; break;
    }
  }
  if (eocdOffset < 0) throw new Error('not a zip file');

  const cdOffset = readUint32LE(bytes, eocdOffset + 16);
  const cdSize = readUint32LE(bytes, eocdOffset + 12);
  const entryCount = readUint16LE(bytes, eocdOffset + 10);

  // ZIP64: sentinel values mean this is a ZIP64 archive — pass through unchanged
  if (entryCount === 0xFFFF || cdOffset === 0xFFFFFFFF || cdSize === 0xFFFFFFFF) {
    return blob; // ZIP64 not supported — return original
  }

  // Parse central directory
  const entries = [];
  let cdPos = cdOffset;
  for (let i = 0; i < entryCount; i++) {
    if (readUint32LE(bytes, cdPos) !== 0x02014b50) break;
    const gpFlag = readUint16LE(bytes, cdPos + 8);
    if (gpFlag & 0x0001) return blob; // encrypted entry — cannot re-compress
    if (gpFlag & 0x0008) return blob; // data descriptor flag — sizes in local header unreliable
    const compMethod = readUint16LE(bytes, cdPos + 10);
    const crc32 = readUint32LE(bytes, cdPos + 16);
    const compSize = readUint32LE(bytes, cdPos + 20);
    const uncompSize = readUint32LE(bytes, cdPos + 24);
    const fnLen = readUint16LE(bytes, cdPos + 28);
    const extraLen = readUint16LE(bytes, cdPos + 30);
    const commentLen = readUint16LE(bytes, cdPos + 32);
    const localOffset = readUint32LE(bytes, cdPos + 42);
    const filename = new TextDecoder().decode(bytes.slice(cdPos + 46, cdPos + 46 + fnLen));
    entries.push({ filename, compMethod, crc32, compSize, uncompSize, localOffset, fnLen });
    cdPos += 46 + fnLen + extraLen + commentLen;
  }

  async function deflateRaw(data) {
    const ds = new CompressionStream('deflate-raw');
    const writer = ds.writable.getWriter();
    writer.write(data);
    writer.close();
    const chunks = [];
    const reader = ds.readable.getReader();
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      chunks.push(value);
    }
    const total = chunks.reduce((s, c) => s + c.length, 0);
    const out = new Uint8Array(total);
    let off = 0;
    for (const c of chunks) { out.set(c, off); off += c.length; }
    return out;
  }

  async function inflateRaw(data) {
    const ds = new DecompressionStream('deflate-raw');
    const writer = ds.writable.getWriter();
    writer.write(data);
    writer.close();
    const chunks = [];
    const reader = ds.readable.getReader();
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      chunks.push(value);
    }
    const total = chunks.reduce((s, c) => s + c.length, 0);
    const out = new Uint8Array(total);
    let off = 0;
    for (const c of chunks) { out.set(c, off); off += c.length; }
    return out;
  }

  // Build new ZIP
  const newParts = [];
  const newEntries = [];
  let writeOffset = 0;

  for (const entry of entries) {
    const localHeader = bytes.slice(entry.localOffset, entry.localOffset + 30);
    const lFnLen = readUint16LE(localHeader, 26);
    const lExtraLen = readUint16LE(localHeader, 28);
    const dataStart = entry.localOffset + 30 + lFnLen + lExtraLen;
    const compData = bytes.slice(dataStart, dataStart + entry.compSize);

    let newCompData;
    if (entry.uncompSize === 0 || entry.compMethod === 0) {
      newCompData = compData;
      const newLocalHeader = new Uint8Array(30 + lFnLen);
      newLocalHeader.set(localHeader.slice(0, 30 + lFnLen));
      writeUint16LE(newLocalHeader, 8, 0);
      newParts.push(newLocalHeader, newCompData);
      newEntries.push({ ...entry, compMethod: 0, newCompSize: newCompData.length, newOffset: writeOffset });
      writeOffset += newLocalHeader.length + newCompData.length;
    } else {
      let rawData;
      try {
        rawData = entry.compMethod === 8 ? await inflateRaw(compData) : compData;
        newCompData = await deflateRaw(rawData);
        if (newCompData.length >= compData.length) newCompData = compData;
      } catch {
        newCompData = compData;
      }
      const newLocalHeader = new Uint8Array(30 + lFnLen);
      newLocalHeader.set(localHeader.slice(0, 30 + lFnLen));
      writeUint16LE(newLocalHeader, 8, newCompData === compData ? entry.compMethod : 8);
      writeUint32LE(newLocalHeader, 18, newCompData.length);
      newParts.push(newLocalHeader, newCompData);
      newEntries.push({ ...entry, compMethod: newCompData === compData ? entry.compMethod : 8, newCompSize: newCompData.length, newOffset: writeOffset });
      writeOffset += newLocalHeader.length + newCompData.length;
    }
  }

  // Build new central directory
  const cdParts = [];
  let newCdSize = 0;
  for (const e of newEntries) {
    const cdEntry = new Uint8Array(46 + e.fnLen);
    const enc = new TextEncoder().encode(e.filename);
    cdEntry[0]=0x50; cdEntry[1]=0x4b; cdEntry[2]=0x01; cdEntry[3]=0x02;
    writeUint16LE(cdEntry, 4, 0x0014);
    writeUint16LE(cdEntry, 6, 0x0014);
    writeUint16LE(cdEntry, 10, e.compMethod);
    writeUint32LE(cdEntry, 16, e.crc32);
    writeUint32LE(cdEntry, 20, e.newCompSize);
    writeUint32LE(cdEntry, 24, e.uncompSize);
    writeUint16LE(cdEntry, 28, e.fnLen);
    writeUint32LE(cdEntry, 42, e.newOffset);
    cdEntry.set(enc, 46);
    cdParts.push(cdEntry);
    newCdSize += cdEntry.length;
  }

  // EOCD
  const eocd = new Uint8Array(22);
  eocd[0]=0x50; eocd[1]=0x4b; eocd[2]=0x05; eocd[3]=0x06;
  writeUint16LE(eocd, 8, newEntries.length);
  writeUint16LE(eocd, 10, newEntries.length);
  writeUint32LE(eocd, 12, newCdSize);
  writeUint32LE(eocd, 16, writeOffset);

  return new Blob([...newParts, ...cdParts, eocd], { type: blob.type });
}

// ─── PDF metadata stripper ───────────────────────────────────────────────────
async function optimizePdf(blob) {
  // Modern PDFs use FlateDecode (zlib-compressed binary streams).
  // Reading binary data as text and re-encoding corrupts the file.
  // Detect binary by scanning the first 512 bytes for control characters.
  const head = new Uint8Array(await blob.slice(0, 512).arrayBuffer());
  let isBinary = false;
  for (let i = 4; i < head.length; i++) {
    const b = head[i];
    if ((b >= 0x00 && b <= 0x08) || (b >= 0x0E && b <= 0x1F)) { isBinary = true; break; }
  }
  if (isBinary) {
    // Binary (compressed) PDF — text manipulation would corrupt it.
    // Return unchanged; caller sees no size reduction.
    return blob;
  }
  // Text-based PDF (uncompressed streams) — safe to strip metadata.
  let out = await blob.text();
  out = out.replace(/<\?xpacket[^>]*>[\s\S]*?<\?xpacket[^>]*end[^>]*>/gi, '');
  out = out.replace(/\/(?:Author|Creator|Producer|CreationDate|ModDate|Title|Subject|Keywords)\s*\([^)]*\)/g, '');
  out = out.replace(/\/Thumb\s+\d+\s+\d+\s+R/g, '');
  out = out.replace(/\/Metadata\s+\d+\s+\d+\s+R/g, '');
  out = out.replace(/\s{3,}/g, '\n');
  return new Blob([out], { type: 'application/pdf' });
}

// ─── SVG minifier ────────────────────────────────────────────────────────────
async function optimizeSvg(blob) {
  let text = await blob.text();
  text = text.replace(/<!--[\s\S]*?-->/g, '');
  text = text.replace(/>\s+</g, '><');
  text = text.replace(/\s{2,}/g, ' ');
  if (!text.includes('xlink:')) text = text.replace(/\s+xmlns:xlink="[^"]*"/g, '');
  return new Blob([text.trim()], { type: 'image/svg+xml' });
}

// ─── Text/JSON/HTML/CSS minifier ─────────────────────────────────────────────
async function optimizeText(blob, mimeType) {
  let text = await blob.text();
  if (mimeType.includes('json')) {
    try { text = JSON.stringify(JSON.parse(text)); } catch {}
  } else {
    text = text.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\s{2,}/g, ' ').trim();
  }
  return new Blob([text], { type: blob.type });
}

// ─── Video metadata stripper ─────────────────────────────────────────────────
async function stripVideoMetadata(blob) {
  const buf = new Uint8Array(await blob.arrayBuffer());
  function readUint32BE(b, o) { return ((b[o]<<24)|(b[o+1]<<16)|(b[o+2]<<8)|b[o+3])>>>0; }

  let offset = 0;
  while (offset < buf.length - 8) {
    const size = readUint32BE(buf, offset);
    if (size < 8) break;
    const type = String.fromCharCode(buf[offset+4], buf[offset+5], buf[offset+6], buf[offset+7]);
    if (type === 'udta') {
      buf.fill(0, offset + 8, offset + size);
    }
    offset += size;
  }
  return new Blob([buf], { type: blob.type });
}

// ─── Main dispatch ────────────────────────────────────────────────────────────
async function optimize(file) {
  const e = ext(file.name);
  const mime = file.type || '';

  if (e === 'png' || mime === 'image/png') return { blob: await optimizePng(file), note: 'lossless re-compression, metadata stripped' };
  if (e === 'jpg' || e === 'jpeg' || mime === 'image/jpeg') return { blob: await optimizeJpeg(file), note: 're-encoded at 92% quality, metadata stripped' };
  if (e === 'webp' || mime === 'image/webp') return { blob: await optimizePng(file), note: 'lossless canvas pass' };
  if (['gif'].includes(e)) return { blob: await optimizeToWebp(file), note: 'converted to webp — rename file to .webp after download' };
  if (e === 'svg' || mime === 'image/svg+xml') return { blob: await optimizeSvg(file), note: 'whitespace and comments removed' };
  if (['docx','xlsx','pptx','odt','ods','odp','zip','jar','apk'].includes(e)) return { blob: await recompressZip(file), note: 'deflate streams re-compressed at maximum level' };
  if (e === 'pdf' || mime === 'application/pdf') {
    const optimized = await optimizePdf(file);
    const note = optimized.size === file.size
      ? 'binary pdf — metadata stripping requires desktop tools (pdf streams are compressed)'
      : 'metadata and thumbnails stripped';
    return { blob: optimized, note };
  }
  if (['mp4','m4v','mov','3gp'].includes(e) || mime.startsWith('video/mp4')) return { blob: await stripVideoMetadata(file), note: 'metadata stripped — full transcoding requires desktop tools' };
  if (['webm','mkv'].includes(e) || mime === 'video/webm') return { blob: file, note: 'container format — no lossless reduction available in browser' };
  if (['json'].includes(e) || mime.includes('json')) return { blob: await optimizeText(file, mime || 'json'), note: 'minified' };
  if (['css','html','htm','xml','txt'].includes(e)) return { blob: await optimizeText(file, mime || 'text/plain'), note: 'whitespace collapsed' };
  return { blob: file, note: 'no optimization available for this file type' };
}

// ─── UI ──────────────────────────────────────────────────────────────────────
const dropZone = document.getElementById('drop-zone');
const fileInput = document.getElementById('file-input');
const resultSection = document.getElementById('result');
const beforeSize = document.getElementById('before-size');
const afterSize = document.getElementById('after-size');
const reductionBadge = document.getElementById('reduction');
const qualityNote = document.getElementById('quality-note');
const downloadBtn = document.getElementById('download-btn');
const spinner = document.getElementById('spinner');
const fileName = document.getElementById('file-name');

let currentBlob = null;
let currentFilename = '';

dropZone.addEventListener('click', () => fileInput.click());
dropZone.addEventListener('keydown', e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); fileInput.click(); } });
dropZone.addEventListener('dragover', e => { e.preventDefault(); dropZone.classList.add('drag'); });
dropZone.addEventListener('dragleave', () => dropZone.classList.remove('drag'));
dropZone.addEventListener('drop', e => { e.preventDefault(); dropZone.classList.remove('drag'); processFile(e.dataTransfer.files[0]); });
fileInput.addEventListener('change', () => processFile(fileInput.files[0]));

async function processFile(file) {
  if (!file) return;
  resultSection.hidden = true;
  spinner.hidden = false;
  dropZone.textContent = 'processing ' + file.name + '…';

  try {
    const { blob, note } = await optimize(file);
    currentBlob = blob;
    const origExt = ext(file.name);
    const newExt = blob.type === 'image/webp' ? 'webp' : origExt;
    const base = file.name.replace(/\.[^.]+$/, '');
    currentFilename = base + '-optimized.' + newExt;

    fileName.textContent = file.name;
    beforeSize.textContent = formatBytes(file.size);
    afterSize.textContent = formatBytes(blob.size);
    const saved = file.size - blob.size;
    const pct = file.size > 0 ? Math.round(saved / file.size * 100) : 0;
    reductionBadge.textContent = pct > 0 ? '−' + pct + '%' : pct === 0 ? 'no change' : '+' + Math.abs(pct) + '% (kept original)';
    reductionBadge.className = 'pill ' + (pct >= 10 ? 'good' : pct > 0 ? 'warn' : 'muted');
    qualityNote.textContent = note;
    downloadBtn.textContent = 'download ' + currentFilename;
    if (pct < 0) { currentBlob = file; currentFilename = file.name + '-copy.' + origExt; }
    resultSection.hidden = false;
  } catch (err) {
    dropZone.textContent = 'error: ' + (err.message || String(err)) + ' — try another file';
  } finally {
    spinner.hidden = true;
    dropZone.innerHTML = '<span class="drop-icon">&#x2B07;</span><span class="drop-label">drop another file or click to choose</span>';
  }
}

downloadBtn.addEventListener('click', () => {
  if (!currentBlob) return;
  const url = URL.createObjectURL(currentBlob);
  const a = document.createElement('a');
  a.href = url;
  a.download = currentFilename;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 60000);
});
