//! `THIRD-PARTY-LICENSES.txt` (security-review-1 M8, as MoneyKeep's B4): every crate
//! compiled into the shipped programs (the `form1099keep` package's desktop app and
//! command line, for this computer's platform), with its
//! version and licence, and the licence texts (with their copyright lines) the crates
//! ship, each distinct text once with the crates that carry it.
//!
//! Made from `cargo metadata` (the dependency graph, filtered to the platform the program
//! is built for) and the licence files in each crate's own source folder (the registry
//! copy cargo already downloaded): no new dependency. Normal dependencies are followed
//! (build scripts' dependencies and test-only ones are not compiled into the program). A
//! crate that ships no licence file is listed with its licence and authors; the MIT and
//! Apache-2.0 texts it then falls under are given at the end (from this repository's own
//! `LICENSE-MIT` body and `LICENSE-APACHE`). `cargo xtask release` writes it into the
//! release folder and the installer installs it beside the programs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The file shipped beside the program.
pub const FILE: &str = "THIRD-PARTY-LICENSES.txt";

/// The package of the shipped programs (form1099keep and form1099keep-cli).
pub const PROGRAM: &str = "form1099keep";

/// One crate in the list.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Crate {
    pub name: String,
    pub version: String,
    /// The SPDX expression from its manifest (`unknown` when it gives none).
    pub licence: String,
    pub authors: Vec<String>,
    /// This repository's own (a workspace member).
    pub own: bool,
    /// The licence files in its source folder: (file name, text).
    pub files: Vec<(String, String)>,
}

/// This computer's platform (`rustc -vV`'s host).
pub fn host_triple() -> Result<String, String> {
    let out = Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|e| format!("cannot run rustc: {e}"))?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(str::to_owned)
        .ok_or_else(|| "rustc -vV gave no host".to_owned())
}

/// `cargo metadata` for `root`, its graph filtered to `target`.
pub fn metadata(root: &Path, target: &str) -> Result<Value, String> {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--filter-platform",
            target,
        ])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: {e}"))
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

fn workspace_ids(meta: &Value) -> BTreeSet<String> {
    meta["workspace_members"]
        .as_array()
        .map(|a| a.iter().map(text).collect())
        .unwrap_or_default()
}

/// The package ids reached from the workspace package named `program` by normal dependencies.
pub fn reached(meta: &Value, program: &str) -> Result<BTreeSet<String>, String> {
    let packages = meta["packages"].as_array().ok_or("no packages")?;
    let nodes = meta
        .pointer("/resolve/nodes")
        .and_then(Value::as_array)
        .ok_or("no resolve")?;
    let by_id: BTreeMap<String, &Value> = nodes.iter().map(|n| (text(&n["id"]), n)).collect();
    let workspace = workspace_ids(meta);
    let mut todo: Vec<String> = packages
        .iter()
        .filter(|p| workspace.contains(&text(&p["id"])) && p["name"].as_str() == Some(program))
        .map(|p| text(&p["id"]))
        .collect();
    if todo.len() != 1 {
        return Err(format!("{program} was not found in cargo metadata"));
    }
    let mut seen = BTreeSet::new();
    while let Some(id) = todo.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(node) = by_id.get(&id) else { continue };
        for d in node["deps"].as_array().into_iter().flatten() {
            let normal = d["dep_kinds"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|k| k["kind"].is_null());
            if normal {
                todo.push(text(&d["pkg"]));
            }
        }
    }
    Ok(seen)
}

/// Whether `name` is a licence file's name (`LICENSE*`, `LICENCE*`, `COPYING*`, `COPYRIGHT*`,
/// `NOTICE*`, `UNLICENSE*`).
pub fn is_licence_file(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        "license",
        "licence",
        "copying",
        "copyright",
        "notice",
        "unlicense",
    ]
    .iter()
    .any(|p| n.starts_with(p))
}

/// The licence files in a crate's folder (and its `LICENSES/` folder), sorted.
fn licence_files(dir: &Path, declared: Option<&str>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut add = |rel: String, path: PathBuf| {
        if out.iter().any(|(r, _)| *r == rel) {
            return;
        }
        if let Ok(t) = std::fs::read_to_string(&path) {
            out.push((rel, t.replace("\r\n", "\n")));
        }
    };
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_file() && is_licence_file(&name) {
                add(name, e.path());
            }
        }
    }
    if let Ok(rd) = std::fs::read_dir(dir.join("LICENSES")) {
        for e in rd.flatten() {
            if e.path().is_file() {
                add(
                    format!("LICENSES/{}", e.file_name().to_string_lossy()),
                    e.path(),
                );
            }
        }
    }
    if let Some(f) = declared {
        add(f.replace('\\', "/"), dir.join(f));
    }
    out.sort();
    out
}

/// The crates of `ids` in `meta`.
pub fn crates_of(meta: &Value, ids: &BTreeSet<String>) -> Vec<Crate> {
    let workspace = workspace_ids(meta);
    let mut out: Vec<Crate> = meta["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| ids.contains(&text(&p["id"])))
        .map(|p| {
            let own = workspace.contains(&text(&p["id"]));
            let dir = Path::new(p["manifest_path"].as_str().unwrap_or("."))
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            Crate {
                name: text(&p["name"]),
                version: text(&p["version"]),
                licence: p["license"]
                    .as_str()
                    .map_or_else(|| "unknown".to_owned(), str::to_owned),
                authors: p["authors"]
                    .as_array()
                    .map(|a| a.iter().map(text).collect())
                    .unwrap_or_default(),
                own,
                files: if own {
                    Vec::new()
                } else {
                    licence_files(&dir, p["license_file"].as_str())
                },
            }
        })
        .collect();
    out.sort();
    out
}

/// Every crate compiled into `program` built for `target`.
pub fn shipped(root: &Path, program: &str, target: &str) -> Result<Vec<Crate>, String> {
    let meta = metadata(root, target)?;
    Ok(crates_of(&meta, &reached(&meta, program)?))
}

fn rule(title: &str) -> String {
    format!("\n{}\n{title}\n{}\n", "=".repeat(78), "=".repeat(78))
}

/// The file's text.
pub fn render(version: &str, target: &str, crates: &[Crate], mit: &str, apache: &str) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "Third-party licences of Form1099Keep {version} ({target})\n\n"
    ));
    s.push_str(
        "This file lists every crate compiled into Form1099Keep, with its version and licence,\n\
         followed by the licence texts those crates ship, with their copyright lines. Where\n\
         a crate offers a choice of licences, Form1099Keep uses it under any one of them. Slint\n\
         is used under the Slint Royalty-free License 2.0 (see NOTICE). Form1099Keep's own code\n\
         is under MIT OR Apache-2.0 (LICENSE-MIT, LICENSE-APACHE), as are the MoneyKeep\n\
         crates it builds on (mk-*). Made by `cargo xtask\n\
         release` from `cargo metadata` and the crates' own licence files.\n",
    );
    s.push_str(&rule("Crates"));
    for c in crates {
        let own = if c.own { "  (Form1099Keep's own)" } else { "" };
        s.push_str(&format!("{} {}: {}{own}\n", c.name, c.version, c.licence));
    }
    // Each distinct text once, with the crates that carry it.
    let mut texts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut bare: Vec<&Crate> = Vec::new();
    for c in crates.iter().filter(|c| !c.own) {
        if c.files.is_empty() {
            bare.push(c);
        }
        for (file, t) in &c.files {
            let key = t.trim().to_owned();
            if !texts.contains_key(&key) {
                order.push(key.clone());
            }
            texts
                .entry(key)
                .or_default()
                .push(format!("{} {} ({file})", c.name, c.version));
        }
    }
    s.push_str(&rule("Licence texts shipped by the crates"));
    for key in &order {
        let users = texts.get(key).cloned().unwrap_or_default();
        s.push_str(&format!("\n--- {}\n\n{key}\n", users.join(", ")));
    }
    if !bare.is_empty() {
        s.push_str(&rule("Crates that ship no licence file"));
        s.push_str(
            "These crates name their licence in their manifest but carry no licence\n\
             file. The copyright holders are the crates' authors, listed here. The MIT\n\
             and Apache-2.0 texts follow; for any other licence, its standard text is\n\
             at https://spdx.org/licenses/.\n\n",
        );
        for c in &bare {
            let authors = if c.authors.is_empty() {
                "the crate's authors".to_owned()
            } else {
                c.authors.join(", ")
            };
            s.push_str(&format!(
                "{} {}: {}; copyright {authors}\n",
                c.name, c.version, c.licence
            ));
        }
        s.push_str(&rule("MIT License (for the crates above under MIT)"));
        s.push_str(&format!(
            "\nCopyright (c) the authors named above for each crate\n\n{}\n",
            mit.trim()
        ));
        s.push_str(&rule(
            "Apache License 2.0 (for the crates above under Apache-2.0)",
        ));
        s.push_str(&format!("\n{}\n", apache.trim()));
    }
    s
}

/// The MIT text without this repository's own copyright lines (its first lines up to the
/// permission notice).
pub fn mit_body(licence_mit: &str) -> String {
    let t = licence_mit.replace("\r\n", "\n");
    match t.find("Permission is hereby granted") {
        Some(i) => {
            let title = t
                .lines()
                .next()
                .filter(|l| l.contains("MIT"))
                .unwrap_or("MIT License");
            format!("{title}\n\n{}", t.get(i..).unwrap_or_default())
        }
        None => t,
    }
}

/// Write `THIRD-PARTY-LICENSES.txt` for this computer's platform into `folder`; returns
/// its path and how many crates it lists.
pub fn write(root: &Path, folder: &Path, version: &str) -> Result<(PathBuf, usize), String> {
    let target = host_triple()?;
    let crates = shipped(root, PROGRAM, &target)?;
    let read = |f: &str| {
        std::fs::read_to_string(root.join(f)).map_err(|e| format!("cannot read {f}: {e}"))
    };
    let body = render(
        version,
        &target,
        &crates,
        &mit_body(&read("LICENSE-MIT")?),
        &read("LICENSE-APACHE")?,
    );
    std::fs::create_dir_all(folder)
        .map_err(|e| format!("cannot create {}: {e}", folder.display()))?;
    let path = folder.join(FILE);
    // Windows line ends: the file is read on Windows (Notepad).
    std::fs::write(&path, body.replace('\n', "\r\n"))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok((path, crates.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn licence_file_names() {
        for n in [
            "LICENSE",
            "LICENSE-MIT",
            "license.txt",
            "LICENCE",
            "COPYING",
            "NOTICE",
        ] {
            assert!(is_licence_file(n), "{n}");
        }
        for n in ["README.md", "Cargo.toml", "src"] {
            assert!(!is_licence_file(n), "{n}");
        }
    }

    #[test]
    fn mit_body_drops_the_copyright_line() {
        let b =
            mit_body("MIT License\n\nCopyright (c) 2026 X\n\nPermission is hereby granted, free\n");
        assert!(b.starts_with("MIT License\n\nPermission is hereby granted"));
        assert!(!b.contains("2026 X"));
    }

    #[test]
    fn render_lists_crates_and_each_text_once() {
        let c = |name: &str, files: Vec<(String, String)>| Crate {
            name: name.into(),
            version: "1.0.0".into(),
            licence: "MIT".into(),
            authors: vec!["A <a@example.org>".into()],
            own: false,
            files,
        };
        let same = (
            "LICENSE".to_owned(),
            "Copyright (c) A\n\nPermission".to_owned(),
        );
        let text = render(
            "0.1.0",
            "x86_64-unknown-linux-gnu",
            &[
                c("a", vec![same.clone()]),
                c("b", vec![same]),
                c("c", Vec::new()),
            ],
            "MIT License\n\nPermission",
            "Apache License",
        );
        assert!(text.contains("a 1.0.0: MIT\nb 1.0.0: MIT\nc 1.0.0: MIT\n"));
        assert_eq!(text.matches("Copyright (c) A\n\nPermission").count(), 1);
        assert!(text.contains("--- a 1.0.0 (LICENSE), b 1.0.0 (LICENSE)"));
        assert!(text.contains("c 1.0.0: MIT; copyright A <a@example.org>"));
        assert!(text.contains("Apache License"));
    }

    /// security-review-1 M8 (SR-B4 for Form1099Keep): the file lists every crate `cargo
    /// metadata` gives for the program on this platform, with its licence, and licence texts.
    #[test]
    fn m8_lists_every_crate_of_the_program() {
        let root = crate::workspace_root();
        let dir =
            std::env::temp_dir().join(format!("form1099keep-licences-{}", std::process::id()));
        let (path, n) = write(&root, &dir, "0.0.0").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.replace("\r\n", "").contains('\n'),
            "Windows line ends"
        );
        let text = text.replace("\r\n", "\n");
        let target = host_triple().unwrap();
        let meta = metadata(&root, &target).unwrap();
        let expected = crates_of(&meta, &reached(&meta, PROGRAM).unwrap());
        assert_eq!(n, expected.len());
        assert!(n > 50, "{n}");
        for c in &expected {
            let line = format!("\n{} {}: {}", c.name, c.version, c.licence);
            assert!(text.contains(&line), "{line:?} is listed");
        }
        for name in ["slint", "serde", "mk-store", "rusqlite"] {
            assert!(expected.iter().any(|c| c.name == name), "{name}");
        }
        assert!(text.contains("Licence texts shipped by the crates"));
        assert!(text.contains("Permission is hereby granted"));
        assert!(text.contains("Apache License"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
