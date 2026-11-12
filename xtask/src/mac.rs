//! macOS packaging (MoneyKeep's `docs/specs/macos-release.md`, Part P, batch MR3).
//!
//! - `cargo xtask installer` on a Mac runs `packaging/macos/package.sh` with the same options
//!   (P7);
//! - `cargo xtask icns --out FILE` writes the app bundle's icon (P2) from the icon already in
//!   `assets/`: the PNG images inside `assets/icon.ico` and any `assets/icon-<size>.png`.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The macOS packaging script.
pub const PACKAGE_SH: &str = "packaging/macos/package.sh";
/// This app's bundle identifier (P2) and the names `package.sh` and `verify.sh` use (they
/// hold the values; the tests check the scripts against these).
#[cfg(test)]
pub const BUNDLE_ID: &str = "com.moneykeep.form1099keep";
#[cfg(test)]
pub const APP_NAME: &str = "Form1099Keep";
#[cfg(test)]
pub const APP_ID: &str = "form1099keep";
/// The programs in `Contents/MacOS` besides the main one (renamed to [`APP_NAME`]).
#[cfg(test)]
pub const EXTRAS: &[&str] = &["form1099keep-cli"];

/// `cargo xtask installer` on a Mac (P7): `packaging/macos/package.sh` with the same options
/// (`--arch`, `--skip-build`, `--dry-run`).
pub fn installer(root: &Path, args: &[String]) -> Result<(), String> {
    println!("==> {PACKAGE_SH} {}", args.join(" "));
    let status = Command::new("bash")
        .arg(root.join(PACKAGE_SH))
        .args(args)
        .current_dir(root)
        .status()
        .map_err(|e| format!("cannot run {PACKAGE_SH}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{PACKAGE_SH} failed"))
    }
}

/// `cargo xtask icns --out FILE`.
pub fn icns_task(root: &Path, args: &[String]) -> Result<(), String> {
    let out = match args {
        [flag, file] if flag == "--out" => PathBuf::from(file),
        _ => return Err("usage: cargo xtask icns --out FILE".into()),
    };
    let icns = app_icns(root)?;
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    std::fs::write(&out, &icns).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    println!("wrote {} ({} bytes)", out.display(), icns.len());
    Ok(())
}

/// The `.icns` entries: (ICNS type, side in pixels). `icp4`–`ic10` are the 1x images from 16
/// to 1024 pixels, `ic11`–`ic14` the Retina (2x) images of the 16, 32, 128 and 256 point
/// sizes. Every image is a PNG (macOS 10.7 and later read PNG in all of these types). An entry
/// is written when the icon has an image of exactly that size; macOS scales the nearest one
/// for the rest.
pub const ICNS_ENTRIES: [([u8; 4], u32); 11] = [
    (*b"icp4", 16),
    (*b"icp5", 32),
    (*b"icp6", 64),
    (*b"ic07", 128),
    (*b"ic08", 256),
    (*b"ic09", 512),
    (*b"ic10", 1024),
    (*b"ic11", 32),
    (*b"ic12", 64),
    (*b"ic13", 256),
    (*b"ic14", 512),
];

/// The sizes an app icon must have (Finder's list, icon and Dock views and the Retina
/// versions of the small ones).
pub const REQUIRED_SIDES: [u32; 4] = [16, 32, 128, 256];

/// The app's icon as a macOS `.icns` file, from the PNG images in `assets/icon.ico` and the
/// `assets/icon-<size>.png` files.
pub fn app_icns(root: &Path) -> Result<Vec<u8>, String> {
    let assets = root.join("assets");
    let ico_path = assets.join("icon.ico");
    let ico =
        std::fs::read(&ico_path).map_err(|e| format!("cannot read {}: {e}", ico_path.display()))?;
    let mut images = ico_pngs(&ico).map_err(|e| format!("{}: {e}", ico_path.display()))?;
    // Larger (or extra) sizes kept beside the .ico, such as an icon-1024.png.
    let mut extra: Vec<PathBuf> = std::fs::read_dir(&assets)
        .map_err(|e| format!("cannot list {}: {e}", assets.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix("icon-"))
                .and_then(|n| n.strip_suffix(".png"))
                .is_some_and(|n| n.parse::<u32>().is_ok())
        })
        .collect();
    extra.sort();
    for path in extra {
        let png =
            std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let (w, h) = png_size(&png).ok_or_else(|| format!("{} is not a PNG", path.display()))?;
        if w == h && !images.iter().any(|(s, _)| *s == w) {
            images.push((w, png));
        }
    }
    icns_from_pngs(&images)
}

/// The PNG images inside a Windows `.ico` file, as (side, PNG). Images stored as bitmaps are
/// skipped; non-square ones too.
pub fn ico_pngs(ico: &[u8]) -> Result<Vec<(u32, Vec<u8>)>, String> {
    let u16_at = |at: usize| -> Option<u16> {
        Some(u16::from_le_bytes(ico.get(at..at + 2)?.try_into().ok()?))
    };
    let u32_at = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(ico.get(at..at + 4)?.try_into().ok()?))
    };
    if u16_at(0) != Some(0) || u16_at(2) != Some(1) {
        return Err("not an .ico file".into());
    }
    let count = usize::from(u16_at(4).ok_or("not an .ico file")?);
    let mut out = Vec::new();
    for i in 0..count {
        let entry = 6 + 16 * i;
        let (Some(size), Some(offset)) = (u32_at(entry + 8), u32_at(entry + 12)) else {
            return Err(format!("image {i}'s directory entry is cut short"));
        };
        let start = usize::try_from(offset).map_err(|_| "bad offset")?;
        let end = start
            .checked_add(usize::try_from(size).map_err(|_| "bad size")?)
            .ok_or("bad size")?;
        let data = ico
            .get(start..end)
            .ok_or_else(|| format!("image {i} lies outside the file"))?;
        if let Some((w, h)) = png_size(data) {
            if w == h && !out.iter().any(|(s, _)| *s == w) {
                out.push((w, data.to_vec()));
            }
        }
    }
    Ok(out)
}

/// A PNG's width and height (from its IHDR chunk), or None if it isn't a PNG.
pub fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    if png.get(..8)? != b"\x89PNG\r\n\x1a\n" || png.get(12..16)? != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(png.get(16..20)?.try_into().ok()?);
    let h = u32::from_be_bytes(png.get(20..24)?.try_into().ok()?);
    Some((w, h))
}

/// The `.icns` container: the magic `icns` and the file's length, then one entry per image:
/// its four-byte type, its length including that eight-byte header, and the PNG data; lengths
/// are big-endian.
pub fn icns_from_pngs(images: &[(u32, Vec<u8>)]) -> Result<Vec<u8>, String> {
    for side in REQUIRED_SIDES {
        if !images.iter().any(|(s, _)| *s == side) {
            return Err(format!(
                "the app icon has no {side}x{side} PNG image (assets/icon.ico or assets/icon-{side}.png)"
            ));
        }
    }
    let mut body = Vec::new();
    for (kind, side) in ICNS_ENTRIES {
        let Some((_, png)) = images.iter().find(|(s, _)| *s == side) else {
            continue;
        };
        let len = u32::try_from(png.len() + 8).map_err(|_| "image too large")?;
        body.extend_from_slice(&kind);
        body.extend_from_slice(&len.to_be_bytes());
        body.extend_from_slice(png);
    }
    let total = u32::try_from(body.len() + 8).map_err(|_| "icon too large")?;
    let mut out = Vec::with_capacity(body.len() + 8);
    out.extend_from_slice(b"icns");
    out.extend_from_slice(&total.to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    use super::*;

    fn root() -> PathBuf {
        crate::workspace_root()
    }

    // Only the Unix-only MAC-3 test uses it.
    #[cfg(unix)]
    fn version() -> String {
        crate::package_version(&root()).unwrap()
    }

    /// The entries of an `.icns` file: (type, data).
    fn entries(icns: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
        assert_eq!(&icns[..4], b"icns");
        let total = u32::from_be_bytes(icns[4..8].try_into().unwrap()) as usize;
        assert_eq!(total, icns.len());
        let mut out = Vec::new();
        let mut at = 8;
        while at < icns.len() {
            let kind: [u8; 4] = icns[at..at + 4].try_into().unwrap();
            let len = u32::from_be_bytes(icns[at + 4..at + 8].try_into().unwrap()) as usize;
            out.push((kind, icns[at + 8..at + len].to_vec()));
            at += len;
        }
        assert_eq!(at, icns.len());
        out
    }

    /// P2: the bundle's icon has every size the app's icon has, each a PNG of the size its type
    /// stands for, and at least Finder's required ones.
    #[test]
    fn app_icns_is_made_from_the_existing_icon() {
        let icns = app_icns(&root()).unwrap();
        let found = entries(&icns);
        for (kind, png) in &found {
            let (_, side) = ICNS_ENTRIES.iter().find(|(k, _)| k == kind).unwrap();
            assert_eq!(
                png_size(png),
                Some((*side, *side)),
                "{}",
                String::from_utf8_lossy(kind)
            );
        }
        let kinds: Vec<&[u8; 4]> = found.iter().map(|(k, _)| k).collect();
        for want in [b"icp4", b"icp5", b"ic07", b"ic08", b"ic11", b"ic13"] {
            assert!(kinds.contains(&want), "{}", String::from_utf8_lossy(want));
        }
        // Form1099Keep's icon goes up to 256 pixels (assets/icon.ico); macOS scales it for the
        // larger Finder views.
        assert!(!kinds.contains(&b"ic10"));
        // `cargo xtask icns --out` writes the same bytes.
        let dir = std::env::temp_dir().join(format!("{APP_ID}-icns-{}", std::process::id()));
        let out = dir.join("App.icns");
        icns_task(&root(), &["--out".into(), out.display().to_string()]).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), icns);
        std::fs::remove_dir_all(&dir).ok();
        assert!(icns_task(&root(), &[]).is_err());
    }

    #[test]
    fn icns_rejects_a_missing_size_and_bad_icons() {
        let png = |side: u32| {
            let mut p = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
            p.extend_from_slice(&side.to_be_bytes());
            p.extend_from_slice(&side.to_be_bytes());
            p
        };
        let all: Vec<(u32, Vec<u8>)> = REQUIRED_SIDES.iter().map(|s| (*s, png(*s))).collect();
        assert!(icns_from_pngs(&all).is_ok());
        let e = icns_from_pngs(&all[1..]).unwrap_err();
        assert!(e.contains("no 16x16"), "{e}");
        assert!(ico_pngs(b"not an icon").is_err());
        // A directory entry pointing past the end of the file.
        let mut ico = vec![0, 0, 1, 0, 1, 0];
        ico.extend_from_slice(&[16, 16, 0, 0, 1, 0, 32, 0]);
        ico.extend_from_slice(&100u32.to_le_bytes());
        ico.extend_from_slice(&22u32.to_le_bytes());
        assert!(ico_pngs(&ico).is_err());
        assert_eq!(png_size(b"GIF89a"), None);
    }

    /// Runs `packaging/macos/package.sh` with `args` and `env`; (success, stdout + stderr).
    #[cfg(unix)]
    fn package_sh(args: &[&str], env: &[(&str, &str)]) -> (bool, String) {
        let mut cmd = Command::new("bash");
        cmd.arg(root().join(PACKAGE_SH))
            .args(args)
            .current_dir(root());
        for k in [
            "MACOS_SIGN_IDENTITY",
            "MACOS_KEYCHAIN",
            "MACOS_NOTARY_PROFILE",
            "MACOS_NOTARY_KEY",
            "MACOS_NOTARY_KEY_ID",
            "MACOS_NOTARY_ISSUER",
            "GITHUB_ACTIONS",
        ] {
            cmd.env_remove(k);
        }
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        (
            out.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    }

    /// MAC-3: `package.sh --dry-run` prints the planned steps (both targets, lipo, the bundle's
    /// layout, the signing identity or "ad hoc", the notarization profile, the disk image's
    /// name) without running Apple's tools or building anything; the bundle identifier and file
    /// name are P2's and P3's.
    #[cfg(unix)]
    #[test]
    fn mac3_package_dry_run_prints_the_plan() {
        let v = version();
        let (ok, out) = package_sh(&["--dry-run"], &[]);
        assert!(ok, "{out}");
        let mut want = vec![
            format!("==> {APP_NAME} {v} for macOS (universal), dry run"),
            "--target aarch64-apple-darwin --target x86_64-apple-darwin".to_owned(),
            format!("-p {}", crate::licences::PROGRAM),
            "MACOSX_DEPLOYMENT_TARGET=11.0".to_owned(),
            format!(
                "lipo -create {} from aarch64-apple-darwin x86_64-apple-darwin",
                crate::licences::PROGRAM
            ),
            format!("identifier {BUNDLE_ID}"),
            format!("{APP_NAME}.app/Contents/MacOS/{APP_NAME}\n"),
            format!("{APP_NAME}.app/Contents/Resources/{APP_NAME}.icns\n"),
            format!("{APP_NAME}.app/Contents/Resources/THIRD-PARTY-LICENSES.txt\n"),
            format!("{APP_NAME}.app/Contents/Resources/NOTICE\n"),
            "cargo xtask icns".to_owned(),
            "cargo xtask release --dry-run".to_owned(),
            "sign: ad hoc".to_owned(),
            format!("notarize {APP_NAME}.app: no (ad hoc"),
            format!("disk image: {APP_ID}-{v}-macos-universal.dmg (volume {APP_NAME}"),
            format!("SHA256SUMS.txt with {APP_ID}-{v}-macos-universal.dmg"),
        ];
        let mut order = String::new();
        for p in EXTRAS {
            want.push(format!("--bin {p}"));
            want.push(format!("lipo -create {p} from"));
            want.push(format!("{APP_NAME}.app/Contents/MacOS/{p}\n"));
            order.push_str(&format!(" {p},"));
        }
        want.push(format!(
            "inside out:{order} {APP_NAME}, then {APP_NAME}.app"
        ));
        for w in want {
            assert!(out.contains(&w), "{w:?} not in:\n{out}");
        }
        for apple in ["codesign --", "hdiutil create", "xcrun ", "plutil "] {
            assert!(!out.contains(apple), "{apple} in:\n{out}");
        }
        // With the project's Mac's keychain (what runner-keychain.sh sets).
        let (ok, out) = package_sh(
            &["--dry-run", "--arch", "aarch64", "--skip-build"],
            &[
                (
                    "MACOS_SIGN_IDENTITY",
                    "0123456789ABCDEF0123456789ABCDEF01234567",
                ),
                (
                    "MACOS_KEYCHAIN",
                    "/Users/mkbuild/Library/Keychains/mk-signing.keychain-db",
                ),
                ("MACOS_NOTARY_PROFILE", "mk-notary"),
            ],
        );
        assert!(ok, "{out}");
        for w in [
            "sign: identity 0123456789ABCDEF0123456789ABCDEF01234567 (keychain /Users/mkbuild/Library/Keychains/mk-signing.keychain-db)",
            "xcrun notarytool submit --keychain-profile mk-notary --wait",
            "build: skipped (--skip-build)",
        ] {
            assert!(out.contains(w), "{w:?} not in:\n{out}");
        }
        assert!(
            out.contains(&format!("{APP_ID}-{v}-macos-aarch64.dmg")),
            "{out}"
        );
        assert!(!out.contains("x86_64-apple-darwin"), "{out}");
        // GitHub's Mac: an App Store Connect API key file, never a password (L9).
        let (ok, out) = package_sh(
            &["--dry-run"],
            &[
                ("MACOS_SIGN_IDENTITY", "ABC"),
                ("MACOS_NOTARY_KEY", "/private/tmp/key.p8"),
                ("MACOS_NOTARY_KEY_ID", "KEYID"),
                ("MACOS_NOTARY_ISSUER", "ISSUER"),
            ],
        );
        assert!(ok, "{out}");
        assert!(
            out.contains("notarytool submit --key <file> --key-id KEYID --wait"),
            "{out}"
        );
        // A real identity without a notarization profile: signed, not notarized.
        let (_, out) = package_sh(&["--dry-run"], &[("MACOS_SIGN_IDENTITY", "ABC")]);
        assert!(out.contains("signed but not notarized"), "{out}");
        // Bad options fail.
        assert!(!package_sh(&["--dry-run", "--arch", "ppc"], &[]).0);
        assert!(!package_sh(&["--bogus"], &[]).0);
        // Off a Mac, a real run stops before building anything.
        if !cfg!(target_os = "macos") {
            let (ok, out) = package_sh(&[], &[]);
            assert!(!ok && out.contains("run it on a Mac"), "{out}");
        }
    }

    /// P2: the bundle's Info.plist template (identifier, minimum macOS, the executable and icon
    /// names the script uses), empty entitlements, and the same values in verify.sh.
    #[test]
    fn mac_p2_bundle_template() {
        let read = |rel: &str| std::fs::read_to_string(root().join(rel)).unwrap();
        let plist = read("packaging/macos/Info.plist.in");
        let script = read(PACKAGE_SH);
        let verify = read("packaging/macos/verify.sh");
        let value = |key: &str| {
            let at = plist.find(&format!("<key>{key}</key>")).unwrap();
            let rest = &plist[at..];
            let s = rest.find("<string>").unwrap() + "<string>".len();
            let e = rest.find("</string>").unwrap();
            rest[s..e].to_owned()
        };
        assert_eq!(value("CFBundleIdentifier"), BUNDLE_ID);
        assert_eq!(value("CFBundleExecutable"), APP_NAME);
        assert_eq!(value("CFBundleIconFile"), APP_NAME);
        assert_eq!(value("CFBundleName"), APP_NAME);
        assert_eq!(value("LSMinimumSystemVersion"), "11.0");
        assert_eq!(value("CFBundleShortVersionString"), "@SHORT_VERSION@");
        assert!(!plist.contains("LSUIElement"));
        assert!(script.contains("export MACOSX_DEPLOYMENT_TARGET=11.0\n"));
        let extras = if EXTRAS.is_empty() {
            "EXTRAS=()\n".to_owned()
        } else {
            format!("EXTRAS=({})\n", EXTRAS.join(" "))
        };
        for s in [&script, &verify] {
            assert!(s.contains(&format!("BUNDLE_ID={BUNDLE_ID}\n")));
            assert!(s.contains(&format!("APP_NAME={APP_NAME}")));
            assert!(s.contains(&format!("APP_ID={APP_ID}")));
            assert!(s.contains(&extras), "{extras}");
        }
        // The programs are the Windows release's (the main one renamed to the app's name).
        assert!(script.contains(&format!("MAIN_BIN={}", crate::licences::PROGRAM)));
        let ent = read("packaging/macos/entitlements.plist");
        assert!(ent.contains("<dict>\n</dict>"), "no entitlements");
    }

    /// Shell scripts are stored executable in git (keep-launcher once shipped them without the
    /// bit, so `./package.sh` failed).
    #[test]
    fn shell_scripts_are_executable_in_git() {
        let Ok(out) = Command::new("git")
            .arg("-C")
            .arg(root())
            .args(["ls-files", "-s", "--", "*.sh"])
            .output()
        else {
            return;
        };
        let listed = String::from_utf8_lossy(&out.stdout);
        if !out.status.success() || listed.trim().is_empty() {
            // Not a git checkout (or not yet added): nothing to check.
            return;
        }
        for line in listed.lines() {
            assert!(line.starts_with("100755 "), "not executable in git: {line}");
        }
        for name in [
            "packaging/env.sh",
            "packaging/macos/package.sh",
            "packaging/macos/verify.sh",
            "packaging/macos/runner-keychain.sh",
            "packaging/macos/import-cert.sh",
        ] {
            assert!(listed.contains(name), "{name}");
        }
    }
}
