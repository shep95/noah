//! Adapt the noah theme to a wallpaper image's colors, and prepare chosen
//! wallpapers for storage.

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

fn clampf(v: f32, lo: f32, hi: f32) -> f32 {
    v.max(lo).min(hi)
}

fn to_u8(v: f32) -> u8 {
    clampf(v, 0.0, 255.0).round() as u8
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

/// Theme keys whose color carries meaning (errors, warnings, git states,
/// program output in the terminal, collaborators' colors) keep their hue, so a
/// red wallpaper cannot make an error look like success or a failing test
/// print in green.
const SEMANTIC_KEY_PREFIXES: &[&str] = &[
    "error",
    "warning",
    "success",
    "created",
    "deleted",
    "modified",
    "conflict",
    "renamed",
    "version_control.",
    "terminal.ansi.",
    "players",
];

/// At or above this saturation a theme color is a colored role (a syntax
/// color), whose hue is what tells one token from another, rather than a
/// neutral (a layer, a border, the text) that takes on the wallpaper's cast.
const COLORED_ROLE_SATURATION: f32 = 0.2;

/// However vivid the wallpaper, the neutrals stay neutral: the layers remain
/// blacks and the text stays near white.
const MAX_NEUTRAL_SATURATION: f32 = 0.2;

/// How strongly the bundled theme's neutrals are tinted, measured as the
/// saturation of the bundled wallpaper's mid tone. A wallpaper twice as
/// saturated tints them twice as strongly, up to [`MAX_NEUTRAL_SATURATION`].
const BUNDLED_WALLPAPER_SATURATION: f32 = 0.118;

/// Below this saturation a wallpaper is monochrome and its hue is noise, so
/// the neutrals go grey with it instead of turning toward that hue.
const MONOCHROME_SATURATION: f32 = 0.08;

/// Below this saturation the wallpaper's most colorful pixel is too grey to
/// lend the accent a color, and the accent keeps the one it was designed with.
const MIN_ACCENT_SOURCE_SATURATION: f32 = 0.25;

/// Re-tints a theme family (the bundled noah theme, as JSON) toward the colors
/// of `palette`, following shepherd's interface rules: dark layers, one accent.
///
/// Neutrals take the wallpaper's hue, with their saturation capped so they
/// stay blacks and near-whites. The accent (every color equal to the theme's
/// `text.accent`) takes the color of the wallpaper's most colorful pixel.
/// Colored roles and semantic colors keep their hue, because their hue is
/// their meaning. Every color keeps its luminance, so layering, alpha and text
/// contrast stay exactly as designed.
pub fn adaptive_theme_json(template: &str, palette: &WallpaperPalette) -> anyhow::Result<String> {
    let mut theme_family: serde_json::Value = serde_json::from_str(template)?;
    let themes = theme_family
        .get_mut("themes")
        .and_then(|themes| themes.as_array_mut())
        .ok_or_else(|| anyhow::anyhow!("theme family has no themes"))?;
    for theme in themes {
        let Some(style) = theme.get_mut("style").and_then(|style| style.as_object_mut()) else {
            continue;
        };
        let accent = style
            .get("text.accent")
            .and_then(|accent| accent.as_str())
            .and_then(parse_hex_color)
            .map(|(color, _)| color);
        let retint = Retint::new(palette, accent);
        for (key, value) in style.iter_mut() {
            if !SEMANTIC_KEY_PREFIXES
                .iter()
                .any(|prefix| key.starts_with(prefix))
            {
                retint_value(value, &retint);
            }
        }
    }
    Ok(serde_json::to_string(&theme_family)?)
}

struct Retint {
    /// The hue the neutrals turn to, or `None` for a monochrome wallpaper.
    neutral_hue: Option<f32>,
    /// Multiplies a neutral's saturation, before the cap.
    neutral_saturation_scale: f32,
    /// The saturation every neutral is held under on a monochrome wallpaper.
    monochrome_saturation: f32,
    /// The theme's accent, and the hue and saturation it becomes, if the
    /// wallpaper has a color to give it.
    accent: Option<(Rgb, f32, f32)>,
}

impl Retint {
    fn new(palette: &WallpaperPalette, accent: Option<Rgb>) -> Self {
        let (mid_hue, mid_saturation, _) = rgb_to_hsl(palette.mid);
        let (accent_hue, accent_saturation, _) = rgb_to_hsl(palette.accent);
        let accent = accent.and_then(|accent| {
            (accent_saturation >= MIN_ACCENT_SOURCE_SATURATION).then(|| {
                // The accent keeps its designed luminance, which is light, and a
                // light color reads as pastel unless it is fairly saturated; the
                // floor keeps it clearly apart from the text.
                (accent, accent_hue, (accent_saturation * 1.2).clamp(0.5, 0.7))
            })
        });
        Self {
            neutral_hue: (mid_saturation >= MONOCHROME_SATURATION).then_some(mid_hue),
            neutral_saturation_scale: (mid_saturation / BUNDLED_WALLPAPER_SATURATION).min(1.6),
            monochrome_saturation: mid_saturation,
            accent,
        }
    }

    fn apply(&self, color: Rgb) -> Rgb {
        if let Some((accent, hue, saturation)) = self.accent
            && color == accent
        {
            return with_hue_and_saturation(color, hue, saturation);
        }
        let (hue, saturation, _) = rgb_to_hsl(color);
        if saturation >= COLORED_ROLE_SATURATION {
            return color;
        }
        match self.neutral_hue {
            Some(neutral_hue) => with_hue_and_saturation(
                color,
                neutral_hue,
                (saturation * self.neutral_saturation_scale).min(MAX_NEUTRAL_SATURATION),
            ),
            None => with_hue_and_saturation(color, hue, saturation.min(self.monochrome_saturation)),
        }
    }
}

/// Gives `color` a new hue and saturation at the same luminance.
fn with_hue_and_saturation(color: Rgb, hue: f32, saturation: f32) -> Rgb {
    let target_luminance = relative_luminance(color);
    // Lightness is solved for rather than copied: the same HSL lightness is
    // darker in blue than in green, and holding luminance is what keeps every
    // contrast ratio of the original theme.
    let (mut low, mut high) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let lightness = (low + high) / 2.0;
        if relative_luminance(hsl_to_rgb(hue, saturation, lightness)) < target_luminance {
            low = lightness;
        } else {
            high = lightness;
        }
    }
    hsl_to_rgb(hue, saturation, (low + high) / 2.0)
}

fn retint_value(value: &mut serde_json::Value, retint: &Retint) {
    match value {
        serde_json::Value::String(text) => {
            if let Some((color, alpha)) = parse_hex_color(text) {
                *text = hex_color(retint.apply(color), alpha);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                retint_value(item, retint);
            }
        }
        serde_json::Value::Object(map) => {
            for item in map.values_mut() {
                retint_value(item, retint);
            }
        }
        _ => {}
    }
}

fn parse_hex_color(text: &str) -> Option<(Rgb, Option<u8>)> {
    let digits = text.strip_prefix('#')?;
    if !digits.is_ascii() || (digits.len() != 6 && digits.len() != 8) {
        return None;
    }
    let channel = |index: usize| u8::from_str_radix(digits.get(index..index + 2)?, 16).ok();
    let color = Rgb {
        r: channel(0)?,
        g: channel(2)?,
        b: channel(4)?,
    };
    let alpha = if digits.len() == 8 {
        Some(channel(6)?)
    } else {
        None
    };
    Some((color, alpha))
}

fn hex_color(color: Rgb, alpha: Option<u8>) -> String {
    match alpha {
        Some(alpha) => format!("#{:02x}{:02x}{:02x}{:02x}", color.r, color.g, color.b, alpha),
        None => format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b),
    }
}

fn relative_luminance(color: Rgb) -> f32 {
    let linear = |channel: u8| {
        let value = channel as f32 / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}

fn rgb_to_hsl(color: Rgb) -> (f32, f32, f32) {
    let red = color.r as f32 / 255.0;
    let green = color.g as f32 / 255.0;
    let blue = color.b as f32 / 255.0;
    let max = red.max(green).max(blue);
    let min = red.min(green).min(blue);
    let lightness = (max + min) / 2.0;
    let delta = max - min;
    if delta <= f32::EPSILON {
        return (0.0, 0.0, lightness);
    }
    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
    let hue = if max == red {
        60.0 * ((green - blue) / delta).rem_euclid(6.0)
    } else if max == green {
        60.0 * ((blue - red) / delta + 2.0)
    } else {
        60.0 * ((red - green) / delta + 4.0)
    };
    (hue, saturation.min(1.0), lightness)
}

fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> Rgb {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue / 60.0;
    let second = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let (red, green, blue) = match sector as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let offset = lightness - chroma / 2.0;
    Rgb {
        r: to_u8((red + offset) * 255.0),
        g: to_u8((green + offset) * 255.0),
        b: to_u8((blue + offset) * 255.0),
    }
}

/// The longest edge a chosen wallpaper is stored at. Larger images only cost
/// memory and GPU upload time behind a translucent editor.
const MAX_WALLPAPER_EDGE: u32 = 3840;

/// Decodes a chosen wallpaper and re-encodes it as a plain JPEG, which drops
/// every piece of metadata the original carried (EXIF, GPS, camera model,
/// timestamps). The EXIF orientation is applied first so phone photos keep
/// their intended rotation once the tag is gone.
pub fn sanitize_wallpaper_image(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    use image::ImageDecoder as _;

    let mut decoder = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()?
        .into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut image = image::DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    if image.width().max(image.height()) > MAX_WALLPAPER_EDGE {
        image = image.resize(
            MAX_WALLPAPER_EDGE,
            MAX_WALLPAPER_EDGE,
            image::imageops::FilterType::Lanczos3,
        );
    }
    let mut encoded = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 90)
        .encode_image(&image.to_rgb8())?;
    Ok(encoded)
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

    const NOAH_THEME: &str = include_str!("../../../assets/themes/noah/noah.json");

    fn style_color(json: &serde_json::Value, key: &str) -> (Rgb, Option<u8>) {
        json["themes"][0]["style"][key]
            .as_str()
            .and_then(parse_hex_color)
            .expect("color")
    }

    fn contrast(first: Rgb, second: Rgb) -> f32 {
        let (first, second) = (relative_luminance(first), relative_luminance(second));
        (first.max(second) + 0.05) / (first.min(second) + 0.05)
    }

    fn palette_with_mid(mid: Rgb) -> WallpaperPalette {
        WallpaperPalette {
            dark: mid,
            mid,
            light: mid,
            accent: mid,
        }
    }

    fn syntax_color(json: &serde_json::Value, name: &str) -> Rgb {
        json["themes"][0]["style"]["syntax"][name]["color"]
            .as_str()
            .and_then(parse_hex_color)
            .map(|(color, _)| color)
            .expect("syntax color")
    }

    fn hue_distance(first: f32, second: f32) -> f32 {
        let distance = (first - second).rem_euclid(360.0);
        distance.min(360.0 - distance)
    }

    #[test]
    fn warm_wallpaper_warms_neutrals_and_accent_without_turning_text_pink() {
        // The halo wallpaper's measured mid tone and most colorful pixel.
        let palette = WallpaperPalette {
            dark: Rgb { r: 12, g: 10, b: 8 },
            mid: Rgb { r: 97, g: 79, b: 63 },
            light: Rgb { r: 200, g: 180, b: 150 },
            accent: Rgb { r: 107, g: 71, b: 39 },
        };
        let adapted = adaptive_theme_json(NOAH_THEME, &palette).expect("adapts");
        let original: serde_json::Value = serde_json::from_str(NOAH_THEME).expect("json");
        let adapted: serde_json::Value = serde_json::from_str(&adapted).expect("json");
        let (mid_hue, _, _) = rgb_to_hsl(palette.mid);
        let (accent_source_hue, _, _) = rgb_to_hsl(palette.accent);

        for key in ["text", "text.muted", "background", "elevated_surface.background"] {
            let (color, _) = style_color(&adapted, key);
            let (hue, saturation, _) = rgb_to_hsl(color);
            assert!(hue_distance(hue, mid_hue) < 20.0, "{key}: {color:?}");
            assert!(saturation <= MAX_NEUTRAL_SATURATION + 0.02, "{key}: {color:?}");
        }

        let (accent, _) = style_color(&adapted, "text.accent");
        let (accent_hue, accent_saturation, _) = rgb_to_hsl(accent);
        assert!(hue_distance(accent_hue, accent_source_hue) < 10.0, "{accent:?}");
        assert!(accent_saturation >= 0.45, "{accent:?}");
        assert_eq!(style_color(&adapted, "icon.accent").0, accent);

        for name in ["keyword", "string", "function"] {
            assert_eq!(syntax_color(&original, name), syntax_color(&adapted, name), "{name}");
        }
        for key in ["terminal.ansi.red", "terminal.ansi.green"] {
            assert_eq!(style_color(&original, key), style_color(&adapted, key), "{key}");
        }
    }

    #[test]
    fn monochrome_wallpaper_greys_the_neutrals_but_keeps_the_accent() {
        let grey = Rgb {
            r: 98,
            g: 98,
            b: 98,
        };
        let adapted = adaptive_theme_json(NOAH_THEME, &palette_with_mid(grey)).expect("adapts");
        let original: serde_json::Value = serde_json::from_str(NOAH_THEME).expect("json");
        let adapted: serde_json::Value = serde_json::from_str(&adapted).expect("json");

        for key in ["text", "background", "border.focused"] {
            let (color, _) = style_color(&adapted, key);
            let (_, saturation, _) = rgb_to_hsl(color);
            assert!(saturation < 0.01, "{key}: {color:?}");
        }
        assert_eq!(style_color(&original, "text.accent"), style_color(&adapted, "text.accent"));
        assert_eq!(syntax_color(&original, "keyword"), syntax_color(&adapted, "keyword"));
    }

    #[test]
    fn adapts_hue_but_keeps_contrast_and_meaning() {
        let blue = Rgb {
            r: 70,
            g: 96,
            b: 150,
        };
        let adapted = adaptive_theme_json(NOAH_THEME, &palette_with_mid(blue)).expect("adapts");
        let original: serde_json::Value = serde_json::from_str(NOAH_THEME).expect("json");
        let adapted: serde_json::Value = serde_json::from_str(&adapted).expect("json");

        let comment = |json: &serde_json::Value| {
            json["themes"][0]["style"]["syntax"]["comment"]["color"]
                .as_str()
                .and_then(parse_hex_color)
                .expect("comment")
        };
        assert_ne!(comment(&original), comment(&adapted));

        let (background_before, _) = style_color(&original, "background");
        let (background_after, _) = style_color(&adapted, "background");
        let (text_before, _) = style_color(&original, "text");
        let (text_after, _) = style_color(&adapted, "text");
        let before = contrast(text_before, background_before);
        let after = contrast(text_after, background_after);
        assert!((before - after).abs() < 0.3, "{before} vs {after}");

        assert_eq!(style_color(&original, "error"), style_color(&adapted, "error"));
        assert_eq!(style_color(&original, "success"), style_color(&adapted, "success"));
    }

    #[test]
    fn hsl_round_trips() {
        for color in [
            Rgb { r: 0, g: 0, b: 0 },
            Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
            Rgb {
                r: 111,
                g: 157,
                b: 120,
            },
            Rgb { r: 200, g: 40, b: 90 },
        ] {
            let (hue, saturation, lightness) = rgb_to_hsl(color);
            let back = hsl_to_rgb(hue, saturation, lightness);
            assert!(color.r.abs_diff(back.r) <= 1 && color.g.abs_diff(back.g) <= 1 && color.b.abs_diff(back.b) <= 1);
        }
    }

    #[test]
    fn sanitizing_strips_metadata_and_keeps_pixels() {
        let mut png = Vec::new();
        image::DynamicImage::new_rgb8(8, 4)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode");
        let sanitized = sanitize_wallpaper_image(&png).expect("sanitizes");
        let decoded = image::load_from_memory(&sanitized).expect("decodes");
        assert_eq!((decoded.width(), decoded.height()), (8, 4));
        assert_eq!(&sanitized[..2], &[0xff, 0xd8]);
    }

    #[test]
    fn sanitizing_removes_exif_and_gps() {
        let mut jpeg = Vec::new();
        image::DynamicImage::new_rgb8(16, 16)
            .write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
            .expect("encode");
        // An APP1 Exif segment carrying a camera model and a GPS tag marker,
        // spliced in right after the start-of-image marker.
        let mut exif_payload = b"Exif\0\0MM\0*\0\0\0\x08".to_vec();
        exif_payload.extend_from_slice(b"TestCam 9000 GPSLatitude 40.7484");
        let segment_length = u16::try_from(exif_payload.len() + 2).expect("fits");
        let mut with_exif = jpeg[..2].to_vec();
        with_exif.extend_from_slice(&[0xff, 0xe1]);
        with_exif.extend_from_slice(&segment_length.to_be_bytes());
        with_exif.extend_from_slice(&exif_payload);
        with_exif.extend_from_slice(&jpeg[2..]);
        assert!(with_exif.windows(7).any(|window| window == b"TestCam"));

        let sanitized = sanitize_wallpaper_image(&with_exif).expect("sanitizes");
        assert!(!sanitized.windows(7).any(|window| window == b"TestCam"));
        assert!(!sanitized.windows(4).any(|window| window == b"Exif"));
        assert!(!sanitized.windows(3).any(|window| window == b"GPS"));
    }
}
