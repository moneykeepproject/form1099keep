//! End to end: a MoneyKeep book with 1099 vendors, bills and payments → a 1099 book
//! → checks → recipient copies → IRIS file.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::str::FromStr;

use form1099keep::check::Severity;
use form1099keep::jobs::{self, Year};
use form1099keep::model::{Address, FormKind, NameType, Payer, TinType};
use form1099keep::{Book, Decimal, import, iris, pdf};
use mk_commands::Session;
use mk_store::KdfParams;
use serde_json::{Value, json};

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

/// A MoneyKeep book: two 1099 vendors paid by check, one paid by card (excluded),
/// one below the threshold.
fn moneykeep_book(path: &std::path::Path) {
    let mut s = Session::create(path, "Church books", "USD", "test").unwrap();
    let mut run =
        |c: &str, a: Value| -> Value { s.invoke(c, a).unwrap_or_else(|e| panic!("{c}: {e}")) };
    run("module.enable", json!({"module": "1099"}));
    let bank = run(
        "account.create",
        json!({"name": "Checking", "type": "asset", "subtype": "bank"}),
    )["id"]
        .clone();
    let card = run(
        "account.create",
        json!({"name": "Visa", "type": "liability", "subtype": "credit-card"}),
    )["id"]
        .clone();
    run(
        "account.create",
        json!({"name": "Contract labor", "type": "expense"}),
    );
    run("account.create", json!({"name": "Rent", "type": "expense"}));
    let vendor = |run: &mut dyn FnMut(&str, Value) -> Value, name: &str, extra: Value| -> String {
        let mut a = json!({"name": name, "roles": ["vendor"], "tracks_1099": true});
        if let (Some(o), Some(e)) = (a.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                o.insert(k.clone(), v.clone());
            }
        }
        run("party.create", a)["id"].as_str().unwrap().to_owned()
    };
    let organist = vendor(
        &mut run,
        "Ada Lovelace",
        json!({"billing_address": ["12 Oak Ave", "Apt 3", "Akron, OH 44301"], "email": "ada@example.org"}),
    );
    let landlord = vendor(
        &mut run,
        "Hall Rentals",
        json!({"company": "Hall Rentals LLC", "form_1099": "misc", "misc_box": "rents",
               "billing_address": ["9 Elm St", "Kent, OH 44240"]}),
    );
    let small = vendor(&mut run, "Small Jobs", json!({}));
    let carded = vendor(&mut run, "Card Paid Co", json!({}));
    let mut check_no = 1000;
    let mut bill_and_pay = |run: &mut dyn FnMut(&str, Value) -> Value,
                            v: &str,
                            acct: &str,
                            amount: &str,
                            from: &Value,
                            method: &str| {
        run(
            "bill.create",
            json!({"vendor": v, "date": "2026-03-01", "lines": [{"account": acct, "amount": amount}]}),
        );
        check_no += 1;
        let mut a = json!({"vendor": v, "date": "2026-03-15", "from_account": from, "method": method, "amount": amount});
        if method == "check" {
            a["check_number"] = json!(check_no);
        }
        run("bill.pay", a);
    };
    bill_and_pay(
        &mut run,
        &organist,
        "Contract labor",
        "2400.00",
        &bank,
        "check",
    );
    bill_and_pay(
        &mut run,
        &organist,
        "Contract labor",
        "600.50",
        &bank,
        "check",
    );
    bill_and_pay(
        &mut run,
        &landlord,
        "Rent",
        "12000.00",
        &bank,
        "bank_transfer",
    );
    bill_and_pay(&mut run, &small, "Contract labor", "500.00", &bank, "check");
    bill_and_pay(
        &mut run,
        &carded,
        "Contract labor",
        "9000.00",
        &card,
        "card",
    );
}

#[test]
fn import_check_print_and_iris() {
    let dir = tempfile::tempdir().unwrap();
    let mk = dir.path().join("church.moneykeep");
    moneykeep_book(&mk);

    let path = dir.path().join("1099s.moneykeep");
    let (mut b, _) = Book::create_with(
        &path,
        "Church 1099s",
        "long enough password",
        KdfParams::TEST,
    )
    .unwrap();
    let payer = b
        .save_payer(Payer {
            name: "Grace Chapel".into(),
            tin: "12-3456789".into(),
            address: Address {
                line1: "100 Church St".into(),
                city: "Akron".into(),
                state: "OH".into(),
                zip: "44301".into(),
                ..Address::default()
            },
            phone: "330-555-0100".into(),
            ..Payer::default()
        })
        .unwrap();

    let opts = import::Options {
        path: mk.clone(),
        password: None,
        year: 2026,
        payer: payer.id.clone(),
        include_below_threshold: false,
    };
    let r = import::import_moneykeep(&mut b, &opts).unwrap();
    assert_eq!(r.forms_created.len(), 2, "{r:?}");
    assert_eq!(r.below_threshold, vec!["Small Jobs".to_owned()]);
    assert!(r.notes.iter().any(|n| n.contains("W-9")));
    assert!(r.notes.iter().any(|n| n.contains("look like people")));

    let forms = b.forms(Some(2026)).unwrap();
    let nec = forms.iter().find(|f| f.kind == FormKind::Nec).unwrap();
    assert_eq!(nec.amount("1a"), d("3000.50"));
    let misc = forms.iter().find(|f| f.kind == FormKind::Misc).unwrap();
    assert_eq!(misc.amount("1"), d("12000.00"));
    let ada = b.recipient(&nec.recipient).unwrap();
    assert_eq!(
        (
            ada.address.city.as_str(),
            ada.address.state.as_str(),
            ada.address.line2.as_str()
        ),
        ("Akron", "OH", "Apt 3")
    );
    let hall = b.recipient(&misc.recipient).unwrap();
    assert_eq!(hall.business_name, "Hall Rentals LLC");

    // Importing again changes nothing.
    let again = import::import_moneykeep(&mut b, &opts).unwrap();
    assert!(
        again.forms_created.is_empty()
            && again.forms_updated.is_empty()
            && again.recipients_created.is_empty(),
        "{again:?}"
    );

    // TINs are missing, so nothing can be printed yet.
    let y = Year::load(&mut b, 2026).unwrap();
    assert!(
        y.findings
            .iter()
            .any(|x| x.severity == Severity::Error && x.field == "tin")
    );
    assert!(jobs::print(&y, &[], &pdf::Options::default()).is_err());

    // Answer the checks: Ada is a person with an SSN; Hall Rentals has an EIN.
    let mut ada = ada;
    ada.name_type = NameType::Person;
    ada.first = "Ada".into();
    ada.last = "Lovelace".into();
    ada.tin_type = Some(TinType::Ssn);
    ada.tin = "123-45-6789".into();
    b.save_recipient(ada).unwrap();
    let mut hall = hall;
    hall.tin_type = Some(TinType::Ein);
    hall.tin = "98-7654321".into();
    b.save_recipient(hall).unwrap();

    let y = Year::load(&mut b, 2026).unwrap();
    let errors: Vec<_> = y
        .findings
        .iter()
        .filter(|x| x.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let printed = jobs::print(&y, &[], &pdf::Options::default()).unwrap();
    assert_eq!((printed.forms, printed.pages), (2, 2));

    let files = jobs::iris_files(&y, FormKind::Nec, None, None).unwrap();
    assert!(files.builtin_headers);
    assert_eq!(files.rows, 1);
    let csv = String::from_utf8(files.built.files[0].content.clone()).unwrap();
    assert!(csv.contains("1099-NEC,2026,EIN,123456789,B,Grace Chapel"));
    assert!(csv.contains(",SSN,123456789,P,,,Ada,,Lovelace,"));
    assert!(csv.contains(",3000.50,"));
    let out = dir.path().join("iris");
    let written = iris::write_files(&out, &files.built.files).unwrap();
    assert_eq!(written.len(), 1);

    // The MoneyKeep book still opens in MoneyKeep, and so does the 1099 book.
    drop(b);
    let mut s = Session::open(&mk, "test").unwrap();
    assert!(s.invoke("report.1099", json!({"year": 2026})).is_ok());
    let mut s2 = Session::open_with(
        &path,
        "test",
        Some(&mk_store::Secret::Password("long enough password".into())),
    )
    .unwrap();
    let hist = s2
        .invoke("history.list", json!({}))
        .or_else(|_| s2.invoke("audit.list", json!({})));
    assert!(
        hist.is_ok(),
        "MoneyKeep cannot read the 1099 book's history: {hist:?}"
    );
}
