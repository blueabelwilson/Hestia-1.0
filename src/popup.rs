//! The always-running part of Hestia: global hotkeys, tray icon and the popup itself.

use crate::apps::{AppEntry, AppList};
use crate::config::{self, Config, Effect};
use crate::fuzzy::{self, UsageStore};
use crate::icons::{IconCache, IconSrc};
use crate::keys;
use crate::layout::{self, Colors, Item, Layout, Scene, MODES};
use crate::platform::{self, PowerAction, WindowInfo};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant, SystemTime};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub const POPUP_TITLE: &str = "Hestia Popup";
pub const DAEMON_MUTEX: &str = "Local\\HestiaDaemon";

const APPS: usize = 0;
const WINDOWS: usize = 1;
const RUN: usize = 2;
const POWER: usize = 3;

#[derive(Clone, Debug)]
enum Action {
    App(AppEntry),
    Window(isize),
    Run(String),
    Power(PowerAction),
}

enum UiEvent {
    Hotkey(u32),
    Menu(MenuId),
    TrayClick,
}

struct TrayIds {
    apps: MenuId,
    windows: MenuId,
    power: MenuId,
    settings: MenuId,
    quit: MenuId,
}

pub struct PopupApp {
    cfg: Config,
    cfg_mtime: Option<SystemTime>,
    last_poll: Instant,

    colors: Colors,
    launcher: Layout,
    power: Layout,
    image_tex: Option<egui::TextureHandle>,
    image_path: Option<std::path::PathBuf>,
    font: String,
    icons: IconCache,

    apps: AppList,
    apps_seen: u64,
    apps_scanned_at: Instant,
    usage: UsageStore,
    windows: Vec<WindowInfo>,
    hibernate: bool,

    hwnd: Option<isize>,
    visible: bool,
    shown_at: Instant,
    had_focus: bool,
    prev_foreground: isize,
    /// Monitor work area + scale the popup was opened on (for re-centring on mode change).
    monitor: Option<platform::MonitorArea>,

    mode: usize,
    query: String,
    last_query: String,
    items: Vec<Item>,
    actions: Vec<Action>,
    selected: usize,
    first_row: usize,
    focus_search: bool,
    confirm: Option<PowerAction>,
    confirm_yes: bool,
    context: Option<(usize, Pos2)>,

    hotkeys: Option<GlobalHotKeyManager>,
    registered: Vec<HotKey>,
    hotkey_modes: HashMap<u32, usize>,
    events: Receiver<UiEvent>,
    tray: Option<TrayIcon>,
    tray_ids: Option<TrayIds>,
}

fn image_texture(ctx: &egui::Context, cfg: &Config) -> egui::TextureHandle {
    let img = cfg
        .look
        .image
        .as_ref()
        .and_then(|p| crate::palette::load_image(p))
        .unwrap_or_else(|| crate::palette::gradient_image(&cfg.look.palette));
    let size = [img.width() as usize, img.height() as usize];
    let ci = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    ctx.load_texture("hestia-picture", ci, egui::TextureOptions::LINEAR)
}

impl PopupApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = cc.egui_ctx.clone();
        let cfg = Config::load();
        crate::fonts::install(&ctx, &cfg.look);

        // Route hotkey and tray events into our loop and wake it up immediately.
        let (tx, rx) = channel::<UiEvent>();
        {
            let tx = tx.clone();
            let c = ctx.clone();
            GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
                if e.state == HotKeyState::Pressed {
                    let _ = tx.send(UiEvent::Hotkey(e.id));
                    c.request_repaint();
                }
            }));
        }
        {
            let tx = tx.clone();
            let c = ctx.clone();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                let _ = tx.send(UiEvent::Menu(e.id));
                c.request_repaint();
            }));
        }
        {
            let c = ctx.clone();
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                    let _ = tx.send(UiEvent::TrayClick);
                    c.request_repaint();
                }
            }));
        }

        let apps = AppList::new();
        apps.refresh(cfg.general.include_store_apps, Some(ctx.clone()));
        if cfg.general.start_with_windows {
            // Keep the startup entry pointing at wherever Hestia lives now.
            platform::set_start_with_windows(true);
        }

        let mut app = PopupApp {
            colors: Colors::new(&cfg.look.palette, cfg.look.opacity),
            launcher: layout::resolve(&cfg.look, false),
            power: layout::resolve(&cfg.look, true),
            image_tex: Some(image_texture(&ctx, &cfg)),
            image_path: cfg.look.image.clone(),
            font: crate::fonts::key(&cfg.look),
            icons: IconCache::new(&ctx, cfg.icons.overrides.clone()),
            cfg_mtime: config::modified_time(&config::config_path()),
            last_poll: Instant::now(),
            apps,
            apps_seen: 0,
            apps_scanned_at: Instant::now(),
            usage: UsageStore::load(),
            windows: Vec::new(),
            hibernate: platform::hibernate_enabled(),
            hwnd: None,
            visible: false,
            shown_at: Instant::now(),
            had_focus: false,
            prev_foreground: 0,
            monitor: None,
            mode: APPS,
            query: String::new(),
            last_query: String::new(),
            items: Vec::new(),
            actions: Vec::new(),
            selected: 0,
            first_row: 0,
            focus_search: false,
            confirm: None,
            confirm_yes: true,
            context: None,
            hotkeys: GlobalHotKeyManager::new().ok(),
            registered: Vec::new(),
            hotkey_modes: HashMap::new(),
            events: rx,
            tray: None,
            tray_ids: None,
            cfg,
        };
        app.register_hotkeys();
        app.sync_tray();
        app
    }

    // -----------------------------------------------------------------------
    // Config
    // -----------------------------------------------------------------------

    fn reload_config(&mut self, ctx: &egui::Context) {
        let new = Config::load();
        let hotkeys_changed = new.hotkeys != self.cfg.hotkeys;
        let look_changed = new.look != self.cfg.look;
        let store_changed = new.general.include_store_apps != self.cfg.general.include_store_apps;
        self.cfg = new;
        if look_changed {
            let fk = crate::fonts::key(&self.cfg.look);
            if fk != self.font {
                self.font = fk;
                crate::fonts::install(ctx, &self.cfg.look);
            }
            self.colors = Colors::new(&self.cfg.look.palette, self.cfg.look.opacity);
            self.launcher = layout::resolve(&self.cfg.look, false);
            self.power = layout::resolve(&self.cfg.look, true);
            // Re-read the picture only if it changed (the fallback gradient follows the colours).
            if self.cfg.look.image != self.image_path || self.cfg.look.image.is_none() {
                self.image_path = self.cfg.look.image.clone();
                self.image_tex = Some(image_texture(ctx, &self.cfg));
            }
            self.apply_effect();
        }
        self.icons.set_overrides(self.cfg.icons.overrides.clone());
        if hotkeys_changed {
            self.register_hotkeys();
        }
        if store_changed {
            self.apps.refresh(self.cfg.general.include_store_apps, Some(ctx.clone()));
        }
        self.sync_tray();
    }

    fn apply_effect(&self) {
        let Some(h) = self.hwnd else { return };
        let p = &self.cfg.look.palette;
        let dark = crate::palette::luminance(p.background) < 0.4;
        // Windows draws blur/acrylic/mica for the whole rectangular window, so it can't follow
        // circles, arches or rings. Those shapes use plain see-through opacity instead.
        let shaped = [&self.launcher, &self.power].iter().any(|l| {
            l.outline != layout::Outline::Rect || matches!(l.root, layout::Node::Rings { .. } | layout::Node::Panel { .. })
        });
        let effect = if self.cfg.look.opacity >= 0.99 || shaped { Effect::None } else { self.cfg.look.effect };
        platform::apply_effect(h, effect, (p.background.0, p.background.1, p.background.2, 0), dark);
    }

    fn register_hotkeys(&mut self) {
        let Some(mgr) = &self.hotkeys else { return };
        let _ = mgr.unregister_all(&self.registered);
        self.registered.clear();
        self.hotkey_modes.clear();
        let mut failed: Vec<&str> = Vec::new();
        let h = &self.cfg.hotkeys;
        for (name, combo, mode) in
            [("apps", &h.apps, APPS), ("windows", &h.windows, WINDOWS), ("run", &h.run, RUN), ("power", &h.power, POWER)]
        {
            let Some(hk) = keys::global_hotkey(combo) else { continue };
            match mgr.register(hk) {
                Ok(()) => {
                    self.hotkey_modes.insert(hk.id(), mode);
                    self.registered.push(hk);
                }
                Err(_) => failed.push(name),
            }
        }
        // Let the settings window know which shortcuts are taken by other apps.
        let status = serde_json::json!({ "hotkey_errors": failed });
        let _ = std::fs::write(config::data_dir().join("status.json"), status.to_string());
    }

    fn sync_tray(&mut self) {
        if !self.cfg.general.show_tray_icon {
            self.tray = None;
            self.tray_ids = None;
            return;
        }
        if self.tray.is_some() {
            return;
        }
        let menu = Menu::new();
        let apps = MenuItem::new("Open launcher", true, None);
        let windows = MenuItem::new("Switch windows", true, None);
        let power = MenuItem::new("Power menu", true, None);
        let settings = MenuItem::new("Settings…", true, None);
        let quit = MenuItem::new("Quit Hestia", true, None);
        let _ = menu.append(&apps);
        let _ = menu.append(&windows);
        let _ = menu.append(&power);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&settings);
        let _ = menu.append(&quit);
        let icon = crate::tray_icon_image();
        let tray = TrayIconBuilder::new()
            .with_tooltip("Hestia — click to open the launcher")
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_icon(icon)
            .build();
        if let Ok(t) = tray {
            self.tray = Some(t);
            self.tray_ids = Some(TrayIds {
                apps: apps.id().clone(),
                windows: windows.id().clone(),
                power: power.id().clone(),
                settings: settings.id().clone(),
                quit: quit.id().clone(),
            });
        }
    }

    // -----------------------------------------------------------------------
    // Showing / hiding
    // -----------------------------------------------------------------------

    fn layout(&self) -> &Layout {
        if self.mode == POWER {
            &self.power
        } else {
            &self.launcher
        }
    }

    fn header(&self) -> String {
        platform::user_at_host()
    }

    fn status(&self) -> String {
        if self.mode == APPS && self.items.is_empty() && self.query.is_empty() {
            "Finding your apps…".into()
        } else {
            String::new()
        }
    }

    /// Size of the popup in points for the current mode.
    fn popup_size(&mut self) -> Vec2 {
        let header = self.header();
        let status = self.status();
        let info = platform::uptime_text();
        let layout = if self.mode == POWER { &self.power } else { &self.launcher };
        let scene = Scene {
            colors: self.colors,
            scale: self.cfg.look.size,
            opts: layout::LookOpts::from(&self.cfg.look),
            alpha: 1.0,
            base_font: 15.0,
            ppp: 1.0,
            image: None,
            items: &[],
            selected: 0,
            first_row: 0,
            mode: self.mode,
            header: &header,
            info: &info,
            status: &status,
            query: None,
            query_preview: "",
            focus_search: false,
            interactive: false,
            icons: &mut self.icons,
            id_salt: "measure",
            edit_picture: false,
            clip: Vec::new(),
        };
        scene.layout_size(layout)
    }

    fn place(&mut self, show: bool) {
        let Some(hwnd) = self.hwnd else { return };
        if show || self.monitor.is_none() {
            self.monitor = Some(platform::monitor_under_mouse());
        }
        let size = self.popup_size();
        let mon = self.monitor.as_ref().unwrap();
        let w = (size.x * mon.scale).round() as i32;
        let h = (size.y * mon.scale).round() as i32;
        let x = mon.left + (mon.width - w) / 2;
        // Slightly above centre feels more natural than dead centre.
        let y = mon.top + ((mon.height - h) as f32 * 0.42) as i32;
        // Cut the window to its visible shape: rings and badge layouts always (so clicks in the
        // empty space go through to the desktop), shaped outlines when blur is on.
        let lay = self.layout();
        let effect_on = self.cfg.look.effect != Effect::None && self.cfg.look.opacity < 0.99;
        let free_form = matches!(lay.root, layout::Node::Rings { .. } | layout::Node::Panel { .. });
        let region = if free_form || (lay.outline != layout::Outline::Rect && effect_on) {
            lay.region(Vec2::new(w as f32, h as f32)).map(|polys| {
                polys
                    .into_iter()
                    .map(|poly| poly.into_iter().map(|p| (p.x.round() as i32, p.y.round() as i32)).collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            })
        } else {
            None
        };
        if show {
            platform::show_popup(hwnd, x, y, w, h);
        } else {
            platform::place_popup(hwnd, x, y, w, h);
        }
        platform::set_region(hwnd, region);
    }

    fn show(&mut self, mode: usize) {
        if self.hwnd.is_none() {
            return;
        }
        self.prev_foreground = platform::foreground_window();
        self.mode = mode;
        self.query.clear();
        self.last_query.clear();
        self.selected = 0;
        self.first_row = 0;
        self.confirm = None;
        self.context = None;
        self.prepare_mode();
        if self.apps_scanned_at.elapsed() > Duration::from_secs(15 * 60) {
            self.apps_scanned_at = Instant::now();
            self.apps.refresh(self.cfg.general.include_store_apps, None);
        }
        self.place(true);
        self.visible = true;
        self.shown_at = Instant::now();
        self.had_focus = false;
        self.focus_search = true;
    }

    fn hide(&mut self, restore_focus: bool) {
        if let Some(h) = self.hwnd {
            platform::park_popup(h);
        }
        self.visible = false;
        self.context = None;
        self.confirm = None;
        if restore_focus {
            platform::restore_focus(self.prev_foreground);
        }
    }

    fn switch_mode(&mut self, mode: usize) {
        let was_power = self.mode == POWER;
        self.mode = mode;
        self.query.clear();
        self.last_query.clear();
        self.selected = 0;
        self.first_row = 0;
        self.confirm = None;
        self.prepare_mode();
        if was_power != (mode == POWER) {
            self.place(false);
        }
        self.focus_search = true;
    }

    fn prepare_mode(&mut self) {
        if self.mode == WINDOWS {
            let own = self.hwnd.unwrap_or(0);
            self.windows = platform::list_windows(own);
        }
        self.rebuild_items();
    }

    // -----------------------------------------------------------------------
    // Items
    // -----------------------------------------------------------------------

    fn rebuild_items(&mut self) {
        let q = self.query.trim().to_string();
        let mut scored: Vec<(i32, Item, Action)> = Vec::new();
        match self.mode {
            APPS => {
                for a in self.apps.snapshot() {
                    if let Some(s) = fuzzy::score(&q, &a.name) {
                        let boost = self.usage.boost(&a.id);
                        let item = Item {
                            id: a.id.clone(),
                            label: a.name.clone(),
                            sub: String::new(),
                            icon: IconSrc::Shell(a.icon_source.clone()),
                        };
                        scored.push((s + boost, item, Action::App(a)));
                    }
                }
            }
            WINDOWS => {
                for w in &self.windows {
                    let s1 = fuzzy::score(&q, &w.title);
                    let s2 = fuzzy::score(&q, &w.exe_name);
                    if let Some(s) = s1.max(s2) {
                        let item = Item {
                            id: format!("exe:{}", w.exe_name.to_lowercase()),
                            label: w.title.clone(),
                            sub: w.exe_name.clone(),
                            icon: if w.exe_path.is_empty() {
                                IconSrc::Builtin("window")
                            } else {
                                IconSrc::Shell(w.exe_path.clone())
                            },
                        };
                        // Keep Windows' own most-recent-first order when not searching.
                        let order = if q.is_empty() { -(scored.len() as i32) } else { s };
                        scored.push((order, item, Action::Window(w.hwnd)));
                    }
                }
            }
            RUN => {
                if !q.is_empty() {
                    let item = Item {
                        id: "run:new".into(),
                        label: format!("Run \u{201C}{q}\u{201D}"),
                        sub: String::new(),
                        icon: IconSrc::Builtin("terminal"),
                    };
                    scored.push((i32::MAX, item, Action::Run(q.clone())));
                }
                for (i, cmd) in self.usage.run_history.iter().enumerate() {
                    if *cmd == q {
                        continue;
                    }
                    if let Some(s) = fuzzy::score(&q, cmd) {
                        let item = Item {
                            id: "run:history".into(),
                            label: cmd.clone(),
                            sub: "recent".into(),
                            icon: IconSrc::Builtin("terminal"),
                        };
                        scored.push((s - i as i32, item, Action::Run(cmd.clone())));
                    }
                }
            }
            _ => {
                let mut list = vec![
                    PowerAction::Lock,
                    PowerAction::Sleep,
                    PowerAction::SignOut,
                    PowerAction::Restart,
                    PowerAction::Shutdown,
                ];
                if self.hibernate {
                    list.insert(3, PowerAction::Hibernate);
                }
                let n = list.len() as i32;
                for (i, a) in list.into_iter().enumerate() {
                    let item = Item {
                        id: format!("power:{}", a.id()),
                        label: a.label().into(),
                        sub: String::new(),
                        icon: IconSrc::Builtin(match a {
                            PowerAction::Lock => "lock",
                            PowerAction::Sleep => "sleep",
                            PowerAction::Hibernate => "hibernate",
                            PowerAction::SignOut => "signout",
                            PowerAction::Restart => "restart",
                            PowerAction::Shutdown => "shutdown",
                        }),
                    };
                    scored.push((n - i as i32, item, Action::Power(a)));
                }
            }
        }
        if self.mode == APPS && q.is_empty() {
            // No search yet: most-used first, then A–Z.
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.label.to_lowercase().cmp(&b.1.label.to_lowercase())));
        } else {
            scored.sort_by(|a, b| b.0.cmp(&a.0));
        }
        scored.truncate(300);
        self.items = scored.iter().map(|s| s.1.clone()).collect();
        self.actions = scored.into_iter().map(|s| s.2).collect();
        self.selected = 0;
        self.first_row = 0;
    }

    fn activate(&mut self, idx: usize, admin: bool) {
        let Some(action) = self.actions.get(idx).cloned() else { return };
        let id = self.items.get(idx).map(|i| i.id.clone()).unwrap_or_default();
        match action {
            Action::App(a) => {
                platform::shell_open(&a.target, if admin { "runas" } else { "open" });
                self.usage.record(&id);
                self.hide(false);
            }
            Action::Window(h) => {
                self.hide(false);
                platform::focus_window(h);
            }
            Action::Run(cmd) => {
                if admin {
                    platform::shell_open_args("cmd.exe", &format!("/C start \"\" {cmd}"), "runas");
                } else {
                    platform::run_command(&cmd);
                }
                self.usage.record_run(&cmd);
                self.hide(false);
            }
            Action::Power(p) => {
                if self.cfg.general.confirm_power && p.needs_confirm() {
                    self.confirm = Some(p);
                    self.confirm_yes = true;
                } else {
                    self.hide(p == PowerAction::Lock);
                    platform::power(p);
                }
            }
        }
    }

    fn move_selection(&mut self, delta: i32) {
        if self.items.is_empty() {
            return;
        }
        let n = self.items.len() as i32;
        let mut s = self.selected as i32 + delta;
        if s < 0 {
            s = if delta == -1 { n - 1 } else { 0 };
        } else if s >= n {
            s = if delta == 1 { 0 } else { n - 1 };
        }
        self.selected = s as usize;
    }

    // -----------------------------------------------------------------------
    // Events
    // -----------------------------------------------------------------------

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.events.try_recv() {
            match ev {
                UiEvent::Hotkey(id) => {
                    if let Some(mode) = self.hotkey_modes.get(&id).copied() {
                        if self.visible && self.mode == mode {
                            self.hide(true);
                        } else if self.visible {
                            self.switch_mode(mode);
                        } else {
                            self.show(mode);
                        }
                    }
                }
                UiEvent::TrayClick => {
                    if self.visible {
                        self.hide(true);
                    } else {
                        self.show(APPS);
                    }
                }
                UiEvent::Menu(id) => {
                    let Some(ids) = &self.tray_ids else { continue };
                    let which = if id == ids.apps {
                        1
                    } else if id == ids.windows {
                        2
                    } else if id == ids.power {
                        3
                    } else if id == ids.settings {
                        4
                    } else if id == ids.quit {
                        5
                    } else {
                        0
                    };
                    match which {
                        1 => self.show(APPS),
                        2 => self.show(WINDOWS),
                        3 => self.show(POWER),
                        4 => crate::open_settings(&[]),
                        5 => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                        _ => {}
                    }
                }
            }
        }
    }

    fn periodic(&mut self, ctx: &egui::Context) {
        if self.last_poll.elapsed() < Duration::from_millis(700) {
            return;
        }
        self.last_poll = Instant::now();
        let m = config::modified_time(&config::config_path());
        if m != self.cfg_mtime {
            self.cfg_mtime = m;
            self.reload_config(ctx);
            if self.visible {
                self.place(false);
            }
        }
        let quit = config::data_dir().join("quit.flag");
        if quit.exists() {
            let _ = std::fs::remove_file(&quit);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let show = config::data_dir().join("show.flag");
        if show.exists() {
            let _ = std::fs::remove_file(&show);
            self.show(APPS);
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let k = self.cfg.keys.clone();
        let cols = self.layout().root.find_list().map(|l| l.0).unwrap_or(1).max(1);
        let has_search = self.layout().root.has_search();

        if let Some(action) = self.confirm {
            let (left, right, open, close) = ctx.input_mut(|i| {
                (
                    keys::consume_any(i, &k.left) || keys::consume_any(i, &k.up),
                    keys::consume_any(i, &k.right) || keys::consume_any(i, &k.down),
                    keys::consume_any(i, &k.open),
                    keys::consume_any(i, &k.close),
                )
            });
            if left || right {
                self.confirm_yes = !self.confirm_yes;
            }
            if close {
                self.confirm = None;
            } else if open {
                if self.confirm_yes {
                    self.hide(false);
                    platform::power(action);
                } else {
                    self.confirm = None;
                }
            }
            return;
        }

        let grid_lr = cols > 1 || !has_search || self.layout().root.is_ring();
        let (prev_mode, next_mode, up, down, left, right, open, close, admin) = ctx.input_mut(|i| {
            let admin =
                i.consume_key(egui::Modifiers { ctrl: true, shift: true, command: true, ..Default::default() }, egui::Key::Enter);
            (
                keys::consume_any(i, &k.prev_mode),
                keys::consume_any(i, &k.next_mode),
                keys::consume_any(i, &k.up),
                keys::consume_any(i, &k.down),
                grid_lr && keys::consume_any(i, &k.left),
                grid_lr && keys::consume_any(i, &k.right),
                keys::consume_any(i, &k.open),
                keys::consume_any(i, &k.close),
                admin,
            )
        });
        if close {
            if self.context.is_some() {
                self.context = None;
            } else {
                self.hide(true);
            }
            return;
        }
        if prev_mode {
            self.switch_mode((self.mode + MODES.len() - 1) % MODES.len());
            return;
        }
        if next_mode {
            self.switch_mode((self.mode + 1) % MODES.len());
            return;
        }
        if up {
            self.move_selection(-(cols as i32));
        }
        if down {
            self.move_selection(cols as i32);
        }
        if left {
            self.move_selection(-1);
        }
        if right {
            self.move_selection(1);
        }
        if open || admin {
            self.activate(self.selected, admin);
        }
    }

    // -----------------------------------------------------------------------
    // Drawing
    // -----------------------------------------------------------------------

    fn draw(&mut self, ctx: &egui::Context) {
        let t = if self.cfg.look.animations { (self.shown_at.elapsed().as_secs_f32() / 0.14).min(1.0) } else { 1.0 };
        let ease = 1.0 - (1.0 - t).powi(3);
        if t < 1.0 {
            ctx.request_repaint();
        }
        let header = self.header();
        let status = self.status();
        let info = if self.mode == POWER { platform::uptime_text() } else { String::new() };
        let ppp = ctx.pixels_per_point();
        let focus = self.focus_search;
        self.focus_search = false;

        let mut out = layout::Output::default();
        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
            let layout = if self.mode == POWER { &self.power } else { &self.launcher };
            let mut scene = Scene {
                colors: self.colors,
                scale: self.cfg.look.size,
                opts: layout::LookOpts::from(&self.cfg.look),
                alpha: ease,
                base_font: 15.0,
                ppp,
                image: self.image_tex.as_ref(),
                items: &self.items,
                selected: self.selected,
                first_row: self.first_row,
                mode: self.mode,
                header: &header,
                info: &info,
                status: &status,
                query: Some(&mut self.query),
                query_preview: "",
                focus_search: focus || self.context.is_none() && self.confirm.is_none(),
                interactive: self.confirm.is_none(),
                icons: &mut self.icons,
                id_salt: "popup",
                edit_picture: false,
                clip: Vec::new(),
            };
            let origin = Pos2::new(0.0, (1.0 - ease) * 8.0);
            out = scene.draw(ui, layout, origin);
        });
        self.first_row = out.first_row;

        if let Some(i) = out.mode_clicked {
            self.switch_mode(i);
        }
        if out.scroll != 0 {
            let cols = self.layout().root.find_list().map(|l| l.0).unwrap_or(1) as i32;
            self.move_selection(out.scroll * cols);
        }
        if let Some(i) = out.clicked {
            self.selected = i;
            self.activate(i, false);
        }
        if let Some((i, pos)) = out.context {
            self.selected = i;
            self.context = Some((i, pos));
        }
        if self.query != self.last_query {
            self.last_query = self.query.clone();
            self.rebuild_items();
        }
        self.draw_context_menu(ctx);
        self.draw_confirm(ctx);
    }

    fn menu_button(ui: &mut egui::Ui, colors: &Colors, text: &str) -> bool {
        let b = egui::Button::new(egui::RichText::new(text).color(colors.fg).size(14.0))
            .fill(Color32::TRANSPARENT)
            .min_size(Vec2::new(200.0, 28.0));
        ui.add(b).clicked()
    }

    fn draw_context_menu(&mut self, ctx: &egui::Context) {
        let Some((idx, pos)) = self.context else { return };
        let Some(action) = self.actions.get(idx).cloned() else {
            self.context = None;
            return;
        };
        let id = self.items[idx].id.clone();
        let colors = self.colors;
        let mut chosen: Option<&str> = None;
        let area =
            egui::Area::new(egui::Id::new("hestia-context")).fixed_pos(pos).order(egui::Order::Foreground).show(ctx, |ui| {
                egui::Frame::none()
                    .fill(colors.bg_alt)
                    .rounding(10.0)
                    .inner_margin(6.0)
                    .stroke(egui::Stroke::new(1.0f32, colors.sel.gamma_multiply(0.5)))
                    .show(ui, |ui| {
                        ui.style_mut().visuals.widgets.hovered.weak_bg_fill = colors.sel.gamma_multiply(0.35);
                        ui.style_mut().visuals.widgets.hovered.bg_fill = colors.sel.gamma_multiply(0.35);
                        match &action {
                            Action::App(a) => {
                                if Self::menu_button(ui, &colors, "Open") {
                                    chosen = Some("open");
                                }
                                if Self::menu_button(ui, &colors, "Run as administrator") {
                                    chosen = Some("admin");
                                }
                                if a.is_shortcut && Self::menu_button(ui, &colors, "Open file location") {
                                    chosen = Some("reveal");
                                }
                                if Self::menu_button(ui, &colors, "Change icon…") {
                                    chosen = Some("icon");
                                }
                            }
                            Action::Window(_) => {
                                if Self::menu_button(ui, &colors, "Switch to") {
                                    chosen = Some("open");
                                }
                                if Self::menu_button(ui, &colors, "Close window") {
                                    chosen = Some("close");
                                }
                            }
                            Action::Run(_) => {
                                if Self::menu_button(ui, &colors, "Run") {
                                    chosen = Some("open");
                                }
                            }
                            Action::Power(_) => {
                                if Self::menu_button(ui, &colors, "Do it") {
                                    chosen = Some("open");
                                }
                                if Self::menu_button(ui, &colors, "Change icon…") {
                                    chosen = Some("icon");
                                }
                            }
                        }
                    });
            });
        let clicked_outside = ctx.input(|i| i.pointer.any_pressed())
            && !area.response.rect.contains(ctx.input(|i| i.pointer.interact_pos().unwrap_or(Pos2::ZERO)));
        match chosen {
            Some("open") => {
                self.context = None;
                self.activate(idx, false);
            }
            Some("admin") => {
                self.context = None;
                self.activate(idx, true);
            }
            Some("reveal") => {
                self.context = None;
                if let Action::App(a) = &action {
                    platform::reveal_in_explorer(&a.target);
                }
                self.hide(false);
            }
            Some("icon") => {
                self.context = None;
                self.hide(false);
                crate::open_settings(&["--icon", &id]);
            }
            Some("close") => {
                self.context = None;
                if let Action::Window(h) = action {
                    platform::close_window(h);
                    self.windows.retain(|w| w.hwnd != h);
                    self.rebuild_items();
                }
            }
            _ => {
                if clicked_outside {
                    self.context = None;
                }
            }
        }
    }

    fn draw_confirm(&mut self, ctx: &egui::Context) {
        let Some(action) = self.confirm else { return };
        let colors = self.colors;
        let s = self.cfg.look.size;
        let screen = ctx.screen_rect();
        let mut result: Option<bool> = None;
        egui::Area::new(egui::Id::new("hestia-confirm")).fixed_pos(screen.min).order(egui::Order::Foreground).show(ctx, |ui| {
            let painter = ui.painter();
            painter.rect_filled(screen, egui::Rounding::same(15.0 * s * self.cfg.look.roundness), colors.bg.gamma_multiply(0.82));
            let card = Rect::from_center_size(screen.center(), Vec2::new(340.0 * s, 170.0 * s));
            painter.rect_filled(card, egui::Rounding::same(14.0 * s * self.cfg.look.roundness), colors.bg_alt);
            let title = format!("{}?", action.label());
            painter.text(
                Pos2::new(card.center().x, card.top() + 42.0 * s),
                egui::Align2::CENTER_CENTER,
                title,
                egui::FontId::new(20.0 * s, egui::FontFamily::Name("bold".into())),
                colors.fg,
            );
            painter.text(
                Pos2::new(card.center().x, card.top() + 72.0 * s),
                egui::Align2::CENTER_CENTER,
                "Unsaved work in open apps may be lost.",
                egui::FontId::proportional(13.0 * s),
                colors.fg.gamma_multiply(0.7),
            );
            let bw = 130.0 * s;
            let bh = 40.0 * s;
            let y = card.bottom() - 22.0 * s - bh;
            for (i, (label, yes)) in [("Yes", true), ("Cancel", false)].iter().enumerate() {
                let x = card.center().x - bw - 8.0 * s + i as f32 * (bw + 16.0 * s);
                let r = Rect::from_min_size(Pos2::new(x, y), Vec2::new(bw, bh));
                let resp = ui.interact(r, egui::Id::new(("confirm", i)), egui::Sense::click());
                if resp.hovered() {
                    self.confirm_yes = *yes;
                }
                let focused = self.confirm_yes == *yes;
                let painter = ui.painter();
                painter.rect_filled(
                    r,
                    egui::Rounding::same(10.0 * s * self.cfg.look.roundness),
                    if focused { colors.sel } else { colors.bg },
                );
                painter.text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    *label,
                    egui::FontId::proportional(15.0 * s),
                    if focused { colors.on_sel } else { colors.fg },
                );
                if resp.clicked() {
                    result = Some(*yes);
                }
            }
        });
        match result {
            Some(true) => {
                self.hide(false);
                platform::power(action);
            }
            Some(false) => self.confirm = None,
            None => {}
        }
    }
}

impl eframe::App for PopupApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.hwnd.is_none() {
            self.hwnd = platform::find_window(POPUP_TITLE);
            if let Some(h) = self.hwnd {
                platform::park_popup(h);
                self.apply_effect();
            }
        }
        self.icons.poll();
        self.handle_events(ctx);
        self.periodic(ctx);

        let v = self.apps.version.load(std::sync::atomic::Ordering::SeqCst);
        if v != self.apps_seen {
            self.apps_seen = v;
            if self.visible && self.mode == APPS {
                let sel = self.selected;
                self.rebuild_items();
                self.selected = sel.min(self.items.len().saturating_sub(1));
            }
        }

        if !self.visible {
            // Idle: wake up now and then to notice settings changes.
            ctx.request_repaint_after(Duration::from_millis(800));
            egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |_| {});
            return;
        }

        let focused = ctx.input(|i| i.viewport().focused);
        if focused == Some(true) {
            self.had_focus = true;
        }
        if self.cfg.general.hide_on_focus_loss
            && self.had_focus
            && focused == Some(false)
            && self.shown_at.elapsed() > Duration::from_millis(250)
        {
            self.hide(false);
            return;
        }

        self.handle_keys(ctx);
        if !self.visible {
            return;
        }
        self.draw(ctx);
        ctx.request_repaint_after(Duration::from_millis(500));
    }
}
