//! Writes sample recipient copies (NEC and MISC) to the path given, for a visual check.
#![allow(clippy::unwrap_used, clippy::print_stdout)]

use std::str::FromStr;

use form1099keep::Decimal;
use form1099keep::model::{Address, Form, FormKind, NameType, Payer, Recipient, StateRow, TinType};
use form1099keep::pdf::{self, Copy, Item, Options};

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "sample-1099.pdf".into());
    let payer = Payer {
        name: "Grace Community Church".into(),
        name2: "Finance Office".into(),
        tin: "123456789".into(),
        address: Address {
            line1: "100 Church Street".into(),
            city: "Akron".into(),
            state: "OH".into(),
            zip: "443011234".into(),
            ..Address::default()
        },
        phone: "(330) 555-0100".into(),
        ..Payer::default()
    };
    let rec = Recipient {
        name_type: NameType::Person,
        first: "Ada".into(),
        middle: "B".into(),
        last: "Lovelace".into(),
        tin_type: Some(TinType::Ssn),
        tin: "123456789".into(),
        address: Address {
            line1: "12 Oak Avenue".into(),
            line2: "Apt 3".into(),
            city: "Cuyahoga Falls".into(),
            state: "OH".into(),
            zip: "44221".into(),
            ..Address::default()
        },
        ..Recipient::default()
    };
    let mut nec = Form::default();
    nec.set_amount("1a", d("12500"));
    nec.set_amount("1b", d("350.25"));
    nec.ttoc = vec!["101".into()];
    nec.set_amount("4", d("3000"));
    nec.account_number = "AV-2026-01".into();
    nec.states.push(StateRow {
        state: "OH".into(),
        payer_state_no: "51-123456".into(),
        tax_withheld: d("125"),
        income: d("12500"),
    });
    let mut misc = Form {
        kind: FormKind::Misc,
        corrected: true,
        ..Form::default()
    };
    misc.set_amount("1", d("24000"));
    misc.set_amount("10", d("1800"));
    misc.set_check("7", true);
    let items = [
        Item {
            form: &nec,
            payer: &payer,
            recipient: &rec,
        },
        Item {
            form: &misc,
            payer: &payer,
            recipient: &rec,
        },
    ];
    let bytes = pdf::render(
        &items,
        &Options {
            copies: vec![Copy::B],
            truncate_recipient_tin: true,
        },
    );
    std::fs::write(&out, bytes).unwrap();
    println!("wrote {out}");
}
