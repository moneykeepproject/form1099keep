//! Import from a MoneyKeep book (form1099keep.md "Import from MoneyKeep"): vendors
//! marked for 1099s and their qualifying payments for a year, through MoneyKeep's
//! own commands (`report.1099`, `party.get`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mk_commands::{CommandError, Session};
use mk_core::Decimal;
use serde::Serialize;
use serde_json::{Value, json};

use crate::book::secret_of;
use crate::model::{Address, Form, FormKind, NameType, Recipient, Source};
use crate::{Book, Error, Result};

#[derive(Debug, Clone)]
pub struct Options {
    pub path: PathBuf,
    /// The MoneyKeep book's password or recovery key, when it is protected.
    pub password: Option<String>,
    pub year: i32,
    /// The payer (in the 1099 book) the forms are for.
    pub payer: String,
    pub include_below_threshold: bool,
}

/// What an import did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub book_name: String,
    pub year: i32,
    pub recipients_created: Vec<String>,
    pub recipients_updated: Vec<String>,
    pub forms_created: Vec<String>,
    pub forms_updated: Vec<String>,
    /// Filed forms left alone.
    pub already_filed: Vec<String>,
    /// Vendors below the threshold, not imported.
    pub below_threshold: Vec<String>,
    /// Thresholds where MoneyKeep's setting differs from the IRS amount used here:
    /// box → (MoneyKeep, Form1099Keep).
    pub threshold_differences: BTreeMap<String, (String, String)>,
    pub notes: Vec<String>,
}

impl Report {
    pub fn summary(&self) -> String {
        format!(
            "{} recipients added, {} updated; {} forms added, {} updated; {} already filed; {} below the threshold.",
            self.recipients_created.len(),
            self.recipients_updated.len(),
            self.forms_created.len(),
            self.forms_updated.len(),
            self.already_filed.len(),
            self.below_threshold.len()
        )
    }
}

/// One vendor line from MoneyKeep.
#[derive(Debug, Clone)]
struct VendorRow {
    id: String,
    name: String,
    kind: FormKind,
    box_key: &'static str,
    total: Decimal,
    mk_threshold: Option<Decimal>,
}

pub fn import_moneykeep(book: &mut Book, o: &Options) -> Result<Report> {
    if book.path().is_some_and(|p| same_file(p, &o.path)) {
        return Err(Error::invalid(
            "that is this 1099 book; choose the MoneyKeep book with the vendors and payments",
        ));
    }
    book.payer(&o.payer)?;
    let secret = o.password.as_deref().map(secret_of);
    let mut s =
        Session::open_with(&o.path, crate::ACTOR, secret.as_ref()).map_err(|e| match e {
            CommandError::Store(mk_store::StoreError::PasswordRequired) => Error::MoneyKeep(
                "that MoneyKeep book is password protected; give its password".into(),
            ),
            other => Error::from(other),
        })?;
    let info = s.with_store(|_, st| st.info())?;
    let report = s
        .invoke(
            "report.1099",
            json!({"year": o.year, "include_below_threshold": true}),
        )
        .map_err(|e| match e {
            CommandError::ModuleDisabled { .. } => Error::MoneyKeep(
                "the 1099 module is off in that MoneyKeep book. Turn it on in MoneyKeep \
                 (Settings → Modules) and mark the vendors who get a 1099"
                    .into(),
            ),
            other => Error::from(other),
        })?;
    let rows = vendor_rows(&report)?;
    let thresholds = book.thresholds(o.year)?;

    let mut out = Report {
        book_name: info.name.clone(),
        year: o.year,
        ..Report::default()
    };
    for r in &rows {
        let ours = thresholds.get(r.kind, r.box_key);
        if let (Some(mk), Some(us)) = (r.mk_threshold, ours)
            && mk != us
        {
            out.threshold_differences.insert(
                format!("{} box {}", r.kind.label(), r.box_key),
                (mk.to_string(), us.to_string()),
            );
        }
    }

    let existing_r = book.recipients()?;
    let existing_f = book.forms(Some(o.year))?;
    let mut put_r: Vec<Recipient> = Vec::new();
    let mut put_f: Vec<Form> = Vec::new();
    let mut guessed_people = 0usize;
    for r in rows {
        if r.total <= Decimal::ZERO {
            continue;
        }
        let required = thresholds
            .get(r.kind, r.box_key)
            .is_none_or(|min| r.total >= min);
        if !required && !o.include_below_threshold {
            out.below_threshold.push(r.name.clone());
            continue;
        }
        let party = s.invoke("party.get", json!({"party": r.id}))?;
        let source = Source {
            book_id: info.id.clone(),
            book_name: info.name.clone(),
            vendor_id: r.id.clone(),
            vendor_name: r.name.clone(),
        };
        let found = existing_r.iter().find(|x| {
            x.source
                .as_ref()
                .is_some_and(|s| s.book_id == source.book_id && s.vendor_id == source.vendor_id)
        });
        let recipient = match found {
            Some(orig) => {
                let mut x = orig.clone();
                // Fill blanks only: what the user typed here wins.
                let fresh = recipient_from(&party, &source);
                if x.address.line1.trim().is_empty() && x.address.city.trim().is_empty() {
                    x.address = fresh.address;
                }
                if x.email.trim().is_empty() {
                    x.email = fresh.email;
                }
                x.source = Some(source.clone());
                if x != *orig {
                    out.recipients_updated.push(x.display_name());
                    put_r.push(x.clone());
                }
                x
            }
            None => {
                let mut x = recipient_from(&party, &source);
                x.id = crate::book::new_id();
                x.created_at = crate::book::now();
                if looks_like_person(&x.business_name) {
                    guessed_people = guessed_people.saturating_add(1);
                }
                out.recipients_created.push(x.display_name());
                put_r.push(x.clone());
                x
            }
        };
        let label = format!("{} — {}", recipient.display_name(), r.kind.label());
        let same = |f: &&Form| {
            f.kind == r.kind
                && f.recipient == recipient.id
                && f.payer == o.payer
                && f.source.as_ref().is_some_and(|s| s.vendor_id == r.id)
        };
        match existing_f.iter().find(same) {
            Some(f) if f.is_filed() => out.already_filed.push(label),
            Some(f) => {
                let mut f = f.clone();
                if f.amount(r.box_key) != r.total {
                    f.set_amount(r.box_key, r.total);
                    f.updated_at = crate::book::now();
                    out.forms_updated.push(label);
                    put_f.push(f);
                }
            }
            None => {
                let mut f = Form {
                    id: crate::book::new_id(),
                    payer: o.payer.clone(),
                    recipient: recipient.id.clone(),
                    tax_year: o.year,
                    kind: r.kind,
                    source: Some(source),
                    created_at: crate::book::now(),
                    ..Form::default()
                };
                f.updated_at = f.created_at.clone();
                f.set_amount(r.box_key, r.total);
                out.forms_created.push(label);
                put_f.push(f);
            }
        }
    }
    drop(s);
    if !out.recipients_created.is_empty() {
        out.notes.push(
            "MoneyKeep does not keep taxpayer id numbers: enter each recipient's TIN from \
             their Form W-9."
                .into(),
        );
    }
    if guessed_people > 0 {
        out.notes.push(format!(
            "{guessed_people} new recipient(s) look like people. Imported recipients start as \
             businesses; change a person to \"Person\" so the IRIS file has first and last names."
        ));
    }
    if !out.threshold_differences.is_empty() {
        out.notes.push(
            "Some of MoneyKeep's 1099 thresholds differ from the IRS amounts used here; the \
             IRS amounts decide which forms are required."
                .into(),
        );
    }
    let summary = json!({
        "book": info.name, "book_id": info.id, "year": o.year,
        "recipients_created": out.recipients_created.len(),
        "forms_created": out.forms_created.len(),
        "forms_updated": out.forms_updated.len(),
    });
    book.import_batch(&put_r, &put_f, summary)?;
    Ok(out)
}

fn vendor_rows(report: &Value) -> Result<Vec<VendorRow>> {
    let Some(list) = report.get("vendors").and_then(Value::as_array) else {
        return Err(Error::MoneyKeep(
            "report.1099 returned no vendor list".into(),
        ));
    };
    let dec = |v: Option<&Value>| -> Option<Decimal> {
        v.and_then(Value::as_str)
            .and_then(|s| s.parse::<Decimal>().ok())
    };
    let mut out = Vec::new();
    for v in list {
        let s = |k: &str| {
            v.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let kind = if s("form") == "1099-MISC" {
            FormKind::Misc
        } else {
            FormKind::Nec
        };
        let box_key: &'static str = match (kind, s("box_key").as_str()) {
            (FormKind::Nec, _) => "1a",
            (FormKind::Misc, "rents") => "1",
            (FormKind::Misc, "royalties") => "2",
            (FormKind::Misc, "medical") => "6",
            (FormKind::Misc, "attorney_gross_proceeds") => "10",
            (FormKind::Misc, _) => "3",
        };
        out.push(VendorRow {
            id: s("vendor"),
            name: s("vendor_name"),
            kind,
            box_key,
            total: dec(v.get("total")).unwrap_or_default(),
            mk_threshold: dec(v.get("threshold")),
        });
    }
    Ok(out)
}

fn recipient_from(party: &Value, source: &Source) -> Recipient {
    let s = |k: &str| {
        party
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    let company = s("company");
    let name = s("name");
    // The company is the legal name when MoneyKeep has one.
    let business_name = if company.is_empty() { name } else { company };
    let lines: Vec<String> = party
        .get("billing_address")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(|l| l.trim().to_owned())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default();
    Recipient {
        name_type: NameType::Business,
        business_name,
        address: parse_address(&lines),
        email: s("email"),
        source: Some(source.clone()),
        ..Recipient::default()
    }
}

/// Street lines, then `City, ST 12345[-6789]` on the last line.
pub fn parse_address(lines: &[String]) -> Address {
    let mut a = Address::default();
    let Some((last, street)) = lines.split_last() else {
        return a;
    };
    if let Some((city, rest)) = last.rsplit_once(',') {
        let mut parts = rest.split_whitespace();
        let st = parts.next().unwrap_or_default();
        let zip = parts.next().unwrap_or_default();
        let zip_ok = !zip.is_empty() && zip.chars().all(|c| c.is_ascii_digit() || c == '-');
        if st.len() == 2
            && st.chars().all(|c| c.is_ascii_alphabetic())
            && (zip_ok || zip.is_empty())
        {
            a.city = city.trim().to_owned();
            a.state = st.to_ascii_uppercase();
            a.zip = zip.to_owned();
        } else {
            a.city = last.clone();
        }
    } else {
        a.city = last.clone();
    }
    let mut it = street.iter();
    if let Some(l1) = it.next() {
        a.line1 = l1.clone();
    }
    let rest: Vec<String> = it.cloned().collect();
    a.line2 = rest.join(", ");
    if street.is_empty() {
        // One line only: it is probably the street, not the city.
        if a.state.is_empty() {
            a.line1 = std::mem::take(&mut a.city);
        }
    }
    a
}

/// Two or three capitalised words without business words.
fn looks_like_person(name: &str) -> bool {
    const BUSINESS: &[&str] = &[
        "inc",
        "llc",
        "ltd",
        "co",
        "corp",
        "company",
        "services",
        "service",
        "group",
        "church",
        "plumbing",
        "electric",
        "partners",
        "associates",
        "studio",
        "&",
        "and",
        "the",
    ];
    let words: Vec<&str> = name.split_whitespace().collect();
    (2..=3).contains(&words.len())
        && words.iter().all(|w| {
            let lw = w
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_ascii_lowercase();
            w.chars().next().is_some_and(char::is_uppercase) && !BUSINESS.contains(&lw.as_str())
        })
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn addresses() {
        let a = parse_address(&v(&["12 Oak Ave", "Apt 3", "Akron, oh 44301-1234"]));
        assert_eq!(
            (
                a.line1.as_str(),
                a.line2.as_str(),
                a.city.as_str(),
                a.state.as_str(),
                a.zip.as_str()
            ),
            ("12 Oak Ave", "Apt 3", "Akron", "OH", "44301-1234")
        );
        let b = parse_address(&v(&["PO Box 9", "Somewhere far"]));
        assert_eq!(
            (b.line1.as_str(), b.city.as_str(), b.state.as_str()),
            ("PO Box 9", "Somewhere far", "")
        );
        let c = parse_address(&v(&["12 Oak Ave"]));
        assert_eq!((c.line1.as_str(), c.city.as_str()), ("12 Oak Ave", ""));
        assert_eq!(parse_address(&[]), Address::default());
    }

    #[test]
    fn people() {
        assert!(looks_like_person("Ada Lovelace"));
        assert!(looks_like_person("Mary Ann Smith"));
        assert!(!looks_like_person("Smith Plumbing"));
        assert!(!looks_like_person("Acme Inc."));
        assert!(!looks_like_person("Cher"));
    }
}
