//! Thin wrappers around the Win32 calls Hestia needs.

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// windows-rs returns some handles as `HWND` and others as `Result<HWND>`
/// depending on the function; this smooths over the difference.
pub trait AsHwnd {
    fn hwnd(self) -> HWND;
}
impl AsHwnd for HWND {
    fn hwnd(self) -> HWND {
        self
    }
}
impl AsHwnd for windows::core::Result<HWND> {
    fn hwnd(self) -> HWND {
        self.unwrap_or_default()
    }
}

/// HWNDs are raw pointers in windows-rs; store them as isize so they can cross threads.
pub fn to_isize(h: HWND) -> isize {
    h.0 as isize
}
pub fn from_isize(v: isize) -> HWND {
    HWND(v as *mut core::ffi::c_void)
}

pub fn find_window(title: &str) -> Option<isize> {
    let h = unsafe { FindWindowW(PCWSTR::null(), &HSTRING::from(title)) }.hwnd();
    if h.is_invalid() {
        None
    } else {
        Some(to_isize(h))
    }
}

pub struct MonitorArea {
    /// Work area (screen minus taskbar), in physical pixels.
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    /// Scale factor, 1.0 = 96 DPI.
    pub scale: f32,
}

/// The monitor that currently contains the mouse pointer.
pub fn monitor_under_mouse() -> MonitorArea {
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut info);
        let mut dx = 96u32;
        let mut dy = 96u32;
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let r: RECT = info.rcWork;
        MonitorArea {
            left: r.left,
            top: r.top,
            width: r.right - r.left,
            height: r.bottom - r.top,
            scale: dx.max(48) as f32 / 96.0,
        }
    }
}

pub fn foreground_window() -> isize {
    to_isize(unsafe { GetForegroundWindow() })
}

/// Place the popup (physical pixels) on top of everything and give it focus.
pub fn show_popup(hwnd: isize, x: i32, y: i32, w: i32, h: i32) {
    let hwnd = from_isize(hwnd);
    unsafe {
        // Move first so Windows applies the target monitor's DPI, then size.
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_SHOWWINDOW);
    }
    force_foreground(hwnd);
}

/// Resize/move without changing focus (used when switching between launcher and power menu).
pub fn place_popup(hwnd: isize, x: i32, y: i32, w: i32, h: i32) {
    let hwnd = from_isize(hwnd);
    unsafe {
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
    }
}

/// "Hide" by parking the window off-screen. A truly hidden window stops receiving
/// paint events, which would freeze the app loop, so we keep it alive off-screen.
pub fn park_popup(hwnd: isize) {
    let hwnd = from_isize(hwnd);
    unsafe {
        let _ = SetWindowPos(hwnd, HWND_BOTTOM, -20000, -20000, 1, 1, SWP_NOACTIVATE);
    }
}

fn force_foreground(hwnd: HWND) {
    unsafe {
        let _ = SetForegroundWindow(hwnd);
        if GetForegroundWindow() != hwnd {
            // Windows only lets the app that received the last input steal focus.
            // Tapping Alt is the well-known (and harmless) way around that.
            let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_MENU, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
            };
            let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

pub fn restore_focus(prev: isize) {
    if prev == 0 {
        return;
    }
    unsafe {
        let _ = SetForegroundWindow(from_isize(prev));
    }
}

// ---------------------------------------------------------------------------
// Window switcher
// ---------------------------------------------------------------------------

pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
    pub exe_path: String,
    pub exe_name: String,
}

struct EnumState {
    own: isize,
    out: Vec<WindowInfo>,
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let state = &mut *(lparam.0 as *mut EnumState);
    if to_isize(hwnd) == state.own || !is_alt_tab_window(hwnd) {
        return BOOL(1);
    }
    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return BOOL(1);
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = GetWindowTextW(hwnd, &mut buf);
    let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
    let exe_path = window_exe(hwnd).unwrap_or_default();
    let exe_name = std::path::Path::new(&exe_path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    state.out.push(WindowInfo { hwnd: to_isize(hwnd), title, exe_path, exe_name });
    BOOL(1)
}

unsafe fn is_alt_tab_window(hwnd: HWND) -> bool {
    if !IsWindowVisible(hwnd).as_bool() {
        return false;
    }
    let owner = GetWindow(hwnd, GW_OWNER).hwnd();
    if !owner.is_invalid() {
        return false;
    }
    let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if ex & WS_EX_TOOLWINDOW.0 != 0 {
        return false;
    }
    // Hidden UWP frames and windows on other virtual desktops are "cloaked".
    let mut cloaked: u32 = 0;
    let _ = DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        &mut cloaked as *mut u32 as *mut core::ffi::c_void,
        std::mem::size_of::<u32>() as u32,
    );
    cloaked == 0
}

unsafe fn window_exe(hwnd: HWND) -> Option<String> {
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return None;
    }
    let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    let mut buf = [0u16; 1024];
    let mut size = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut size);
    let _ = windows::Win32::Foundation::CloseHandle(h);
    ok.ok()?;
    Some(String::from_utf16_lossy(&buf[..size as usize]))
}

pub fn list_windows(own: isize) -> Vec<WindowInfo> {
    let mut state = EnumState { own, out: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut state as *mut EnumState as isize));
    }
    state.out
}

pub fn focus_window(hwnd: isize) {
    let h = from_isize(hwnd);
    unsafe {
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        let _ = SetForegroundWindow(h);
    }
}

pub fn close_window(hwnd: isize) {
    unsafe {
        let _ = PostMessageW(from_isize(hwnd), WM_CLOSE, windows::Win32::Foundation::WPARAM(0), LPARAM(0));
    }
}

// ---------------------------------------------------------------------------
// Launching things
// ---------------------------------------------------------------------------

/// Open a file, shortcut, folder, URL or `shell:AppsFolder\...` id.
pub fn shell_open(target: &str, verb: &str) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    unsafe {
        ShellExecuteW(
            HWND::default(),
            &HSTRING::from(verb),
            &HSTRING::from(target),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// Like `shell_open` but with command-line arguments (used for "Run as administrator").
pub fn shell_open_args(file: &str, args: &str, verb: &str) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    unsafe {
        ShellExecuteW(
            HWND::default(),
            &HSTRING::from(verb),
            &HSTRING::from(file),
            &HSTRING::from(args),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// Show a file selected in Explorer.
pub fn reveal_in_explorer(path: &str) {
    let _ = std::process::Command::new("explorer.exe").arg(format!("/select,{path}")).spawn();
}

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Run what the user typed in the Run box, like Win+R does.
pub fn run_command(cmd: &str) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("cmd").raw_arg(format!("/C start \"\" {cmd}")).creation_flags(CREATE_NO_WINDOW).spawn();
}

pub fn hidden_command(program: &str, args: &[&str]) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new(program);
    c.args(args).creation_flags(CREATE_NO_WINDOW);
    c
}

// ---------------------------------------------------------------------------
// Power
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerAction {
    Lock,
    Sleep,
    Hibernate,
    SignOut,
    Restart,
    Shutdown,
}

impl PowerAction {
    pub fn id(&self) -> &'static str {
        match self {
            PowerAction::Lock => "lock",
            PowerAction::Sleep => "sleep",
            PowerAction::Hibernate => "hibernate",
            PowerAction::SignOut => "signout",
            PowerAction::Restart => "restart",
            PowerAction::Shutdown => "shutdown",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            PowerAction::Lock => "Lock",
            PowerAction::Sleep => "Sleep",
            PowerAction::Hibernate => "Hibernate",
            PowerAction::SignOut => "Sign out",
            PowerAction::Restart => "Restart",
            PowerAction::Shutdown => "Shut down",
        }
    }
    /// Lock and sleep are harmless; the rest close your apps.
    pub fn needs_confirm(&self) -> bool {
        !matches!(self, PowerAction::Lock | PowerAction::Sleep)
    }
}

pub fn hibernate_enabled() -> bool {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    winreg::RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey("SYSTEM\\CurrentControlSet\\Control\\Power")
        .and_then(|k| k.get_value::<u32, _>("HibernateEnabled"))
        .map(|v| v != 0)
        .unwrap_or(false)
}

pub fn power(action: PowerAction) {
    use windows::Win32::Foundation::BOOLEAN;
    match action {
        PowerAction::Lock => unsafe {
            let _ = windows::Win32::System::Shutdown::LockWorkStation();
        },
        PowerAction::Sleep => unsafe {
            windows::Win32::System::Power::SetSuspendState(BOOLEAN(0), BOOLEAN(0), BOOLEAN(0));
        },
        PowerAction::Hibernate => unsafe {
            windows::Win32::System::Power::SetSuspendState(BOOLEAN(1), BOOLEAN(0), BOOLEAN(0));
        },
        PowerAction::SignOut => {
            let _ = hidden_command("shutdown", &["/l"]).spawn();
        }
        PowerAction::Restart => {
            let _ = hidden_command("shutdown", &["/r", "/t", "0"]).spawn();
        }
        PowerAction::Shutdown => {
            let _ = hidden_command("shutdown", &["/s", "/t", "0"]).spawn();
        }
    }
}

pub fn uptime_text() -> String {
    let ms = unsafe { windows::Win32::System::SystemInformation::GetTickCount64() };
    let mins = ms / 60_000;
    let (d, h, m) = (mins / 1440, (mins % 1440) / 60, mins % 60);
    let plural = |n: u64, w: &str| format!("{n} {w}{}", if n == 1 { "" } else { "s" });
    if d > 0 {
        format!("Up {}, {}", plural(d, "day"), plural(h, "hour"))
    } else if h > 0 {
        format!("Up {}, {}", plural(h, "hour"), plural(m, "minute"))
    } else {
        format!("Up {}", plural(m, "minute"))
    }
}

pub fn user_at_host() -> String {
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "you".into());
    let host = std::env::var("COMPUTERNAME").unwrap_or_default();
    if host.is_empty() {
        user
    } else {
        format!("{user}@{}", host.to_lowercase())
    }
}

// ---------------------------------------------------------------------------
// System info / settings
// ---------------------------------------------------------------------------

pub fn windows_build() -> u32 {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    winreg::RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion")
        .and_then(|k| k.get_value::<String, _>("CurrentBuildNumber"))
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

pub fn is_windows_11() -> bool {
    windows_build() >= 22000
}

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

pub fn set_start_with_windows(enabled: bool) {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};
    let Ok(key) = winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, KEY_WRITE) else {
        return;
    };
    if enabled {
        if let Ok(exe) = std::env::current_exe() {
            let _ = key.set_value("Hestia", &format!("\"{}\"", exe.display()));
        }
    } else {
        let _ = key.delete_value("Hestia");
    }
}

// ---------------------------------------------------------------------------
// Single instance
// ---------------------------------------------------------------------------

/// Returns true if this is the first process holding `name`.
/// The mutex handle is intentionally leaked so it lives as long as the process.
pub fn claim_single_instance(name: &str) -> bool {
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    unsafe {
        let r = CreateMutexW(None, true, &HSTRING::from(name));
        let already = GetLastError() == ERROR_ALREADY_EXISTS;
        r.is_ok() && !already
    }
}

pub fn instance_running(name: &str) -> bool {
    use windows::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};
    unsafe {
        match OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &HSTRING::from(name)) {
            Ok(h) => {
                let _ = windows::Win32::Foundation::CloseHandle(h);
                true
            }
            Err(_) => false,
        }
    }
}

pub fn bring_to_front(title: &str) -> bool {
    if let Some(h) = find_window(title) {
        focus_window(h);
        true
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Background effects
// ---------------------------------------------------------------------------

struct RawHwnd(isize);

impl raw_window_handle::HasWindowHandle for RawHwnd {
    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let nz = std::num::NonZeroIsize::new(self.0).ok_or(raw_window_handle::HandleError::Unavailable)?;
        let raw = raw_window_handle::RawWindowHandle::Win32(raw_window_handle::Win32WindowHandle::new(nz));
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw) })
    }
}

/// Apply blur / acrylic / mica. Returns false if this Windows version can't do it.
pub fn apply_effect(hwnd: isize, effect: crate::config::Effect, tint: (u8, u8, u8, u8), dark: bool) -> bool {
    use crate::config::Effect;
    let w = RawHwnd(hwnd);
    let _ = window_vibrancy::clear_blur(&w);
    let _ = window_vibrancy::clear_acrylic(&w);
    let _ = window_vibrancy::clear_mica(&w);
    let ok = match effect {
        Effect::None => true,
        Effect::Blur => window_vibrancy::apply_blur(&w, Some(tint)).is_ok(),
        Effect::Acrylic => window_vibrancy::apply_acrylic(&w, Some(tint)).is_ok(),
        Effect::Mica => window_vibrancy::apply_mica(&w, Some(dark)).is_ok(),
    };
    unsafe {
        use windows::Win32::Graphics::Dwm::{
            DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMWCP_ROUND,
        };
        // With a blur, let Windows 11 round the corners so the blur matches our rounded panel.
        // Without one, ask for no frame at all so nothing shows around shaped windows.
        let pref = if effect != Effect::None { DWMWCP_ROUND } else { DWMWCP_DONOTROUND };
        let _ = DwmSetWindowAttribute(
            from_isize(hwnd),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&pref) as u32,
        );
        // DWMWA_COLOR_NONE: no Windows 11 border line around the window.
        let none: u32 = 0xFFFF_FFFE;
        let _ = DwmSetWindowAttribute(
            from_isize(hwnd),
            DWMWA_BORDER_COLOR,
            &none as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        );
    }
    ok
}

/// Cut the window to one or more polygons (physical pixels, relative to the window), or restore
/// the full rectangle with None.
pub fn set_region(hwnd: isize, polys: Option<Vec<Vec<(i32, i32)>>>) {
    use windows::Win32::Graphics::Gdi::{
        CombineRgn, CreatePolygonRgn, DeleteObject, SetWindowRgn, HGDIOBJ, HRGN, RGN_OR, WINDING,
    };
    unsafe {
        let mut total: Option<HRGN> = None;
        for p in polys.unwrap_or_default() {
            if p.len() < 3 {
                continue;
            }
            let pts: Vec<POINT> = p.iter().map(|(x, y)| POINT { x: *x, y: *y }).collect();
            let rgn = CreatePolygonRgn(&pts, WINDING);
            match total {
                None => total = Some(rgn),
                Some(t) => {
                    let _ = CombineRgn(t, t, rgn, RGN_OR);
                    let _ = DeleteObject(HGDIOBJ(rgn.0));
                }
            }
        }
        // The system owns the region after SetWindowRgn, so we don't delete it.
        let _ = SetWindowRgn(from_isize(hwnd), total.unwrap_or_default(), true);
        let _ = HRGN::default();
    }
}
