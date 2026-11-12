//! Files for the IRS IRIS Taxpayer Portal's "Upload CSV with Form Data"
//! (form1099keep.md "IRIS Taxpayer Portal CSV").
//!
//! The IRS accepts only its own current template with the headers spelled exactly,
//! and the tax-year 2026 templates are published inside the portal. So the header
//! row comes from the template the user downloaded; every header is mapped by
//! meaning, and headers this build does not recognise are left blank and reported.
//! Without a template the built-in draft header list is used, with a warning.

use std::path::{Path, PathBuf};

use mk_core::Decimal;

use crate::boxes::{self, BoxKind};
use crate::model::{Address, Form, FormKind, NameType, Payer, Recipient, TinType};
use crate::{Error, Result};

/// The portal takes at most 100 rows per template file.
pub const MAX_ROWS: usize = 100;

/// Built-in header lists (tax year 2026 draft): the party columns follow the
/// portal's templates of earlier years; the box columns use the 2026 box numbers.
/// Always check against the template from the portal.
pub fn builtin_headers(kind: FormKind) -> Vec<String> {
    let mut h: Vec<String> = [
        "Form Type",
        "Tax Year",
        "Payer TIN Type",
        "Payer Taxpayer ID Number",
        "Payer Name Type",
        "Payer Business or Entity Name Line 1",
        "Payer Business or Entity Name Line 2",
        "Payer First Name",
        "Payer Middle Name",
        "Payer Last Name (Surname)",
        "Payer Suffix",
        "Payer Country",
        "Payer Address Line 1",
        "Payer Address Line 2",
        "Payer City/Town",
        "Payer State/Province/Territory",
        "Payer ZIP/Postal Code",
        "Payer Phone Type",
        "Payer Phone",
        "Payer Email Address",
        "Recipient TIN Type",
        "Recipient Taxpayer ID Number",
        "Recipient Name Type",
        "Recipient Business or Entity Name Line 1",
        "Recipient Business or Entity Name Line 2",
        "Recipient First Name",
        "Recipient Middle Name",
        "Recipient Last Name (Surname)",
        "Recipient Suffix",
        "Recipient Country",
        "Recipient Address Line 1",
        "Recipient Address Line 2",
        "Recipient City/Town",
        "Recipient State/Province/Territory",
        "Recipient ZIP/Postal Code",
        "Recipient Email Address",
        "Account Number",
        "Second TIN Notice",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    for b in boxes::layout(kind) {
        if b.key == "fatca" {
            h.push("FATCA Filing Requirement".into());
        } else {
            h.push(format!("Box {} - {}", b.key, b.caption));
        }
    }
    let (w, no, inc) = boxes::state_boxes(kind);
    for i in 1..=2 {
        h.push(format!("State {i}"));
        h.push(format!("Box {w} - State {i} Tax Withheld"));
        h.push(format!("Box {no} - State {i} Payer State Number"));
        h.push(format!("Box {inc} - State {i} Income"));
    }
    h
}

/// The header row of a template file (first CSV record), and whether it began
/// with a UTF-8 byte order mark.
pub fn read_template(path: &Path) -> Result<(Vec<String>, bool)> {
    let bytes = std::fs::read(path).map_err(|e| Error::io("read", path, &e))?;
    let (bom, body) = match bytes.strip_prefix(b"\xEF\xBB\xBF") {
        Some(rest) => (true, rest),
        None => (false, bytes.as_slice()),
    };
    let text = String::from_utf8_lossy(body);
    let headers = parse_first_record(&text);
    if headers.iter().all(|h| h.trim().is_empty()) {
        return Err(Error::invalid(format!(
            "{} has no header row; download the template again from the IRIS portal",
            path.display()
        )));
    }
    Ok((headers, bom))
}

fn parse_first_record(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            (true, '"') => quoted = false,
            (true, _) => cur.push(c),
            (false, '"') => quoted = true,
            (false, ',') => out.push(std::mem::take(&mut cur)),
            (false, '\r' | '\n') => break,
            (false, _) => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// One form ready for the file.
#[derive(Debug, Clone)]
pub struct Row<'a> {
    pub form: &'a Form,
    pub payer: &'a Payer,
    pub recipient: &'a Recipient,
}

/// A file to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvFile {
    pub name: String,
    pub content: Vec<u8>,
    pub rows: usize,
}

/// What [`build`] made.
#[derive(Debug, Clone, Default)]
pub struct Built {
    pub files: Vec<CsvFile>,
    /// Headers left blank because their meaning is not known to this build.
    pub unmapped: Vec<String>,
    /// Headers this build fills with a value it could not confirm (check them).
    pub unconfirmed: Vec<String>,
}

/// Build the upload files for forms of one kind (split by payer, ≤ 100 rows each).
pub fn build(year: i32, kind: FormKind, rows: &[Row<'_>], headers: &[String], bom: bool) -> Built {
    let fields: Vec<Field> = headers.iter().map(|h| classify(h, kind)).collect();
    let mut built = Built {
        unmapped: headers
            .iter()
            .zip(&fields)
            .filter(|(h, f)| matches!(f, Field::Unknown) && !h.trim().is_empty())
            .map(|(h, _)| h.clone())
            .collect(),
        unconfirmed: headers
            .iter()
            .zip(&fields)
            .filter(|(_, f)| matches!(f, Field::Party(_, PartyField::PhoneType)))
            .map(|(h, _)| h.clone())
            .collect(),
        ..Built::default()
    };
    let mut payer_ids: Vec<&str> = rows.iter().map(|r| r.payer.id.as_str()).collect();
    payer_ids.sort_unstable();
    payer_ids.dedup();
    for pid in payer_ids {
        let mine: Vec<&Row<'_>> = rows
            .iter()
            .filter(|r| r.payer.id == pid && r.form.kind == kind && r.form.tax_year == year)
            .collect();
        let Some(first) = mine.first() else { continue };
        let slug = slug(&first.payer.name);
        for (i, chunk) in mine.chunks(MAX_ROWS).enumerate() {
            let mut out: Vec<u8> = if bom {
                b"\xEF\xBB\xBF".to_vec()
            } else {
                Vec::new()
            };
            write_record(&mut out, headers.iter().map(String::as_str));
            for r in chunk {
                let vals: Vec<String> = fields.iter().map(|f| value(f, r)).collect();
                write_record(&mut out, vals.iter().map(String::as_str));
            }
            built.files.push(CsvFile {
                name: format!(
                    "{}_{year}_{slug}_{:02}.csv",
                    kind.label(),
                    i.saturating_add(1)
                ),
                content: out,
                rows: chunk.len(),
            });
        }
    }
    built
}

/// Write the files into a folder; returns their paths. Existing files of the same
/// name are replaced.
pub fn write_files(folder: &Path, files: &[CsvFile]) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(folder).map_err(|e| Error::io("create", folder, &e))?;
    let mut paths = Vec::new();
    for f in files {
        let p = folder.join(&f.name);
        std::fs::write(&p, &f.content).map_err(|e| Error::io("write", &p, &e))?;
        paths.push(p);
    }
    Ok(paths)
}

// ----- header meanings -------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Party {
    Payer,
    Recipient,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartyField {
    TinType,
    Tin,
    NameType,
    Name1,
    Name2,
    First,
    Middle,
    Last,
    Suffix,
    Country,
    Line1,
    Line2,
    City,
    State,
    Zip,
    PhoneType,
    Phone,
    Email,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StateField {
    Code,
    Withheld,
    Number,
    Income,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Field {
    FormType,
    TaxYear,
    Party(Party, PartyField),
    AccountNumber,
    SecondTin,
    Corrected,
    Box(&'static str),
    State(usize, StateField),
    Unknown,
}

/// Lower case, punctuation as spaces, single spaces.
fn norm(h: &str) -> String {
    let s: String = h
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn classify(header: &str, kind: FormKind) -> Field {
    let h = norm(header);
    let has = |w: &str| h.contains(w);
    if h.is_empty() {
        return Field::Unknown;
    }
    if h == "form type" {
        return Field::FormType;
    }
    if h == "tax year" {
        return Field::TaxYear;
    }
    // Combined Federal/State Filing is not offered: left blank and reported.
    if has("combined") {
        return Field::Unknown;
    }
    // State lines: "state 1 tax withheld", "box 16 state 2 income"…
    if let Some(i) = state_index(&h) {
        let f = if has("withheld") {
            StateField::Withheld
        } else if has("income") {
            StateField::Income
        } else if has("number") || has(" id") || has(" no") {
            StateField::Number
        } else {
            StateField::Code
        };
        return Field::State(i, f);
    }
    for (prefix, party) in [("payer", Party::Payer), ("recipient", Party::Recipient)] {
        if let Some(rest) = h.strip_prefix(prefix) {
            let rest = rest.trim();
            let w = |x: &str| rest.contains(x);
            let pf = if w("tin type") {
                PartyField::TinType
            } else if w("taxpayer id") || rest == "tin" {
                PartyField::Tin
            } else if w("name type") {
                PartyField::NameType
            } else if w("first name") {
                PartyField::First
            } else if w("middle") {
                PartyField::Middle
            } else if w("last name") || w("surname") {
                PartyField::Last
            } else if w("suffix") {
                PartyField::Suffix
            } else if w("line 2") && (w("name") || w("entity") || w("business")) {
                PartyField::Name2
            } else if (w("line 1") && (w("name") || w("entity") || w("business")))
                || rest == "name"
                || rest == "business name"
            {
                PartyField::Name1
            } else if w("country") {
                PartyField::Country
            } else if w("address line 1") || rest == "address" || w("street") {
                PartyField::Line1
            } else if w("address line 2") {
                PartyField::Line2
            } else if w("city") || w("town") {
                PartyField::City
            } else if w("state") || w("province") || w("territory") {
                PartyField::State
            } else if w("zip") || w("postal") {
                PartyField::Zip
            } else if w("phone type") {
                PartyField::PhoneType
            } else if w("phone") || w("telephone") {
                PartyField::Phone
            } else if w("email") || w("e mail") {
                PartyField::Email
            } else {
                return Field::Unknown;
            };
            return Field::Party(party, pf);
        }
    }
    if has("account number") {
        return Field::AccountNumber;
    }
    if has("2nd tin") || has("second tin") {
        return Field::SecondTin;
    }
    if has("corrected") {
        return Field::Corrected;
    }
    if has("fatca") {
        return Field::Box("fatca");
    }
    // "box 1a nonemployee compensation", "nonemployee compensation box 1a"
    let words: Vec<&str> = h.split(' ').collect();
    for (i, w) in words.iter().enumerate() {
        if *w == "box"
            && let Some(next) = words.get(i.saturating_add(1))
            && let Some(b) = boxes::find(kind, next)
        {
            return Field::Box(b.key);
        }
    }
    if has("direct sales") {
        return Field::Box(match kind {
            FormKind::Nec => "2",
            FormKind::Misc => "7",
        });
    }
    // By caption alone.
    let mut best: Option<(&'static str, usize)> = None;
    for b in boxes::layout(kind) {
        let c = norm(b.caption);
        if !c.is_empty() && h.contains(&c) && best.is_none_or(|(_, n)| c.len() > n) {
            best = Some((b.key, c.len()));
        }
    }
    best.map_or(Field::Unknown, |(k, _)| Field::Box(k))
}

/// `state 1 …`, `… state 2 …` → 0 or 1; a plain "state" with no number is the
/// first line. Payer and recipient address columns are excluded.
fn state_index(h: &str) -> Option<usize> {
    if h.starts_with("payer") || h.starts_with("recipient") || !h.contains("state") {
        return None;
    }
    let words: Vec<&str> = h.split(' ').collect();
    for (i, w) in words.iter().enumerate() {
        if *w == "state" {
            return match words.get(i.saturating_add(1)) {
                Some(&"2") => Some(1),
                _ => Some(0),
            };
        }
    }
    None
}

fn value(f: &Field, r: &Row<'_>) -> String {
    let form = r.form;
    let yn = |b: bool| if b { "Y" } else { "N" }.to_owned();
    match f {
        Field::FormType => form.kind.label().to_owned(),
        Field::TaxYear => form.tax_year.to_string(),
        Field::AccountNumber => form.account_number.trim().to_owned(),
        Field::SecondTin => yn(form.second_tin_notice),
        Field::Corrected => yn(form.corrected),
        Field::Box(key) => match boxes::find(form.kind, key).map(|b| b.kind) {
            Some(BoxKind::Amount) => amount(form.amount(key)),
            Some(BoxKind::Check) => yn(form.checked(key)),
            Some(BoxKind::Codes) => form.ttoc.join(","),
            None => String::new(),
        },
        Field::State(i, sf) => form.states.get(*i).map_or_else(String::new, |s| match sf {
            StateField::Code => s.state.trim().to_ascii_uppercase(),
            StateField::Withheld => amount(s.tax_withheld),
            StateField::Number => s.payer_state_no.trim().to_owned(),
            StateField::Income => amount(s.income),
        }),
        Field::Party(Party::Payer, pf) => {
            let p = r.payer;
            party_value(
                *pf,
                Some(p.tin_type),
                &p.tin,
                NameType::Business,
                [&p.name, &p.name2, "", "", "", ""],
                &p.address,
                &p.phone,
                &p.email,
            )
        }
        Field::Party(Party::Recipient, pf) => {
            let x = r.recipient;
            let (n1, n2) = match x.name_type {
                NameType::Business => (x.business_name.as_str(), x.business_name2.as_str()),
                NameType::Person => ("", x.business_name2.as_str()),
            };
            let person = x.name_type == NameType::Person;
            party_value(
                *pf,
                x.tin_type,
                &x.tin,
                x.name_type,
                [
                    n1,
                    n2,
                    if person { &x.first } else { "" },
                    if person { &x.middle } else { "" },
                    if person { &x.last } else { "" },
                    if person { &x.suffix } else { "" },
                ],
                &x.address,
                "",
                &x.email,
            )
        }
        Field::Unknown => String::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn party_value(
    f: PartyField,
    tin_type: Option<TinType>,
    tin: &str,
    name_type: NameType,
    names: [&str; 6],
    a: &Address,
    phone: &str,
    email: &str,
) -> String {
    let [n1, n2, first, middle, last, suffix] = names;
    let t = |s: &str| clean(s.trim());
    match f {
        PartyField::TinType => tin_type.map(|t| t.code().to_owned()).unwrap_or_default(),
        PartyField::Tin => tin.to_owned(),
        PartyField::NameType => match name_type {
            NameType::Business => "B".into(),
            NameType::Person => "P".into(),
        },
        PartyField::Name1 => t(n1),
        PartyField::Name2 => t(n2),
        PartyField::First => t(first),
        PartyField::Middle => t(middle),
        PartyField::Last => t(last),
        PartyField::Suffix => t(suffix),
        PartyField::Country => {
            if a.is_us() {
                "US".into()
            } else {
                t(&a.country).to_ascii_uppercase()
            }
        }
        PartyField::Line1 => t(&a.line1),
        PartyField::Line2 => t(&a.line2),
        PartyField::City => t(&a.city),
        PartyField::State => t(&a.state).to_ascii_uppercase(),
        PartyField::Zip => {
            if a.is_us() {
                a.zip.chars().filter(char::is_ascii_digit).collect()
            } else {
                t(&a.zip)
            }
        }
        PartyField::PhoneType => {
            if phone.trim().is_empty() {
                String::new()
            } else {
                "Domestic".into()
            }
        }
        PartyField::Phone => phone.chars().filter(char::is_ascii_digit).collect(),
        PartyField::Email => t(email),
    }
}

/// `1234.56`; zero is blank.
fn amount(d: Decimal) -> String {
    if d.is_zero() {
        String::new()
    } else {
        format!("{:.2}", d.round_dp(2))
    }
}

/// Control and invisible characters removed (MoneyKeep input-safety R3 cleaning).
fn clean(s: &str) -> String {
    mk_core::text::clean(s, 200).text
}

fn write_record<'a>(out: &mut Vec<u8>, vals: impl Iterator<Item = &'a str>) {
    let mut first = true;
    for v in vals {
        if !first {
            out.push(b',');
        }
        first = false;
        if v.contains([',', '"', '\n', '\r']) {
            out.push(b'"');
            out.extend(v.replace('"', "\"\"").as_bytes());
            out.push(b'"');
        } else {
            out.extend(v.as_bytes());
        }
    }
    out.extend(b"\r\n");
}

fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let s = s
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let s: String = s.chars().take(30).collect();
    if s.is_empty() { "payer".into() } else { s }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn parties() -> (Payer, Recipient) {
        let a = Address {
            line1: "1 Main St, Suite 2".into(),
            city: "Akron".into(),
            state: "oh".into(),
            zip: "44301-1234".into(),
            ..Address::default()
        };
        (
            Payer {
                id: "p1".into(),
                name: "Grace Chapel".into(),
                tin: "123456789".into(),
                address: a.clone(),
                phone: "(330) 555-0100".into(),
                ..Payer::default()
            },
            Recipient {
                id: "r1".into(),
                name_type: NameType::Person,
                first: "Ada".into(),
                last: "Lovelace".into(),
                tin_type: Some(TinType::Ssn),
                tin: "987654321".into(),
                address: a,
                ..Recipient::default()
            },
        )
    }

    fn csv_rows(f: &CsvFile) -> Vec<Vec<String>> {
        let text = String::from_utf8(f.content.clone()).unwrap();
        text.lines().map(parse_first_record).collect()
    }

    #[test]
    fn builtin_headers_are_all_mapped() {
        for kind in [FormKind::Nec, FormKind::Misc] {
            for h in builtin_headers(kind) {
                assert_ne!(classify(&h, kind), Field::Unknown, "{h}");
            }
        }
    }

    #[test]
    fn nec_row() {
        let (p, r) = parties();
        let mut f = Form {
            payer: "p1".into(),
            recipient: "r1".into(),
            ..Form::default()
        };
        f.set_amount("1a", Decimal::from_str("2500").unwrap());
        f.set_check("2", true);
        let rows = [Row {
            form: &f,
            payer: &p,
            recipient: &r,
        }];
        let headers = builtin_headers(FormKind::Nec);
        let b = build(2026, FormKind::Nec, &rows, &headers, false);
        assert_eq!(b.files.len(), 1);
        assert!(b.unmapped.is_empty());
        let t = csv_rows(&b.files[0]);
        let col = |name: &str| headers.iter().position(|h| h == name).unwrap();
        assert_eq!(t[1][col("Form Type")], "1099-NEC");
        assert_eq!(t[1][col("Recipient Taxpayer ID Number")], "987654321");
        assert_eq!(t[1][col("Recipient Name Type")], "P");
        assert_eq!(t[1][col("Recipient Business or Entity Name Line 1")], "");
        assert_eq!(t[1][col("Recipient Last Name (Surname)")], "Lovelace");
        assert_eq!(t[1][col("Payer State/Province/Territory")], "OH");
        assert_eq!(t[1][col("Payer ZIP/Postal Code")], "443011234");
        assert_eq!(t[1][col("Payer Phone")], "3305550100");
        assert_eq!(t[1][col("Payer Address Line 1")], "1 Main St, Suite 2");
        assert_eq!(t[1][col("Box 1a - Nonemployee compensation")], "2500.00");
        assert!(
            t[1][col(
                "Box 2 - Payer made direct sales totaling $5,000 or more of consumer products to recipient for resale"
            )] == "Y"
        );
        assert_eq!(t[1][col("Second TIN Notice")], "N");
    }

    #[test]
    fn template_headers_by_meaning_and_unknowns() {
        let kind = FormKind::Misc;
        assert_eq!(classify("Rents (Box 1)", kind), Field::Box("1"));
        assert_eq!(
            classify("Gross proceeds paid to an attorney", kind),
            Field::Box("10")
        );
        assert_eq!(
            classify("*Recipient TIN Type", kind),
            Field::Party(Party::Recipient, PartyField::TinType)
        );
        assert_eq!(
            classify("State 2 Income", kind),
            Field::State(1, StateField::Income)
        );
        assert_eq!(
            classify("Payer State/Province/Territory", kind),
            Field::Party(Party::Payer, PartyField::State)
        );
        assert_eq!(
            classify("Combined Federal/State Filing", kind),
            Field::Unknown
        );
        assert_eq!(classify("Something new", kind), Field::Unknown);
    }

    #[test]
    fn splits_at_100_rows_and_reads_templates() {
        let (p, r) = parties();
        let forms: Vec<Form> = (0..150)
            .map(|i| {
                let mut f = Form {
                    id: i.to_string(),
                    ..Form::default()
                };
                f.set_amount("1a", Decimal::from(3000));
                f
            })
            .collect();
        let rows: Vec<Row<'_>> = forms
            .iter()
            .map(|f| Row {
                form: f,
                payer: &p,
                recipient: &r,
            })
            .collect();
        let b = build(
            2026,
            FormKind::Nec,
            &rows,
            &builtin_headers(FormKind::Nec),
            true,
        );
        assert_eq!(
            b.files.iter().map(|f| f.rows).collect::<Vec<_>>(),
            vec![100, 50]
        );
        assert!(b.files[0].content.starts_with(b"\xEF\xBB\xBF"));
        assert_eq!(b.files[1].name, "1099-NEC_2026_Grace-Chapel_02.csv");

        let dir = tempfile::tempdir().unwrap();
        let t = dir.path().join("t.csv");
        std::fs::write(
            &t,
            b"\xEF\xBB\xBF\"Form Type\",Tax Year,\"Box 1a, NEC\"\r\n",
        )
        .unwrap();
        let (h, bom) = read_template(&t).unwrap();
        assert!(bom);
        assert_eq!(h, vec!["Form Type", "Tax Year", "Box 1a, NEC"]);
    }
}
