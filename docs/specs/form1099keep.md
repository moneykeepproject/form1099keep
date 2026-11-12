# Spec: Form1099Keep — an independent Form 1099 creator (revision 1)

- **Area / app:** `form1099keep` (separate program in the MoneyKeep family)
- **Spec writer:** Claude session 2026-11-04 (spec-writer role; IRS publications and
  MoneyKeep's own specs only; no target product or prior-art source read)
- **Date:** 2026-11-04
- **Status:** draft

## Sources

| # | Type | Reference | Notes |
|---|---|---|---|
| 1 | Government | IRS, *Instructions for Forms 1099-MISC and 1099-NEC* (Rev. December 2026), irs.gov/pub/irs-pdf/i1099mec.pdf | Box maps, thresholds, due dates, account number and 2nd TIN notice rules |
| 2 | Government | IRS, Form 1099-NEC (Rev. December 2026), irs.gov/pub/irs-pdf/f1099nec.pdf | Copy B face, copy labels |
| 3 | Government | IRS Publication 1179 (Rev. 07-2026), *General Rules and Specifications for Substitute Forms 1096, 1098, 1099, 5498…* | Part 4: substitute recipient statements; §2.2.1 TIN truncation; §2.1.5 and §5.3 Copy A red ink |
| 4 | Government | IRS, *E-file information returns with IRIS*, irs.gov/iris; IRIS Working Group minutes 2025–2026; Publication 5717 | Taxpayer Portal CSV upload: templates per form, 100 rows per file, headers must match exactly, Y/N check boxes |
| 5 | Government | IRS Publication 5718 (Processing Year 2026), IRIS A2A specifications | A2A needs an IRIS TCC, an API client id, ATS testing and the schema package from the Secure Object Repository |
| 6 | Spec | MoneyKeep `book-encryption.md`, `business-purchases.md` (1099 tracking), `branding.md`, `ui-slint.md` | Book format, encryption, `report.1099` |

Record-keeping and form preparation only. Form1099Keep does not give tax advice and
does not transmit anything to the IRS.

## Purpose

A small business, church or bookkeeper makes its year-end Forms 1099-NEC and
1099-MISC without an accounting system: enter payers and recipients (or pull them
from a MoneyKeep book), check the forms, print recipient copies and produce the file
to upload to the IRS's free IRIS Taxpayer Portal.

## Behaviour

### The 1099 book

- A **1099 book** is an ordinary MoneyKeep book file (`mk-store`, SQLite image,
  schema v15 or later) holding only 1099 data. Default extension `.moneykeep`, so
  MoneyKeep can open it; Form1099Keep marks it with book setting
  `form1099keep / book_kind = "form1099"`.
- **Opening never changes the book's format** (MoneyKeep security-review-1 M6). A book
  at an older format is refused with "Open this book in MoneyKeep first, so it can
  update it safely."; a newer one with "This book was made by a newer MoneyKeep.
  Update Form1099Keep." Opening writes nothing: for an unmarked MoneyKeep book the app
  asks "Open as a 1099 book?" and writes the marker only after the user confirms, and
  only to a book at the format it understands.
- **Password required.** Form1099Keep creates only encrypted books (MoneyKeep
  `book-encryption.md`), with a recovery key shown once. Opening a plain book is
  allowed (for example one made by MoneyKeep), but every write that would store a
  TIN fails with `tin_requires_password` until the book is encrypted; the app offers
  **Protect with a password**. There is no "remove password" in Form1099Keep.
- Data lives in three document tables added by MoneyKeep migration v15:
  `f1099_payer`, `f1099_recipient`, `f1099_form` (grp = tax year).
- Every change writes one audit entry (actor `form1099keep`, command
  `f1099.<entity>.<verb>`). Audit entries never contain a TIN: arguments and results
  are written with TINs masked (`***-**-1234`, `**-***1234`), and no before/after
  snapshots are written, so MoneyKeep's History shows them as not undoable.

### Payers

| Field | Rules |
|---|---|
| `name`, `name2` | Business or entity name, ≤ 75 each; `name` required |
| `tin_type` | `ein` (default) or `ssn` (sole proprietor) |
| `tin` | 9 digits, stored without punctuation; see TIN rules |
| `address` | `line1` (required), `line2`, `city` (required), `state` (2 letters, required in the US), `zip` (5 or 9 digits, required in the US), `country` (default `US`) |
| `phone` | Required: Pub 1179 §4.1.3 needs a direct telephone number on 1099-NEC and 1099-MISC recipient copies |
| `email`, `contact` | Optional |

### Recipients

| Field | Rules |
|---|---|
| `name_type` | `business` or `person` |
| `business_name`, `business_name2` | For `business` |
| `first`, `middle`, `last`, `suffix` | For `person`; `first` and `last` required |
| `tin_type` | `ein`, `ssn`, `itin`, `atin`; may be empty until the W-9 arrives |
| `tin` | 9 digits |
| `address` | As payer; `line1`, `city` required; state and ZIP required in the US |
| `email`, `account_number` | Optional; account number ≤ 20 |
| `source` | Set by import: `{book_id, vendor_id, vendor_name}` |

### TIN rules

Digits only after removing spaces and `-`; exactly 9. EIN: not `00…`, first two
digits not an unassigned prefix (`07`, `08`, `09`, `17`, `18`, `19`, `28`, `29`, `49`, `69`,
`70`, `78`, `79`, `89`, `96`, `97` — a warning, since the IRS may assign new prefixes).
SSN: area not `000`, `666` or `9xx`; group not `00`; serial not `0000`. ITIN: starts
with `9`, fourth and fifth digits in 50–65, 70–88, 90–92 or 94–99. ATIN: starts
with `9`, fourth and fifth digits `93`. Shown masked everywhere except the edit field
of the record being edited. Recipient copies truncate the recipient TIN to its last
four digits by default (Pub 1179 §2.2.1); the payer TIN is never truncated.

### Forms (tax year 2026 revision)

One record per form: `payer`, `recipient`, `tax_year` (2026 or later; later years use
the 2026 layout with a note to check for a newer revision), `kind` (`nec`|`misc`),
`account_number`, `second_tin_notice`, `corrected`, `status`
(`draft`|`ready`|`filed`), `filed` {date, receipt_id}, `notes`, `boxes`, `states`
(≤ 2 rows: `state`, `payer_state_no`, `tax_withheld`, `income`).

**1099-NEC boxes:** 1a Nonemployee compensation; 1b Cash tips; 1c TTOC (up to two
3-digit codes); 1d Overtime compensation; 2 Direct sales of $5,000 or more (check);
3 Excess golden parachute payments; 4 Federal income tax withheld; 5–7 state.

**1099-MISC boxes:** 1 Rents; 2 Royalties; 3 Other income; 4 Federal income tax
withheld; 5 Fishing boat proceeds; 6 Medical and health care payments; 7 Direct
sales of $5,000 or more (check); 8 Substitute payments in lieu of dividends or
interest; 9 Crop insurance proceeds; 10 Gross proceeds paid to an attorney; 11 Fish
purchased for resale; 12 Section 409A deferrals; 13a Cash tips; 13b TTOC; 14 Overtime
compensation; 15 Nonqualified deferred compensation; FATCA filing requirement
(check, unnumbered); 16–18 state.

Amounts are exact decimals (`mk_core::Decimal`), two places, ≥ 0.

**Thresholds** (book setting `form1099keep / thresholds` may override per year):
NEC 1a $2,000; MISC 1, 3, 6, 9, 12, 15 $2,000; MISC 2 and 8 $10; MISC 10 and 11
$600; direct sales check $5,000. Any federal withholding (NEC 4, MISC 4) makes the
form required regardless of amount. From 2027 the $2,000 amounts are inflation
indexed: later years copy 2026 and show "check this year's threshold".

### Checks (validation)

Each finding has a severity (`error` blocks marking a form ready, exporting it to
IRIS or printing it; `warning` does not), the field and a plain question.

- Payer: name, TIN valid, address complete, phone present.
- Recipient: name, TIN present and valid, address complete; TIN type matches
  `name_type` (an EIN for a person is a warning, an SSN for a business is a warning).
- Form: at least one non-zero amount or check; 1b ≤ 1a and 1d ≤ 1a (NEC, error);
  1b needs 1c and MISC 13a needs 13b (error); TTOC codes are 3 digits; FATCA needs an account
  number; two forms of the same kind for the same payer, recipient and year need
  distinct account numbers; below every threshold with no withholding → warning
  "not required — file anyway?"; state rows need a 2-letter state.
- Book: 10 or more forms for one payer and year → note "must be e-filed" (Source 1).
- Filed forms are read-only; changing one needs **Correct**, which clears `filed`,
  sets `corrected` and keeps the old receipt id in `notes`.

### Import from MoneyKeep

`f1099.import_moneykeep {path, password?, year, payer, include_below_threshold
(false)}` opens the MoneyKeep book through `mk-commands` (`Session`) — it changes
no data there, though opening applies any pending MoneyKeep migrations exactly as
MoneyKeep itself would — calls `report.1099 {year, include_below_threshold: true}`
and `party.get` for each vendor, and then, in one write to the 1099 book:

- Matches a recipient by `source.book_id + source.vendor_id`, else creates one from
  the vendor (company or name → business name; billing address: last line parsed as
  `City, ST 12345[-6789]`, earlier lines → line 1 and 2; email). TIN is left empty
  (MoneyKeep never stores TINs) and shows as a check to answer. Imported recipients
  start as businesses; a note says how many look like people.
- Box: NEC → 1a; MISC `rents`→1, `royalties`→2, `other_income`→3, `medical`→6,
  `attorney_gross_proceeds`→10.
- Creates the form or updates the amount of a draft/ready form from the same
  source; a filed form is left alone and listed as "already filed".
- Vendors below the threshold are skipped unless asked (their count is reported).
- Result: created/updated/skipped lists, and the thresholds MoneyKeep used, so a
  difference (MoneyKeep's 2026 default for MISC box 10 is $2,000; the IRS amount is
  $600) is visible.

### Recipient copies (PDF)

`f1099.print {forms, copies (default ["B"]), truncate_tin (true), path}` writes one
PDF, letter size, one form per page: the form face in the top half and
"Instructions for Recipient" in the bottom half. Copies: `B` "For Recipient", `1`
"For State Tax Department", `2` "To be filed with recipient's state income tax
return, when required", `C` "For Payer". Per Pub 1179 Part 4: year, form number and
title together at the upper right; box numbers and captions as on the official
form; "Federal income tax withheld" in bold; OMB No. 1545-0116 (NEC) or 1545-0115
(MISC); the Copy B legend ("This is important tax information and is being
furnished to the IRS…"); payer telephone; CORRECTED when set. Black text only; no
Copy A (it must be the red-ink scannable form, Pub 1179 §2.1.5 and §5.3). Errors
block printing that form; the result lists the forms printed and skipped.

### IRIS Taxpayer Portal CSV

The IRS accepts only its own current template, headers spelled exactly
(Source 4), and the tax-year 2026 templates are published inside the portal.
Therefore:

- `f1099.iris_csv {year, kind, payer, template?, folder}`: if `template` (a CSV file
  downloaded from the portal) is given, its first row is the header list and is
  written back unchanged; otherwise the built-in tax-year 2026 draft header list is
  used and the result warns to check it against the portal's template.
- Each header is mapped by meaning (normalised: case, spaces and punctuation
  ignored): form type, tax year, payer and recipient TIN type, TIN, name type, name
  lines, person name parts, country, address lines, city, state, ZIP, phone, email,
  account number, 2nd TIN notice, FATCA, corrected, and each box by its number
  (`box 1a`, `box 13b`…) or caption. Unknown headers stay blank and are listed in
  the result.
- Values: form type `1099-NEC`/`1099-MISC`; TIN types `EIN`, `SSN`, `ITIN`, `ATIN`;
  name type `B` (business) or `P` (person); checks `Y`/`N`; amounts `1234.56`
  without `$` or commas; TINs 9 digits, untruncated; text with commas quoted.
- Only forms without errors are written; at most 100 rows per file
  (`1099-NEC_2026_<payer>_01.csv`, `_02`…). Result: files, rows, skipped forms and
  unmapped headers.
- A2A XML is out of scope for revision 1: it needs an IRIS TCC with a software
  developer role, ATS testing and the non-public schema package (Source 5).

### Front ends

- **Desktop app** (`form1099keep`, Slint 1.18, Fluent style, MoneyKeep colours):
  start screen (new 1099 book, open, recent books); then a rail in order of use:
  **Payers**, **Import** (from a MoneyKeep book), **Recipients**, **Forms** (year
  picker, list with recipient, form, total, status and check count; editor), **Print and
  e-file** (recipient copies, IRIS file, mark filed), **History** (audit entries). A
  book opens on Payers until it has one, then on Forms. Settings are per user in the platform
  config folder `Form1099Keep` (recent books, theme), never inside a book.
- **CLI** (`form1099keep-cli`): `init`, `payer add|list`, `recipient add|list`,
  `form add|list|check`, `import`, `print`, `iris-csv`, `history`; passwords from
  the prompt or `F1099KEEP_PASSWORD`.
- Both call the same library functions; neither touches SQLite directly.

## Out of scope (revision 1)

Other 1099 forms (INT, DIV, K…), Form 1096, IRIS A2A transmission, TIN Matching,
state filing, e-mail delivery of recipient copies, undo.
