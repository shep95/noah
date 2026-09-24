//! Derive a coherent dark editor theme from a wallpaper image's colors.
//!
//! The image is decoded elsewhere (the app uses the `image` crate); this module
//! stays dependency-free so it is cheap to compile and unit-test. It takes raw
//! RGBA8 pixels, samples them into dark / mid / light buckets, and emits a
//! partial theme-family JSON string. Chrome and syntax colors are set; every
//! other theme key falls back to defaults during theme refinement, so a short
//! JSON is a valid theme.

/// A sampled color, 0-255 per channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb {
    /// Red channel, 0-255.
    pub r: u8,
    /// Green channel, 0-255.
    pub g: u8,
    /// Blue channel, 0-255.
    pub b: u8,
}

/// The colors distilled from a wallpaper, used to build an adaptive theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallpaperPalette {
    /// Average of the darkest pixels — the basis for backgrounds.
    pub dark: Rgb,
    /// Average of mid-luminance pixels.
    pub mid: Rgb,
    /// Average of the lightest pixels — the basis for text.
    pub light: Rgb,
    /// The most saturated mid-luminance pixel — the basis for the accent.
    pub accent: Rgb,
}

fn luminance(c: Rgb) -> f32 {
    0.2126 * c.r as f32 + 0.7152 * c.g as f32 + 0.0722 * c.b as f32
}

fn mix(from: Rgb, to: Rgb, amount: f32) -> Rgb {
    let channel = |a: u8, b: u8| to_u8(a as f32 + (b as f32 - a as f32) * amount);
    Rgb {
        r: channel(from.r, to.r),
        g: channel(from.g, to.g),
        b: channel(from.b, to.b),
    }
}

fn clampf(v: f32, lo: f32, hi: f32) -> f32 {
    v.max(lo).min(hi)
}

fn to_u8(v: f32) -> u8 {
    clampf(v, 0.0, 255.0).round() as u8
}

fn hex_rgba(c: Rgb, alpha: u8) -> String {
    format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, alpha)
}

/// Sample RGBA8 pixels (4 bytes per pixel) into a wallpaper palette. Robust to
/// empty or malformed input: falls back to a neutral dark-green palette.
pub fn sample_palette(rgba: &[u8]) -> WallpaperPalette {
    let fallback = WallpaperPalette {
        dark: Rgb { r: 10, g: 16, b: 10 },
        mid: Rgb { r: 58, g: 100, b: 73 },
        light: Rgb { r: 196, g: 212, b: 188 },
        accent: Rgb { r: 58, g: 100, b: 73 },
    };
    if rgba.len() < 4 {
        return fallback;
    }

    let pixel_count = rgba.len() / 4;
    // Sample at most ~4000 pixels for speed.
    let step = (pixel_count / 4000).max(1);

    let mut dark = [0f64; 3];
    let mut dark_n = 0u64;
    let mut mid = [0f64; 3];
    let mut mid_n = 0u64;
    let mut light = [0f64; 3];
    let mut light_n = 0u64;
    // Accent = the most saturated mid-luminance pixel seen.
    let mut accent = Rgb { r: 58, g: 100, b: 73 };
    let mut best_sat = -1.0f32;

    let mut i = 0;
    while i < pixel_count {
        let base = i * 4;
        let a = rgba[base + 3];
        if a >= 128 {
            let c = Rgb {
                r: rgba[base],
                g: rgba[base + 1],
                b: rgba[base + 2],
            };
            let lum = luminance(c);
            if lum < 60.0 {
                dark[0] += c.r as f64;
                dark[1] += c.g as f64;
                dark[2] += c.b as f64;
                dark_n += 1;
            } else if lum < 165.0 {
                mid[0] += c.r as f64;
                mid[1] += c.g as f64;
                mid[2] += c.b as f64;
                mid_n += 1;
                let max = c.r.max(c.g).max(c.b) as f32;
                let min = c.r.min(c.g).min(c.b) as f32;
                let sat = if max <= 0.0 { 0.0 } else { (max - min) / max };
                if sat > best_sat {
                    best_sat = sat;
                    accent = c;
                }
            } else {
                light[0] += c.r as f64;
                light[1] += c.g as f64;
                light[2] += c.b as f64;
                light_n += 1;
            }
        }
        i += step;
    }

    let avg = |sum: [f64; 3], n: u64, default: Rgb| -> Rgb {
        if n == 0 {
            default
        } else {
            Rgb {
                r: (sum[0] / n as f64) as u8,
                g: (sum[1] / n as f64) as u8,
                b: (sum[2] / n as f64) as u8,
            }
        }
    };

    WallpaperPalette {
        dark: avg(dark, dark_n, fallback.dark),
        mid: avg(mid, mid_n, fallback.mid),
        light: avg(light, light_n, fallback.light),
        accent,
    }
}

/// Build a partial theme-family JSON (a single dark theme named `name`) whose
/// chrome colors are derived from `palette`. The editor and surface backgrounds
/// are semi-transparent so a wallpaper painted behind the workspace shows
/// through while code stays legible.
pub fn adaptive_theme_json(name: &str, palette: &WallpaperPalette) -> String {
    // Push the background toward near-black while keeping the image's hue tint.
    let bg_base = Rgb {
        r: to_u8(palette.dark.r as f32 * 0.35 + 4.0),
        g: to_u8(palette.dark.g as f32 * 0.40 + 5.0),
        b: to_u8(palette.dark.b as f32 * 0.35 + 4.0),
    };
    let bg_surface = Rgb {
        r: to_u8(bg_base.r as f32 * 1.6 + 4.0),
        g: to_u8(bg_base.g as f32 * 1.6 + 5.0),
        b: to_u8(bg_base.b as f32 * 1.6 + 4.0),
    };
    let bg_elevated = Rgb {
        r: to_u8(bg_surface.r as f32 * 1.4 + 4.0),
        g: to_u8(bg_surface.g as f32 * 1.4 + 5.0),
        b: to_u8(bg_surface.b as f32 * 1.4 + 4.0),
    };
    // Text pulled from the light bucket into a legible range.
    let text = Rgb {
        r: to_u8(clampf(palette.light.r as f32, 170.0, 224.0)),
        g: to_u8(clampf(palette.light.g as f32, 180.0, 230.0)),
        b: to_u8(clampf(palette.light.b as f32, 170.0, 224.0)),
    };
    let text_muted = Rgb {
        r: to_u8(text.r as f32 * 0.62),
        g: to_u8(text.g as f32 * 0.64),
        b: to_u8(text.b as f32 * 0.62),
    };
    // Accent kept in a mid range so it reads against both bg and text.
    let accent = Rgb {
        r: to_u8(clampf(palette.accent.r as f32, 40.0, 150.0)),
        g: to_u8(clampf(palette.accent.g as f32, 60.0, 170.0)),
        b: to_u8(clampf(palette.accent.b as f32, 40.0, 150.0)),
    };
    let border = Rgb {
        r: to_u8((bg_elevated.r as f32 + accent.r as f32) * 0.5),
        g: to_u8((bg_elevated.g as f32 + accent.g as f32) * 0.5),
        b: to_u8((bg_elevated.b as f32 + accent.b as f32) * 0.5),
    };

    // Syntax stays inside the wallpaper's own colors. The one fixed warm note
    // keeps literals distinguishable when an image has almost no hue range.
    let warm = Rgb {
        r: 200,
        g: 176,
        b: 112,
    };
    let keyword = mix(text, accent, 0.45);
    let string = mix(accent, text, 0.35);
    let literal = mix(text, warm, 0.5);
    let type_name = mix(text, accent, 0.25);
    let property = mix(text, palette.mid, 0.25);
    let variable = mix(text, text_muted, 0.3);
    let comment = mix(bg_elevated, text_muted, 0.6);

    // Semi-transparent so the wallpaper shows through the editor/panels.
    let translucent = 0xC0u8;
    let opaque = 0xFFu8;

    format!(
        r##"{{
  "$schema": "https://zed.dev/schema/themes/v0.2.0.json",
  "name": "{name}",
  "author": "#houseofasher / asherin.com",
  "themes": [
    {{
      "name": "{name}",
      "appearance": "dark",
      "style": {{
        "background": "{bg_base_o}",
        "editor.background": "{bg_base_t}",
        "editor.gutter.background": "{bg_base_t}",
        "editor.foreground": "{text_o}",
        "surface.background": "{bg_surface_t}",
        "elevated_surface.background": "{bg_elevated_o}",
        "panel.background": "{bg_surface_t}",
        "status_bar.background": "{bg_base_o}",
        "title_bar.background": "{bg_base_o}",
        "toolbar.background": "{bg_base_t}",
        "tab_bar.background": "{bg_base_o}",
        "tab.active_background": "{bg_base_t}",
        "tab.inactive_background": "{bg_base_o}",
        "terminal.background": "{bg_base_t}",
        "terminal.foreground": "{text_o}",
        "text": "{text_o}",
        "text.muted": "{text_muted_o}",
        "text.accent": "{accent_o}",
        "icon": "{text_o}",
        "icon.muted": "{text_muted_o}",
        "icon.accent": "{accent_o}",
        "border": "{border_o}",
        "border.variant": "{bg_elevated_o}",
        "border.focused": "{accent_o}",
        "border.selected": "{accent_o}",
        "element.hover": "{bg_elevated_o}",
        "element.selected": "{bg_elevated_o}",
        "link_text.hover": "{accent_o}",
        "players": [
          {{ "cursor": "{text_o}", "background": "{text_o}", "selection": "{selection}" }}
        ],
        "syntax": {{
          "keyword": {{ "color": "{keyword}" }},
          "preproc": {{ "color": "{keyword}" }},
          "function": {{ "color": "{text_o}" }},
          "constructor": {{ "color": "{text_o}" }},
          "variant": {{ "color": "{text_o}" }},
          "type": {{ "color": "{type_name}" }},
          "enum": {{ "color": "{type_name}" }},
          "tag": {{ "color": "{type_name}" }},
          "attribute": {{ "color": "{type_name}" }},
          "string": {{ "color": "{string}" }},
          "text.literal": {{ "color": "{string}" }},
          "number": {{ "color": "{literal}" }},
          "boolean": {{ "color": "{literal}" }},
          "constant": {{ "color": "{literal}" }},
          "string.escape": {{ "color": "{literal}" }},
          "string.special": {{ "color": "{literal}" }},
          "property": {{ "color": "{property}" }},
          "variable.parameter": {{ "color": "{property}" }},
          "variable": {{ "color": "{variable}" }},
          "punctuation": {{ "color": "{text_muted_o}" }},
          "operator": {{ "color": "{text_muted_o}" }},
          "comment": {{ "color": "{comment}", "font_style": "italic" }},
          "comment.doc": {{ "color": "{comment}", "font_style": "italic" }},
          "link_text": {{ "color": "{accent_o}", "font_style": "italic" }},
          "title": {{ "color": "{text_o}", "font_weight": 600 }}
        }}
      }}
    }}
  ]
}}"##,
        name = name,
        bg_base_o = hex_rgba(bg_base, opaque),
        bg_base_t = hex_rgba(bg_base, translucent),
        bg_surface_t = hex_rgba(bg_surface, translucent),
        bg_elevated_o = hex_rgba(bg_elevated, opaque),
        text_o = hex_rgba(text, opaque),
        text_muted_o = hex_rgba(text_muted, opaque),
        accent_o = hex_rgba(accent, opaque),
        border_o = hex_rgba(border, opaque),
        selection = hex_rgba(text, 0x24),
        keyword = hex_rgba(keyword, opaque),
        string = hex_rgba(string, opaque),
        literal = hex_rgba(literal, opaque),
        type_name = hex_rgba(type_name, opaque),
        property = hex_rgba(property, opaque),
        variable = hex_rgba(variable, opaque),
        comment = hex_rgba(comment, opaque),
    )
}

/// Decode image bytes (jpeg / png / webp / etc.) and derive a wallpaper palette.
/// Returns `None` if the bytes cannot be decoded as an image.
pub fn palette_from_image_bytes(bytes: &[u8]) -> Option<WallpaperPalette> {
    let image = image::load_from_memory(bytes).ok()?;
    let rgba = image.to_rgba8().into_raw();
    Some(sample_palette(&rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_falls_back() {
        let p = sample_palette(&[]);
        assert_eq!(p.mid, Rgb { r: 58, g: 100, b: 73 });
    }

    #[test]
    fn samples_buckets() {
        // 3 pixels: one dark, one mid-green, one light. RGBA8.
        let px: Vec<u8> = vec![
            5, 8, 5, 255, // dark
            60, 110, 70, 255, // mid green
            210, 220, 205, 255, // light
        ];
        let p = sample_palette(&px);
        assert!(luminance(p.dark) < 60.0);
        assert!(luminance(p.light) >= 165.0);
    }

    #[test]
    fn generates_valid_json() {
        let p = sample_palette(&[60, 110, 70, 255, 5, 8, 5, 255, 210, 220, 205, 255]);
        let json = adaptive_theme_json("noah (adaptive)", &p);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(parsed["name"], "noah (adaptive)");
        assert_eq!(parsed["themes"][0]["appearance"], "dark");
        assert!(
            parsed["themes"][0]["style"]["syntax"]["keyword"]["color"]
                .as_str()
                .is_some_and(|color| color.starts_with('#'))
        );
        assert!(
            parsed["themes"][0]["style"]["editor.background"]
                .as_str()
                .unwrap()
                .starts_with('#')
        );
    }
}
