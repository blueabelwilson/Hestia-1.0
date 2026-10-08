//! Colour helpers: pulling colours out of an image and turning them into a theme palette.

use crate::config::{AccentSource, ColorMode, LookConfig, Palette, Rgb};

// ---------------------------------------------------------------------------
// HSL
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Hsl {
    pub h: f32, // 0..360
    pub s: f32, // 0..1
    pub l: f32, // 0..1
}

pub fn to_hsl(c: Rgb) -> Hsl {
    let r = c.0 as f32 / 255.0;
    let g = c.1 as f32 / 255.0;
    let b = c.2 as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d < 1e-6 {
        return Hsl { h: 0.0, s: 0.0, l };
    }
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        ((g - b) / d + if g < b { 6.0 } else { 0.0 }) * 60.0
    } else if max == g {
        ((b - r) / d + 2.0) * 60.0
    } else {
        ((r - g) / d + 4.0) * 60.0
    };
    Hsl { h, s, l }
}

pub fn from_hsl(h: Hsl) -> Rgb {
    let s = h.s.clamp(0.0, 1.0);
    let l = h.l.clamp(0.0, 1.0);
    if s < 1e-6 {
        let v = (l * 255.0).round() as u8;
        return Rgb(v, v, v);
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hk = (h.h.rem_euclid(360.0)) / 360.0;
    let f = |t: f32| {
        let t = t.rem_euclid(1.0);
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (v * 255.0).round().clamp(0.0, 255.0) as u8
    };
    Rgb(f(hk + 1.0 / 3.0), f(hk), f(hk - 1.0 / 3.0))
}

fn with(c: Rgb, s: Option<f32>, l: Option<f32>) -> Rgb {
    let mut h = to_hsl(c);
    if let Some(s) = s {
        h.s = s;
    }
    if let Some(l) = l {
        h.l = l;
    }
    from_hsl(h)
}

pub fn luminance(c: Rgb) -> f32 {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2)
}

pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// White or near-black, whichever reads better on `bg`.
pub fn readable_on(bg: Rgb) -> Rgb {
    let white = Rgb(255, 255, 255);
    let dark = Rgb(0x16, 0x16, 0x16);
    if contrast(white, bg) >= contrast(dark, bg) {
        white
    } else {
        dark
    }
}

pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

// ---------------------------------------------------------------------------
// Image colour extraction (k-means on a small thumbnail)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Swatch {
    pub color: Rgb,
    /// Share of the image covered by this colour, 0..1.
    pub weight: f32,
}

/// Returns up to `k` dominant colours, most common first.
pub fn extract_colors(img: &image::RgbaImage, k: usize) -> Vec<Swatch> {
    let thumb = image::imageops::thumbnail(img, 64, 64);
    let pixels: Vec<[f32; 3]> =
        thumb.pixels().filter(|p| p.0[3] > 128).map(|p| [p.0[0] as f32, p.0[1] as f32, p.0[2] as f32]).collect();
    if pixels.is_empty() {
        return vec![];
    }
    let k = k.min(pixels.len()).max(1);
    // Deterministic start: spread over pixels sorted by brightness.
    let mut sorted = pixels.clone();
    sorted.sort_by(|a, b| (a[0] + a[1] + a[2]).partial_cmp(&(b[0] + b[1] + b[2])).unwrap());
    let mut centers: Vec<[f32; 3]> = (0..k).map(|i| sorted[(i * (sorted.len() - 1)) / (k.max(2) - 1).max(1)]).collect();
    let mut assign = vec![0usize; pixels.len()];
    for _ in 0..12 {
        for (i, p) in pixels.iter().enumerate() {
            let mut best = 0;
            let mut bd = f32::MAX;
            for (j, c) in centers.iter().enumerate() {
                let d = (p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2);
                if d < bd {
                    bd = d;
                    best = j;
                }
            }
            assign[i] = best;
        }
        let mut sums = vec![[0f32; 4]; k];
        for (i, p) in pixels.iter().enumerate() {
            let s = &mut sums[assign[i]];
            s[0] += p[0];
            s[1] += p[1];
            s[2] += p[2];
            s[3] += 1.0;
        }
        for (j, s) in sums.iter().enumerate() {
            if s[3] > 0.0 {
                centers[j] = [s[0] / s[3], s[1] / s[3], s[2] / s[3]];
            }
        }
    }
    let mut counts = vec![0f32; k];
    for a in &assign {
        counts[*a] += 1.0;
    }
    let total = pixels.len() as f32;
    let mut out: Vec<Swatch> = centers
        .iter()
        .zip(counts.iter())
        .filter(|(_, n)| **n > 0.0)
        .map(|(c, n)| Swatch { color: Rgb(c[0].round() as u8, c[1].round() as u8, c[2].round() as u8), weight: n / total })
        .collect();
    out.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap());
    out
}

/// The most "eye-catching" colour: saturated, not too dark or light, reasonably common.
pub fn vibrant(swatches: &[Swatch]) -> Option<Rgb> {
    swatches
        .iter()
        .map(|s| {
            let h = to_hsl(s.color);
            let light_ok = 1.0 - ((h.l - 0.55).abs() * 2.0).min(1.0);
            (s.color, h.s * 2.0 + light_ok + s.weight.sqrt())
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|x| x.0)
}

/// Second accent: the vibrant-ish colour whose hue is furthest from `accent`.
fn secondary(swatches: &[Swatch], accent: Rgb) -> Rgb {
    let ah = to_hsl(accent).h;
    swatches
        .iter()
        .map(|s| {
            let h = to_hsl(s.color);
            let dh = ((h.h - ah).abs()).min(360.0 - (h.h - ah).abs()) / 180.0;
            (s.color, dh + h.s)
        })
        .filter(|(c, _)| *c != accent)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|x| x.0)
        .unwrap_or_else(|| {
            let mut h = to_hsl(accent);
            h.h += 150.0;
            from_hsl(h)
        })
}

/// Build a full palette from the image swatches, the chosen mode and accent.
pub fn generate(swatches: &[Swatch], mode: ColorMode, accent: Rgb) -> Palette {
    let base = swatches.first().map(|s| s.color).unwrap_or(Rgb(0x28, 0x16, 0x57));
    let base_h = to_hsl(base);
    let acc_h = to_hsl(accent);
    // Make sure the accent is lively enough to stand out.
    let accent = with(accent, Some(acc_h.s.max(0.45)), Some(acc_h.l.clamp(0.42, 0.62)));
    let second = secondary(swatches, accent);
    let second = with(second, Some(to_hsl(second).s.max(0.4)), Some(to_hsl(second).l.clamp(0.45, 0.65)));

    let (background, background_alt) = match mode {
        ColorMode::Dark => {
            let s = (base_h.s * 0.6).min(0.45);
            let bg = from_hsl(Hsl { h: base_h.h, s, l: 0.09 });
            let alt = from_hsl(Hsl { h: base_h.h, s: (s + 0.1).min(0.5), l: 0.17 });
            (bg, alt)
        }
        ColorMode::Colour => {
            let s = base_h.s.max(0.35).min(0.75);
            let bg = from_hsl(Hsl { h: base_h.h, s, l: 0.22 });
            let alt = from_hsl(Hsl { h: base_h.h, s, l: 0.32 });
            (bg, alt)
        }
        ColorMode::Light => {
            let s = (base_h.s * 0.5).min(0.35);
            let bg = from_hsl(Hsl { h: base_h.h, s, l: 0.94 });
            let alt = from_hsl(Hsl { h: base_h.h, s, l: 0.86 });
            (bg, alt)
        }
    };
    let foreground = match mode {
        ColorMode::Light => from_hsl(Hsl { h: base_h.h, s: 0.15, l: 0.12 }),
        _ => readable_on(background),
    };
    let urgent = with(accent, None, Some((to_hsl(accent).l * 0.7).max(0.25)));
    Palette { background, background_alt, foreground, selected: accent, active: second, urgent }
}

/// Text colour to put on top of the `selected` colour.
pub fn on_selected(p: &Palette) -> Rgb {
    if contrast(p.foreground, p.selected) >= 3.0 {
        p.foreground
    } else {
        readable_on(p.selected)
    }
}

// ---------------------------------------------------------------------------
// Windows accent
// ---------------------------------------------------------------------------

pub fn windows_accent() -> Option<Rgb> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Software\\Microsoft\\Windows\\DWM").ok()?;
    // Stored as 0xAABBGGRR.
    let v: u32 = key.get_value("AccentColor").ok()?;
    Some(Rgb((v & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, ((v >> 16) & 0xFF) as u8))
}

// ---------------------------------------------------------------------------
// Presets (from adi1090x/rofi, GPL-3.0, plus the well-known schemes it ships)
// ---------------------------------------------------------------------------

pub struct PalettePreset {
    pub id: &'static str,
    pub name: &'static str,
    pub colors: [u32; 6],
}

pub const PRESETS: &[PalettePreset] = &[
    PalettePreset { id: "t6-1", name: "Portal Magenta", colors: [0x201A41, 0x392684, 0xFFFFFF, 0xF801E8, 0x00CCF5, 0x8D0083] },
    PalettePreset { id: "t6-2", name: "Violet Night", colors: [0x180F39, 0x32197D, 0xFFFFFF, 0xFF00F1, 0x9878FF, 0x7D0075] },
    PalettePreset { id: "t6-3", name: "Deep Blue", colors: [0x09164C, 0x102886, 0xFFFFFF, 0xFA00E9, 0x3860FF, 0xBB00AF] },
    PalettePreset { id: "t6-4", name: "Ember", colors: [0x2D1B14, 0x462D23, 0xFFFFFF, 0xE25F3E, 0x7B6C5B, 0x934A1C] },
    PalettePreset { id: "t6-5", name: "Moss", colors: [0x231419, 0x2D1E23, 0xFFFFFF, 0x426647, 0x2E3F34, 0xD08261] },
    PalettePreset { id: "t6-6", name: "Paper Grey", colors: [0xD0D0D0, 0xE9E9E9, 0x161616, 0xBEBEBE, 0x999999, 0x808080] },
    PalettePreset { id: "t6-7", name: "Charcoal", colors: [0x101010, 0x252525, 0xFFFFFF, 0x505050, 0x909090, 0x707070] },
    PalettePreset { id: "t6-8", name: "Midnight Neon", colors: [0x030B16, 0x0A1B37, 0xFFFFFF, 0xCB43A6, 0x095873, 0x2FC6D8] },
    PalettePreset { id: "t6-9", name: "Teal Dusk", colors: [0x131D1F, 0x183A43, 0xFFFFFF, 0x649094, 0xE9CC9D, 0xFEA861] },
    PalettePreset { id: "t6-10", name: "Synthwave", colors: [0x11092D, 0x281657, 0xFFFFFF, 0xDF5296, 0x6E77FF, 0x8E3596] },
    PalettePreset { id: "adapta", name: "Adapta", colors: [0x222D32, 0x29353B, 0xB8C2C6, 0x00BCD4, 0x21FF90, 0xFF4B60] },
    PalettePreset { id: "arc", name: "Arc", colors: [0x2F343F, 0x383C4A, 0xBAC5D0, 0x5294E2, 0x98C379, 0xE06B74] },
    PalettePreset { id: "black", name: "Black", colors: [0x000000, 0x101010, 0xFFFFFF, 0x62AEEF, 0x98C379, 0xE06B74] },
    PalettePreset { id: "catppuccin", name: "Catppuccin", colors: [0x1E1D2F, 0x282839, 0xD9E0EE, 0x7AA2F7, 0xABE9B3, 0xF28FAD] },
    PalettePreset { id: "cyberpunk", name: "Cyberpunk", colors: [0x000B1E, 0x0A1528, 0x0ABDC6, 0x0ABDC6, 0x00FF00, 0xFF0000] },
    PalettePreset { id: "dracula", name: "Dracula", colors: [0x1E1F29, 0x282A36, 0xFFFFFF, 0xBD93F9, 0x50FA7B, 0xFF5555] },
    PalettePreset { id: "everforest", name: "Everforest", colors: [0x323D43, 0x3C474D, 0xDAD1BE, 0x7FBBB3, 0xA7C080, 0xE67E80] },
    PalettePreset { id: "gruvbox", name: "Gruvbox", colors: [0x282828, 0x353535, 0xEBDBB2, 0x83A598, 0xB8BB26, 0xFB4934] },
    PalettePreset { id: "lovelace", name: "Lovelace", colors: [0x1D1F28, 0x282A36, 0xFDFDFD, 0x79E6F3, 0x5ADECD, 0xF37F97] },
    PalettePreset { id: "navy", name: "Navy", colors: [0x021B21, 0x0C252B, 0xF2F1B9, 0x44B5B1, 0x7CBF9E, 0xC2454E] },
    PalettePreset { id: "nord", name: "Nord", colors: [0x2E3440, 0x383E4A, 0xE5E9F0, 0x81A1C1, 0xA3BE8C, 0xBF616A] },
    PalettePreset { id: "onedark", name: "One Dark", colors: [0x1E2127, 0x282B31, 0xFFFFFF, 0x61AFEF, 0x98C379, 0xE06C75] },
    PalettePreset { id: "paper", name: "Paper", colors: [0xF1F1F1, 0xE0E0E0, 0x252525, 0x008EC4, 0x10A778, 0xC30771] },
    PalettePreset { id: "solarized", name: "Solarized", colors: [0x002B36, 0x073642, 0xEEE8D5, 0x268BD2, 0x859900, 0xDC322F] },
    PalettePreset { id: "tokyonight", name: "Tokyo Night", colors: [0x15161E, 0x1A1B26, 0xC0CAF5, 0x33467C, 0x414868, 0xF7768E] },
    PalettePreset { id: "yousai", name: "Yousai", colors: [0xF5E7DE, 0xEBDCD2, 0x34302D, 0xD97742, 0xBF8F60, 0xB23636] },
];

fn rgb_u32(v: u32) -> Rgb {
    Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

impl PalettePreset {
    pub fn palette(&self) -> Palette {
        let c = self.colors;
        Palette {
            background: rgb_u32(c[0]),
            background_alt: rgb_u32(c[1]),
            foreground: rgb_u32(c[2]),
            selected: rgb_u32(c[3]),
            active: rgb_u32(c[4]),
            urgent: rgb_u32(c[5]),
        }
    }
}

pub fn preset(id: &str) -> Option<&'static PalettePreset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// Work out the accent colour the user asked for.
pub fn chosen_accent(look: &LookConfig, swatches: &[Swatch]) -> Rgb {
    match look.accent_source {
        AccentSource::Custom => look.custom_accent,
        AccentSource::Windows => windows_accent().unwrap_or(look.custom_accent),
        AccentSource::Image => vibrant(swatches).unwrap_or(look.custom_accent),
    }
}

/// Recompute `look.palette` from its sources (image + mode + accent, or a preset).
pub fn regenerate(look: &mut LookConfig, swatches: &[Swatch]) {
    if look.palette_source == "auto" {
        let accent = chosen_accent(look, swatches);
        look.palette = generate(swatches, look.mode, accent);
    } else if let Some(p) = preset(&look.palette_source) {
        let mut pal = p.palette();
        // Accent choice still applies on top of a preset, unless "image" (keep preset's own).
        match look.accent_source {
            AccentSource::Custom => pal.selected = look.custom_accent,
            AccentSource::Windows => {
                if let Some(a) = windows_accent() {
                    pal.selected = a;
                }
            }
            AccentSource::Image => {}
        }
        look.palette = pal;
    }
}

/// Load an image file from disk as RGBA (used for the background and for colour extraction).
pub fn load_image(path: &std::path::Path) -> Option<image::RgbaImage> {
    let img = image::open(path).ok()?;
    // Keep textures a sensible size; big photos would waste GPU memory.
    let img = if img.width() > 1600 || img.height() > 1600 {
        img.resize(1600, 1600, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    Some(img.to_rgba8())
}

/// A soft gradient used when the user hasn't chosen an image.
pub fn gradient_image(p: &Palette) -> image::RgbaImage {
    let (w, h) = (256u32, 256u32);
    let a = p.selected;
    let b = p.active;
    let c = p.background_alt;
    image::RgbaImage::from_fn(w, h, |x, y| {
        let t = (x as f32 / w as f32) * 0.5 + (y as f32 / h as f32) * 0.5;
        let col = if t < 0.5 { mix(a, b, t * 2.0) } else { mix(b, c, (t - 0.5) * 2.0) };
        image::Rgba([col.0, col.1, col.2, 255])
    })
}
