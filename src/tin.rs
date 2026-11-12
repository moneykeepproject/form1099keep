//! Taxpayer identification numbers: normalising, checking and masking
//! (form1099keep.md "TIN rules").

use crate::model::TinType;

/// EIN prefixes the IRS has not assigned (a warning, not an error: new prefixes
/// may be assigned).
const UNASSIGNED_EIN_PREFIXES: &[&str] = &[
    "00", "07", "08", "09", "17", "18", "19", "28", "29", "49", "69", "70", "78", "79", "89", "96",
    "97",
];

/// Digits only, with spaces and hyphens removed; `None` if anything else is present.
pub fn normalise(input: &str) -> Option<String> {
    let mut out = String::with_capacity(9);
    for c in input.chars() {
        match c {
            '0'..='9' => out.push(c),
            ' ' | '-' => {}
            _ => return None,
        }
    }
    Some(out)
}

/// The outcome of checking a TIN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TinCheck {
    Ok,
    /// Plausible but unusual (for example an unassigned EIN prefix).
    Warning(String),
    Error(String),
}

/// Check a normalised TIN of the given type.
pub fn check(tin_type: TinType, tin: &str) -> TinCheck {
    if tin.is_empty() {
        return TinCheck::Error("the taxpayer id number is missing".into());
    }
    if tin.len() != 9 || !tin.bytes().all(|b| b.is_ascii_digit()) {
        return TinCheck::Error("a taxpayer id number has exactly 9 digits".into());
    }
    let (area, rest) = tin.split_at(3);
    let (group, serial) = rest.split_at(2);
    let first = tin.get(..1).unwrap_or_default();
    let mid: u32 = group.parse().unwrap_or(0);
    match tin_type {
        TinType::Ein => {
            let prefix = tin.get(..2).unwrap_or_default();
            if tin.bytes().all(|b| b == b'0') {
                TinCheck::Error("an EIN cannot be all zeros".into())
            } else if UNASSIGNED_EIN_PREFIXES.contains(&prefix) {
                TinCheck::Warning(format!(
                    "EIN prefix {prefix} is not one the IRS assigns; check the number"
                ))
            } else {
                TinCheck::Ok
            }
        }
        TinType::Ssn => {
            if area == "000" || area == "666" || first == "9" {
                TinCheck::Error(format!(
                    "an SSN cannot start with {area}{}",
                    if first == "9" {
                        " (numbers starting with 9 are ITINs or ATINs)"
                    } else {
                        ""
                    }
                ))
            } else if group == "00" {
                TinCheck::Error("an SSN's middle two digits cannot be 00".into())
            } else if serial == "0000" {
                TinCheck::Error("an SSN's last four digits cannot be 0000".into())
            } else {
                TinCheck::Ok
            }
        }
        TinType::Itin => {
            let ok_mid = matches!(mid, 50..=65 | 70..=88 | 90..=92 | 94..=99);
            if first != "9" || !ok_mid {
                TinCheck::Error(
                    "an ITIN starts with 9 and its 4th–5th digits are 50–65, 70–88, 90–92 or 94–99"
                        .into(),
                )
            } else {
                TinCheck::Ok
            }
        }
        TinType::Atin => {
            if first != "9" || mid != 93 {
                TinCheck::Error("an ATIN starts with 9 and its 4th–5th digits are 93".into())
            } else {
                TinCheck::Ok
            }
        }
    }
}

/// Masked for display: `***-**-1234` (SSN, ITIN, ATIN) or `**-***1234` (EIN).
/// An empty TIN shows as an empty string; a malformed one as `•••`.
pub fn mask(tin_type: Option<TinType>, tin: &str) -> String {
    if tin.is_empty() {
        return String::new();
    }
    let Some(last4) = tin
        .get(tin.len().saturating_sub(4)..)
        .filter(|_| tin.len() >= 4)
    else {
        return "•••".into();
    };
    match tin_type {
        Some(TinType::Ein) => format!("**-***{last4}"),
        _ => format!("***-**-{last4}"),
    }
}

/// Written out in full with the usual punctuation (`12-3456789`, `123-45-6789`).
pub fn display(tin_type: Option<TinType>, tin: &str) -> String {
    if tin.len() != 9 || !tin.is_ascii() {
        return tin.to_owned();
    }
    let g = |r: std::ops::Range<usize>| tin.get(r).unwrap_or_default();
    match tin_type {
        Some(TinType::Ein) => format!("{}-{}", g(0..2), g(2..9)),
        _ => format!("{}-{}-{}", g(0..3), g(3..5), g(5..9)),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn normalising() {
        assert_eq!(normalise("12-3456789").unwrap(), "123456789");
        assert_eq!(normalise(" 123 45 6789 ").unwrap(), "123456789");
        assert!(normalise("12a").is_none());
    }

    #[test]
    fn ssn_rules() {
        assert_eq!(check(TinType::Ssn, "123456789"), TinCheck::Ok);
        assert!(matches!(
            check(TinType::Ssn, "000456789"),
            TinCheck::Error(_)
        ));
        assert!(matches!(
            check(TinType::Ssn, "666456789"),
            TinCheck::Error(_)
        ));
        assert!(matches!(
            check(TinType::Ssn, "923456789"),
            TinCheck::Error(_)
        ));
        assert!(matches!(
            check(TinType::Ssn, "123006789"),
            TinCheck::Error(_)
        ));
        assert!(matches!(
            check(TinType::Ssn, "123450000"),
            TinCheck::Error(_)
        ));
        assert!(matches!(
            check(TinType::Ssn, "12345678"),
            TinCheck::Error(_)
        ));
    }

    #[test]
    fn ein_itin_atin() {
        assert_eq!(check(TinType::Ein, "123456789"), TinCheck::Ok);
        assert!(matches!(
            check(TinType::Ein, "071234567"),
            TinCheck::Warning(_)
        ));
        assert!(matches!(
            check(TinType::Ein, "000000000"),
            TinCheck::Error(_)
        ));
        assert_eq!(check(TinType::Itin, "912701234"), TinCheck::Ok);
        assert!(matches!(
            check(TinType::Itin, "912931234"),
            TinCheck::Error(_)
        ));
        assert_eq!(check(TinType::Atin, "912931234"), TinCheck::Ok);
    }

    #[test]
    fn masking() {
        assert_eq!(mask(Some(TinType::Ssn), "123456789"), "***-**-6789");
        assert_eq!(mask(Some(TinType::Ein), "123456789"), "**-***6789");
        assert_eq!(mask(None, ""), "");
        assert_eq!(display(Some(TinType::Ein), "123456789"), "12-3456789");
        assert_eq!(display(Some(TinType::Ssn), "123456789"), "123-45-6789");
    }
}
