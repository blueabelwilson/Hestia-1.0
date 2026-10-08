//! Finds installed apps: Start-menu shortcuts plus Microsoft Store apps.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppEntry {
    /// Stable id used for usage stats and icon overrides.
    pub id: String,
    pub name: String,
    /// What gets opened: a .lnk path or `shell:AppsFolder\<AUMID>`.
    pub target: String,
    /// Path or shell parsing-name used to fetch the real icon.
    pub icon_source: String,
    /// True for shortcuts (we can "open file location").
    pub is_shortcut: bool,
}

fn start_menu_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(p) = std::env::var_os("ProgramData") {
        dirs.push(PathBuf::from(p).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    if let Some(p) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(p).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    dirs
}

fn is_junk(name: &str) -> bool {
    let l = name.to_lowercase();
    ["uninstall", "readme", "read me", "release notes", "license", "help", "documentation", "website"]
        .iter()
        .any(|w| l.contains(w))
}

pub fn scan_shortcuts() -> Vec<AppEntry> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for dir in start_menu_dirs() {
        for entry in walkdir::WalkDir::new(&dir).max_depth(4).into_iter().flatten() {
            let path = entry.path();
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ext != "lnk" && ext != "url" {
                continue;
            }
            let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if name.is_empty() || is_junk(&name) || !seen.insert(name.to_lowercase()) {
                continue;
            }
            let p = path.to_string_lossy().to_string();
            out.push(AppEntry {
                id: format!("app:{}", name.to_lowercase()),
                name,
                target: p.clone(),
                icon_source: p,
                is_shortcut: true,
            });
        }
    }
    out
}

/// Store (UWP) apps, via PowerShell's Get-StartApps. Takes ~1 s, so it runs in the background.
pub fn scan_store_apps() -> Vec<AppEntry> {
    let script = "[Console]::OutputEncoding=[Text.Encoding]::UTF8; Get-StartApps | ConvertTo-Json -Compress";
    let Ok(out) = crate::platform::hidden_command("powershell", &["-NoProfile", "-NonInteractive", "-Command", script]).output()
    else {
        return vec![];
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text.trim()) else {
        return vec![];
    };
    let list = match v {
        serde_json::Value::Array(a) => a,
        other => vec![other],
    };
    list.iter()
        .filter_map(|o| {
            let name = o.get("Name")?.as_str()?.to_string();
            let aumid = o.get("AppID")?.as_str()?.to_string();
            // Store apps have "Package!App" ids; desktop apps are already covered by shortcuts.
            if !aumid.contains('!') || is_junk(&name) {
                return None;
            }
            let target = format!("shell:AppsFolder\\{aumid}");
            Some(AppEntry {
                id: format!("app:{}", name.to_lowercase()),
                name,
                target: target.clone(),
                icon_source: target,
                is_shortcut: false,
            })
        })
        .collect()
}

fn cache_path() -> PathBuf {
    crate::config::data_dir().join("apps.json")
}

pub fn load_cache() -> Vec<AppEntry> {
    std::fs::read_to_string(cache_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn merge(mut a: Vec<AppEntry>, b: Vec<AppEntry>) -> Vec<AppEntry> {
    let mut seen: HashSet<String> = a.iter().map(|e| e.id.clone()).collect();
    for e in b {
        if seen.insert(e.id.clone()) {
            a.push(e);
        }
    }
    a.sort_by(|x, y| x.name.to_lowercase().cmp(&y.name.to_lowercase()));
    a
}

/// Shared, background-refreshed list of apps.
#[derive(Clone)]
pub struct AppList {
    pub apps: Arc<Mutex<Vec<AppEntry>>>,
    pub version: Arc<std::sync::atomic::AtomicU64>,
    scanning: Arc<std::sync::atomic::AtomicBool>,
}

impl AppList {
    pub fn new() -> Self {
        AppList {
            apps: Arc::new(Mutex::new(load_cache())),
            version: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            scanning: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn snapshot(&self) -> Vec<AppEntry> {
        self.apps.lock().map(|a| a.clone()).unwrap_or_default()
    }

    pub fn refresh(&self, include_store: bool, ctx: Option<eframe::egui::Context>) {
        use std::sync::atomic::Ordering;
        if self.scanning.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            let shortcuts = scan_shortcuts();
            // Publish shortcuts right away, store apps when they arrive.
            let first = merge(shortcuts.clone(), vec![]);
            if !first.is_empty() {
                if let Ok(mut a) = me.apps.lock() {
                    // Keep previously cached store apps until the fresh scan finishes.
                    let old_store: Vec<AppEntry> = a.iter().filter(|e| !e.is_shortcut).cloned().collect();
                    *a = merge(first.clone(), old_store);
                }
                me.version.fetch_add(1, Ordering::SeqCst);
            }
            let all = if include_store { merge(shortcuts, scan_store_apps()) } else { first };
            if let Ok(s) = serde_json::to_string(&all) {
                let _ = std::fs::write(cache_path(), s);
            }
            if let Ok(mut a) = me.apps.lock() {
                *a = all;
            }
            me.version.fetch_add(1, Ordering::SeqCst);
            me.scanning.store(false, Ordering::SeqCst);
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }
}
