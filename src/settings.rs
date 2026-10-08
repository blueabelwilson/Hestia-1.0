//! The settings window and first-run setup wizard.

use crate::apps::AppList;
use crate::config::{
    self, AccentSource, ButtonShape, ColorMode, Config, Effect, IconOverride, KeyCombo, MenuStyle, PictureShape, Rgb, WindowShape,
};
use crate::icons::{self, IconCache, IconSrc};
use crate::keys;
use crate::layout::{self, Colors, Item, Layout, Scene};
use crate::palette::{self, Swatch};
use crate::platform;
use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, RichText, Rounding, Sense, Stroke, Vec2};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const SETTINGS_TITLE: &str = "Hestia Settings";
pub const SETTINGS_MUTEX: &str = "Local\\HestiaSettings";

// Settings window's own colours (warm, hearth-like dark theme).
const BG: Color32 = Color32::from_rgb(0x16, 0x13, 0x1C);
const PANEL: Color32 = Color32::from_rgb(0x1E, 0x1A, 0x26);
const CARD: Color32 = Color32::from_rgb(0x26, 0x21, 0x30);
const TEXT: Color32 = Color32::from_rgb(0xEE, 0xEA, 0xF2);
const MUTED: Color32 = Color32::from_rgb(0xA8, 0xA0, 0xB4);
const FLAME: Color32 = Color32::from_rgb(0xF5, 0x89, 0x3A);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Look,
    Icons,
    Hotkeys,
    General,
    About,
}

#[derive(Clone, PartialEq)]
enum Recording {
    Global(usize),
    /// (action index, Some(slot) to replace or None to add)
    Popup(usize, Option<usize>),
}

const WIZARD_STEPS: usize = 7;

pub struct SettingsApp {
    cfg: Config,
    saved: Config,
    changed_at: Option<Instant>,
    saved_at: Option<Instant>,

    wizard: Option<usize>,
    page: Page,

    image_tex: Option<egui::TextureHandle>,
    image_loaded: Option<Option<PathBuf>>,
    swatches: Vec<Swatch>,
    image_error: Option<String>,
    pending_regen: bool,

    icons: IconCache,
    apps: AppList,
    preview_power: bool,

    recording: Option<Recording>,
    hotkey_errors: Vec<String>,
    last_status: Instant,

    icon_filter: String,
    icon_focus: Option<String>,
    icon_error: Option<String>,

    win11: bool,
    daemon_running: bool,
    last_daemon_check: Instant,
    font_catalog: crate::fonts::FontCatalog,
    font_filter: String,
    font_installed: String,
}

pub fn run(wizard: bool, icon_target: Option<String>) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(SETTINGS_TITLE)
            .with_inner_size([1220.0, 800.0])
            .with_min_inner_size([980.0, 640.0])
            .with_icon(window_icon()),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(SETTINGS_TITLE, options, Box::new(move |cc| Ok(Box::new(SettingsApp::new(cc, wizard, icon_target)))))
}

fn window_icon() -> egui::IconData {
    let svg = icons::logo_svg(icons::BRAND);
    let r = icons::rasterize_svg(svg.as_bytes(), 64, false);
    match r {
        Some(r) => egui::IconData {
            rgba: r.image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect(),
            width: 64,
            height: 64,
        },
        None => egui::IconData::default(),
    }
}

fn apply_style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(0x10, 0x0E, 0x15);
    v.faint_bg_color = CARD;
    v.selection.bg_fill = FLAME.gamma_multiply(0.8);
    v.selection.stroke = Stroke::new(1.0f32, TEXT);
    v.hyperlink_color = FLAME;
    v.override_text_color = Some(TEXT);
    let r = Rounding::same(8.0);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.rounding = r;
    }
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x33, 0x2C, 0x3E);
    v.widgets.inactive.bg_fill = Color32::from_rgb(0x33, 0x2C, 0x3E);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x44, 0x3A, 0x52);
    v.widgets.hovered.bg_fill = Color32::from_rgb(0x44, 0x3A, 0x52);
    v.widgets.active.weak_bg_fill = FLAME.gamma_multiply(0.7);
    v.window_rounding = Rounding::same(12.0);
    ctx.set_visuals(v);

    let mut st = (*ctx.style()).clone();
    st.spacing.item_spacing = Vec2::new(10.0, 10.0);
    st.spacing.button_padding = Vec2::new(14.0, 7.0);
    st.spacing.interact_size.y = 30.0;
    st.spacing.slider_width = 220.0;
    use egui::TextStyle::*;
    st.text_styles.insert(Body, FontId::proportional(15.0));
    st.text_styles.insert(Button, FontId::proportional(15.0));
    st.text_styles.insert(Small, FontId::proportional(12.5));
    st.text_styles.insert(Heading, FontId::new(26.0, FontFamily::Name("bold".into())));
    ctx.set_style(st);
}

/// An iOS-style on/off switch.
fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = Vec2::new(44.0, 24.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let bg = Color32::from_rgb(0x44, 0x3A, 0x52).lerp_to_gamma(FLAME, t);
    ui.painter().rect_filled(rect, Rounding::same(12.0), bg);
    let x = egui::lerp(rect.left() + 12.0..=rect.right() - 12.0, t);
    ui.painter().circle_filled(Pos2::new(x, rect.center().y), 9.0, Color32::WHITE);
    resp
}

/// A labelled setting row: title, grey description, and a control on the right.
fn setting_row(ui: &mut egui::Ui, title: &str, desc: &str, control: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width((ui.available_width() - 120.0).max(200.0));
            ui.label(RichText::new(title).strong());
            if !desc.is_empty() {
                ui.label(RichText::new(desc).color(MUTED).small());
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}

fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::none()
        .fill(CARD)
        .rounding(12.0)
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(title).size(13.0).color(FLAME).strong());
}

fn keycaps(ui: &mut egui::Ui, combo: &KeyCombo) {
    if combo.is_empty() {
        ui.label(RichText::new("Not set").color(MUTED));
        return;
    }
    let label = combo.label();
    for (i, part) in label.split(" + ").enumerate() {
        if i > 0 {
            ui.label(RichText::new("+").color(MUTED));
        }
        egui::Frame::none()
            .fill(Color32::from_rgb(0x3A, 0x32, 0x46))
            .rounding(6.0)
            .inner_margin(egui::Margin::symmetric(8.0, 3.0))
            .show(ui, |ui| {
                ui.label(RichText::new(part).monospace());
            });
    }
}

/// A clickable tile with a little drawing of a shape and its name.
fn shape_choice(ui: &mut egui::Ui, label: &str, selected: bool, draw: impl FnOnce(&egui::Painter, Rect)) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(92.0, 78.0), Sense::click());
    let bg = if selected {
        Color32::from_rgb(0x3A, 0x2C, 0x24)
    } else if resp.hovered() {
        Color32::from_rgb(0x30, 0x2A, 0x3C)
    } else {
        Color32::from_rgb(0x1E, 0x1A, 0x26)
    };
    ui.painter().rect_filled(rect, Rounding::same(10.0), bg);
    if selected {
        ui.painter().rect_stroke(rect, Rounding::same(10.0), Stroke::new(2.0f32, FLAME));
    }
    let art = Rect::from_center_size(Pos2::new(rect.center().x, rect.top() + 28.0), Vec2::new(60.0, 36.0));
    draw(ui.painter(), art);
    ui.painter().text(
        Pos2::new(rect.center().x, rect.bottom() - 14.0),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(12.5),
        if selected { TEXT } else { MUTED },
    );
    resp.clicked()
}

fn draw_outline_icon(p: &egui::Painter, r: Rect, o: layout::Outline) {
    let c = FLAME.gamma_multiply(0.85);
    let area = match o {
        layout::Outline::Polygon(_) => Rect::from_center_size(r.center(), Vec2::splat(r.height())),
        layout::Outline::Arch => Rect::from_center_size(r.center(), Vec2::new(r.height() * 0.75, r.height())),
        _ => r,
    };
    if o == layout::Outline::Rect {
        p.rect_filled(area, Rounding::same(5.0), c);
    } else {
        p.add(egui::Shape::convex_polygon(layout::outline_points(o, area), c, Stroke::NONE));
    }
}

fn draw_style_icon(p: &egui::Painter, r: Rect, style: MenuStyle, sides: u8) {
    let c = FLAME.gamma_multiply(0.85);
    let faint = FLAME.gamma_multiply(0.45);
    match style {
        MenuStyle::Rings => {
            let ctr = r.center();
            p.add(egui::Shape::closed_line(layout::poly_path(sides, ctr, 17.0), Stroke::new(5.0f32, faint)));
            p.add(egui::Shape::convex_polygon(layout::poly_path(sides, ctr, 10.0), c, Stroke::NONE));
            for pt in layout::poly_path(0, ctr, 17.0).iter().step_by(12) {
                p.circle_filled(*pt, 2.5, c);
            }
        }
        MenuStyle::Box => {
            let ctr = Pos2::new(r.left() + 16.0, r.center().y);
            p.rect_filled(
                Rect::from_min_max(Pos2::new(ctr.x, r.top() + 5.0), Pos2::new(r.right(), r.bottom() - 5.0)),
                Rounding::same(4.0),
                faint,
            );
            p.add(egui::Shape::convex_polygon(layout::poly_path(sides, ctr, 15.0), c, Stroke::NONE));
        }
        MenuStyle::Inside => {
            let ctr = r.center();
            p.add(egui::Shape::convex_polygon(layout::poly_path(sides, ctr, 18.0), faint, Stroke::NONE));
            for i in 0..3 {
                let y = ctr.y - 6.0 + i as f32 * 6.0;
                p.line_segment([Pos2::new(ctr.x - 8.0, y), Pos2::new(ctr.x + 8.0, y)], Stroke::new(2.0f32, c));
            }
        }
    }
}

fn draw_picture_icon(p: &egui::Painter, r: Rect, shape: PictureShape) {
    let c = FLAME.gamma_multiply(0.85);
    let side = r.height();
    let sq = Rect::from_center_size(r.center(), Vec2::splat(side));
    let poly = |pts: Vec<Pos2>| {
        p.add(egui::Shape::convex_polygon(pts, c, Stroke::NONE));
    };
    match shape {
        PictureShape::Fill => {
            p.rect_filled(r, Rounding::same(4.0), c);
        }
        PictureShape::Circle => {
            p.circle_filled(r.center(), side / 2.0, c);
        }
        PictureShape::Squircle => {
            p.rect_filled(sq, Rounding::same(side * 0.32), c);
        }
        PictureShape::Arch => {
            let w = side * 0.78;
            let ar = Rect::from_center_size(r.center(), Vec2::new(w, side));
            p.rect_filled(ar, Rounding { nw: w / 2.0, ne: w / 2.0, sw: 2.0, se: 2.0 }, c);
        }
        PictureShape::Hexagon => {
            let rad = side / 2.0;
            poly(
                (0..6)
                    .map(|i| {
                        let a = (-90.0f32 + 60.0 * i as f32).to_radians();
                        Pos2::new(r.center().x + rad * a.cos(), r.center().y + rad * a.sin())
                    })
                    .collect(),
            );
        }
        PictureShape::Diamond => {
            let cc = r.center();
            let h = side / 2.0;
            poly(vec![
                Pos2::new(cc.x, cc.y - h),
                Pos2::new(cc.x + h, cc.y),
                Pos2::new(cc.x, cc.y + h),
                Pos2::new(cc.x - h, cc.y),
            ]);
        }
    }
}

fn draw_button_icon(p: &egui::Painter, r: Rect, shape: ButtonShape) {
    let c = FLAME.gamma_multiply(0.85);
    let b = Rect::from_center_size(r.center(), Vec2::new(r.width(), 22.0));
    let (l, rt, t, bt) = (b.left(), b.right(), b.top(), b.bottom());
    match shape {
        ButtonShape::Theme => {
            p.rect_filled(b, Rounding::same(6.0), c);
        }
        ButtonShape::Square => {
            p.rect_filled(b, Rounding::ZERO, c);
        }
        ButtonShape::Pill => {
            p.rect_filled(b, Rounding::same(11.0), c);
        }
        ButtonShape::Slanted => {
            p.add(egui::Shape::convex_polygon(
                vec![Pos2::new(l + 8.0, t), Pos2::new(rt, t), Pos2::new(rt - 8.0, bt), Pos2::new(l, bt)],
                c,
                Stroke::NONE,
            ));
        }
        ButtonShape::Hexagon => {
            let cy = b.center().y;
            p.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(l + 11.0, t),
                    Pos2::new(rt - 11.0, t),
                    Pos2::new(rt, cy),
                    Pos2::new(rt - 11.0, bt),
                    Pos2::new(l + 11.0, bt),
                    Pos2::new(l, cy),
                ],
                c,
                Stroke::NONE,
            ));
        }
        ButtonShape::Circle => {
            for i in 0..3 {
                p.circle_filled(Pos2::new(r.left() + 10.0 + i as f32 * 20.0, r.center().y), 8.5, c);
            }
        }
    }
}

impl SettingsApp {
    fn new(cc: &eframe::CreationContext<'_>, wizard: bool, icon_target: Option<String>) -> Self {
        let ctx = cc.egui_ctx.clone();
        let mut cfg = Config::load();
        apply_style(&ctx);
        let first_time = !Config::exists();
        let wizard = wizard || first_time;
        if first_time {
            // Nicer starting point than the raw defaults: follow the Windows accent colour.
            if palette::windows_accent().is_some() {
                cfg.look.accent_source = AccentSource::Windows;
            }
        }
        crate::fonts::install(&ctx, &cfg.look);
        let font_catalog = crate::fonts::FontCatalog::load(&ctx);
        let apps = AppList::new();
        if apps.snapshot().is_empty() {
            apps.refresh(cfg.general.include_store_apps, Some(ctx.clone()));
        }
        let mut app = SettingsApp {
            icons: IconCache::new(&ctx, cfg.icons.overrides.clone()),
            saved: cfg.clone(),
            font_installed: crate::fonts::key(&cfg.look),
            cfg,
            changed_at: None,
            saved_at: None,
            wizard: if wizard { Some(0) } else { None },
            page: if icon_target.is_some() { Page::Icons } else { Page::Look },
            image_tex: None,
            image_loaded: None,
            swatches: Vec::new(),
            image_error: None,
            pending_regen: false,
            apps,
            preview_power: false,
            recording: None,
            hotkey_errors: Vec::new(),
            last_status: Instant::now() - Duration::from_secs(10),
            icon_filter: String::new(),
            icon_focus: icon_target.clone(),
            icon_error: None,
            win11: platform::is_windows_11(),
            daemon_running: platform::instance_running(crate::popup::DAEMON_MUTEX),
            last_daemon_check: Instant::now(),
            font_catalog,
            font_filter: String::new(),
        };
        // Pre-fill the icon search with the app the user right-clicked.
        if let Some(id) = icon_target {
            let name = app
                .apps
                .snapshot()
                .into_iter()
                .find(|a| a.id == id)
                .map(|a| a.name)
                .or_else(|| icons::OVERRIDABLE_BUILTINS.iter().find(|b| b.0 == id).map(|b| b.1.to_string()))
                .unwrap_or_default();
            app.icon_filter = name;
        }
        app.ensure_image(&ctx);
        if first_time {
            palette::regenerate(&mut app.cfg.look, &app.swatches);
        }
        app
    }

    // -----------------------------------------------------------------------
    // State helpers
    // -----------------------------------------------------------------------

    /// (Re)load the picture texture and its colours when the path changes.
    fn ensure_image(&mut self, ctx: &egui::Context) {
        if self.image_loaded.as_ref() == Some(&self.cfg.look.image) {
            return;
        }
        self.image_loaded = Some(self.cfg.look.image.clone());
        self.image_error = None;
        let img = match &self.cfg.look.image {
            Some(p) => match palette::load_image(p) {
                Some(i) => Some(i),
                None => {
                    self.image_error = Some("That picture couldn't be opened. Try a PNG or JPG.".into());
                    None
                }
            },
            None => None,
        };
        self.swatches = img.as_ref().map(|i| palette::extract_colors(i, 6)).unwrap_or_default();
        let img = img.unwrap_or_else(|| palette::gradient_image(&self.cfg.look.palette));
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
        self.image_tex = Some(ctx.load_texture("settings-picture", ci, egui::TextureOptions::LINEAR));
    }

    fn regen(&mut self) {
        palette::regenerate(&mut self.cfg.look, &self.swatches);
        if self.cfg.look.image.is_none() {
            // The fallback gradient follows the colours, so rebuild it.
            self.image_loaded = None;
        }
    }

    fn pick_image(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .set_title("Choose a picture for Hestia")
            .add_filter("Pictures", &["png", "jpg", "jpeg", "webp", "bmp", "gif"])
            .pick_file()
        {
            self.cfg.look.image = Some(p);
            self.cfg.look.image_zoom = 1.0;
            self.cfg.look.image_focus = [0.5, 0.5];
            self.image_loaded = None;
            self.pending_regen = true;
        }
    }

    fn autosave(&mut self, ctx: &egui::Context) {
        if self.cfg != self.saved {
            let since = *self.changed_at.get_or_insert_with(Instant::now);
            if since.elapsed() > Duration::from_millis(350) {
                if self.cfg.general.start_with_windows != self.saved.general.start_with_windows {
                    platform::set_start_with_windows(self.cfg.general.start_with_windows);
                }
                if self.cfg.save().is_ok() {
                    self.saved = self.cfg.clone();
                    self.saved_at = Some(Instant::now());
                }
                self.changed_at = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(120));
            }
        } else {
            self.changed_at = None;
        }
        let fk = crate::fonts::key(&self.cfg.look);
        if fk != self.font_installed {
            self.font_installed = fk;
            crate::fonts::install(ctx, &self.cfg.look);
        }
    }

    fn poll_status(&mut self) {
        if self.last_status.elapsed() > Duration::from_secs(1) {
            self.last_status = Instant::now();
            self.hotkey_errors = std::fs::read_to_string(config::data_dir().join("status.json"))
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| {
                    v.get("hotkey_errors")?.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                })
                .unwrap_or_default();
        }
        if self.last_daemon_check.elapsed() > Duration::from_secs(2) {
            self.last_daemon_check = Instant::now();
            self.daemon_running = platform::instance_running(crate::popup::DAEMON_MUTEX);
        }
    }

    fn start_daemon(&mut self) {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe).spawn();
        }
        self.daemon_running = true;
    }

    fn quit_daemon(&mut self) {
        let _ = std::fs::write(config::data_dir().join("quit.flag"), "quit");
        self.daemon_running = false;
    }

    fn preview_items(&self) -> Vec<Item> {
        if self.preview_power {
            let mut list = vec!["lock", "sleep", "signout", "restart", "shutdown"];
            if platform::hibernate_enabled() {
                list.insert(3, "hibernate");
            }
            return list
                .into_iter()
                .map(|n| Item {
                    id: format!("power:{n}"),
                    label: match n {
                        "lock" => "Lock",
                        "sleep" => "Sleep",
                        "signout" => "Sign out",
                        "hibernate" => "Hibernate",
                        "restart" => "Restart",
                        _ => "Shut down",
                    }
                    .into(),
                    sub: String::new(),
                    icon: IconSrc::Builtin(match n {
                        "shutdown" => "shutdown",
                        other => match other {
                            "lock" => "lock",
                            "sleep" => "sleep",
                            "signout" => "signout",
                            "hibernate" => "hibernate",
                            _ => "restart",
                        },
                    }),
                })
                .collect();
        }
        let apps = self.apps.snapshot();
        if apps.is_empty() {
            return ["Browser", "Files", "Terminal", "Music", "Photos", "Mail", "Calendar", "Notes", "Games", "Settings"]
                .iter()
                .map(|n| Item {
                    id: format!("sample:{n}"),
                    label: n.to_string(),
                    sub: String::new(),
                    icon: IconSrc::Builtin("app"),
                })
                .collect();
        }
        let usage = crate::fuzzy::UsageStore::load();
        let mut apps = apps;
        apps.sort_by(|a, b| usage.boost(&b.id).cmp(&usage.boost(&a.id)).then(a.name.cmp(&b.name)));
        apps.into_iter()
            .take(30)
            .map(|a| Item { id: a.id, label: a.name, sub: String::new(), icon: IconSrc::Shell(a.icon_source) })
            .collect()
    }

    /// Draw a layout scaled to fit inside `area`. Returns the rect it used.
    #[allow(clippy::too_many_arguments)]
    fn draw_layout(
        &mut self,
        ui: &mut egui::Ui,
        layout: &Layout,
        area: Rect,
        power: bool,
        salt: &str,
        items: &[Item],
        edit: bool,
    ) -> Rect {
        let colors = Colors::new(&self.cfg.look.palette, self.cfg.look.opacity);
        let header = platform::user_at_host();
        let info = platform::uptime_text();
        let ppp = ui.ctx().pixels_per_point();
        let mut measure = Scene {
            colors,
            scale: 1.0,
            opts: layout::LookOpts::from(&self.cfg.look),
            alpha: 1.0,
            base_font: 15.0,
            ppp,
            image: self.image_tex.as_ref(),
            items,
            selected: 0,
            first_row: 0,
            mode: if power { 3 } else { 0 },
            header: &header,
            info: &info,
            status: "",
            query: None,
            query_preview: "",
            focus_search: false,
            interactive: false,
            icons: &mut self.icons,
            id_salt: salt,
            edit_picture: edit,
            clip: Vec::new(),
        };
        let natural = measure.layout_size(layout);
        let fit = (area.width() / natural.x).min(area.height() / natural.y).min(self.cfg.look.size);
        measure.scale = fit;
        let size = measure.layout_size(layout);
        let origin = Pos2::new(area.center().x - size.x / 2.0, area.center().y - size.y / 2.0);
        let out = measure.draw(ui, layout, origin);
        // Dragging / scrolling the picture in the preview reframes it.
        if out.pan != Vec2::ZERO || out.zoom != 1.0 {
            let look = &mut self.cfg.look;
            look.image_zoom = (look.image_zoom * out.zoom).clamp(1.0, 5.0);
            // Start from what's actually on screen so there's no "dead zone" at the edges.
            let c = out.picture_uv.map(|r| r.center()).unwrap_or(Pos2::new(look.image_focus[0], look.image_focus[1]));
            look.image_focus[0] = (c.x + out.pan.x).clamp(0.0, 1.0);
            look.image_focus[1] = (c.y + out.pan.y).clamp(0.0, 1.0);
        }
        Rect::from_min_size(origin, size)
    }

    // -----------------------------------------------------------------------
    // Shared panels
    // -----------------------------------------------------------------------

    fn preview_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Live preview").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.selectable_value(&mut self.preview_power, true, "Power menu");
                ui.selectable_value(&mut self.preview_power, false, "Launcher");
            });
        });
        ui.label(RichText::new("Drag the picture to move it, scroll over it to zoom.").color(MUTED).small());
        let avail = ui.available_rect_before_wrap();
        let area = Rect::from_min_size(avail.min, Vec2::new(avail.width(), (avail.height() - 10.0).max(200.0)));
        // A soft "desktop" behind the popup.
        ui.painter().rect_filled(area, Rounding::same(14.0), Color32::from_rgb(0x0E, 0x0C, 0x12));
        let layout = layout::resolve(&self.cfg.look, self.preview_power);
        let items = self.preview_items();
        let inner = area.shrink(24.0);
        let power = self.preview_power;
        self.draw_layout(ui, &layout, inner, power, "preview", &items, true);
        ui.allocate_rect(area, Sense::hover());
    }

    fn gallery(&mut self, ui: &mut egui::Ui, power: bool, tile_w: f32) {
        let layouts = if power { layout::power_layouts() } else { layout::launcher_layouts() };
        let current = if power { self.cfg.look.power_layout.clone() } else { self.cfg.look.launcher_layout.clone() };
        let items = {
            let keep = self.preview_power;
            self.preview_power = power;
            let it = self.preview_items();
            self.preview_power = keep;
            it
        };
        let tile_h = tile_w * 0.62 + 52.0;
        let per_row = ((ui.available_width() + 12.0) / (tile_w + 12.0)).floor().max(1.0) as usize;
        let mut chosen: Option<String> = None;
        for chunk in layouts.chunks(per_row) {
            ui.horizontal(|ui| {
                for l in chunk {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(tile_w, tile_h), Sense::click());
                    let selected = l.id == current;
                    let bg = if resp.hovered() { Color32::from_rgb(0x30, 0x2A, 0x3C) } else { CARD };
                    ui.painter().rect_filled(rect, Rounding::same(12.0), bg);
                    if selected {
                        ui.painter().rect_stroke(rect, Rounding::same(12.0), Stroke::new(2.5f32, FLAME));
                    }
                    let thumb = Rect::from_min_max(
                        rect.min + Vec2::new(12.0, 12.0),
                        Pos2::new(rect.right() - 12.0, rect.bottom() - 52.0),
                    );
                    let salt = format!("g-{}-{}", power, l.id);
                    self.draw_layout(ui, l, thumb, power, &salt, &items, false);
                    ui.painter().text(
                        Pos2::new(rect.left() + 14.0, rect.bottom() - 40.0),
                        egui::Align2::LEFT_TOP,
                        l.name,
                        FontId::new(15.0, FontFamily::Name("bold".into())),
                        TEXT,
                    );
                    // Small tiles: show the description as a tooltip instead.
                    if tile_w >= 250.0 {
                        ui.painter().text(
                            Pos2::new(rect.left() + 14.0, rect.bottom() - 20.0),
                            egui::Align2::LEFT_TOP,
                            l.blurb,
                            FontId::proportional(12.0),
                            MUTED,
                        );
                    }
                    let resp = resp.on_hover_text(l.blurb);
                    if resp.clicked() {
                        chosen = Some(l.id.to_string());
                    }
                }
            });
            ui.add_space(4.0);
        }
        if let Some(id) = chosen {
            if power {
                self.cfg.look.power_layout = id;
                self.preview_power = true;
            } else {
                self.cfg.look.launcher_layout = id;
                self.preview_power = false;
            }
        }
    }

    fn picture_card(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            section(ui, "PICTURE");
            ui.horizontal(|ui| {
                if let Some(tex) = &self.image_tex {
                    let size = tex.size_vec2();
                    let h = 90.0;
                    let w = (size.x / size.y * h).min(160.0);
                    ui.add(
                        egui::Image::new(egui::load::SizedTexture::new(tex.id(), size))
                            .fit_to_exact_size(Vec2::new(w, h))
                            .rounding(8.0),
                    );
                }
                ui.vertical(|ui| {
                    let name = self
                        .cfg
                        .look
                        .image
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "No picture — using a colour gradient".into());
                    ui.label(RichText::new(name).color(MUTED));
                    ui.horizontal(|ui| {
                        if ui.button("Choose picture…").clicked() {
                            self.pick_image();
                        }
                        if self.cfg.look.image.is_some() && ui.button("Remove").clicked() {
                            self.cfg.look.image = None;
                            self.image_loaded = None;
                            self.pending_regen = true;
                        }
                    });
                    if let Some(e) = &self.image_error {
                        ui.colored_label(Color32::from_rgb(0xFF, 0x7A, 0x7A), e);
                    }
                });
            });
            egui::Grid::new("framing").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                ui.label("Zoom");
                ui.horizontal(|ui| {
                    ui.add(egui::Slider::new(&mut self.cfg.look.image_zoom, 1.0..=5.0).show_value(false));
                    if ui.button("Reset framing").clicked() {
                        self.cfg.look.image_zoom = 1.0;
                        self.cfg.look.image_focus = [0.5, 0.5];
                    }
                });
                ui.end_row();
                ui.label("Buttons over picture")
                    .on_hover_text("How solid the search box and buttons are where they sit on your picture");
                ui.horizontal(|ui| {
                    let mut pct = self.cfg.look.overlay_opacity * 100.0;
                    if ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix("%").integer()).changed() {
                        self.cfg.look.overlay_opacity = pct / 100.0;
                    }
                });
                ui.end_row();
                ui.label("Background darkening")
                    .on_hover_text("For layouts that use your picture as the whole background (like Grid)");
                ui.horizontal(|ui| {
                    let mut pct = self.cfg.look.picture_dim * 100.0;
                    if ui.add(egui::Slider::new(&mut pct, 0.0..=90.0).suffix("%").integer()).changed() {
                        self.cfg.look.picture_dim = pct / 100.0;
                    }
                });
                ui.end_row();
            });
            ui.label(
                RichText::new("Tip: drag the picture in the preview to frame it, and scroll over it to zoom.")
                    .color(MUTED)
                    .small(),
            );
        });
    }

    fn shape_card(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            section(ui, "SHAPE");
            ui.label(RichText::new("The shape of the Hestia window. Everything inside rearranges to fit.").color(MUTED).small());
            let sides = self.cfg.look.sides.clamp(3, 12) as u8;
            let shapes = [
                (WindowShape::Rectangle, "Rectangle", layout::Outline::Rect),
                (WindowShape::Arch, "Arch", layout::Outline::Arch),
                (WindowShape::Circle, "Circle", layout::Outline::Polygon(0)),
                (WindowShape::Polygon, "Polygon", layout::Outline::Polygon(sides)),
            ];
            ui.horizontal_wrapped(|ui| {
                for (shape, label, outline) in shapes {
                    if shape_choice(ui, label, self.cfg.look.shape == shape, |p, r| draw_outline_icon(p, r, outline)) {
                        self.cfg.look.shape = shape;
                    }
                }
            });
            if self.cfg.look.shape == WindowShape::Polygon {
                ui.horizontal(|ui| {
                    ui.label("Sides");
                    ui.add(egui::Slider::new(&mut self.cfg.look.sides, 3..=12).integer());
                    let name = match self.cfg.look.sides {
                        3 => "Triangle",
                        4 => "Square",
                        5 => "Pentagon",
                        6 => "Hexagon",
                        7 => "Heptagon",
                        8 => "Octagon",
                        9 => "Nonagon",
                        10 => "Decagon",
                        _ => "",
                    };
                    ui.label(RichText::new(name).color(MUTED));
                });
            }
            if self.cfg.look.shape != WindowShape::Rectangle {
                ui.horizontal(|ui| {
                    ui.label("Shape size");
                    ui.add(egui::Slider::new(&mut self.cfg.look.shape_size, 0.7..=1.4).show_value(false));
                    if ui.small_button("Reset").clicked() {
                        self.cfg.look.shape_size = 1.0;
                    }
                });
            }
            match self.cfg.look.shape {
                WindowShape::Circle | WindowShape::Polygon => {
                    section(ui, "MENU STYLE");
                    let styles = [
                        (MenuStyle::Rings, "Rings", "Results circle your picture; buttons form an outer ring"),
                        (MenuStyle::Box, "Box", "A flat menu comes out of the side of your picture"),
                        (MenuStyle::Inside, "Inside", "Everything fits inside the shape"),
                    ];
                    let outline = if self.cfg.look.shape == WindowShape::Circle { 0 } else { sides };
                    ui.horizontal_wrapped(|ui| {
                        for (style, label, _) in styles {
                            let picked = shape_choice(ui, label, self.cfg.look.menu_style == style, |p, r| {
                                draw_style_icon(p, r, style, outline)
                            });
                            if picked {
                                self.cfg.look.menu_style = style;
                            }
                        }
                    });
                    let tip = styles.iter().find(|s| s.0 == self.cfg.look.menu_style).map(|s| s.2).unwrap_or("");
                    ui.label(RichText::new(tip).color(MUTED).small());
                    ui.label(RichText::new("Your power menu matches this shape.").color(MUTED).small());
                }
                WindowShape::Arch => {
                    ui.label(
                        RichText::new("Your picture fills the dome; apps sit in the flat part underneath.").color(MUTED).small(),
                    );
                }
                WindowShape::Rectangle => {
                    section(ui, "LAYOUT");
                    self.gallery(ui, false, 170.0);
                    section(ui, "POWER MENU LAYOUT");
                    self.gallery(ui, true, 170.0);
                    section(ui, "PICTURE FRAME");
                    ui.label(
                        RichText::new("Fill uses the whole picture area. The other frames sit in the open space, so nothing covers your picture.")
                            .color(MUTED)
                            .small(),
                    );
                    let pics = [
                        (PictureShape::Fill, "Fill"),
                        (PictureShape::Circle, "Circle"),
                        (PictureShape::Squircle, "Soft square"),
                        (PictureShape::Arch, "Arch"),
                        (PictureShape::Hexagon, "Hexagon"),
                        (PictureShape::Diamond, "Diamond"),
                    ];
                    ui.horizontal_wrapped(|ui| {
                        for (shape, label) in pics {
                            if shape_choice(ui, label, self.cfg.look.picture_shape == shape, |p, r| {
                                draw_picture_icon(p, r, shape)
                            }) {
                                self.cfg.look.picture_shape = shape;
                            }
                        }
                    });
                }
            }
        });
    }

    fn details_card(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            section(ui, "BUTTONS");
            let btns = [
                (ButtonShape::Theme, "Default"),
                (ButtonShape::Square, "Square"),
                (ButtonShape::Pill, "Pill"),
                (ButtonShape::Slanted, "Slanted"),
                (ButtonShape::Hexagon, "Hexagon"),
                (ButtonShape::Circle, "Bubbles"),
            ];
            ui.horizontal_wrapped(|ui| {
                for (shape, label) in btns {
                    if shape_choice(ui, label, self.cfg.look.button_shape == shape, |p, r| draw_button_icon(p, r, shape)) {
                        self.cfg.look.button_shape = shape;
                    }
                }
            });
            if self.cfg.look.button_shape == ButtonShape::Circle {
                ui.label(
                    RichText::new("Bubbles: tiles become circles, mode buttons become round icons, and list items get a round bubble behind the icon.")
                        .color(MUTED)
                        .small(),
                );
            }
            section(ui, "SIZE & FEEL");
            egui::Grid::new("style").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                ui.label("Size");
                ui.add(egui::Slider::new(&mut self.cfg.look.size, 0.6..=1.6).show_value(false));
                ui.end_row();
                ui.label("Roundness");
                ui.add(egui::Slider::new(&mut self.cfg.look.roundness, 0.0..=2.0).show_value(false));
                ui.end_row();
                ui.label("Animations");
                toggle(ui, &mut self.cfg.look.animations);
                ui.end_row();
            });
            section(ui, "SEE-THROUGH EFFECT");
            ui.horizontal_wrapped(|ui| {
                let before = self.cfg.look.effect;
                ui.selectable_value(&mut self.cfg.look.effect, Effect::None, "None");
                ui.selectable_value(&mut self.cfg.look.effect, Effect::Blur, "Blur");
                ui.selectable_value(&mut self.cfg.look.effect, Effect::Acrylic, "Acrylic");
                let mica = ui.add_enabled(self.win11, egui::SelectableLabel::new(self.cfg.look.effect == Effect::Mica, "Mica"));
                if mica.clicked() {
                    self.cfg.look.effect = Effect::Mica;
                }
                if !self.win11 {
                    mica.on_disabled_hover_text("Mica needs Windows 11");
                }
                if before != self.cfg.look.effect && self.cfg.look.effect != Effect::None && self.cfg.look.opacity > 0.95 {
                    self.cfg.look.opacity = 0.8;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Opacity");
                ui.add(egui::Slider::new(&mut self.cfg.look.opacity, 0.3..=1.0).show_value(false));
            });
            let note = if self.cfg.look.shape != WindowShape::Rectangle && self.cfg.look.effect != Effect::None {
                "Blur effects only work on rectangle windows, so this shape uses plain see-through instead. Lower the opacity to see your desktop through it."
            } else {
                match (self.cfg.look.effect, self.win11) {
                    (Effect::None, _) => "Lower the opacity to let your desktop show through.",
                    (_, false) if self.cfg.look.effect != Effect::Mica => {
                        "On Windows 10, blur on rectangle layouts shows square corners. Mica isn't available on Windows 10."
                    }
                    (Effect::Mica, _) => "Mica tints the window with your wallpaper colours. Works best with low opacity.",
                    _ => "Blur shows through wherever the panel is see-through.",
                }
            };
            ui.label(RichText::new(note).color(MUTED).small());
        });
    }

    fn fonts_card(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            section(ui, "FONTS");
            let fonts = self.font_catalog.list();
            let ready = self.font_catalog.ready.load(std::sync::atomic::Ordering::SeqCst);
            for heading in [false, true] {
                ui.horizontal(|ui| {
                    ui.label(if heading { "Titles & buttons" } else { "Main text" });
                    let current = if heading {
                        if self.cfg.look.heading_font.is_empty() {
                            "Same as main (bold)".to_string()
                        } else {
                            self.cfg.look.heading_font.clone()
                        }
                    } else {
                        self.cfg.look.font.clone()
                    };
                    let id = if heading { "font-heading" } else { "font-main" };
                    egui::ComboBox::from_id_salt(id).selected_text(current).width(240.0).height(360.0).show_ui(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.font_filter)
                                .hint_text("Type to search fonts…")
                                .desired_width(220.0),
                        );
                        if !ready {
                            ui.label(RichText::new("Finding your fonts…").color(MUTED));
                        }
                        if heading && ui.selectable_label(self.cfg.look.heading_font.is_empty(), "Same as main (bold)").clicked()
                        {
                            self.cfg.look.heading_font.clear();
                            self.cfg.look.heading_font_file = None;
                        }
                        let f = self.font_filter.to_lowercase();
                        for e in fonts.iter().filter(|e| f.is_empty() || e.family.to_lowercase().contains(&f)) {
                            let sel =
                                if heading { self.cfg.look.heading_font == e.family } else { self.cfg.look.font == e.family };
                            if ui.selectable_label(sel, &e.family).clicked() {
                                if heading {
                                    self.cfg.look.heading_font = e.family.clone();
                                    // Prefer the bold cut for titles when there is one.
                                    self.cfg.look.heading_font_file = e.bold.clone().or_else(|| Some(e.regular.clone()));
                                } else {
                                    self.cfg.look.font = e.family.clone();
                                    self.cfg.look.font_file = Some(e.regular.clone());
                                    self.cfg.look.font_bold_file = e.bold.clone();
                                }
                            }
                        }
                    });
                    if ui.small_button("From file…").on_hover_text("Use a .ttf or .otf file that isn't installed").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("Font", &["ttf", "otf"]).pick_file() {
                            let name = crate::fonts::describe(&p)
                                .map(|d| d.0)
                                .unwrap_or_else(|| p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
                            if heading {
                                self.cfg.look.heading_font = name;
                                self.cfg.look.heading_font_file = Some(p);
                            } else {
                                self.cfg.look.font = name;
                                self.cfg.look.font_file = Some(p);
                                self.cfg.look.font_bold_file = None;
                            }
                        }
                    }
                });
            }
            // A sample in the chosen fonts.
            ui.label(RichText::new("Hestia — The quick brown fox").family(FontFamily::Name("bold".into())).size(18.0));
            ui.label(RichText::new("Firefox · Files · Terminal · Music · Photos").size(15.0));
        });
    }

    fn colours_card(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            section(ui, "COLOURS");
            let mut regen = false;
            ui.horizontal(|ui| {
                ui.label("Style");
                for (m, label) in [(ColorMode::Dark, "Dark"), (ColorMode::Colour, "Colourful"), (ColorMode::Light, "Light")] {
                    if ui.selectable_label(self.cfg.look.mode == m && self.cfg.look.palette_source == "auto", label).clicked() {
                        self.cfg.look.mode = m;
                        self.cfg.look.palette_source = "auto".into();
                        regen = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Accent");
                let win_ok = palette::windows_accent().is_some();
                if ui.selectable_label(self.cfg.look.accent_source == AccentSource::Image, "From my picture").clicked() {
                    self.cfg.look.accent_source = AccentSource::Image;
                    regen = true;
                }
                if ui
                    .add_enabled(
                        win_ok,
                        egui::SelectableLabel::new(self.cfg.look.accent_source == AccentSource::Windows, "Windows accent"),
                    )
                    .clicked()
                {
                    self.cfg.look.accent_source = AccentSource::Windows;
                    regen = true;
                }
                if ui.selectable_label(self.cfg.look.accent_source == AccentSource::Custom, "My own").clicked() {
                    self.cfg.look.accent_source = AccentSource::Custom;
                    regen = true;
                }
                if self.cfg.look.accent_source == AccentSource::Custom {
                    let mut c = [self.cfg.look.custom_accent.0, self.cfg.look.custom_accent.1, self.cfg.look.custom_accent.2];
                    if egui::color_picker::color_edit_button_srgb(ui, &mut c).changed() {
                        self.cfg.look.custom_accent = Rgb(c[0], c[1], c[2]);
                        regen = true;
                    }
                }
            });
            if !self.swatches.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Colours in your picture:").color(MUTED).small());
                    for s in self.swatches.clone() {
                        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
                        ui.painter().circle_filled(rect.center(), 12.0, s.color.c32());
                        if resp.hovered() {
                            ui.painter().circle_stroke(rect.center(), 13.0, Stroke::new(2.0f32, TEXT));
                        }
                        if resp.on_hover_text("Use as accent").clicked() {
                            self.cfg.look.custom_accent = s.color;
                            self.cfg.look.accent_source = AccentSource::Custom;
                            regen = true;
                        }
                    }
                });
            }
            ui.horizontal(|ui| {
                ui.label("Ready-made scheme");
                let current = palette::preset(&self.cfg.look.palette_source).map(|p| p.name).unwrap_or("Made from my picture");
                egui::ComboBox::from_id_salt("scheme").selected_text(current).width(220.0).show_ui(ui, |ui| {
                    if ui.selectable_label(self.cfg.look.palette_source == "auto", "Made from my picture").clicked() {
                        self.cfg.look.palette_source = "auto".into();
                        regen = true;
                    }
                    for p in palette::PRESETS {
                        if ui.selectable_label(self.cfg.look.palette_source == p.id, p.name).clicked() {
                            self.cfg.look.palette_source = p.id.into();
                            if self.cfg.look.accent_source == AccentSource::Image {
                                // Keep the scheme's own accent.
                            }
                            regen = true;
                        }
                    }
                });
            });
            if regen {
                self.regen();
            }
            ui.collapsing("Fine-tune each colour", |ui| {
                ui.label(RichText::new("Changing the picture, style or accent recalculates these.").color(MUTED).small());
                let p = &mut self.cfg.look.palette;
                let rows: [(&str, &mut Rgb); 6] = [
                    ("Background", &mut p.background),
                    ("Panels & buttons", &mut p.background_alt),
                    ("Text", &mut p.foreground),
                    ("Selected item", &mut p.selected),
                    ("Second accent", &mut p.active),
                    ("Third accent", &mut p.urgent),
                ];
                egui::Grid::new("colours").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                    for (label, c) in rows {
                        ui.label(label);
                        let mut a = [c.0, c.1, c.2];
                        if egui::color_picker::color_edit_button_srgb(ui, &mut a).changed() {
                            *c = Rgb(a[0], a[1], a[2]);
                        }
                        ui.end_row();
                    }
                });
            });
        });
    }

    // -----------------------------------------------------------------------
    // Pages
    // -----------------------------------------------------------------------

    fn page_look(&mut self, ui: &mut egui::Ui) {
        // (The live preview has its own side panel; see `update`.)
        {
            egui::ScrollArea::vertical().id_salt("look-left").show(ui, |ui| {
                ui.heading("Look");
                self.picture_card(ui);
                self.colours_card(ui);
                self.shape_card(ui);
                self.fonts_card(ui);
                self.details_card(ui);
                ui.add_space(20.0);
            });
        }
    }

    fn page_icons(&mut self, ui: &mut egui::Ui) {
        ui.heading("Icons");
        ui.label(
            RichText::new(
                "Give any app or button your own icon (.svg or .ico). Single-colour SVG icons can follow your theme's colours automatically.",
            )
            .color(MUTED),
        );
        if let Some(e) = &self.icon_error {
            ui.colored_label(Color32::from_rgb(0xFF, 0x7A, 0x7A), e);
        }
        ui.horizontal(|ui| {
            ui.label("Search");
            ui.add(egui::TextEdit::singleline(&mut self.icon_filter).hint_text("App or button name").desired_width(280.0));
        });
        let filter = self.icon_filter.to_lowercase();
        let mut rows: Vec<(String, String, IconSrc)> = icons::OVERRIDABLE_BUILTINS
            .iter()
            .map(|(id, label, icon)| (id.to_string(), format!("{label} (Hestia)"), IconSrc::Builtin(icon)))
            .collect();
        rows.extend(self.apps.snapshot().into_iter().map(|a| (a.id, a.name, IconSrc::Shell(a.icon_source))));
        rows.retain(|r| filter.is_empty() || r.1.to_lowercase().contains(&filter) || r.0.contains(&filter));

        let tint = self.cfg.look.palette.foreground.c32();
        let row_h = 46.0;
        let mut action: Option<(String, &'static str)> = None;
        egui::ScrollArea::vertical().id_salt("icons").show_rows(ui, row_h, rows.len(), |ui, range| {
            for (id, label, src) in &rows[range] {
                let highlighted = self.icon_focus.as_deref() == Some(id.as_str());
                let frame = egui::Frame::none()
                    .fill(if highlighted { Color32::from_rgb(0x3A, 0x2C, 0x24) } else { CARD })
                    .rounding(10.0)
                    .inner_margin(egui::Margin::symmetric(12.0, 6.0));
                frame.show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_height(row_h - 16.0);
                    ui.horizontal_centered(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
                        // Icon preview on a theme-coloured tile.
                        ui.painter().rect_filled(
                            rect.expand(3.0),
                            Rounding::same(6.0),
                            self.cfg.look.palette.background_alt.c32(),
                        );
                        let px = (30.0 * ui.ctx().pixels_per_point()) as u32;
                        if let Some(ic) = self.icons.get(id, src, px).or_else(|| self.icons.placeholder(px)) {
                            let t = if ic.tintable { tint } else { Color32::WHITE };
                            ui.painter().image(ic.id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), t);
                        }
                        ui.label(label);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let has = self.cfg.icons.overrides.contains_key(id);
                            if has {
                                if ui.button("Reset").clicked() {
                                    action = Some((id.clone(), "reset"));
                                }
                                let is_svg = self.cfg.icons.overrides.get(id).map(|o| icons::is_svg_path(&o.svg)).unwrap_or(true);
                                let mut rec = self.cfg.icons.overrides.get(id).map(|o| o.recolor).unwrap_or(true);
                                // .ico/.png icons keep their own colours, so there's nothing to recolour.
                                if is_svg && ui.checkbox(&mut rec, "Match theme colours").changed() {
                                    if let Some(o) = self.cfg.icons.overrides.get_mut(id) {
                                        o.recolor = rec;
                                    }
                                    action = Some((id.clone(), "refresh"));
                                }
                            }
                            if ui
                                .button(if has { "Change icon…" } else { "Choose icon…" })
                                .on_hover_text("An .svg or .ico file")
                                .clicked()
                            {
                                action = Some((id.clone(), "choose"));
                            }
                        });
                    });
                });
            }
        });
        if let Some((id, what)) = action {
            self.icon_error = None;
            match what {
                "reset" => {
                    self.cfg.icons.overrides.remove(&id);
                }
                "choose" => {
                    if let Some(p) = rfd::FileDialog::new()
                        .set_title("Choose an icon")
                        .add_filter("Icons (.svg, .ico)", &["svg", "ico", "png"])
                        .pick_file()
                    {
                        match std::fs::read(&p) {
                            Ok(bytes)
                                if (icons::is_svg_path(&p) && icons::rasterize_svg(&bytes, 32, true).is_some())
                                    || (!icons::is_svg_path(&p) && icons::raster_icon(&bytes, 32).is_some()) =>
                            {
                                // Keep a copy so the icon survives if the original file moves.
                                let dir = config::data_dir().join("icons");
                                let _ = std::fs::create_dir_all(&dir);
                                let safe: String = id.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
                                let ext =
                                    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "svg".into());
                                // Remove copies with other extensions so only one icon file remains.
                                for old in ["svg", "ico", "png"] {
                                    let _ = std::fs::remove_file(dir.join(format!("{safe}.{old}")));
                                }
                                let dest = dir.join(format!("{safe}.{ext}"));
                                let svg = if std::fs::write(&dest, &bytes).is_ok() { dest } else { p };
                                self.cfg.icons.overrides.insert(id.clone(), IconOverride { svg, recolor: true });
                                self.icon_focus = Some(id.clone());
                            }
                            _ => {
                                self.icon_error = Some("That file doesn't look like an .svg or .ico icon Hestia can draw.".into())
                            }
                        }
                    }
                }
                _ => {}
            }
            self.icons.set_overrides(self.cfg.icons.overrides.clone());
            self.icons.clear();
        }
    }

    fn hotkey_rows(&mut self, ui: &mut egui::Ui) {
        let names = [
            ("Open the app launcher", "apps"),
            ("Switch between open windows", "windows"),
            ("Run a command", "run"),
            ("Open the power menu", "power"),
        ];
        for (i, (title, key)) in names.iter().enumerate() {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(260.0);
                        ui.label(RichText::new(*title).strong());
                        if self.hotkey_errors.iter().any(|e| e == key) {
                            ui.colored_label(
                                Color32::from_rgb(0xFF, 0x9A, 0x7A),
                                "Another app is already using this shortcut. Try a different one.",
                            );
                        }
                    });
                    let combo = self.global_combo(i).clone();
                    if self.recording == Some(Recording::Global(i)) {
                        ui.label(RichText::new("Press the new shortcut now…").color(FLAME));
                        if ui.button("Cancel").clicked() {
                            self.recording = None;
                        }
                    } else {
                        keycaps(ui, &combo);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Clear").clicked() {
                                *self.global_combo_mut(i) = KeyCombo::default();
                            }
                            if ui.button("Change").clicked() {
                                self.recording = Some(Recording::Global(i));
                            }
                            let c = self.global_combo_mut(i);
                            ui.checkbox(&mut c.win, "Win");
                        });
                    }
                });
            });
        }
        ui.label(
            RichText::new("Tip: tick “Win” to add the Windows key — Hestia can't detect it while recording.")
                .color(MUTED)
                .small(),
        );
    }

    fn global_combo(&self, i: usize) -> &KeyCombo {
        let h = &self.cfg.hotkeys;
        [&h.apps, &h.windows, &h.run, &h.power][i]
    }

    fn global_combo_mut(&mut self, i: usize) -> &mut KeyCombo {
        let h = &mut self.cfg.hotkeys;
        match i {
            0 => &mut h.apps,
            1 => &mut h.windows,
            2 => &mut h.run,
            _ => &mut h.power,
        }
    }

    fn popup_key_list(&mut self, i: usize) -> &mut Vec<KeyCombo> {
        let k = &mut self.cfg.keys;
        match i {
            0 => &mut k.up,
            1 => &mut k.down,
            2 => &mut k.left,
            3 => &mut k.right,
            4 => &mut k.open,
            5 => &mut k.close,
            6 => &mut k.next_mode,
            _ => &mut k.prev_mode,
        }
    }

    fn page_hotkeys(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("hotkeys").show(ui, |ui| {
            ui.heading("Shortcuts");
            ui.label(RichText::new("These work anywhere in Windows to open Hestia.").color(MUTED));
            self.hotkey_rows(ui);
            ui.add_space(14.0);
            ui.heading("Keys inside Hestia");
            ui.label(
                RichText::new("Optional — the mouse works everywhere, so you never need to learn these. Click a key to replace it, × to remove it.")
                    .color(MUTED),
            );
            let actions = [
                "Move up",
                "Move down",
                "Move left (grids)",
                "Move right (grids)",
                "Open the selected item",
                "Close Hestia",
                "Next tab",
                "Previous tab",
            ];
            card(ui, |ui| {
                egui::Grid::new("popupkeys").num_columns(2).spacing([20.0, 10.0]).show(ui, |ui| {
                    for (i, label) in actions.iter().enumerate() {
                        ui.label(*label);
                        ui.horizontal(|ui| {
                            let list = self.popup_key_list(i).clone();
                            let mut remove: Option<usize> = None;
                            for (j, combo) in list.iter().enumerate() {
                                if self.recording == Some(Recording::Popup(i, Some(j))) {
                                    ui.label(RichText::new("Press a key…").color(FLAME));
                                    continue;
                                }
                                if ui.button(RichText::new(combo.label()).monospace()).on_hover_text("Click to change").clicked() {
                                    self.recording = Some(Recording::Popup(i, Some(j)));
                                }
                                if ui.small_button("×").clicked() {
                                    remove = Some(j);
                                }
                            }
                            if self.recording == Some(Recording::Popup(i, None)) {
                                ui.label(RichText::new("Press a key…").color(FLAME));
                                if ui.button("Cancel").clicked() {
                                    self.recording = None;
                                }
                            } else if ui.button("+ Add").clicked() {
                                self.recording = Some(Recording::Popup(i, None));
                            }
                            if let Some(j) = remove {
                                self.popup_key_list(i).remove(j);
                            }
                        });
                        ui.end_row();
                    }
                });
            });
            if ui.button("Restore default keys").clicked() {
                self.cfg.keys = Default::default();
            }
            ui.label(RichText::new("Right-click any item for more options, such as “Run as administrator”. Ctrl+Shift+Enter does the same.").color(MUTED).small());
        });
    }

    fn capture_recording(&mut self, ctx: &egui::Context) {
        let Some(rec) = self.recording.clone() else { return };
        let pressed = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        let Some((key, mods)) = pressed else { return };
        let name = keys::name_of(key);
        match rec {
            Recording::Global(i) => {
                if !keys::hotkey_key_supported(&name) {
                    return;
                }
                let win = self.global_combo(i).win;
                *self.global_combo_mut(i) = KeyCombo { ctrl: mods.ctrl, alt: mods.alt, shift: mods.shift, win, key: name };
            }
            Recording::Popup(i, slot) => {
                let combo = KeyCombo { ctrl: mods.ctrl, alt: mods.alt, shift: mods.shift, win: false, key: name };
                let list = self.popup_key_list(i);
                match slot {
                    Some(j) if j < list.len() => list[j] = combo,
                    _ => list.push(combo),
                }
            }
        }
        self.recording = None;
    }

    fn page_general(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("general").show(ui, |ui| {
            ui.heading("General");
            card(ui, |ui| {
                setting_row(ui, "Start with Windows", "Hestia is ready as soon as you sign in.", |ui| {
                    toggle(ui, &mut self.cfg.general.start_with_windows);
                });
                ui.separator();
                setting_row(ui, "Show tray icon", "The Hestia icon near the clock. Click it to open the launcher.", |ui| {
                    toggle(ui, &mut self.cfg.general.show_tray_icon);
                });
                ui.separator();
                setting_row(ui, "Close when I click elsewhere", "Hestia hides when another window gets focus.", |ui| {
                    toggle(ui, &mut self.cfg.general.hide_on_focus_loss);
                });
                ui.separator();
                setting_row(ui, "Ask before shutting down", "Confirm before restart, shut down, sign out and hibernate.", |ui| {
                    toggle(ui, &mut self.cfg.general.confirm_power);
                });
                ui.separator();
                setting_row(ui, "Include Microsoft Store apps", "Finding these takes a second in the background.", |ui| {
                    toggle(ui, &mut self.cfg.general.include_store_apps);
                });
            });
            if !self.cfg.general.show_tray_icon && !self.cfg.general.start_with_windows {
                ui.label(
                    RichText::new("With both of these off, open Hestia from the Start menu or by running hestia.exe; its shortcuts still work while it runs.")
                        .color(MUTED)
                        .small(),
                );
            }
            card(ui, |ui| {
                section(ui, "HESTIA");
                ui.horizontal(|ui| {
                    if self.daemon_running {
                        ui.label(RichText::new("● Running").color(Color32::from_rgb(0x7A, 0xD9, 0x8B)));
                        if ui.button("Quit Hestia").clicked() {
                            self.quit_daemon();
                        }
                    } else {
                        ui.label(RichText::new("● Not running").color(MUTED));
                        if ui.button("Start Hestia").clicked() {
                            self.start_daemon();
                        }
                    }
                    if ui.button("Run the setup again").clicked() {
                        self.wizard = Some(0);
                    }
                    if ui.button("Open settings folder").clicked() {
                        platform::shell_open(&config::data_dir().to_string_lossy(), "open");
                    }
                });
                ui.label(RichText::new(format!("Settings file: {}", config::config_path().display())).color(MUTED).small());
            });
        });
    }

    fn page_about(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("about").show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
                if let Some(ic) = self.icons.get("logo", &IconSrc::Builtin("logo"), 128) {
                    ui.painter().image(ic.id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), FLAME);
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new("Hestia").size(32.0).family(FontFamily::Name("bold".into())));
                    ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(MUTED));
                    ui.hyperlink_to("Project page, updates and source code", env!("CARGO_PKG_REPOSITORY"));
                });
            });
            ui.label(
                RichText::new(
                    "A friendly, good-looking launcher for Windows, named after Hestia, the Greek goddess of home and hearth. \
                     Press a shortcut, type a few letters, and open what you need.",
                )
                .size(16.0),
            );
            card(ui, |ui| {
                section(ui, "WHAT HESTIA DOES");
                for line in [
                    "Opens your apps (including Microsoft Store apps), with your most-used apps first",
                    "Switches between open windows",
                    "Runs commands, like the Windows Run box",
                    "Locks, sleeps, restarts or shuts down your PC",
                    "Builds its look around a picture you choose, with colours taken from it",
                    "Comes in many shapes: rectangle layouts, arches, circles and polygons, with rings or a box",
                    "Lets you use your own .svg and .ico icons, fonts, and shortcuts",
                ] {
                    ui.label(format!("•  {line}"));
                }
            });
            card(ui, |ui| {
                section(ui, "YOUR SHORTCUTS");
                let h = self.cfg.hotkeys.clone();
                egui::Grid::new("about-keys").num_columns(2).spacing([20.0, 8.0]).show(ui, |ui| {
                    for (label, combo) in
                        [("App launcher", &h.apps), ("Window switcher", &h.windows), ("Run a command", &h.run), ("Power menu", &h.power)]
                    {
                        ui.label(label);
                        ui.horizontal(|ui| keycaps(ui, combo));
                        ui.end_row();
                    }
                });
                ui.label(
                    RichText::new("Inside Hestia: type to search, use the mouse or arrow keys, Enter to open, Tab to switch tabs, Esc to close. Right-click anything for more options.")
                        .color(MUTED)
                        .small(),
                );
            });
            card(ui, |ui| {
                section(ui, "YOUR SETTINGS");
                ui.label(format!("Saved in {}", config::config_path().display()));
                ui.label(RichText::new("To share your look with a friend, send them the [look] section of that file.").color(MUTED).small());
                if ui.button("Open settings folder").clicked() {
                    platform::shell_open(&config::data_dir().to_string_lossy(), "open");
                }
            });
            card(ui, |ui| {
                section(ui, "THANKS");
                ui.label("Rectangle layouts and colour schemes are recreated from adi1090x's rofi themes (GPL-3.0):");
                ui.hyperlink("https://github.com/adi1090x/rofi");
                ui.label("Several built-in icons follow the shapes of Feather Icons (MIT).");
                ui.label("Built in Rust with egui.");
                ui.add_space(6.0);
                ui.label(RichText::new("Hestia is free software under the GNU General Public License v3 or later.").color(MUTED));
            });
            ui.add_space(20.0);
        });
    }

    // -----------------------------------------------------------------------
    // Wizard
    // -----------------------------------------------------------------------

    fn wizard_ui(&mut self, ctx: &egui::Context, step: usize) {
        let mut next = step;
        egui::TopBottomPanel::bottom("wizard-nav")
            .exact_height(64.0)
            .frame(egui::Frame::none().fill(PANEL).inner_margin(14.0))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if step > 0 && ui.button("  Back  ").clicked() {
                        next = step - 1;
                    }
                    // Progress dots.
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width() - 160.0, 20.0), Sense::hover());
                    let start = rect.center().x - (WIZARD_STEPS as f32 * 18.0) / 2.0;
                    for i in 0..WIZARD_STEPS {
                        let c = if i == step {
                            FLAME
                        } else if i < step {
                            FLAME.gamma_multiply(0.5)
                        } else {
                            Color32::from_gray(80)
                        };
                        ui.painter().circle_filled(
                            Pos2::new(start + i as f32 * 18.0 + 9.0, rect.center().y),
                            if i == step { 5.5 } else { 4.0 },
                            c,
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = match step {
                            0 => "  Let's go  ",
                            s if s == WIZARD_STEPS - 1 => "  Finish  ",
                            _ => "  Next  ",
                        };
                        let b = egui::Button::new(RichText::new(label).color(Color32::BLACK).strong()).fill(FLAME);
                        if ui.add(b).clicked() {
                            if step == WIZARD_STEPS - 1 {
                                next = usize::MAX;
                            } else {
                                next = step + 1;
                            }
                        }
                    });
                });
            });

        let with_preview = (1..=4).contains(&step);
        if with_preview {
            egui::SidePanel::right("wizard-preview")
                .exact_width((ctx.screen_rect().width() * 0.5).max(420.0))
                .resizable(false)
                .frame(egui::Frame::none().fill(BG).inner_margin(20.0))
                .show(ctx, |ui| self.preview_panel(ui));
        }

        egui::CentralPanel::default().frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::symmetric(36.0, 28.0))).show(
            ctx,
            |ui| {
                egui::ScrollArea::vertical().id_salt("wizard").show(ui, |ui| match step {
                    0 => {
                        ui.add_space(60.0);
                        ui.vertical_centered(|ui| {
                            let (rect, _) = ui.allocate_exact_size(Vec2::splat(96.0), Sense::hover());
                            if let Some(ic) = self.icons.get("logo", &IconSrc::Builtin("logo"), 192) {
                                ui.painter().image(ic.id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), FLAME);
                            }
                            ui.add_space(10.0);
                            ui.label(RichText::new("Welcome to Hestia").size(40.0).family(FontFamily::Name("bold".into())));
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new("A quick, good-looking way to open apps, switch windows and more.")
                                    .size(18.0)
                                    .color(MUTED),
                            );
                            ui.add_space(24.0);
                            ui.label(
                                RichText::new(
                                    "Let's make it yours. This takes about a minute,\nand you can change everything later.",
                                )
                                .size(16.0),
                            );
                        });
                    }
                    1 => {
                        ui.heading("Pick a picture");
                        ui.label(
                            RichText::new("Hestia is built around a picture you like. We'll also borrow its colours. Drag it in the preview to frame it just right.")
                                .color(MUTED),
                        );
                        ui.add_space(10.0);
                        self.picture_card(ui);
                        ui.label(
                            RichText::new("No picture? That's fine — click Next and pick a colour scheme instead.")
                                .color(MUTED)
                                .small(),
                        );
                    }
                    2 => {
                        ui.heading("Choose your colours");
                        ui.label(
                            RichText::new("These were picked from your picture. Try a style or a different accent.").color(MUTED),
                        );
                        ui.add_space(10.0);
                        self.colours_card(ui);
                    }
                    3 => {
                        ui.heading("Shape & layout");
                        ui.label(RichText::new("Pick the shape of Hestia. The preview shows it with your picture and colours.").color(MUTED));
                        ui.add_space(10.0);
                        self.shape_card(ui);
                    }
                    4 => {
                        ui.heading("Fonts & details");
                        ui.label(RichText::new("Make it feel like yours. Watch the preview as you go.").color(MUTED));
                        ui.add_space(10.0);
                        self.fonts_card(ui);
                        self.details_card(ui);
                    }
                    5 => {
                        ui.heading("Shortcuts");
                        ui.label(
                            RichText::new(
                                "Press these anywhere to open Hestia. The defaults are fine — change them if you like.",
                            )
                            .color(MUTED),
                        );
                        ui.add_space(10.0);
                        self.hotkey_rows(ui);
                    }
                    _ => {
                        ui.heading("Almost done");
                        ui.add_space(10.0);
                        card(ui, |ui| {
                            setting_row(ui, "Start with Windows", "Hestia is ready as soon as you sign in.", |ui| {
                                toggle(ui, &mut self.cfg.general.start_with_windows);
                            });
                            ui.separator();
                            setting_row(
                                ui,
                                "Show tray icon",
                                "The Hestia icon near the clock. Click it to open the launcher.",
                                |ui| {
                                    toggle(ui, &mut self.cfg.general.show_tray_icon);
                                },
                            );
                        });
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new(format!(
                                "When you click Finish, Hestia opens so you can try it. Next time, press {}.",
                                self.cfg.hotkeys.apps.label()
                            ))
                            .size(16.0),
                        );
                    }
                });
            },
        );

        if next == usize::MAX {
            self.finish_wizard(ctx);
        } else if next != step {
            self.wizard = Some(next);
        }
    }

    fn finish_wizard(&mut self, ctx: &egui::Context) {
        self.cfg.general.setup_done = true;
        platform::set_start_with_windows(self.cfg.general.start_with_windows);
        let _ = self.cfg.save();
        self.saved = self.cfg.clone();
        if !platform::instance_running(crate::popup::DAEMON_MUTEX) {
            self.start_daemon();
        }
        // Ask the launcher to pop up so the user can try it straight away.
        let _ = std::fs::write(config::data_dir().join("show.flag"), "show");
        self.wizard = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn nav(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("nav")
            .exact_width(220.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(PANEL).inner_margin(egui::Margin::symmetric(14.0, 20.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
                    if let Some(ic) = self.icons.get("logo", &IconSrc::Builtin("logo"), 64) {
                        ui.painter().image(ic.id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), FLAME);
                    }
                    ui.label(RichText::new("Hestia").size(24.0).family(FontFamily::Name("bold".into())));
                });
                ui.add_space(18.0);
                for (p, label) in [
                    (Page::Look, "🎨  Look"),
                    (Page::Icons, "🖼  Icons"),
                    (Page::Hotkeys, "⌨  Shortcuts"),
                    (Page::General, "⚙  General"),
                    (Page::About, "ℹ  About"),
                ] {
                    let selected = self.page == p;
                    let b = egui::Button::new(RichText::new(label).size(16.0).color(if selected { TEXT } else { MUTED }))
                        .fill(if selected { CARD } else { Color32::TRANSPARENT })
                        .min_size(Vec2::new(190.0, 38.0));
                    if ui.add(b).clicked() {
                        self.page = p;
                        self.recording = None;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    if let Some(t) = self.saved_at {
                        if t.elapsed() < Duration::from_secs(2) {
                            ui.label(RichText::new("✔ Saved").color(Color32::from_rgb(0x7A, 0xD9, 0x8B)));
                            ui.ctx().request_repaint_after(Duration::from_millis(300));
                        }
                    }
                    if self.daemon_running {
                        if ui.button("Try it now").clicked() {
                            let _ = std::fs::write(config::data_dir().join("show.flag"), "show");
                        }
                    }
                    ui.label(RichText::new("Changes save automatically.").color(MUTED).small());
                });
            });
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.icons.poll();
        self.poll_status();
        self.capture_recording(ctx);
        self.ensure_image(ctx);
        if std::mem::take(&mut self.pending_regen) {
            self.regen();
            self.ensure_image(ctx);
        }

        if let Some(step) = self.wizard {
            self.wizard_ui(ctx, step);
        } else {
            self.nav(ctx);
            if self.page == Page::Look {
                egui::SidePanel::right("look-preview")
                    .exact_width((ctx.screen_rect().width() - 220.0) * 0.55)
                    .resizable(false)
                    .frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::symmetric(16.0, 22.0)))
                    .show(ctx, |ui| self.preview_panel(ui));
            }
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(BG).inner_margin(egui::Margin::symmetric(28.0, 22.0)))
                .show(ctx, |ui| match self.page {
                    Page::Look => self.page_look(ui),
                    Page::Icons => self.page_icons(ui),
                    Page::Hotkeys => self.page_hotkeys(ui),
                    Page::General => self.page_general(ui),
                    Page::About => self.page_about(ui),
                });
        }
        self.autosave(ctx);
    }
}
