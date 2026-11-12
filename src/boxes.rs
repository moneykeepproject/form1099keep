//! The boxes of Forms 1099-NEC and 1099-MISC (Rev. December 2026) and their
//! reporting thresholds (Instructions for Forms 1099-MISC and 1099-NEC, Rev. 12-2026).

use std::collections::BTreeMap;
use std::str::FromStr;

use mk_core::Decimal;

use crate::model::FormKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxKind {
    /// A dollar amount.
    Amount,
    /// A check box.
    Check,
    /// Up to two Treasury Tipped Occupation Codes.
    Codes,
}

/// What makes a form required.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Required when the box is at least this amount (a decimal string).
    AtLeast(&'static str),
    /// Required for any amount above zero (withholding, golden parachute, fishing boat).
    Any,
    /// Required when checked (direct sales of $5,000 or more).
    WhenChecked,
    /// Never on its own (a part of another box, or the FATCA check).
    Never,
}

#[derive(Debug, Clone, Copy)]
pub struct BoxDef {
    /// `1a`, `13b`, `fatca`…
    pub key: &'static str,
    /// The caption as on the form, without the box number.
    pub caption: &'static str,
    pub kind: BoxKind,
    pub trigger: Trigger,
    /// Captions printed in bold (Pub 1179: federal income tax withheld).
    pub bold: bool,
    /// For amounts the instructions say are included in another box: that box.
    pub part_of: Option<&'static str>,
}

impl BoxDef {
    const fn amount(key: &'static str, caption: &'static str, trigger: Trigger) -> Self {
        Self {
            key,
            caption,
            kind: BoxKind::Amount,
            trigger,
            bold: false,
            part_of: None,
        }
    }

    /// The box number as printed (`FATCA` has none).
    pub fn number(&self) -> &'static str {
        if self.key == "fatca" { "" } else { self.key }
    }

    pub fn counts_in_total(&self) -> bool {
        // Parts of another box (tips, overtime) and informational boxes have trigger
        // `Never`; withholding is not income.
        self.kind == BoxKind::Amount && self.trigger != Trigger::Never && !self.is_withholding()
    }

    pub fn is_withholding(&self) -> bool {
        self.key == "4"
    }

    /// `Box 1a Nonemployee compensation`.
    pub fn title(&self) -> String {
        if self.number().is_empty() {
            self.caption.to_owned()
        } else {
            format!("Box {} {}", self.number(), self.caption)
        }
    }
}

const NEC: &[BoxDef] = &[
    BoxDef::amount("1a", "Nonemployee compensation", Trigger::AtLeast("2000")),
    BoxDef {
        part_of: Some("1a"),
        ..BoxDef::amount("1b", "Cash tips", Trigger::Never)
    },
    BoxDef {
        kind: BoxKind::Codes,
        ..BoxDef::amount("1c", "TTOC", Trigger::Never)
    },
    BoxDef {
        part_of: Some("1a"),
        ..BoxDef::amount("1d", "Overtime compensation", Trigger::Never)
    },
    BoxDef {
        kind: BoxKind::Check,
        ..BoxDef::amount(
            "2",
            "Payer made direct sales totaling $5,000 or more of consumer products to recipient for resale",
            Trigger::WhenChecked,
        )
    },
    BoxDef::amount("3", "Excess golden parachute payments", Trigger::Any),
    BoxDef {
        bold: true,
        ..BoxDef::amount("4", "Federal income tax withheld", Trigger::Any)
    },
];

const MISC: &[BoxDef] = &[
    BoxDef::amount("1", "Rents", Trigger::AtLeast("2000")),
    BoxDef::amount("2", "Royalties", Trigger::AtLeast("10")),
    BoxDef::amount("3", "Other income", Trigger::AtLeast("2000")),
    BoxDef {
        bold: true,
        ..BoxDef::amount("4", "Federal income tax withheld", Trigger::Any)
    },
    BoxDef::amount("5", "Fishing boat proceeds", Trigger::Any),
    BoxDef::amount(
        "6",
        "Medical and health care payments",
        Trigger::AtLeast("2000"),
    ),
    BoxDef {
        kind: BoxKind::Check,
        ..BoxDef::amount(
            "7",
            "Payer made direct sales totaling $5,000 or more of consumer products to recipient for resale",
            Trigger::WhenChecked,
        )
    },
    BoxDef::amount(
        "8",
        "Substitute payments in lieu of dividends or interest",
        Trigger::AtLeast("10"),
    ),
    BoxDef::amount("9", "Crop insurance proceeds", Trigger::AtLeast("2000")),
    BoxDef::amount(
        "10",
        "Gross proceeds paid to an attorney",
        Trigger::AtLeast("600"),
    ),
    BoxDef::amount("11", "Fish purchased for resale", Trigger::AtLeast("600")),
    BoxDef::amount("12", "Section 409A deferrals", Trigger::Never),
    BoxDef::amount("13a", "Cash tips", Trigger::Never),
    BoxDef {
        kind: BoxKind::Codes,
        ..BoxDef::amount("13b", "TTOC", Trigger::Never)
    },
    BoxDef::amount("14", "Overtime compensation", Trigger::Never),
    BoxDef::amount(
        "15",
        "Nonqualified deferred compensation",
        Trigger::AtLeast("2000"),
    ),
    BoxDef {
        kind: BoxKind::Check,
        ..BoxDef::amount("fatca", "FATCA filing requirement", Trigger::Never)
    },
];

/// The boxes of a form, in printed order (state boxes are separate).
pub fn layout(kind: FormKind) -> &'static [BoxDef] {
    match kind {
        FormKind::Nec => NEC,
        FormKind::Misc => MISC,
    }
}

pub fn find(kind: FormKind, key: &str) -> Option<&'static BoxDef> {
    layout(kind).iter().find(|b| b.key == key)
}

/// The box keys of the state lines: (tax withheld, payer state no., state income).
pub fn state_boxes(kind: FormKind) -> (&'static str, &'static str, &'static str) {
    match kind {
        FormKind::Nec => ("5", "6", "7"),
        FormKind::Misc => ("16", "17", "18"),
    }
}

/// OMB number printed on recipient copies.
pub fn omb(kind: FormKind) -> &'static str {
    match kind {
        FormKind::Nec => "1545-0116",
        FormKind::Misc => "1545-0115",
    }
}

/// Threshold amounts for one year, `form:box` → amount (book setting overrides
/// merged over the 2026 defaults).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thresholds {
    pub year: i32,
    pub amounts: BTreeMap<String, Decimal>,
    /// True when `year` is after 2026 and no override was entered: the amounts are
    /// 2026's and may have been indexed for inflation.
    pub copied_from_2026: bool,
}

impl Thresholds {
    pub fn defaults(year: i32) -> Self {
        let mut amounts = BTreeMap::new();
        for kind in [FormKind::Nec, FormKind::Misc] {
            for b in layout(kind) {
                if let Trigger::AtLeast(a) = b.trigger
                    && let Ok(d) = Decimal::from_str(a)
                {
                    amounts.insert(key(kind, b.key), d);
                }
            }
        }
        Self {
            year,
            amounts,
            copied_from_2026: year > crate::FIRST_YEAR,
        }
    }

    pub fn get(&self, kind: FormKind, box_key: &str) -> Option<Decimal> {
        self.amounts.get(&key(kind, box_key)).copied()
    }
}

/// `nec:1a`, `misc:10`.
pub fn key(kind: FormKind, box_key: &str) -> String {
    let k = match kind {
        FormKind::Nec => "nec",
        FormKind::Misc => "misc",
    };
    format!("{k}:{box_key}")
}

/// Whether a form must be filed: some box meets its threshold, any withholding, or
/// a direct-sales check. Returns the reasons (box titles).
pub fn required_by(form: &crate::Form, t: &Thresholds) -> Vec<String> {
    let mut why = Vec::new();
    for b in layout(form.kind) {
        let hit = match (b.kind, b.trigger) {
            (BoxKind::Amount, Trigger::AtLeast(_)) => t
                .get(form.kind, b.key)
                .is_some_and(|min| form.amount(b.key) >= min && !form.amount(b.key).is_zero()),
            (BoxKind::Amount, Trigger::Any) => form.amount(b.key) > Decimal::ZERO,
            (BoxKind::Check, Trigger::WhenChecked) => form.checked(b.key),
            _ => false,
        };
        if hit {
            why.push(b.title());
        }
    }
    if form.states.iter().any(|s| s.tax_withheld > Decimal::ZERO) {
        why.push("State tax withheld".into());
    }
    why
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::Form;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn thresholds_2026() {
        let t = Thresholds::defaults(2026);
        assert_eq!(t.get(FormKind::Nec, "1a"), Some(d("2000")));
        assert_eq!(t.get(FormKind::Misc, "2"), Some(d("10")));
        assert_eq!(t.get(FormKind::Misc, "10"), Some(d("600")));
        assert_eq!(t.get(FormKind::Misc, "11"), Some(d("600")));
        assert!(!t.copied_from_2026);
        assert!(Thresholds::defaults(2027).copied_from_2026);
    }

    #[test]
    fn required() {
        let t = Thresholds::defaults(2026);
        let mut f = Form::default();
        f.set_amount("1a", d("1999.99"));
        assert!(required_by(&f, &t).is_empty());
        f.set_amount("1a", d("2000"));
        assert_eq!(required_by(&f, &t).len(), 1);
        let mut g = Form::default();
        g.set_amount("1a", d("100"));
        g.set_amount("4", d("24"));
        assert_eq!(
            required_by(&g, &t),
            vec!["Box 4 Federal income tax withheld"]
        );
        let mut m = Form {
            kind: FormKind::Misc,
            ..Form::default()
        };
        m.set_amount("10", d("600"));
        assert_eq!(required_by(&m, &t).len(), 1);
    }

    #[test]
    fn totals_leave_out_parts_and_withholding() {
        let mut f = Form::default();
        f.set_amount("1a", d("3000"));
        f.set_amount("1b", d("200"));
        f.set_amount("4", d("720"));
        assert_eq!(f.income_total(), d("3000"));
    }
}
