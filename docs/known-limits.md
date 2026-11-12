# Known limits

What Form1099Keep 0.1.0 doesn't do yet, or doesn't do on purpose. This list is updated
with each release. For what it does do, see the [release notes](release-notes/0.1.0.md).

## Forms and filing

- **Only Forms 1099-NEC and 1099-MISC.** Other 1099 forms (such as 1099-INT, 1099-DIV
  or 1099-K) and Form 1096 are not covered.
- **Form1099Keep doesn't send anything to the IRS.** It makes CSV files that you upload
  to the IRS's IRIS Taxpayer Portal yourself. Sending directly from the app (IRIS
  "A2A" transmission) is not included.
- **No Copy A on paper.** Copy A has to be the IRS's red, scannable form, so
  Form1099Keep doesn't print it: e-file instead.
- **The IRIS files are only as current as the template you choose.** Without the
  template downloaded from the IRIS portal, a built-in draft list of columns is used,
  and Form1099Keep warns you to check it against the portal's template.
- **No ID number checking with the IRS** (TIN Matching), no state filing, and no
  e-mailing of recipient copies.
- **Tax year 2026 layout.** Later years use the 2026 layout and thresholds, with a note
  to check for a newer form and this year's threshold.
- **No undo.** Changes are recorded in the history, but they can't be undone from
  MoneyKeep's History.
- **Not tax advice.** Form1099Keep prepares forms; check anything you're unsure about
  with a tax professional.

## Importing from MoneyKeep

- **ID numbers are not imported.** MoneyKeep doesn't store them, so each recipient's
  number is left empty and shows up as a question to answer.
- **Imported recipients start as businesses;** a note says how many look like people,
  so you can change them.
- **Attorney gross proceeds:** MoneyKeep's 2026 threshold for 1099-MISC box 10 is
  $2,000, while the IRS amount is $600. The import shows the thresholds MoneyKeep used
  so you can spot the difference.

## Books

- **Books made by an older MoneyKeep** must be opened in MoneyKeep first, so it can
  update them safely. Form1099Keep never changes a book's format.
- **ID numbers need a password-protected book.** A plain book opens, but saving an ID
  number fails until you choose **Protect with a password**. There is no way to remove
  the password in Form1099Keep.
- **A forgotten password can't be recovered.** Without the password and without the
  recovery key, nobody can open the book. Store the recovery key somewhere safe when it
  is shown.
- **Early software:** keep backups of your 1099 books.

## Installing

- **The Windows setup is not signed yet.** Windows SmartScreen may warn: choose
  **More info**, then **Run anyway**.
- **The Mac build is not signed with an Apple Developer ID yet** (it is signed "ad
  hoc"). macOS warns the first time you open it: right-click Form1099Keep, then choose
  **Open** (or System Settings > Privacy & Security > **Open Anyway**).
- **MoneyKeep Launcher can't install Form1099Keep on a Mac yet.** It installs only Mac
  builds signed with the project's Developer ID. Until then, install from the
  release's `.dmg`; the launcher opens the copy you installed and links to new
  releases.
- **Release files are for Windows (64-bit) and Mac only.** Form1099Keep also runs on
  Linux, but there is no Linux download yet: build it from the source.
