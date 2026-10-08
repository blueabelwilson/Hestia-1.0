//! Fonts: finding every font installed on the PC and loading the user's choice into egui.

use crate::config::LookConfig;
use eframe::egui::{self, FontFamily};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FontEntry {
    pub family: String,
    pub regular: PathBuf,
    pub bold: Option<PathBuf>,
}

fn font_dirs() -> Vec<PathBuf> {
    let mut v = vec![PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts")];
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        v.push(PathBuf::from(l).join("Microsoft\\Windows\\Fonts"));
    }
    v
}

/// Read a font file's family name, weight and italic flag.
pub fn describe(path: &Path) -> Option<(String, u16, bool)> {
    let data = std::fs::read(path).ok()?;
    let face = ttf_parser::Face::parse(&data, 0).ok()?;
    let pick = |id: u16| -> Option<String> {
        let mut best: Option<String> = None;
        for n in face.names() {
            if n.name_id != id {
                continue;
            }
            if let Some(s) = n.to_string() {
                if n.language_id == 0x0409 {
                    return Some(s);
                }
                best.get_or_insert(s);
            }
        }
        best
    };
    let family = pick(ttf_parser::name_id::TYPOGRAPHIC_FAMILY).or_else(|| pick(ttf_parser::name_id::FAMILY))?;
    Some((family, face.weight().to_number(), face.is_italic()))
}

/// Scan the Fonts folders (slow-ish: reads every font file).
pub fn scan() -> Vec<FontEntry> {
    let mut fam: BTreeMap<String, Vec<(u16, bool, PathBuf)>> = BTreeMap::new();
    for dir in font_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ext != "ttf" && ext != "otf" {
                continue;
            }
            if let Some((name, weight, italic)) = describe(&p) {
                fam.entry(name).or_default().push((weight, italic, p));
            }
        }
    }
    fam.into_iter()
        .filter_map(|(family, mut files)| {
            files.retain(|f| !f.1);
            if files.is_empty() {
                return None;
            }
            let closest = |target: i32, files: &[(u16, bool, PathBuf)]| {
                files.iter().min_by_key(|f| (f.0 as i32 - target).abs()).map(|f| f.2.clone())
            };
            let regular = closest(400, &files)?;
            let bolds: Vec<_> = files.iter().filter(|f| f.0 >= 600).cloned().collect();
            let bold = closest(700, &bolds);
            Some(FontEntry { family, regular, bold })
        })
        .collect()
}

/// Background-loaded list of installed fonts, cached in fonts.json.
#[derive(Clone)]
pub struct FontCatalog {
    pub fonts: Arc<Mutex<Vec<FontEntry>>>,
    pub ready: Arc<std::sync::atomic::AtomicBool>,
}

impl FontCatalog {
    pub fn load(ctx: &egui::Context) -> Self {
        let cache = crate::config::data_dir().join("fonts.json");
        let cached: Vec<FontEntry> =
            std::fs::read_to_string(&cache).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let cat = FontCatalog {
            ready: Arc::new(std::sync::atomic::AtomicBool::new(!cached.is_empty())),
            fonts: Arc::new(Mutex::new(cached)),
        };
        // Refresh in the background (picks up newly installed fonts).
        let c = cat.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let list = scan();
            if let Ok(s) = serde_json::to_string(&list) {
                let _ = std::fs::write(cache, s);
            }
            if let Ok(mut f) = c.fonts.lock() {
                *f = list;
            }
            c.ready.store(true, std::sync::atomic::Ordering::SeqCst);
            ctx.request_repaint();
        });
        cat
    }

    pub fn list(&self) -> Vec<FontEntry> {
        self.fonts.lock().map(|f| f.clone()).unwrap_or_default()
    }
}

/// The few fonts we know by file name (used before the catalog exists).
const KNOWN: &[(&str, &str, &str)] = &[
    ("Segoe UI", "segoeui.ttf", "seguisb.ttf"),
    ("Bahnschrift", "bahnschrift.ttf", "bahnschrift.ttf"),
    ("Consolas", "consola.ttf", "consolab.ttf"),
    ("Calibri", "calibri.ttf", "calibrib.ttf"),
];

fn known(name: &str) -> (Option<PathBuf>, Option<PathBuf>) {
    let dir = &font_dirs()[0];
    let (r, b) = KNOWN.iter().find(|k| k.0 == name).map(|k| (k.1, k.2)).unwrap_or(("segoeui.ttf", "seguisb.ttf"));
    (Some(dir.join(r)), Some(dir.join(b)))
}

/// Which files to load for the user's choices: (main, main bold, headings).
pub fn files_for(look: &LookConfig) -> (Option<PathBuf>, Option<PathBuf>, Option<PathBuf>) {
    let (mut reg, mut bold) = (look.font_file.clone(), look.font_bold_file.clone());
    if reg.is_none() {
        let k = known(&look.font);
        reg = k.0;
        bold = k.1;
    }
    let heading = look.heading_font_file.clone().or_else(|| bold.clone()).or_else(|| reg.clone());
    (reg, bold, heading)
}

/// Load fonts into egui. "Proportional" is the main font, "bold" is used for titles and buttons.
pub fn install(ctx: &egui::Context, look: &LookConfig) {
    let (reg, _bold, heading) = files_for(look);
    let mut defs = egui::FontDefinitions::default();
    let mut add = |key: &str, path: &Option<PathBuf>| -> bool {
        match path.as_ref().and_then(|p| std::fs::read(p).ok()) {
            Some(bytes) => {
                defs.font_data.insert(key.to_string(), egui::FontData::from_owned(bytes));
                true
            }
            None => false,
        }
    };
    let has_reg = add("hestia-regular", &reg);
    let has_head = add("hestia-heading", &heading);
    let mut base = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if has_reg {
        base.insert(0, "hestia-regular".into());
    }
    let mut head = base.clone();
    if has_head {
        head.insert(0, "hestia-heading".into());
    }
    defs.families.insert(FontFamily::Proportional, base);
    // The "bold" family must always exist or egui will panic.
    defs.families.insert(FontFamily::Name("bold".into()), head);
    ctx.set_fonts(defs);
}

/// A key that changes whenever the fonts to load change.
pub fn key(look: &LookConfig) -> String {
    format!("{:?}", files_for(look))
}
