# Form1099Keep

**Forms 1099-NEC and 1099-MISC — a MoneyKeep app**

An independent Form 1099 creator for small businesses, churches and bookkeepers, in
pure Rust with a Slint desktop interface. A 1099 book is an ordinary MoneyKeep book
(`.moneykeep`, the same file format, password protection and audit log) that holds
only payers, recipients and Forms 1099.

- **Password protected.** New 1099 books are always encrypted (they hold taxpayer
  id numbers), with a recovery key shown once. TINs are masked on screen and never
  written to the history.
- **Tax year 2026 forms.** The December 2026 revisions: NEC boxes 1a–1d, 2, 3, 4
  and MISC boxes 1–15 with the renumbering, the FATCA check and two state lines.
  Thresholds: $2,000 for NEC and most MISC boxes, $10 royalties and substitute
  payments, $600 attorney gross proceeds and fish purchases; withholding always
  requires a form.
- **Checks as questions.** Missing TINs, addresses, a payer phone number, tips
  without an occupation code, duplicate forms without account numbers, forms below
  the threshold, and the 10-return e-file rule.
- **Import from MoneyKeep.** Vendors marked for 1099s in a MoneyKeep book and what
  was paid them (card payments excluded), through MoneyKeep's own `report.1099`.
  Re-running updates amounts; filed forms are never changed.
- **Recipient copies.** One PDF, a page per form and copy (B, 1, 2, C), laid out
  under IRS Publication 1179 for substitute statements, recipient TIN truncated by
  default. Copy A is not printed (it must be the red scannable form): e-file instead.
- **IRIS e-file.** CSV files for the IRS's free IRIS Taxpayer Portal, ≤ 100 forms
  each. Choose the template you downloaded from the portal and the columns are
  filled by meaning; without it a draft column list is used, with a warning.
- **Filed and corrected.** Mark forms filed with the IRIS receipt id; filed forms
  are locked until reopened with Correct.

Form1099Keep prepares forms; it does not give tax advice or transmit to the IRS.

Version 0.1.0: [release notes](docs/release-notes/0.1.0.md) · [known limits](docs/known-limits.md).

## Build

Form1099Keep builds against MoneyKeep in the sibling folder:

```
moneykeep/        MoneyKeep (needs migration v15: the f1099_* tables)
form1099keep/     this app
```

```sh
git clone https://github.com/moneykeepproject/moneykeep
git clone https://github.com/moneykeepproject/form1099keep
cd form1099keep
```

```sh
cargo run --release                         # the desktop app
cargo run --release --bin form1099keep-cli -- --help
cargo test
cargo clippy --all-targets
```

On Linux the app needs a desktop session; file dialogs use the XDG portal.

## Windows installer (NSIS)

Same setup as CalcKeep, GroupKeep and MoneyKeep: the script is `installer/form1099keep.nsi`,
built by an xtask (which, like the app, needs the sibling `../moneykeep` checkout).

```powershell
winget install NSIS.NSIS        # once (or https://nsis.sourceforge.io/Download); then open a new terminal
cargo xtask installer           # -> dist\form1099keep-<version>-windows-x64-setup.exe and dist\SHA256SUMS.txt
```

- `cargo xtask release` builds the release programs and gathers `dist\Form1099Keep-<version>\`
  (form1099keep.exe, form1099keep-cli.exe, form1099keep.ico, LICENSE-MIT, LICENSE-APACHE,
  NOTICE, README.md). `--dry-run` lists the steps only.
- `cargo xtask installer` runs `release`, then `makensis`, then writes `SHA256SUMS.txt`
  (`<sha256>  <name>`) beside the setup. `--skip-release` reuses an existing release folder.
- The version comes from `Cargo.toml` (`[package] version`); Windows' four-number version is
  derived from it (`0.1.0-alpha.1` → `0.1.0.1`).
- **Release files:** publish `form1099keep-<version>-windows-x64-setup.exe` and
  `SHA256SUMS.txt` on the GitHub release. These are the names MoneyKeep Launcher looks for:
  it checks the setup against `SHA256SUMS.txt` and installs, updates and removes
  Form1099Keep with it.
- **The setup:** installs for all users in `C:\Program Files\Form1099Keep`, so it asks for
  administrator permission. It shows the licence (NOTICE, MIT and Apache-2.0), has a Start
  menu shortcut (on) and a Desktop shortcut (off), registers in Installed apps (publisher
  MoneyKeep Project), updates in place, and asks you to close Form1099Keep if it's open. The
  uninstaller asks before deleting your settings (the recent books list,
  `%APPDATA%\Form1099Keep\config\settings.json`); your books are never touched.
- **Icon:** `assets/icon.ico` is made from `assets/icon.svg` (16 to 256 px); `build.rs`
  embeds it in form1099keep.exe on Windows, and the setup uses it too. Make it again when
  the drawing changes.
- **Silent mode** (what MoneyKeep Launcher uses): `form1099keep-<version>-windows-x64-setup.exe /S`
  installs or updates with the default options and no questions (`/D=<folder>` picks the
  folder of a new install). If Form1099Keep is open it is asked to close, and after 30 s the
  setup stops with exit code 2; 0 means done, 3 any other failure. Windows still asks for
  administrator permission. The silent uninstall
  (`"C:\Program Files\Form1099Keep\uninstall.exe" /S`, the `QuietUninstallString`) never
  deletes your settings.
- **Signing:** set `FK_SIGN_CERT` (path to a `.pfx`) and `FK_SIGN_PASSWORD` to sign the
  programs and the setup with `signtool`; `FK_SIGN_TIMESTAMP` overrides the timestamp server.
  Without them the setup is unsigned and SmartScreen will warn.
- `cargo test -p xtask` checks the script's silent mode and compiles it with placeholder
  programs when NSIS is installed.

## Command line

```sh
form1099keep-cli init church-1099s.moneykeep --name "Grace Chapel 1099s"
form1099keep-cli payer add church-1099s.moneykeep --name "Grace Chapel" --tin 12-3456789 \
    --line1 "100 Church St" --city Akron --state OH --zip 44301 --phone 330-555-0100
form1099keep-cli import church-1099s.moneykeep --from 2026.moneykeep --year 2026 --payer "Grace Chapel"
form1099keep-cli check church-1099s.moneykeep --year 2026
form1099keep-cli print church-1099s.moneykeep --year 2026 --copies B --out copies.pdf
form1099keep-cli iris-csv church-1099s.moneykeep --year 2026 --kind nec \
    --template 1099-NEC-template.csv --out iris
form1099keep-cli mark-filed church-1099s.moneykeep --year 2026 --date 2027-01-20 --receipt <id>
```

The book password is read from `F1099KEEP_PASSWORD` or prompted; a protected
MoneyKeep book's from `F1099KEEP_MK_PASSWORD`.

## Layout

| Path | What |
|---|---|
| `src/model.rs`, `src/boxes.rs` | Payers, recipients, forms; the 2026 box layouts and thresholds |
| `src/book.rs` | The 1099 book over `mk-store` (writes, audit entries, password rule) |
| `src/check.rs`, `src/tin.rs` | Checks and TIN rules |
| `src/import.rs` | Import from a MoneyKeep book through `mk-commands` |
| `src/pdf.rs`, `src/iris.rs`, `src/jobs.rs` | Recipient copies, IRIS CSV, the year-end jobs |
| `src/main.rs`, `ui/` | The desktop app (Slint) |
| `src/bin/form1099keep-cli.rs` | The command line |
| `docs/specs/form1099keep.md` | The spec |

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option, as MoneyKeep.

The interface is built with [Slint](https://slint.dev), used under the Slint
Royalty-free Desktop, Mobile, and Web Applications License 2.0 (its text is in
`THIRD-PARTY-LICENSES.txt`, with the slint crates); this badge is its attribution
(licence §2(b)):

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)

Every third-party crate compiled into Form1099Keep is listed with its licence in
`THIRD-PARTY-LICENSES.txt`, which `cargo xtask release` writes and the installer
installs beside the programs.
