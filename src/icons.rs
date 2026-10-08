//! Icons: built-in SVGs, user SVG overrides (with theme recolouring), and real app icons
//! pulled from Windows in a background thread.

use crate::config::IconOverride;
use eframe::egui::{self, ColorImage, TextureHandle, TextureId, TextureOptions};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};

// ---------------------------------------------------------------------------
// Built-in icons (24×24 line icons; several shapes follow Feather Icons, MIT licence)
// ---------------------------------------------------------------------------

fn wrap(body: &str) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#ffffff" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"##
    )
}

/// The Hestia logo (a house shaped like an H, with a window), in the given colour.
pub fn logo_svg(color: &str) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="80 80 452 452"><g fill="{color}">
<polygon points="230.31 96 160.31 96 160.31 252.72 230.31 209.48 230.31 96"/>
<polygon points="381.81 96 381.81 209.48 451.81 252.72 451.81 96 381.81 96"/>
<polygon points="160.19 287.98 160.19 516 230.19 516 230.19 327.02 306.19 276.03 381.69 325.47 381.69 516 451.69 516 451.69 289.53 306.19 191.97 160.19 287.98"/>
<rect x="270.44" y="360" width="32" height="32" rx="7.33" ry="7.33"/>
<rect x="309.44" y="360" width="32" height="32" rx="7.33" ry="7.33"/>
<rect x="270.44" y="398" width="32" height="32" rx="7.33" ry="7.33"/>
<rect x="309.44" y="398" width="32" height="32" rx="7.33" ry="7.33"/>
</g></svg>"##
    )
}

/// Hestia's brand orange.
pub const BRAND: &str = "#F5893A";

pub fn builtin_svg(name: &str) -> Option<String> {
    if name == "logo" {
        return Some(logo_svg("#ffffff"));
    }
    let body = match name {
        "search" => r#"<circle cx="11" cy="11" r="7"/><line x1="16.5" y1="16.5" x2="21" y2="21"/>"#,
        "apps" => {
            r#"<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/>"#
        }
        "windows" => {
            r#"<rect x="3" y="8" width="13" height="11" rx="2"/><path d="M8 8V6a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-3"/>"#
        }
        "run" => {
            r#"<rect x="2" y="4" width="20" height="16" rx="2"/><polyline points="6 9 10 12 6 15"/><line x1="12" y1="16" x2="17" y2="16"/>"#
        }
        "power" | "shutdown" => r#"<path d="M18.36 6.64a9 9 0 1 1-12.73 0"/><line x1="12" y1="2" x2="12" y2="12"/>"#,
        "lock" => r#"<rect x="4" y="11" width="16" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/>"#,
        "sleep" => r#"<path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/>"#,
        "hibernate" => {
            r#"<circle cx="12" cy="12" r="9"/><line x1="10" y1="9" x2="10" y2="15"/><line x1="14" y1="9" x2="14" y2="15"/>"#
        }
        "signout" => {
            r#"<path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" y1="12" x2="9" y2="12"/>"#
        }
        "restart" => {
            r#"<polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/>"#
        }
        "app" => r#"<rect x="3" y="3" width="18" height="18" rx="4"/><rect x="9" y="9" width="6" height="6" rx="1"/>"#,
        "window" => r#"<rect x="3" y="4" width="18" height="16" rx="2"/><line x1="3" y1="9" x2="21" y2="9"/>"#,
        "terminal" => r#"<polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/>"#,
        "check" => r#"<polyline points="20 6 9 17 4 12"/>"#,
        "close" => r#"<line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>"#,
        "user" => r#"<path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/>"#,
        "clock" => r#"<circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/>"#,
        "flame" => {
            r#"<path d="M12 22c4 0 7-2.7 7-7 0-3.5-2.3-6.2-4.2-8.2-.6 2-1.6 3.2-2.8 3.7.3-3-1-6-3.5-8.5-.4 3.8-3.5 6.2-3.5 10.5 0 5.6 3 9.5 7 9.5z"/>"#
        }
        _ => return None,
    };
    Some(wrap(body))
}

/// Built-in icon names a user can override, with friendly labels (for the settings screen).
pub const OVERRIDABLE_BUILTINS: &[(&str, &str, &str)] = &[
    ("mode:apps", "Apps button", "apps"),
    ("mode:windows", "Windows button", "windows"),
    ("mode:run", "Run button", "run"),
    ("mode:power", "Power button", "power"),
    ("ui:search", "Search box", "search"),
    ("power:lock", "Lock", "lock"),
    ("power:sleep", "Sleep", "sleep"),
    ("power:hibernate", "Hibernate", "hibernate"),
    ("power:signout", "Sign out", "signout"),
    ("power:restart", "Restart", "restart"),
    ("power:shutdown", "Shut down", "shutdown"),
    ("ui:user", "User badge", "user"),
    ("ui:clock", "Uptime badge", "clock"),
];

// ---------------------------------------------------------------------------
// SVG rasterising
// ---------------------------------------------------------------------------

pub struct Raster {
    pub image: ColorImage,
    /// True when the icon is a white mask that should be tinted with the theme colour.
    pub tintable: bool,
}

/// Render an SVG to `px`×`px`. With `recolor`, single-colour icons become a white mask
/// that is tinted at draw time; multi-colour icons keep their colours.
pub fn rasterize_svg(data: &[u8], px: u32, recolor: bool) -> Option<Raster> {
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(data, &opt).ok()?;
    let size = tree.size();
    let px = px.clamp(8, 512);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(px, px)?;
    let s = (px as f32 / size.width()).min(px as f32 / size.height());
    let tx = (px as f32 - size.width() * s) / 2.0;
    let ty = (px as f32 - size.height() * s) / 2.0;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_row(s, 0.0, 0.0, s, tx, ty), &mut pixmap.as_mut());
    let mut data = pixmap.take();
    let tintable = recolor && is_single_colour(&data);
    if tintable {
        for p in data.chunks_exact_mut(4) {
            let a = p[3];
            p[0] = a;
            p[1] = a;
            p[2] = a;
        }
    }
    Some(Raster { image: ColorImage::from_rgba_premultiplied([px as usize, px as usize], &data), tintable })
}

/// Does this (premultiplied RGBA) image use essentially one colour?
fn is_single_colour(data: &[u8]) -> bool {
    let mut first: Option<[f32; 3]> = None;
    for p in data.chunks_exact(4) {
        let a = p[3] as f32;
        if a < 200.0 {
            continue;
        }
        let c = [p[0] as f32 * 255.0 / a, p[1] as f32 * 255.0 / a, p[2] as f32 * 255.0 / a];
        match first {
            None => first = Some(c),
            Some(f) => {
                let d = (c[0] - f[0]).abs() + (c[1] - f[1]).abs() + (c[2] - f[2]).abs();
                if d > 60.0 {
                    return false;
                }
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Real app icons from Windows
// ---------------------------------------------------------------------------

fn shell_icon(source: &str, px: u32) -> Option<ColorImage> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY};
    unsafe {
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(source), None).ok()?;
        let hbmp = factory.GetImage(SIZE { cx: px as i32, cy: px as i32 }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK).ok()?;
        let gdi = HGDIOBJ(hbmp.0);
        let mut bm = BITMAP::default();
        GetObjectW(gdi, std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut BITMAP as *mut core::ffi::c_void));
        let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
        if w <= 0 || h <= 0 {
            let _ = DeleteObject(gdi);
            return None;
        }
        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let hdc = CreateCompatibleDC(HDC::default());
        let lines = GetDIBits(hdc, hbmp, 0, h as u32, Some(buf.as_mut_ptr() as *mut core::ffi::c_void), &mut bi, DIB_RGB_COLORS);
        let _ = DeleteDC(hdc);
        let _ = DeleteObject(gdi);
        if lines == 0 {
            return None;
        }
        let has_alpha = buf.chunks_exact(4).any(|p| p[3] != 0);
        for p in buf.chunks_exact_mut(4) {
            p.swap(0, 2); // BGRA -> RGBA
            if !has_alpha {
                p[3] = 255;
            }
        }
        Some(ColorImage::from_rgba_premultiplied([w as usize, h as usize], &buf))
    }
}

struct ShellWorker {
    tx: Sender<(String, String, u32)>,
    rx: Receiver<(String, Option<ColorImage>)>,
}

impl ShellWorker {
    fn start(ctx: egui::Context) -> Self {
        let (req_tx, req_rx) = channel::<(String, String, u32)>();
        let (res_tx, res_rx) = channel::<(String, Option<ColorImage>)>();
        std::thread::spawn(move || {
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
            }
            while let Ok((key, source, px)) = req_rx.recv() {
                let img = shell_icon(&source, px);
                if res_tx.send((key, img)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        ShellWorker { tx: req_tx, rx: res_rx }
    }
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

/// Where an item's default icon comes from.
#[derive(Clone, Debug)]
pub enum IconSrc {
    Builtin(&'static str),
    /// File path or shell parsing name (e.g. `shell:AppsFolder\...`).
    Shell(String),
}

#[derive(Clone, Copy)]
pub struct Icon {
    pub id: TextureId,
    pub tintable: bool,
}

pub struct IconCache {
    ctx: egui::Context,
    textures: HashMap<String, Option<(TextureHandle, bool)>>,
    pending: HashSet<String>,
    worker: ShellWorker,
    overrides: BTreeMap<String, IconOverride>,
}

/// Round up to a few fixed sizes so we don't make a texture per pixel size.
fn bucket(px: u32) -> u32 {
    for b in [24, 32, 48, 64, 96, 128, 192, 256] {
        if px <= b {
            return b;
        }
    }
    256
}

impl IconCache {
    pub fn new(ctx: &egui::Context, overrides: BTreeMap<String, IconOverride>) -> Self {
        IconCache {
            ctx: ctx.clone(),
            textures: HashMap::new(),
            pending: HashSet::new(),
            worker: ShellWorker::start(ctx.clone()),
            overrides,
        }
    }

    pub fn set_overrides(&mut self, overrides: BTreeMap<String, IconOverride>) {
        if overrides != self.overrides {
            self.overrides = overrides;
            // Drop everything that isn't a shell icon (those can't be overridden-then-reverted cheaply anyway).
            self.textures.clear();
            self.pending.clear();
        }
    }

    /// Forget all icons (e.g. after the user picks a new SVG).
    pub fn clear(&mut self) {
        self.textures.clear();
        self.pending.clear();
    }

    /// Move finished background icons into textures. Call once per frame.
    pub fn poll(&mut self) {
        while let Ok((key, img)) = self.worker.rx.try_recv() {
            self.pending.remove(&key);
            let tex = img.map(|i| (self.ctx.load_texture(&key, i, TextureOptions::LINEAR), false));
            self.textures.insert(key, tex);
        }
    }

    fn load_svg_bytes(&mut self, key: &str, bytes: &[u8], px: u32, recolor: bool) -> Option<Icon> {
        let r = rasterize_svg(bytes, px, recolor);
        let entry = r.map(|r| (self.ctx.load_texture(key, r.image, TextureOptions::LINEAR), r.tintable));
        let out = entry.as_ref().map(|(t, tint)| Icon { id: t.id(), tintable: *tint });
        self.textures.insert(key.to_string(), entry);
        out
    }

    /// Icon for an item. `px` is the physical pixel size it will be drawn at.
    /// Returns None while a Windows icon is still loading.
    pub fn get(&mut self, item_id: &str, src: &IconSrc, px: u32) -> Option<Icon> {
        let px = bucket(px);
        let key = format!("{item_id}@{px}");
        if let Some(entry) = self.textures.get(&key) {
            return entry.as_ref().map(|(t, tint)| Icon { id: t.id(), tintable: *tint });
        }
        // 1. User's own icon (SVG, or a picture icon like .ico/.png).
        if let Some(ov) = self.overrides.get(item_id).cloned() {
            if let Ok(bytes) = std::fs::read(&ov.svg) {
                if is_svg_path(&ov.svg) {
                    if let Some(icon) = self.load_svg_bytes(&key, &bytes, px, ov.recolor) {
                        return Some(icon);
                    }
                } else if let Some(img) = raster_icon(&bytes, px) {
                    let tex = self.ctx.load_texture(&key, img, TextureOptions::LINEAR);
                    let icon = Icon { id: tex.id(), tintable: false };
                    self.textures.insert(key.clone(), Some((tex, false)));
                    return Some(icon);
                }
            }
        }
        match src {
            IconSrc::Builtin(name) => {
                let svg = builtin_svg(name).or_else(|| builtin_svg("app"))?;
                self.load_svg_bytes(&key, svg.as_bytes(), px, true)
            }
            IconSrc::Shell(source) => {
                if self.pending.insert(key.clone()) {
                    let _ = self.worker.tx.send((key, source.clone(), px));
                }
                None
            }
        }
    }

    /// The generic placeholder shown while a real icon loads (or if it has none).
    pub fn placeholder(&mut self, px: u32) -> Option<Icon> {
        self.get("builtin:app", &IconSrc::Builtin("app"), px)
    }
}

pub fn is_svg_path(p: &std::path::Path) -> bool {
    p.extension().map(|e| e.to_string_lossy().eq_ignore_ascii_case("svg")).unwrap_or(true)
}

/// Decode an .ico/.png icon (picking the largest image in an .ico) and scale it to `px`.
pub fn raster_icon(bytes: &[u8], px: u32) -> Option<ColorImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let img =
        if img.width() != px || img.height() != px { img.resize(px, px, image::imageops::FilterType::Lanczos3) } else { img };
    let rgba = img.to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}
