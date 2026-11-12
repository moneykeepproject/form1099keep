//! Checks on payers, recipients and forms (form1099keep.md "Checks").

use std::collections::BTreeMap;

use mk_core::Decimal;
use serde::Serialize;

use crate::boxes::{self, BoxKind, Thresholds};
use crate::model::{Address, Form, FormKind, NameType, Payer, Recipient, Status, TinType};
use crate::tin::{self, TinCheck};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Blocks marking ready, printing and the IRIS file.
    Error,
    /// Worth a look; does not block.
    Warning,
    /// For information.
    Note,
}

/// What a finding is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase", tag = "type", content = "id")]
pub enum Subject {
    Payer(String),
    Recipient(String),
    Form(String),
    Year(i32),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub subject: Subject,
    /// The field, such as `tin`, `address.zip` or `box 1a`.
    pub field: String,
    /// What is wrong, phrased as something to answer or fix.
    pub message: String,
}

fn push(v: &mut Vec<Finding>, severity: Severity, subject: &Subject, field: &str, msg: String) {
    v.push(Finding {
        severity,
        subject: subject.clone(),
        field: field.to_owned(),
        message: msg,
    });
}

fn check_address(v: &mut Vec<Finding>, s: &Subject, who: &str, a: &Address) {
    if a.line1.trim().is_empty() {
        push(
            v,
            Severity::Error,
            s,
            "address.line1",
            format!("What is {who}'s street address?"),
        );
    }
    if a.city.trim().is_empty() {
        push(
            v,
            Severity::Error,
            s,
            "address.city",
            format!("What city is {who} in?"),
        );
    }
    if a.is_us() {
        let st = a.state.trim();
        if st.len() != 2 || !st.chars().all(|c| c.is_ascii_alphabetic()) {
            push(
                v,
                Severity::Error,
                s,
                "address.state",
                format!("{who}'s state should be its 2-letter code (for example OH)."),
            );
        }
        let digits: String = a.zip.chars().filter(char::is_ascii_digit).collect();
        let shape_ok = a.zip.chars().all(|c| c.is_ascii_digit() || c == '-');
        if !(shape_ok && (digits.len() == 5 || digits.len() == 9)) {
            push(
                v,
                Severity::Error,
                s,
                "address.zip",
                format!("{who}'s ZIP code should be 5 or 9 digits."),
            );
        }
    }
}

fn check_tin(
    v: &mut Vec<Finding>,
    s: &Subject,
    who: &str,
    tin_type: Option<TinType>,
    tin_value: &str,
) {
    let Some(tt) = tin_type else {
        push(
            v,
            Severity::Error,
            s,
            "tin",
            format!("What is {who}'s taxpayer id number (from the W-9)?"),
        );
        return;
    };
    match tin::check(tt, tin_value) {
        TinCheck::Ok => {}
        TinCheck::Warning(m) => push(v, Severity::Warning, s, "tin", format!("{who}: {m}")),
        TinCheck::Error(_) if tin_value.is_empty() => push(
            v,
            Severity::Error,
            s,
            "tin",
            format!("What is {who}'s {} (from the W-9)?", tt.code()),
        ),
        TinCheck::Error(m) => push(v, Severity::Error, s, "tin", format!("{who}: {m}.")),
    }
}

pub fn payer(p: &Payer) -> Vec<Finding> {
    let mut v = Vec::new();
    let s = Subject::Payer(p.id.clone());
    let who = if p.name.is_empty() {
        "the payer"
    } else {
        p.name.as_str()
    };
    if p.name.trim().is_empty() {
        push(
            &mut v,
            Severity::Error,
            &s,
            "name",
            "What is the payer's name?".into(),
        );
    }
    check_tin(&mut v, &s, who, Some(p.tin_type), &p.tin);
    check_address(&mut v, &s, who, &p.address);
    if p.phone.chars().filter(char::is_ascii_digit).count() < 10 {
        push(
            &mut v,
            Severity::Error,
            &s,
            "phone",
            format!(
                "What telephone number should recipients call with questions? \
                 Recipient copies must show one for {who}."
            ),
        );
    }
    v
}

pub fn recipient(r: &Recipient) -> Vec<Finding> {
    let mut v = Vec::new();
    let s = Subject::Recipient(r.id.clone());
    let name = r.display_name();
    let who = if name.is_empty() {
        "the recipient".to_owned()
    } else {
        name
    };
    match r.name_type {
        NameType::Business if r.business_name.trim().is_empty() => push(
            &mut v,
            Severity::Error,
            &s,
            "business_name",
            "What is the recipient's business name?".into(),
        ),
        NameType::Person if r.first.trim().is_empty() || r.last.trim().is_empty() => push(
            &mut v,
            Severity::Error,
            &s,
            "name",
            "What are the recipient's first and last names?".into(),
        ),
        _ => {}
    }
    check_tin(&mut v, &s, &who, r.tin_type, &r.tin);
    match (r.name_type, r.tin_type) {
        (NameType::Person, Some(TinType::Ein)) => push(
            &mut v,
            Severity::Warning,
            &s,
            "tin_type",
            format!(
                "{who} is a person with an EIN. If the W-9 names a business, use the business name."
            ),
        ),
        (NameType::Business, Some(TinType::Ssn)) => push(
            &mut v,
            Severity::Warning,
            &s,
            "tin_type",
            format!(
                "{who} is a business with an SSN. A sole proprietor is usually entered as a person \
                 (business name on line 2)."
            ),
        ),
        _ => {}
    }
    check_address(&mut v, &s, &who, &r.address);
    if r.account_number.chars().count() > 20 {
        push(
            &mut v,
            Severity::Error,
            &s,
            "account_number",
            "The account number can have at most 20 characters.".into(),
        );
    }
    v
}

/// Checks on one form alone (amounts, codes, thresholds).
pub fn form(f: &Form, t: &Thresholds) -> Vec<Finding> {
    let mut v = Vec::new();
    let s = Subject::Form(f.id.clone());
    let has_amount = f.amounts.values().any(|a| !a.is_zero());
    let has_check = f.checks.iter().any(|c| c != "fatca");
    let has_state = f.states.iter().any(|r| r.tax_withheld > Decimal::ZERO);
    if !has_amount && !has_check && !has_state {
        push(
            &mut v,
            Severity::Error,
            &s,
            "boxes",
            "This form has no amounts. What was paid?".into(),
        );
    }
    for (k, a) in &f.amounts {
        if a.is_sign_negative() {
            push(
                &mut v,
                Severity::Error,
                &s,
                &format!("box {k}"),
                format!("Box {k} cannot be negative."),
            );
        }
    }
    let (tips, codes, part_checks): (&str, &str, &[(&str, &str)]) = match f.kind {
        FormKind::Nec => ("1b", "1c", &[("1b", "1a"), ("1d", "1a")]),
        FormKind::Misc => ("13a", "13b", &[]),
    };
    for (part, whole) in part_checks {
        if f.amount(part) > f.amount(whole) {
            push(
                &mut v,
                Severity::Error,
                &s,
                &format!("box {part}"),
                format!(
                    "Box {part} is part of box {whole}, so it cannot be larger than box {whole}."
                ),
            );
        }
    }
    if f.amount(tips) > Decimal::ZERO && f.ttoc.is_empty() {
        push(
            &mut v,
            Severity::Error,
            &s,
            &format!("box {codes}"),
            format!(
                "Box {tips} has cash tips. What is the Treasury Tipped Occupation Code for box {codes} \
                 (or 000 if the tips do not qualify)?"
            ),
        );
    }
    for c in &f.ttoc {
        if c.len() != 3 || !c.chars().all(|x| x.is_ascii_digit()) {
            push(
                &mut v,
                Severity::Error,
                &s,
                &format!("box {codes}"),
                format!("Occupation code {c:?} should be 3 digits."),
            );
        }
    }
    if f.kind == FormKind::Misc && f.checked("fatca") && f.account_number.trim().is_empty() {
        push(
            &mut v,
            Severity::Error,
            &s,
            "account_number",
            "The FATCA box is checked, so the form needs an account number.".into(),
        );
    }
    for (i, r) in f.states.iter().enumerate() {
        let st = r.state.trim();
        if st.len() != 2 || !st.chars().all(|c| c.is_ascii_alphabetic()) {
            push(
                &mut v,
                Severity::Error,
                &s,
                &format!("state {}", i.saturating_add(1)),
                "Each state line needs the state's 2-letter code.".into(),
            );
        }
    }
    if f.states.len() > 2 {
        push(
            &mut v,
            Severity::Error,
            &s,
            "states",
            "A form has room for two state lines.".into(),
        );
    }
    if has_amount && boxes::required_by(f, t).is_empty() {
        let main = boxes::layout(f.kind)
            .iter()
            .filter(|b| b.kind == BoxKind::Amount)
            .find_map(|b| {
                t.get(f.kind, b.key)
                    .filter(|_| f.amount(b.key) > Decimal::ZERO)
                    .map(|m| (b, m))
            });
        let msg = match main {
            Some((b, m)) => format!(
                "Box {} is below the ${} threshold and nothing was withheld, so this form is not \
                 required. File it anyway?",
                b.key,
                money(m)
            ),
            None => {
                "No box reaches its threshold, so this form is not required. File it anyway?".into()
            }
        };
        push(&mut v, Severity::Warning, &s, "threshold", msg);
    }
    if t.copied_from_2026 {
        push(
            &mut v,
            Severity::Note,
            &s,
            "threshold",
            format!(
                "Thresholds for {} are 2026's; from 2027 the IRS adjusts them for inflation. \
                 Check this year's amounts.",
                t.year
            ),
        );
    }
    v
}

/// Every check for a year: the forms with their payer and recipient, duplicates,
/// and the e-file note. Payers and recipients are checked once each.
pub fn year(
    year: i32,
    forms: &[Form],
    payers: &[Payer],
    recipients: &[Recipient],
    t: &Thresholds,
) -> Vec<Finding> {
    let mut v = Vec::new();
    let mut seen_p = std::collections::BTreeSet::new();
    let mut seen_r = std::collections::BTreeSet::new();
    let mut groups: BTreeMap<(String, String, FormKind), Vec<&Form>> = BTreeMap::new();
    let mut per_payer: BTreeMap<String, usize> = BTreeMap::new();
    for f in forms.iter().filter(|f| f.tax_year == year) {
        v.extend(form(f, t));
        if seen_p.insert(f.payer.clone()) {
            match payers.iter().find(|p| p.id == f.payer) {
                Some(p) => v.extend(payer(p)),
                None => push(
                    &mut v,
                    Severity::Error,
                    &Subject::Form(f.id.clone()),
                    "payer",
                    "The form's payer is missing.".into(),
                ),
            }
        }
        if seen_r.insert(f.recipient.clone()) {
            match recipients.iter().find(|r| r.id == f.recipient) {
                Some(r) => v.extend(recipient(r)),
                None => push(
                    &mut v,
                    Severity::Error,
                    &Subject::Form(f.id.clone()),
                    "recipient",
                    "The form's recipient is missing.".into(),
                ),
            }
        }
        groups
            .entry((f.payer.clone(), f.recipient.clone(), f.kind))
            .or_default()
            .push(f);
        let n = per_payer.entry(f.payer.clone()).or_default();
        *n = n.saturating_add(1);
    }
    for ((_, _, kind), fs) in &groups {
        if fs.len() > 1 {
            let mut numbers: Vec<&str> = fs.iter().map(|f| f.account_number.trim()).collect();
            numbers.sort_unstable();
            let distinct =
                numbers.windows(2).all(|w| w.first() != w.get(1)) && !numbers.contains(&"");
            if !distinct {
                for f in fs {
                    push(
                        &mut v,
                        Severity::Error,
                        &Subject::Form(f.id.clone()),
                        "account_number",
                        format!(
                            "This recipient has {} {} forms from this payer for {year}. Give each \
                             its own account number, or combine them into one form.",
                            fs.len(),
                            kind.label()
                        ),
                    );
                }
            }
        }
    }
    for (pid, n) in per_payer {
        if n >= 10 {
            let name = payers
                .iter()
                .find(|p| p.id == pid)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            push(
                &mut v,
                Severity::Note,
                &Subject::Year(year),
                "e-file",
                format!(
                    "{name} has {n} forms for {year}. With 10 or more information returns the IRS \
                     requires e-filing (the IRIS file does this)."
                ),
            );
        }
    }
    v
}

/// Whether any finding blocks this form (its own, its payer's or its recipient's).
pub fn blocks(findings: &[Finding], f: &Form) -> bool {
    findings.iter().any(|x| {
        x.severity == Severity::Error
            && match &x.subject {
                Subject::Form(id) => *id == f.id,
                Subject::Payer(id) => *id == f.payer,
                Subject::Recipient(id) => *id == f.recipient,
                Subject::Year(_) => false,
            }
    })
}

/// Findings that concern one form (its own, its payer's and its recipient's).
pub fn for_form<'a>(findings: &'a [Finding], f: &Form) -> Vec<&'a Finding> {
    findings
        .iter()
        .filter(|x| match &x.subject {
            Subject::Form(id) => *id == f.id,
            Subject::Payer(id) => *id == f.payer,
            Subject::Recipient(id) => *id == f.recipient,
            Subject::Year(_) => false,
        })
        .collect()
}

/// A form may be marked ready only without errors.
pub fn can_be_ready(findings: &[Finding], f: &Form) -> bool {
    f.status != Status::Filed && !blocks(findings, f)
}

/// `1,234.56`.
pub fn money(d: Decimal) -> String {
    let s = format!("{:.2}", d.round_dp(2));
    let (int, frac) = s.split_once('.').unwrap_or((&s, "00"));
    let (sign, digits) = int.strip_prefix('-').map_or(("", int), |d| ("-", d));
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    format!("{sign}{out}.{frac}")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn good_payer() -> Payer {
        Payer {
            id: "p".into(),
            name: "Grace Chapel".into(),
            tin: "123456789".into(),
            address: Address {
                line1: "1 Main St".into(),
                city: "Akron".into(),
                state: "OH".into(),
                zip: "44301".into(),
                ..Address::default()
            },
            phone: "(330) 555-0100".into(),
            ..Payer::default()
        }
    }

    fn good_recipient() -> Recipient {
        Recipient {
            id: "r".into(),
            name_type: NameType::Person,
            first: "Ada".into(),
            last: "Lovelace".into(),
            tin_type: Some(TinType::Ssn),
            tin: "123456789".into(),
            address: good_payer().address,
            ..Recipient::default()
        }
    }

    fn nec(amount: &str) -> Form {
        let mut f = Form {
            id: "f".into(),
            payer: "p".into(),
            recipient: "r".into(),
            ..Form::default()
        };
        f.set_amount("1a", d(amount));
        f
    }

    #[test]
    fn clean_form_has_no_findings() {
        let t = Thresholds::defaults(2026);
        let v = year(
            2026,
            &[nec("2500")],
            &[good_payer()],
            &[good_recipient()],
            &t,
        );
        assert!(v.is_empty(), "{v:?}");
    }

    #[test]
    fn missing_things_are_questions() {
        let t = Thresholds::defaults(2026);
        let mut r = good_recipient();
        r.tin.clear();
        r.address.zip = "4430".into();
        let mut p = good_payer();
        p.phone.clear();
        let v = year(2026, &[nec("2500")], &[p], &[r], &t);
        let fields: Vec<_> = v.iter().map(|f| f.field.as_str()).collect();
        assert!(fields.contains(&"tin"));
        assert!(fields.contains(&"address.zip"));
        assert!(fields.contains(&"phone"));
        assert!(blocks(&v, &nec("1")));
    }

    #[test]
    fn parts_tips_and_threshold() {
        let t = Thresholds::defaults(2026);
        let mut f = nec("1500");
        f.set_amount("1b", d("1600"));
        let v = form(&f, &t);
        assert!(
            v.iter()
                .any(|x| x.field == "box 1b" && x.severity == Severity::Error)
        );
        assert!(v.iter().any(|x| x.field == "box 1c"));
        assert!(
            v.iter()
                .any(|x| x.field == "threshold" && x.severity == Severity::Warning)
        );
    }

    #[test]
    fn duplicates_need_account_numbers() {
        let t = Thresholds::defaults(2026);
        let a = nec("3000");
        let mut b = nec("4000");
        b.id = "g".into();
        let v = year(
            2026,
            &[a.clone(), b.clone()],
            &[good_payer()],
            &[good_recipient()],
            &t,
        );
        assert_eq!(v.iter().filter(|x| x.field == "account_number").count(), 2);
        let mut a2 = a;
        a2.account_number = "A1".into();
        let mut b2 = b;
        b2.account_number = "B2".into();
        assert!(year(2026, &[a2, b2], &[good_payer()], &[good_recipient()], &t).is_empty());
    }

    #[test]
    fn money_format() {
        assert_eq!(money(d("1234567.5")), "1,234,567.50");
        assert_eq!(money(d("12")), "12.00");
        assert_eq!(money(d("-1000")), "-1,000.00");
    }
}
