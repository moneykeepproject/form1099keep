//! Form1099Keep developer tasks, patterned on CalcKeep's and MoneyKeep's xtask.
//!
//! Form1099Keep builds against the sibling MoneyKeep checkout (`../moneykeep`), so this
//! xtask needs it too: cargo reads the whole workspace's dependencies.
//!
//! - `cargo xtask release [--dry-run]`: builds Form1099Keep in release mode and gathers
//!   dist\Form1099Keep-<version>\ (form1099keep.exe, form1099keep-cli.exe, form1099keep.ico,
//!   LICENSE-MIT, LICENSE-APACHE, NOTICE, README.md).
//!   `--dry-run` only lists the steps and files.
//! - `cargo xtask installer [--skip-release]` (Windows): `release`, then `makensis` on
//!   installer\form1099keep.nsi, giving dist\form1099keep-<version>-windows-x64-setup.exe (the
//!   name MoneyKeep Launcher installs), then dist\SHA256SUMS.txt. The program and the setup
//!   are signed with `signtool` (before the checksums are taken) when `FK_SIGN_CERT` (a .pfx)
//!   and `FK_SIGN_PASSWORD` are set; `FK_SIGN_TIMESTAMP` overrides the timestamp server.
//!   On a Mac it runs `packaging/macos/package.sh` instead (MoneyKeep's
//!   docs/specs/macos-release.md P7): dist/form1099keep-<version>-macos-universal.dmg and
//!   dist/SHA256SUMS.txt, with package.sh's options (`--arch`, `--skip-build`, `--dry-run`).
//! - `cargo xtask icns --out FILE`: the macOS app icon, from assets/icon.ico and
//!   assets/icon-<size>.png (P2).
//! - `cargo xtask workflows`: the self-hosted Mac runner rules for .github/workflows (W9).

mod licences;
mod mac;
mod workflows;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What this repository releases.
struct App {
    /// Lower-case id: the asset stem MoneyKeep Launcher looks for.
    id: &'static str,
    /// Display name (the release folder, the setup's title).
    product: &'static str,
    /// The cargo package that builds the programs.
    package: &'static str,
    /// The programs (without `.exe`) copied into the release.
    programs: &'static [&'static str],
    /// The NSIS script, relative to the root.
    script: &'static str,
    /// The prefix of the signing variables (`<P>_SIGN_CERT`, ...).
    sign_prefix: &'static str,
}

const APP: App = App {
    id: "form1099keep",
    product: "Form1099Keep",
    package: "form1099keep",
    programs: &["form1099keep", "form1099keep-cli"],
    script: "installer/form1099keep.nsi",
    sign_prefix: "FK",
};

const USAGE: &str =
    "usage: cargo xtask release [--dry-run] | cargo xtask installer [--skip-release] (macOS: packaging/macos/package.sh's options) | cargo xtask icns --out FILE | cargo xtask workflows";

/// The tasks `main` runs (release.yml may name only these).
pub(crate) const TASKS: [&str; 4] = ["release", "installer", "icns", "workflows"];

/// The checksum list published beside the setup.
const SUMS_FILE: &str = "SHA256SUMS.txt";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or_default();
    let result = match args.first().map(String::as_str) {
        Some("release") => release(rest).map(|_| ()),
        Some("installer") => installer(rest),
        Some("icns") => mac::icns_task(&workspace_root(), rest),
        Some("workflows") => workflows::run(&workspace_root()),
        _ => Err(USAGE.into()),
    };
    if let Err(e) = result {
        eprintln!("xtask: {e}");
        std::process::exit(1);
    }
}

/// The repository root: the folder above this crate.
fn workspace_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap_or(manifest).to_path_buf()
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

/// The version in the root Cargo.toml (`[package]` or `[workspace.package]`).
fn package_version(root: &Path) -> Result<String, String> {
    let toml = std::fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|e| format!("cannot read Cargo.toml: {e}"))?;
    parse_package_version(&toml)
        .ok_or_else(|| "no version in Cargo.toml's [package] or [workspace.package]".into())
}

fn parse_package_version(toml: &str) -> Option<String> {
    let mut in_package = false;
    for line in toml.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_package = t == "[package]" || t == "[workspace.package]";
            continue;
        }
        if in_package {
            if let Some(rest) = t.strip_prefix("version") {
                if let Some(value) = rest.trim_start().strip_prefix('=') {
                    return Some(value.trim().trim_matches('"').to_owned());
                }
            }
        }
    }
    None
}

fn target_dir(root: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"))
}

struct Release {
    version: String,
    folder: PathBuf,
}

/// Documents copied into the release folder: (source relative to the root, name there).
fn release_files() -> Vec<(PathBuf, String)> {
    let icon = (
        PathBuf::from("assets").join("icon.ico"),
        "form1099keep.ico".to_owned(),
    );
    std::iter::once(icon).chain(release_documents()).collect()
}

/// The documents among [`release_files`].
fn release_documents() -> Vec<(PathBuf, String)> {
    ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "README.md"]
        .iter()
        .map(|f| (PathBuf::from(f), (*f).to_owned()))
        .collect()
}

/// `cargo xtask release`.
fn release(args: &[String]) -> Result<Release, String> {
    let dry_run = args.iter().any(|a| a == "--dry-run");
    if let Some(other) = args.iter().find(|a| *a != "--dry-run") {
        return Err(format!("release: unknown option {other}"));
    }
    run_release(&workspace_root(), dry_run)
}

fn run_release(root: &Path, dry_run: bool) -> Result<Release, String> {
    let version = package_version(root)?;
    let folder = root.join("dist").join(format!("{}-{version}", APP.product));
    let built = target_dir(root).join("release");
    println!("==> {} {version} -> {}", APP.product, folder.display());
    if dry_run {
        println!("    cargo build --release -p {}", APP.package);
        for program in APP.programs {
            println!(
                "    {} -> {}",
                built.join(exe(program)).display(),
                exe(program)
            );
        }
        for (src, name) in release_files() {
            println!("    {} -> {name}", src.display());
        }
        println!(
            "    {} (every crate compiled into the programs, from cargo metadata)",
            licences::FILE
        );
        let (t, n) = licences::write(root, &folder, &version)?;
        println!("wrote {} ({n} crates)", t.display());
        println!("  installer (cargo xtask installer, on Windows), in dist:");
        println!("    {}", setup_name(&version));
        println!("    {SUMS_FILE}");
        return Ok(Release { version, folder });
    }
    println!("==> cargo build --release -p {}", APP.package);
    let status = Command::new(env!("CARGO"))
        .args(["build", "--release", "-p", APP.package])
        .current_dir(root)
        .status()
        .map_err(|e| format!("failed to start cargo: {e}"))?;
    if !status.success() {
        return Err("the release build failed".into());
    }
    if folder.exists() {
        std::fs::remove_dir_all(&folder)
            .map_err(|e| format!("cannot clear {}: {e}", folder.display()))?;
    }
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("cannot create {}: {e}", folder.display()))?;
    for program in APP.programs {
        copy(&built.join(exe(program)), &folder.join(exe(program)))?;
    }
    for (src, name) in release_files() {
        copy(&root.join(src), &folder.join(name))?;
    }
    let (t, n) = licences::write(root, &folder, &version)?;
    println!("wrote {} ({n} crates)", t.display());
    println!("\nxtask release: {}", folder.display());
    Ok(Release { version, folder })
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("cannot copy {} to {}: {e}", from.display(), to.display()))
}

// ----- the Windows installer -------------------------------------------------------------

/// `1.2.3` as the four numbers Windows file versions use (`1.2.3.0`). A pre-release
/// such as `0.1.0-alpha.1` puts its number last (`0.1.0.1`), as the other MoneyKeep apps do.
fn version_quad(v: &str) -> String {
    let core = v.split(['-', '+']).next().unwrap_or(v);
    let mut parts: Vec<String> = core
        .split('.')
        .map(|n| {
            n.parse::<u16>()
                .map_or_else(|_| "0".to_owned(), |x| x.to_string())
        })
        .take(3)
        .collect();
    while parts.len() < 3 {
        parts.push("0".into());
    }
    let pre = v
        .split('+')
        .next()
        .and_then(|s| s.split_once('-'))
        .and_then(|(_, p)| p.rsplit('.').next())
        .and_then(|n| n.parse::<u16>().ok())
        .unwrap_or(0);
    parts.push(pre.to_string());
    parts.join(".")
}

/// The setup's file name: `<app>-<version>-<os>-<arch>-setup.exe`, the name MoneyKeep
/// Launcher installs.
fn setup_name(version: &str) -> String {
    format!("{}-{version}-windows-x64-setup.exe", APP.id)
}

/// The SHA-256 of a file, as lower-case hex.
fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        match buf.get(..n) {
            Some(chunk) if !chunk.is_empty() => hasher.update(chunk),
            _ => break,
        }
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Writes `dir/SHA256SUMS.txt`: one `<sha256>  <name>` line (two spaces, as `sha256sum`
/// writes) for each of the files `names` in `dir`.
fn write_sums(dir: &Path, names: &[String]) -> Result<PathBuf, String> {
    let mut text = String::new();
    for name in names {
        text.push_str(&format!("{}  {name}\n", sha256_file(&dir.join(name))?));
    }
    let sums = dir.join(SUMS_FILE);
    std::fs::write(&sums, text).map_err(|e| format!("cannot write {}: {e}", sums.display()))?;
    Ok(sums)
}

/// `makensis`'s arguments (the `-` option form works on Windows and elsewhere).
fn makensis_args(version: &str, source: &Path, out_file: &Path, script: &Path) -> Vec<OsString> {
    let define = |k: &str, v: &str| OsString::from(format!("-D{k}={v}"));
    vec![
        OsString::from("-V2"),
        define("VERSION", version),
        define("VERSION_QUAD", &version_quad(version)),
        define("SOURCE_DIR", &source.display().to_string()),
        define("OUT_FILE", &out_file.display().to_string()),
        script.as_os_str().to_owned(),
    ]
}

/// `makensis` on `PATH`, or in NSIS's usual install folders on Windows.
fn find_makensis() -> Option<PathBuf> {
    let name = exe("makensis");
    let on_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(&name))
            .find(|p| p.is_file())
    });
    on_path.or_else(|| {
        ["ProgramFiles(x86)", "ProgramFiles"]
            .iter()
            .filter_map(std::env::var_os)
            .map(|d| PathBuf::from(d).join("NSIS").join(&name))
            .find(|p| p.is_file())
    })
}

/// The licence page: NOTICE, then the MIT and Apache-2.0 texts, with Windows line ends.
fn licence_text(root: &Path) -> Result<String, String> {
    let read = |f: &str| {
        std::fs::read_to_string(root.join(f)).map_err(|e| format!("cannot read {f}: {e}"))
    };
    let text = format!(
        "{}\n\n{}\n\n{}",
        read("NOTICE")?.trim_end(),
        read("LICENSE-MIT")?.trim_end(),
        read("LICENSE-APACHE")?
    );
    Ok(text.replace("\r\n", "\n").replace('\n', "\r\n"))
}

/// `signtool` on `PATH`, or the newest one in the Windows SDK.
fn find_signtool() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join("signtool.exe"))
            .find(|p| p.is_file())
    });
    on_path.or_else(|| {
        let kits = PathBuf::from(std::env::var_os("ProgramFiles(x86)")?)
            .join("Windows Kits")
            .join("10")
            .join("bin");
        let mut found: Vec<PathBuf> = std::fs::read_dir(&kits)
            .ok()?
            .flatten()
            .map(|e| e.path().join("x64").join("signtool.exe"))
            .filter(|p| p.is_file())
            .collect();
        found.sort();
        found.pop()
    })
}

struct Signing {
    tool: PathBuf,
    cert: OsString,
    password: OsString,
    timestamp: String,
}

/// Signing settings from `<P>_SIGN_CERT` (a .pfx) and `<P>_SIGN_PASSWORD`, with
/// `<P>_SIGN_TIMESTAMP` overriding the timestamp server.
fn signing() -> Result<Option<Signing>, String> {
    let var = |name: &str| {
        std::env::var_os(format!("{}_SIGN_{name}", APP.sign_prefix)).filter(|v| !v.is_empty())
    };
    let (Some(cert), Some(password)) = (var("CERT"), var("PASSWORD")) else {
        return Ok(None);
    };
    let tool = find_signtool().ok_or_else(|| {
        format!(
            "{}_SIGN_CERT is set but signtool.exe was not found: install the Windows SDK's signing tools, or put signtool on PATH",
            APP.sign_prefix
        )
    })?;
    let timestamp = var("TIMESTAMP").map_or_else(
        || "http://timestamp.digicert.com".to_owned(),
        |t| t.to_string_lossy().into_owned(),
    );
    Ok(Some(Signing {
        tool,
        cert,
        password,
        timestamp,
    }))
}

fn sign(s: &Signing, file: &Path) -> Result<(), String> {
    println!("==> sign {}", file.display());
    let status = Command::new(&s.tool)
        .args([
            "sign",
            "/fd",
            "SHA256",
            "/td",
            "SHA256",
            "/tr",
            &s.timestamp,
            "/f",
        ])
        .arg(&s.cert)
        .arg("/p")
        .arg(&s.password)
        .arg(file)
        .status()
        .map_err(|e| format!("cannot run signtool: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("signing {} failed", file.display()))
    }
}

const NO_MAKENSIS: &str = "makensis (NSIS) was not found. Install NSIS 3 from https://nsis.sourceforge.io/Download (or: winget install NSIS.NSIS), open a new terminal, and run cargo xtask installer again.";

/// `cargo xtask installer`.
fn installer(args: &[String]) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        return mac::installer(&workspace_root(), args);
    }
    let skip_release = args.iter().any(|a| a == "--skip-release");
    if let Some(other) = args.iter().find(|a| *a != "--skip-release") {
        return Err(format!("installer: unknown option {other}"));
    }
    if !cfg!(windows) {
        return Err(format!("cargo xtask installer makes the Windows setup on Windows and the macOS disk image on a Mac. Here, cargo xtask release --dry-run lists what it would make ({}).", APP.product));
    }
    let makensis = find_makensis().ok_or(NO_MAKENSIS)?;
    let sign_with = signing()?;
    let root = workspace_root();
    let rel = if skip_release {
        let r = run_release(&root, true)?;
        if APP
            .programs
            .iter()
            .any(|p| !r.folder.join(exe(p)).is_file())
        {
            return Err(format!(
                "{} has no release: run cargo xtask installer without --skip-release",
                r.folder.display()
            ));
        }
        r
    } else {
        run_release(&root, false)?
    };
    if let Some(s) = &sign_with {
        for program in APP.programs {
            sign(s, &rel.folder.join(exe(program)))?;
        }
    }
    std::fs::write(rel.folder.join("licence.txt"), licence_text(&root)?)
        .map_err(|e| format!("cannot write licence.txt: {e}"))?;
    let dist = root.join("dist");
    let setup = dist.join(setup_name(&rel.version));
    println!("==> makensis {}", APP.script);
    let status = Command::new(&makensis)
        .args(makensis_args(
            &rel.version,
            &rel.folder,
            &setup,
            &root.join(APP.script),
        ))
        .current_dir(&root)
        .status()
        .map_err(|e| format!("cannot run makensis: {e}"))?;
    if !status.success() || !setup.is_file() {
        return Err("makensis failed".into());
    }
    match &sign_with {
        Some(s) => sign(s, &setup)?,
        None => println!("Not signed: Windows SmartScreen will warn on other computers."),
    }
    // After signing: the checksum is of the file that is published.
    let sums = write_sums(&dist, &[setup_name(&rel.version)])?;
    println!("\nxtask installer: {}", setup.display());
    println!("                 {}", sums.display());
    Ok(())
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

    #[test]
    fn versions_and_names() {
        assert_eq!(version_quad("0.1.0"), "0.1.0.0");
        assert_eq!(version_quad("1.2.3-beta.1"), "1.2.3.1");
        assert_eq!(version_quad("0.1.0-alpha.1"), "0.1.0.1");
        assert_eq!(version_quad("1.2.3-beta"), "1.2.3.0");
        assert_eq!(
            setup_name("0.1.0-alpha.1"),
            format!("{}-0.1.0-alpha.1-windows-x64-setup.exe", APP.id)
        );
        assert_eq!(setup_name("1.0.0"), setup_name("1.0.0").to_lowercase());
        let toml =
            "[package]\nname = \"x\"\nversion = \"0.4.2\"\n\n[dependencies]\nversion = \"9\"\n";
        assert_eq!(parse_package_version(toml).as_deref(), Some("0.4.2"));
        let ws =
            "[workspace]\nmembers = [\"a\"]\n\n[workspace.package]\nversion = \"0.1.0-alpha.1\"\n";
        assert_eq!(parse_package_version(ws).as_deref(), Some("0.1.0-alpha.1"));
        // This repository's release: 0.1.0, four numbers 0.1.0.0.
        let v = package_version(&workspace_root()).unwrap();
        assert_eq!(v, "0.1.0");
        assert_eq!(version_quad(&v), "0.1.0.0");
    }

    #[test]
    fn the_dry_run_names_the_release_folder() {
        let r = run_release(&workspace_root(), true).unwrap();
        assert!(
            r.folder
                .ends_with(format!("dist/{}-0.1.0", APP.product)),
            "{}",
            r.folder.display()
        );
        assert!(!r.folder.exists() || r.folder.is_dir());
    }

    #[test]
    fn sha256sums_lines_match_the_files() {
        let dir = std::env::temp_dir().join(format!("{}-sums-{}", APP.id, std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a-setup.exe"), b"abc").unwrap();
        let big = vec![b'x'; 200_000];
        std::fs::write(dir.join("big.bin"), &big).unwrap();
        let sums = write_sums(&dir, &["a-setup.exe".into(), "big.bin".into()]).unwrap();
        assert_eq!(sums, dir.join("SHA256SUMS.txt"));
        let text = std::fs::read_to_string(&sums).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        // The published SHA-256 test vector for "abc".
        assert_eq!(
            lines[0],
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  a-setup.exe"
        );
        use sha2::{Digest, Sha256};
        let want: String = Sha256::digest(&big)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(lines[1], format!("{want}  big.bin"));
        assert_eq!(lines.len(), 2);
        assert!(write_sums(&dir, &["missing.exe".into()]).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn licence_page_has_windows_line_ends() {
        let t = licence_text(&workspace_root()).unwrap();
        assert!(!t.replace("\r\n", "").contains('\n'));
        assert!(t.starts_with(APP.product));
        assert!(t.contains("MIT License"));
        assert!(t.contains("Apache License"));
    }

    /// The script's silent mode (/S, as MoneyKeep Launcher runs it), checked by reading it:
    /// every question has a silent default, the running-app wait and the exit codes are
    /// there, and both .onInit and un.onInit wait for the app when silent.
    #[test]
    fn installer_script_has_the_silent_mode_paths() {
        let script = std::fs::read_to_string(workspace_root().join(APP.script)).unwrap();
        let code: Vec<&str> = script
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with(';'))
            .collect();
        for line in code.iter().filter(|l| l.starts_with("MessageBox")) {
            assert!(
                line.contains("/SD "),
                "a question without a silent default: {line}"
            );
        }
        // A question about the user's data defaults to No, and No when silent.
        for line in code
            .iter()
            .filter(|l| l.starts_with("MessageBox") && l.contains("MB_YESNO"))
        {
            assert!(
                line.contains("MB_DEFBUTTON2") && line.contains("/SD IDNO"),
                "{line}"
            );
        }
        let has = |needle: &str| code.iter().any(|l| l.contains(needle));
        for needle in [
            &format!("!define APP_EXE \"{}\"", exe_name()),
            &format!("!define PRODUCT \"{}\"", APP.product),
            &format!(
                "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{}",
                APP.product
            ),
            "!define PUBLISHER \"MoneyKeep Project\"",
            "InstallDir \"$PROGRAMFILES64\\${PRODUCT}\"",
            "RequestExecutionLevel admin",
            "!define EXIT_APP_OPEN 2",
            "!define EXIT_FAILED 3",
            "SetErrorLevel ${EXIT_APP_OPEN}",
            "Function ${un}SilentCloseApp",
            "${If} $R5 >= 30",
            "taskkill.exe\" /IM ${APP_EXE}",
            "\"DisplayVersion\" \"${VERSION}\"",
            "\"Publisher\" \"${PUBLISHER}\"",
            "\"InstallLocation\" \"$INSTDIR\"",
            "\"QuietUninstallString\" '\"$INSTDIR\\uninstall.exe\" /S'",
            "CreateShortcut \"$SMPROGRAMS\\${PRODUCT}.lnk\"",
            "!insertmacro MUI_PAGE_LICENSE \"${SOURCE_DIR}\\licence.txt\"",
            "VIProductVersion \"${VERSION_QUAD}\"",
        ] {
            assert!(has(needle), "missing: {needle}");
        }
        assert!(!has("taskkill.exe\" /F"), "never forces the app closed");
        let body = |name: &str| -> String {
            let (_, from) = script.split_once(&format!("Function {name}\n")).unwrap();
            from.split_once("FunctionEnd").unwrap().0.to_owned()
        };
        for (init, call) in [
            (".onInit", "Call SilentCloseApp"),
            ("un.onInit", "Call un.SilentCloseApp"),
        ] {
            let b = body(init);
            assert!(
                b.contains("${If} ${Silent}")
                    && b.contains(call)
                    && b.contains("SetErrorLevel ${EXIT_FAILED}"),
                "{init}"
            );
        }
        let (_, off) = script
            .split_once("Section \"-Shortcuts turned off\"\n")
            .unwrap();
        assert!(off.trim_start().starts_with("${IfNot} ${Silent}"));
        assert!(script.contains("Section \"-Done\"\n  SetErrorLevel 0\nSectionEnd"));
        // Every program in the release is installed and removed.
        for program in APP.programs {
            assert!(has(&format!("{program}.exe")), "{program}");
        }
    }

    /// security-review-1 SR-B4: the dry run writes THIRD-PARTY-LICENSES.txt; the installer
    /// installs it beside the programs and the uninstaller removes it.
    #[test]
    fn sr_b4_release_and_installer_carry_the_third_party_licences() {
        let root = workspace_root();
        let r = run_release(&root, true).unwrap();
        let text = std::fs::read_to_string(r.folder.join(licences::FILE)).unwrap();
        assert!(text.contains("\r\nslint "), "lists slint");
        let script = std::fs::read_to_string(root.join(APP.script)).unwrap();
        assert!(script.contains(r#"File "${SOURCE_DIR}\THIRD-PARTY-LICENSES.txt""#));
        assert!(script.contains(r#"Delete "$INSTDIR\THIRD-PARTY-LICENSES.txt""#));
    }

    /// The settings question: only the settings file, default No (and No when silent).
    #[test]
    fn the_uninstaller_asks_before_removing_settings_and_never_touches_books() {
        let script = std::fs::read_to_string(workspace_root().join(APP.script)).unwrap();
        assert!(script.contains("!define DATA_DIR \"Form1099Keep\""));
        assert!(script.contains("!define SETTINGS \"config\\settings.json\""));
        assert!(script.contains("MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 \"Also remove your Form1099Keep settings"));
        // security-review-1 L8: the original user's %APPDATA% (from the unelevated stub),
        // and no link or junction on the way.
        assert!(script.contains(r#""UninstallString" '"$SYSDIR\cmd.exe" /d /c start "" "$INSTDIR\uninstall.exe" /APPDATA="%APPDATA%"'"#));
        assert!(script.contains(r#"StrCpy $0 "$OrigAppData\${DATA_DIR}""#));
        assert!(script.contains(r#"${GetFileAttributes} "$0\config" "REPARSE_POINT" $2"#));
        assert!(script.contains("Delete \"$0\\${SETTINGS}\""));
        assert!(
            !script.contains("$APPDATA\\${DATA_DIR}"),
            "never the uninstaller's own $APPDATA"
        );
        // Nothing of the user's is removed recursively.
        assert!(
            !script.contains("RMDir /r"),
            "the uninstaller never removes a folder with what is in it"
        );
        // The icon is installed and is the setup's, the shortcuts' and Installed apps' icon.
        assert!(script.contains("!define MUI_ICON \"${SOURCE_DIR}\\${ICON_FILE}\""));
        assert!(script.contains("\"DisplayIcon\" \"$INSTDIR\\${ICON_FILE}\""));
        let ico = std::fs::read(workspace_root().join("assets").join("icon.ico")).unwrap();
        assert_eq!(
            ico.get(..4),
            Some(&[0u8, 0, 1, 0][..]),
            "assets/icon.ico is an icon file"
        );
    }

    /// The main program's Windows name.
    fn exe_name() -> String {
        format!("{}.exe", APP.programs[0])
    }

    /// makensis compiles the script with placeholder programs and the real licence;
    /// skipped with a note when NSIS isn't installed.
    #[test]
    fn installer_script_compiles() {
        let Some(makensis) = find_makensis() else {
            eprintln!(
                "SKIPPED: makensis (NSIS) is not installed; {} was not compiled",
                APP.script
            );
            return;
        };
        let root = workspace_root();
        let tmp = std::env::temp_dir().join(format!("{}-nsis-{}", APP.id, std::process::id()));
        let src = tmp.join("src");
        std::fs::create_dir_all(&src).unwrap();
        for program in APP.programs {
            std::fs::write(src.join(format!("{program}.exe")), b"placeholder").unwrap();
        }
        for (from, name) in release_files() {
            std::fs::copy(root.join(from), src.join(name)).unwrap();
        }
        std::fs::write(src.join(licences::FILE), b"crates").unwrap();
        std::fs::write(src.join("licence.txt"), licence_text(&root).unwrap()).unwrap();
        let setup = tmp.join(setup_name("0.1.0"));
        let out = Command::new(&makensis)
            .args(makensis_args("0.1.0", &src, &setup, &root.join(APP.script)))
            .output()
            .unwrap();
        assert!(
            out.status.success() && setup.is_file(),
            "makensis failed:\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        std::fs::remove_dir_all(&tmp).ok();
    }
}
