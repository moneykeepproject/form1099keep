fn main() {
    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
    if let Err(e) = slint_build::compile_with_config("ui/app.slint", config) {
        println!("cargo:warning=slint: {e}");
        std::process::exit(1);
    }

    // On Windows, embed the app icon (assets/icon.ico, made from assets/icon.svg) and the
    // version resource: Explorer, the taskbar, Alt-Tab and the shortcuts show the icon, and
    // the file's Properties show the version. As in CalcKeep's build.rs.
    #[cfg(windows)]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
            println!("cargo:rerun-if-changed=assets/icon.ico");
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/icon.ico");
            res.set("ProductName", "Form1099Keep");
            res.set(
                "FileDescription",
                "Form1099Keep: Forms 1099-NEC and 1099-MISC",
            );
            res.set("CompanyName", "MoneyKeep Project");
            // Windows wants four numeric parts: 0.1.0-alpha.1 -> 0.1.0.1 (the pre-release
            // number goes last; a plain release gets .0). Each part is 16 bits.
            let part = |name: &str| -> u64 {
                std::env::var(name)
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0)
                    .min(0xffff)
            };
            let build = std::env::var("CARGO_PKG_VERSION_PRE")
                .ok()
                .and_then(|pre| pre.rsplit('.').next().and_then(|n| n.parse::<u64>().ok()))
                .filter(|n| *n <= 0xffff)
                .unwrap_or(0);
            let (major, minor, patch) = (
                part("CARGO_PKG_VERSION_MAJOR"),
                part("CARGO_PKG_VERSION_MINOR"),
                part("CARGO_PKG_VERSION_PATCH"),
            );
            let four = format!("{major}.{minor}.{patch}.{build}");
            let packed = (major << 48) | (minor << 32) | (patch << 16) | build;
            res.set_version_info(winresource::VersionInfo::FILEVERSION, packed);
            res.set_version_info(winresource::VersionInfo::PRODUCTVERSION, packed);
            res.set("FileVersion", &four);
            res.set(
                "ProductVersion",
                &std::env::var("CARGO_PKG_VERSION").unwrap_or_default(),
            );
            if let Err(e) = res.compile() {
                println!("cargo:warning=could not embed the Windows icon: {e}");
            }
        }
    }
}
