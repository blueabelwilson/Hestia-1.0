//! User settings, stored as a readable TOML file in %APPDATA%\Hestia\config.toml.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Colours
// ---------------------------------------------------------------------------

/// An sRGB colour stored as "#RRGGBB" in the config file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn hex(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }
    pub fn parse(s: &str) -> Option<Rgb> {
        let s = s.trim().trim_start_matches('#');
        if s.len() < 6 {
            return None;
        }
        let v = u32::from_str_radix(&s[..6], 16).ok()?;
        Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }
    pub fn c32(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgb(self.0, self.1, self.2)
    }
}

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.hex())
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Rgb::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("bad colour '{s}'")))
    }
}

/// The six colour roles every theme uses (same idea as adi1090x's rofi themes).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub background: Rgb,
    pub background_alt: Rgb,
    pub foreground: Rgb,
    pub selected: Rgb,
    pub active: Rgb,
    pub urgent: Rgb,
}

impl Default for Palette {
    fn default() -> Self {
        // adi1090x launcher type-6 style-10
        Palette {
            background: Rgb(0x11, 0x09, 0x2D),
            background_alt: Rgb(0x28, 0x16, 0x57),
            foreground: Rgb(0xFF, 0xFF, 0xFF),
            selected: Rgb(0xDF, 0x52, 0x96),
            active: Rgb(0x6E, 0x77, 0xFF),
            urgent: Rgb(0x8E, 0x35, 0x96),
        }
    }
}

// ---------------------------------------------------------------------------
// Look
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ColorMode {
    #[default]
    Dark,
    Colour,
    Light,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AccentSource {
    #[default]
    Image,
    Windows,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    #[default]
    None,
    Blur,
    Acrylic,
    Mica,
}

/// Overall shape of the Hestia window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WindowShape {
    #[default]
    Rectangle,
    Arch,
    Circle,
    /// Regular polygon with `sides` sides.
    Polygon,
}

/// How the menu is arranged around a circle/polygon picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MenuStyle {
    /// Results on a ring around the picture, mode buttons on an outer ring.
    #[default]
    Rings,
    /// A flat menu panel coming out of the side of the picture.
    Box,
    /// Everything fitted inside the shape.
    Inside,
}

/// Shape of the picture frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PictureShape {
    /// Fill the whole picture area (classic rofi look).
    #[default]
    Fill,
    Circle,
    Arch,
    Hexagon,
    Diamond,
    Squircle,
}

/// Shape of search boxes, buttons and list items.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ButtonShape {
    /// Whatever the layout uses (rounded rectangles, usually).
    #[default]
    Theme,
    Square,
    Pill,
    Slanted,
    Hexagon,
    /// Round icon tiles where there's room (power menu), pills elsewhere.
    Circle,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LookConfig {
    pub shape: WindowShape,
    /// Number of sides when `shape` is Polygon (3..=12).
    pub sides: u32,
    pub menu_style: MenuStyle,
    /// Size of the circle/polygon/arch shape (1.0 = standard).
    pub shape_size: f32,
    /// Launcher layout id for rectangle windows (see layout.rs presets).
    pub launcher_layout: String,
    /// Power menu layout id.
    pub power_layout: String,
    /// "auto" = colours generated from the image, otherwise the name of a preset palette.
    pub palette_source: String,
    pub mode: ColorMode,
    pub accent_source: AccentSource,
    pub custom_accent: Rgb,
    pub image: Option<PathBuf>,
    /// Overall size multiplier (1.0 = like the original themes at 1080p).
    pub size: f32,
    /// Corner roundness multiplier (0 = square, 1 = theme default, 2 = extra round).
    pub roundness: f32,
    /// Font name shown in settings.
    pub font: String,
    /// Font files actually loaded (None = look `font` up in the built-in list).
    pub font_file: Option<PathBuf>,
    pub font_bold_file: Option<PathBuf>,
    /// Font for titles and buttons. Empty = bold version of the main font.
    pub heading_font: String,
    pub heading_font_file: Option<PathBuf>,
    /// Picture framing: zoom (1 = just fills the space) and the point kept in view (0..1).
    pub image_zoom: f32,
    pub image_focus: [f32; 2],
    pub picture_shape: PictureShape,
    pub button_shape: ButtonShape,
    /// Sides for circle/polygon windows and frames: 0 = circle, 3..=12 = polygon.
    pub polygon_sides: u8,
    /// Opacity of boxes and buttons drawn on top of the picture.
    pub overlay_opacity: f32,
    /// How much layouts that use the picture as a full background darken it.
    pub picture_dim: f32,
    pub effect: Effect,
    /// Panel opacity, 0.3..=1.0. Below 1.0 the desktop (or blur) shows through.
    pub opacity: f32,
    pub animations: bool,
    /// Final colours used for drawing. Regenerated when image/mode/accent change,
    /// but the user may hand-edit any of them. (Kept last: TOML tables go after values.)
    pub palette: Palette,
}

impl Default for LookConfig {
    fn default() -> Self {
        LookConfig {
            shape: WindowShape::Rectangle,
            sides: 6,
            menu_style: MenuStyle::Rings,
            shape_size: 1.0,
            launcher_layout: "hearth".into(),
            power_layout: "hearth".into(),
            palette_source: "auto".into(),
            mode: ColorMode::Dark,
            accent_source: AccentSource::Image,
            custom_accent: Rgb(0xDF, 0x52, 0x96),
            image: None,
            size: 1.0,
            roundness: 1.0,
            font: "Segoe UI".into(),
            font_file: None,
            font_bold_file: None,
            heading_font: String::new(),
            heading_font_file: None,
            image_zoom: 1.0,
            image_focus: [0.5, 0.5],
            picture_shape: PictureShape::Fill,
            button_shape: ButtonShape::Theme,
            polygon_sides: 0,
            overlay_opacity: 0.35,
            picture_dim: 0.6,
            effect: Effect::None,
            opacity: 1.0,
            animations: true,
            palette: Palette::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Hotkeys
// ---------------------------------------------------------------------------

/// A key combination. `key` uses egui key names ("Space", "A", "F1", "ArrowDown"...).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct KeyCombo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Windows key. Only meaningful for global hotkeys.
    pub win: bool,
    pub key: String,
}

impl KeyCombo {
    pub fn new(ctrl: bool, alt: bool, shift: bool, win: bool, key: &str) -> Self {
        KeyCombo { ctrl, alt, shift, win, key: key.to_string() }
    }
    pub fn key(key: &str) -> Self {
        Self::new(false, false, false, false, key)
    }
    pub fn is_empty(&self) -> bool {
        self.key.is_empty()
    }
    pub fn label(&self) -> String {
        if self.key.is_empty() {
            return "Not set".into();
        }
        let mut parts: Vec<String> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".into());
        }
        if self.alt {
            parts.push("Alt".into());
        }
        if self.shift {
            parts.push("Shift".into());
        }
        if self.win {
            parts.push("Win".into());
        }
        parts.push(pretty_key(&self.key));
        parts.join(" + ")
    }
}

pub fn pretty_key(k: &str) -> String {
    match k {
        "ArrowUp" => "↑".into(),
        "ArrowDown" => "↓".into(),
        "ArrowLeft" => "←".into(),
        "ArrowRight" => "→".into(),
        "Escape" => "Esc".into(),
        other => other.to_string(),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalHotkeys {
    pub apps: KeyCombo,
    pub windows: KeyCombo,
    pub run: KeyCombo,
    pub power: KeyCombo,
}

impl Default for GlobalHotkeys {
    fn default() -> Self {
        GlobalHotkeys {
            apps: KeyCombo::new(false, true, false, false, "Space"),
            windows: KeyCombo::new(true, true, false, false, "W"),
            run: KeyCombo::new(true, true, false, false, "R"),
            power: KeyCombo::new(true, true, false, false, "P"),
        }
    }
}

/// Keys used while the popup is open. Each action can have several combos.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PopupKeys {
    pub up: Vec<KeyCombo>,
    pub down: Vec<KeyCombo>,
    pub left: Vec<KeyCombo>,
    pub right: Vec<KeyCombo>,
    pub open: Vec<KeyCombo>,
    pub close: Vec<KeyCombo>,
    pub next_mode: Vec<KeyCombo>,
    pub prev_mode: Vec<KeyCombo>,
}

impl Default for PopupKeys {
    fn default() -> Self {
        let ctrl = |k: &str| KeyCombo::new(true, false, false, false, k);
        PopupKeys {
            up: vec![KeyCombo::key("ArrowUp"), ctrl("P")],
            down: vec![KeyCombo::key("ArrowDown"), ctrl("N")],
            left: vec![KeyCombo::key("ArrowLeft")],
            right: vec![KeyCombo::key("ArrowRight")],
            open: vec![KeyCombo::key("Enter")],
            close: vec![KeyCombo::key("Escape")],
            next_mode: vec![KeyCombo::key("Tab")],
            prev_mode: vec![KeyCombo::new(false, false, true, false, "Tab")],
        }
    }
}

// ---------------------------------------------------------------------------
// Icons
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IconOverride {
    pub svg: PathBuf,
    /// Recolour single-colour icons to match the theme.
    #[serde(default = "yes")]
    pub recolor: bool,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct IconConfig {
    /// Item id (e.g. "lnk:firefox", "power:shutdown", "mode:apps") -> SVG override.
    pub overrides: BTreeMap<String, IconOverride>,
}

// ---------------------------------------------------------------------------
// General
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub start_with_windows: bool,
    pub show_tray_icon: bool,
    pub hide_on_focus_loss: bool,
    pub confirm_power: bool,
    pub include_store_apps: bool,
    /// Set once the first-run wizard has been completed.
    pub setup_done: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        GeneralConfig {
            start_with_windows: true,
            show_tray_icon: true,
            hide_on_focus_loss: true,
            confirm_power: true,
            include_store_apps: true,
            setup_done: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub look: LookConfig,
    pub hotkeys: GlobalHotkeys,
    pub keys: PopupKeys,
    pub icons: IconConfig,
}

// ---------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir());
    let dir = base.join("Hestia");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.toml")
}

impl Config {
    /// Older test versions stored shaped windows as layout ids.
    fn migrate(&mut self) {
        let l = &mut self.look;
        let (shape, style) = match l.launcher_layout.as_str() {
            "arch" => (Some(WindowShape::Arch), None),
            "orb" => (Some(WindowShape::Circle), Some(MenuStyle::Inside)),
            "honeycomb" => (Some(WindowShape::Polygon), Some(MenuStyle::Inside)),
            _ => (None, None),
        };
        if let Some(sh) = shape {
            l.shape = sh;
            if let Some(st) = style {
                l.menu_style = st;
            }
            if sh == WindowShape::Polygon {
                l.sides = 6;
            }
            l.launcher_layout = "hearth".into();
            l.power_layout = "hearth".into();
        }
        l.sides = l.sides.clamp(3, 12);
        if !(0.5..=2.0).contains(&l.shape_size) {
            l.shape_size = 1.0;
        }
    }

    pub fn exists() -> bool {
        config_path().exists()
    }

    pub fn load() -> Config {
        match std::fs::read_to_string(config_path()) {
            Ok(text) => match toml::from_str::<Config>(&text) {
                Ok(mut c) => {
                    c.migrate();
                    c
                }
                Err(e) => {
                    eprintln!("config.toml could not be read ({e}); using defaults");
                    // Keep the broken file so the user doesn't lose it.
                    let _ = std::fs::copy(config_path(), data_dir().join("config.broken.toml"));
                    Config::default()
                }
            },
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let text = toml::to_string_pretty(self).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        // Write atomically so the running launcher never reads half a file.
        let tmp = data_dir().join("config.toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, config_path())
    }
}

pub fn modified_time(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}
