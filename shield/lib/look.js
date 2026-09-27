// The look: a picture the person chose (their own or one of noah's), and the
// colors drawn from it for the shield's pages, the new tab page and, on
// Firefox, the browser itself. Runs in pages, never in the worker: it needs a
// canvas to read the picture.
(() => {
  "use strict";
  const Shield = (globalThis.Shield = globalThis.Shield || {});

  Shield.LOOKS = [
    { id: "sea", name: "sea" },
    { id: "halo", name: "halo" },
    { id: "lights", name: "lights" },
    { id: "rings", name: "rings" },
    { id: "meteor", name: "meteor" },
    { id: "glitch", name: "glitch" },
  ];

  // The built-in look: noah's blacks and its one green.
  Shield.DEFAULT_LOOK = {
    id: "noah",
    name: "noah",
    image: null,
    palette: { bg: "#070909", surface: "rgba(8, 11, 9, 0.92)", surface2: "rgba(10, 14, 11, 0.9)", raise: "#0a100c", line: "rgba(180, 210, 190, 0.08)", lineMid: "rgba(180, 210, 190, 0.14)", text: "#d8ddd6", sub: "#b9beb7", muted: "#9aa298", faint: "#6f766e", accent: "#5f8a58", accentBright: "#72a868", accentSoft: "#a9cf9f", bright: "#f1f4ef" },
  };

  const clamp = (value, low, high) => Math.min(high, Math.max(low, value));
  const toHex = (r, g, b) => "#" + [r, g, b].map((channel) => Math.round(clamp(channel, 0, 255)).toString(16).padStart(2, "0")).join("");
  const rgba = (r, g, b, a) => `rgba(${Math.round(r)}, ${Math.round(g)}, ${Math.round(b)}, ${a})`;

  function rgbToHsl(r, g, b) {
    r /= 255; g /= 255; b /= 255;
    const max = Math.max(r, g, b), min = Math.min(r, g, b);
    const l = (max + min) / 2;
    if (max === min) return [0, 0, l];
    const d = max - min;
    const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    let h;
    if (max === r) h = ((g - b) / d + (g < b ? 6 : 0)) / 6;
    else if (max === g) h = ((b - r) / d + 2) / 6;
    else h = ((r - g) / d + 4) / 6;
    return [h, s, l];
  }

  function hslToRgb(h, s, l) {
    if (s === 0) return [l * 255, l * 255, l * 255];
    const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
    const p = 2 * l - q;
    const channel = (t) => {
      if (t < 0) t += 1;
      if (t > 1) t -= 1;
      if (t < 1 / 6) return p + (q - p) * 6 * t;
      if (t < 1 / 2) return q;
      if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
      return p;
    };
    return [channel(h + 1 / 3) * 255, channel(h) * 255, channel(h - 1 / 3) * 255];
  }

  const luminance = (r, g, b) => {
    const lin = (c) => { c /= 255; return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
    return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
  };
  const contrast = (a, b) => { const [l1, l2] = [luminance(...a), luminance(...b)].sort((x, y) => y - x); return (l1 + 0.05) / (l2 + 0.05); };

  // Reads the picture at thumbnail size and buckets its colors: the largest
  // bucket by weight sets the mood, the most saturated one with some weight
  // becomes the accent.
  Shield.paletteFromImage = function paletteFromImage(image) {
    const size = 72;
    const canvas = document.createElement("canvas");
    canvas.width = size;
    canvas.height = size;
    const context = canvas.getContext("2d", { willReadFrequently: true });
    context.drawImage(image, 0, 0, size, size);
    const data = context.getImageData(0, 0, size, size).data;
    const buckets = new Map();
    let sumR = 0, sumG = 0, sumB = 0, count = 0;
    for (let index = 0; index < data.length; index += 4) {
      const r = data[index], g = data[index + 1], b = data[index + 2];
      sumR += r; sumG += g; sumB += b; count++;
      const key = ((r >> 4) << 8) | ((g >> 4) << 4) | (b >> 4);
      const bucket = buckets.get(key) || { r: 0, g: 0, b: 0, n: 0 };
      bucket.r += r; bucket.g += g; bucket.b += b; bucket.n++;
      buckets.set(key, bucket);
    }
    const colors = [...buckets.values()].map((bucket) => {
      const r = bucket.r / bucket.n, g = bucket.g / bucket.n, b = bucket.b / bucket.n;
      const [h, s, l] = rgbToHsl(r, g, b);
      return { r, g, b, h, s, l, n: bucket.n };
    }).sort((left, right) => right.n - left.n);
    const average = { r: sumR / count, g: sumG / count, b: sumB / count };
    const dominant = colors[0] || { ...average, ...Object.fromEntries(["h", "s", "l"].map((key, index) => [key, rgbToHsl(average.r, average.g, average.b)[index]])) };
    // The accent: saturated, not too dark, not too light, and seen enough to matter.
    const accentCandidate = colors
      .filter((color) => color.n >= count * 0.004 && color.s >= 0.25 && color.l >= 0.22 && color.l <= 0.78)
      .sort((left, right) => (right.s * Math.sqrt(right.n)) - (left.s * Math.sqrt(left.n)))[0]
      || colors.filter((color) => color.s >= 0.12).sort((left, right) => right.s - left.s)[0]
      || dominant;
    return Shield.buildPalette(dominant, accentCandidate, average);
  };

  // Turns the two colors into the whole set: blacks that carry the picture's
  // temperature, text that always reads, one accent that is seen only where
  // it matters. The rules are the ones people find easy on the eye: low
  // contrast between the surfaces, high contrast for words, one warm or cool
  // family throughout.
  Shield.buildPalette = function buildPalette(dominant, accent, average) {
    const hue = dominant.s > 0.06 ? dominant.h : accent.h;
    const bgSat = clamp(dominant.s * 0.6, 0.04, 0.35);
    const bg = hslToRgb(hue, bgSat, 0.035);
    const surface = hslToRgb(hue, bgSat, 0.055);
    const surface2 = hslToRgb(hue, bgSat, 0.07);
    const raise = hslToRgb(hue, bgSat, 0.085);
    const textTint = hslToRgb(hue, clamp(bgSat * 0.4, 0.02, 0.12), 0.86);
    const sub = hslToRgb(hue, clamp(bgSat * 0.4, 0.02, 0.1), 0.74);
    const muted = hslToRgb(hue, clamp(bgSat * 0.4, 0.02, 0.1), 0.62);
    const faint = hslToRgb(hue, clamp(bgSat * 0.4, 0.02, 0.1), 0.45);
    const bright = hslToRgb(hue, clamp(bgSat * 0.3, 0.02, 0.08), 0.95);
    let accentL = clamp(accent.l, 0.42, 0.62);
    const accentS = clamp(accent.s, 0.28, 0.72);
    let accentRgb = hslToRgb(accent.h, accentS, accentL);
    // The accent must read against the background on its own.
    let tries = 0;
    while (contrast(accentRgb, bg) < 3.2 && tries++ < 8) {
      accentL = clamp(accentL + 0.05, 0, 0.8);
      accentRgb = hslToRgb(accent.h, accentS, accentL);
    }
    const accentBright = hslToRgb(accent.h, accentS, clamp(accentL + 0.1, 0, 0.85));
    const accentSoft = hslToRgb(accent.h, clamp(accentS * 0.8, 0.2, 0.6), clamp(accentL + 0.25, 0, 0.9));
    const lineBase = hslToRgb(hue, clamp(bgSat * 0.6, 0.05, 0.3), 0.75);
    return {
      bg: toHex(...bg),
      surface: rgba(...surface, 0.92),
      surface2: rgba(...surface2, 0.9),
      raise: toHex(...raise),
      line: rgba(...lineBase, 0.09),
      lineMid: rgba(...lineBase, 0.16),
      text: toHex(...textTint),
      sub: toHex(...sub),
      muted: toHex(...muted),
      faint: toHex(...faint),
      accent: toHex(...accentRgb),
      accentBright: toHex(...accentBright),
      accentSoft: toHex(...accentSoft),
      bright: toHex(...bright),
      hue: Math.round(hue * 360),
      warm: hue < 0.19 || hue > 0.86,
      averageLuminance: average ? luminance(average.r, average.g, average.b) : 0.2,
    };
  };

  // Sets the palette on the page. Called by every shield page on load.
  Shield.applyLook = function applyLook(look) {
    const palette = (look && look.palette) || Shield.DEFAULT_LOOK.palette;
    const root = document.documentElement.style;
    const map = { bg: "--bg", surface: "--surface", surface2: "--surface2", raise: "--raise", line: "--line", lineMid: "--line-mid", text: "--text", sub: "--sub", muted: "--muted", faint: "--faint", accent: "--accent", accentBright: "--accent-bright", accentSoft: "--accent-soft", bright: "--bright" };
    for (const [key, variable] of Object.entries(map)) {
      if (palette[key]) root.setProperty(variable, palette[key]);
    }
    document.documentElement.dataset.look = look && look.id ? look.id : "noah";
  };

  Shield.loadLook = async function loadLook() {
    const stored = await Shield.api.storage.local.get("look");
    return stored.look || Shield.DEFAULT_LOOK;
  };

  // The picture's address for an <img> or a background: a preset from the
  // package, or the data the person uploaded.
  Shield.lookImageUrl = function lookImageUrl(look) {
    if (!look || !look.image) return null;
    if (look.image.startsWith("data:")) return look.image;
    return Shield.api.runtime.getURL("looks/" + look.image);
  };

  // Firefox can wear the look on its frame and toolbars; nothing else can.
  Shield.themeFromPalette = function themeFromPalette(palette) {
    return {
      colors: {
        frame: palette.bg,
        frame_inactive: palette.bg,
        toolbar: palette.raise,
        toolbar_text: palette.text,
        tab_background_text: palette.muted,
        tab_selected: palette.raise,
        tab_line: palette.accent,
        tab_loading: palette.accentBright,
        toolbar_field: palette.bg,
        toolbar_field_text: palette.text,
        toolbar_field_border: palette.lineMid,
        toolbar_field_focus: palette.raise,
        toolbar_field_border_focus: palette.accent,
        toolbar_top_separator: palette.line,
        toolbar_bottom_separator: palette.line,
        popup: palette.raise,
        popup_text: palette.text,
        popup_border: palette.lineMid,
        popup_highlight: palette.accent,
        popup_highlight_text: palette.bright,
        sidebar: palette.bg,
        sidebar_text: palette.text,
        sidebar_border: palette.line,
        ntp_background: palette.bg,
        ntp_text: palette.text,
        icons: palette.sub,
        icons_attention: palette.accentBright,
        button_background_hover: palette.lineMid,
        button_background_active: palette.accent,
      },
      properties: { color_scheme: "dark", content_color_scheme: "dark" },
    };
  };

  if (typeof document !== "undefined" && Shield.api && Shield.api.storage) {
    Shield.loadLook().then((look) => Shield.applyLook(look)).catch(() => {});
    Shield.api.storage.onChanged.addListener((changes, area) => {
      if (area === "local" && changes.look) Shield.applyLook(changes.look.newValue || Shield.DEFAULT_LOOK);
    });
  }
})();
