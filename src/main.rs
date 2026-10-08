// Hide the console window in release builds (debug builds keep it for log messages).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apps;
mod config;
mod fonts;
mod fuzzy;
mod icons;
mod keys;
mod layout;
mod palette;
mod platform;
mod popup;
mod settings;

use eframe::egui;

/// Start the settings window as its own process (or bring it forward if already open).
pub fn open_settings(extra: &[&str]) {
    if platform::instance_running(settings::SETTINGS_MUTEX) && extra.is_empty() {
        platform::bring_to_front(settings::SETTINGS_TITLE);
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut args = vec!["--settings"];
        args.extend_from_slice(extra);
        let _ = std::process::Command::new(exe).args(&args).spawn();
    }
}

/// The Hestia logo shown in the system tray (in brand orange so it shows on light and dark taskbars).
pub fn tray_icon_image() -> tray_icon::Icon {
    let svg = icons::logo_svg(icons::BRAND);
    let px = 64u32;
    let rgba = icons::rasterize_svg(svg.as_bytes(), px, false)
        .map(|r| {
            // egui images are premultiplied; the tray wants straight alpha.
            r.image
                .pixels
                .iter()
                .flat_map(|c| {
                    let [r, g, b, a] = c.to_srgba_unmultiplied();
                    [r, g, b, a]
                })
                .collect::<Vec<u8>>()
        })
        .unwrap_or_else(|| vec![255; (px * px * 4) as usize]);
    tray_icon::Icon::from_rgba(rgba, px, px).expect("tray icon")
}

fn run_popup() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(popup::POPUP_TITLE)
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_position([-20000.0, -20000.0])
            .with_inner_size([10.0, 10.0]),
        ..Default::default()
    };
    eframe::run_native("Hestia", options, Box::new(|cc| Ok(Box::new(popup::PopupApp::new(cc)))))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |a: &str| args.iter().any(|x| x == a);

    if has("--settings") || has("--wizard") {
        if !platform::claim_single_instance(settings::SETTINGS_MUTEX) {
            platform::bring_to_front(settings::SETTINGS_TITLE);
            return;
        }
        let icon_target = args.iter().position(|a| a == "--icon").and_then(|i| args.get(i + 1)).cloned();
        if let Err(e) = settings::run(has("--wizard"), icon_target) {
            eprintln!("Settings window failed: {e}");
        }
        return;
    }

    // Normal start: the background launcher.
    if !platform::claim_single_instance(popup::DAEMON_MUTEX) {
        // Already running: double-clicking Hestia again opens its settings.
        open_settings(&[]);
        return;
    }
    let cfg = config::Config::load();
    if !config::Config::exists() || !cfg.general.setup_done {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe).arg("--wizard").spawn();
        }
    }
    if let Err(e) = run_popup() {
        eprintln!("Hestia failed to start: {e}");
    }
}
