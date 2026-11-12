//! Payers, recipients and forms as stored in the book (JSON documents).

use std::collections::{BTreeMap, BTreeSet};

use mk_core::Decimal;
use serde::{Deserialize, Serialize};

use crate::tin;

fn us() -> String {
    "US".into()
}

/// A postal address, one field per box (the 2026 forms split the address).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Address {
    pub line1: String,
    pub line2: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub country: String,
}

impl Default for Address {
    fn default() -> Self {
        Self {
            line1: String::new(),
            line2: String::new(),
            city: String::new(),
            state: String::new(),
            zip: String::new(),
            country: us(),
        }
    }
}

impl Address {
    pub fn is_us(&self) -> bool {
        let c = self.country.trim();
        c.is_empty() || c.eq_ignore_ascii_case("US") || c.eq_ignore_ascii_case("USA")
    }

    /// `City, ST 12345` (or `City, Region, Country` outside the US).
    pub fn city_line(&self) -> String {
        let mut s = self.city.trim().to_owned();
        let st = self.state.trim();
        if !st.is_empty() {
            if !s.is_empty() {
                s.push_str(", ");
            }
            s.push_str(st);
        }
        let zip = self.zip.trim();
        if !zip.is_empty() {
            s.push(' ');
            s.push_str(&format_zip(zip));
        }
        if !self.is_us() {
            s.push_str(", ");
            s.push_str(self.country.trim());
        }
        s
    }

    /// Street lines that are not empty.
    pub fn street_lines(&self) -> Vec<String> {
        [&self.line1, &self.line2]
            .into_iter()
            .map(|l| l.trim().to_owned())
            .filter(|l| !l.is_empty())
            .collect()
    }
}

/// `123456789` → `12345-6789`; anything else unchanged.
pub fn format_zip(zip: &str) -> String {
    let digits: String = zip.chars().filter(char::is_ascii_digit).collect();
    if digits.len() == 9 && zip.chars().all(|c| c.is_ascii_digit() || c == '-') {
        format!(
            "{}-{}",
            digits.get(..5).unwrap_or_default(),
            digits.get(5..).unwrap_or_default()
        )
    } else {
        zip.to_owned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum TinType {
    Ein,
    Ssn,
    Itin,
    Atin,
}

impl TinType {
    pub fn code(self) -> &'static str {
        match self {
            TinType::Ein => "EIN",
            TinType::Ssn => "SSN",
            TinType::Itin => "ITIN",
            TinType::Atin => "ATIN",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ein" => Some(Self::Ein),
            "ssn" => Some(Self::Ssn),
            "itin" => Some(Self::Itin),
            "atin" => Some(Self::Atin),
            _ => None,
        }
    }

    pub const ALL: [TinType; 4] = [TinType::Ein, TinType::Ssn, TinType::Itin, TinType::Atin];
}

/// Who pays: the business, church or person filing the forms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Payer {
    pub id: String,
    pub name: String,
    pub name2: String,
    pub tin_type: TinType,
    pub tin: String,
    pub address: Address,
    pub phone: String,
    pub email: String,
    pub contact: String,
    pub created_at: String,
}

impl Default for Payer {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            name2: String::new(),
            tin_type: TinType::Ein,
            tin: String::new(),
            address: Address::default(),
            phone: String::new(),
            email: String::new(),
            contact: String::new(),
            created_at: String::new(),
        }
    }
}

impl Payer {
    pub fn masked_tin(&self) -> String {
        tin::mask(Some(self.tin_type), &self.tin)
    }

    /// A copy with the TIN masked (for audit entries and logs).
    pub fn redacted(&self) -> Self {
        Self {
            tin: self.masked_tin(),
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NameType {
    #[default]
    Business,
    Person,
}

/// Where an imported record came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Source {
    /// The MoneyKeep book's id.
    pub book_id: String,
    pub book_name: String,
    pub vendor_id: String,
    pub vendor_name: String,
}

/// Who is paid: one Form 1099 recipient.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Recipient {
    pub id: String,
    pub name_type: NameType,
    pub business_name: String,
    pub business_name2: String,
    pub first: String,
    pub middle: String,
    pub last: String,
    pub suffix: String,
    /// Empty until known (for example after a MoneyKeep import, until the W-9 comes).
    pub tin_type: Option<TinType>,
    pub tin: String,
    pub address: Address,
    pub email: String,
    pub account_number: String,
    pub source: Option<Source>,
    pub created_at: String,
}

impl Recipient {
    /// The name as it appears on the form.
    pub fn display_name(&self) -> String {
        match self.name_type {
            NameType::Business => self.business_name.trim().to_owned(),
            NameType::Person => {
                let parts = [&self.first, &self.middle, &self.last, &self.suffix];
                parts
                    .iter()
                    .map(|p| p.trim())
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            }
        }
    }

    /// Name lines for the form face (a second business line when present).
    pub fn name_lines(&self) -> Vec<String> {
        let mut v = vec![self.display_name()];
        if self.name_type == NameType::Business && !self.business_name2.trim().is_empty() {
            v.push(self.business_name2.trim().to_owned());
        }
        v
    }

    /// Sort key: last name for people, the name for businesses.
    pub fn sort_key(&self) -> String {
        match self.name_type {
            NameType::Business => self.business_name.to_lowercase(),
            NameType::Person => format!("{} {}", self.last, self.first).to_lowercase(),
        }
    }

    pub fn masked_tin(&self) -> String {
        tin::mask(self.tin_type, &self.tin)
    }

    pub fn redacted(&self) -> Self {
        Self {
            tin: self.masked_tin(),
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum FormKind {
    Nec,
    Misc,
}

impl FormKind {
    /// `1099-NEC` or `1099-MISC`.
    pub fn label(self) -> &'static str {
        match self {
            FormKind::Nec => "1099-NEC",
            FormKind::Misc => "1099-MISC",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            FormKind::Nec => "Nonemployee Compensation",
            FormKind::Misc => "Miscellaneous Information",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let t = s.trim().to_ascii_lowercase();
        match t.trim_start_matches("1099-").trim_start_matches("1099") {
            "nec" => Some(Self::Nec),
            "misc" => Some(Self::Misc),
            _ => None,
        }
    }

    /// The box that a plain amount goes into by default.
    pub fn main_box(self) -> &'static str {
        match self {
            FormKind::Nec => "1a",
            FormKind::Misc => "3",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[default]
    Draft,
    Ready,
    Filed,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Draft => "Draft",
            Status::Ready => "Ready",
            Status::Filed => "Filed",
        }
    }
}

/// One state line (boxes 5–7 of the NEC, 16–18 of the MISC).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct StateRow {
    pub state: String,
    pub payer_state_no: String,
    pub tax_withheld: Decimal,
    pub income: Decimal,
}

/// When and how the form was filed with the IRS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Filed {
    pub date: String,
    /// The IRIS receipt id, if the user noted it.
    pub receipt_id: String,
}

/// One Form 1099.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Form {
    pub id: String,
    pub payer: String,
    pub recipient: String,
    pub tax_year: i32,
    pub kind: FormKind,
    pub account_number: String,
    pub second_tin_notice: bool,
    pub corrected: bool,
    pub status: Status,
    pub filed: Option<Filed>,
    pub notes: String,
    /// Money boxes by box key (`1a`, `4`, `10`…). Zero amounts are not stored.
    pub amounts: BTreeMap<String, Decimal>,
    /// Check boxes that are set (`2`, `7`, `fatca`).
    pub checks: BTreeSet<String>,
    /// Treasury Tipped Occupation Codes (NEC 1c, MISC 13b), at most two.
    pub ttoc: Vec<String>,
    pub states: Vec<StateRow>,
    pub source: Option<Source>,
    pub created_at: String,
    pub updated_at: String,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            id: String::new(),
            payer: String::new(),
            recipient: String::new(),
            tax_year: crate::FIRST_YEAR,
            kind: FormKind::Nec,
            account_number: String::new(),
            second_tin_notice: false,
            corrected: false,
            status: Status::Draft,
            filed: None,
            notes: String::new(),
            amounts: BTreeMap::new(),
            checks: BTreeSet::new(),
            ttoc: Vec::new(),
            states: Vec::new(),
            source: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }
}

impl Form {
    pub fn amount(&self, key: &str) -> Decimal {
        self.amounts.get(key).copied().unwrap_or_default()
    }

    /// Set a box amount (rounded to cents); zero removes it.
    pub fn set_amount(&mut self, key: &str, value: Decimal) {
        let v = value.round_dp(2);
        if v.is_zero() {
            self.amounts.remove(key);
        } else {
            self.amounts.insert(key.to_owned(), v);
        }
    }

    pub fn checked(&self, key: &str) -> bool {
        self.checks.contains(key)
    }

    pub fn set_check(&mut self, key: &str, on: bool) {
        if on {
            self.checks.insert(key.to_owned());
        } else {
            self.checks.remove(key);
        }
    }

    /// The total of the income boxes (withholding and boxes that are parts of
    /// another box are left out), for lists.
    pub fn income_total(&self) -> Decimal {
        crate::boxes::layout(self.kind)
            .iter()
            .filter(|b| b.counts_in_total())
            .map(|b| self.amount(b.key))
            .fold(Decimal::ZERO, |s, a| s.checked_add(a).unwrap_or(s))
    }

    pub fn is_filed(&self) -> bool {
        self.status == Status::Filed
    }
}
