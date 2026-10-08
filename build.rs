fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // Close any running copy of Hestia so Windows lets the new hestia.exe replace it.
    // (Cargo re-runs this script whenever the source changes.)
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill").args(["/im", "hestia.exe", "/f"]).output();
    }

    // Give hestia.exe its logo and a friendly description in Explorer.
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/hestia.ico");
    res.set("ProductName", "Hestia");
    res.set("FileDescription", "Hestia launcher");
    res.set("LegalCopyright", "GPL-3.0-or-later");
    if let Err(e) = res.compile() {
        println!("cargo:warning=Couldn't add the app icon to hestia.exe: {e}");
    }
}
