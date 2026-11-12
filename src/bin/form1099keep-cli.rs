//! `form1099keep-cli`: the command-line front end (form1099keep.md "Front ends").
//!
//! The book's password comes from `F1099KEEP_PASSWORD` or a prompt; a MoneyKeep
//! book's from `F1099KEEP_MK_PASSWORD` or a prompt. A MoneyKeep book that is not yet a
//! 1099 book is marked as one only after the user confirms (a prompt, or
//! `F1099KEEP_USE_AS_1099_BOOK=yes`; security-review-1 M6).
#![allow(clippy::print_stdout, clippy::large_enum_variant)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;

use clap::{Parser, Subcommand};
use form1099keep::check::{self, Severity};
use form1099keep::jobs::{self, Year};
use form1099keep::model::{Address, Form, FormKind, NameType, Payer, Recipient, StateRow, TinType};
use form1099keep::{Book, Decimal, Error, Result, import, iris, pdf};

#[derive(Parser, Debug)]
#[command(
    name = "form1099keep-cli",
    version,
    about = "Form1099Keep: Forms 1099-NEC and 1099-MISC in a MoneyKeep book"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Args, Debug, Clone)]
struct AddressArgs {
    #[arg(long)]
    line1: Option<String>,
    #[arg(long)]
    line2: Option<String>,
    #[arg(long)]
    city: Option<String>,
    #[arg(long)]
    state: Option<String>,
    #[arg(long)]
    zip: Option<String>,
    #[arg(long, default_value = "US")]
    country: String,
}

impl AddressArgs {
    fn address(&self) -> Address {
        let s = |o: &Option<String>| o.clone().unwrap_or_default();
        Address {
            line1: s(&self.line1),
            line2: s(&self.line2),
            city: s(&self.city),
            state: s(&self.state).to_ascii_uppercase(),
            zip: s(&self.zip),
            country: self.country.clone(),
        }
    }
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Create a new password-protected 1099 book.
    Init {
        book: PathBuf,
        #[arg(long, default_value = "1099s")]
        name: String,
    },
    /// Add or list payers.
    Payer {
        #[command(subcommand)]
        cmd: PayerCmd,
    },
    /// Add or list recipients.
    Recipient {
        #[command(subcommand)]
        cmd: RecipientCmd,
    },
    /// Add or list forms.
    Form {
        #[command(subcommand)]
        cmd: FormCmd,
    },
    /// Show every check for a year.
    Check {
        book: PathBuf,
        #[arg(long)]
        year: i32,
    },
    /// Import 1099 vendors and payments from a MoneyKeep book.
    Import {
        book: PathBuf,
        /// The MoneyKeep book with the vendors and payments.
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        year: i32,
        /// Payer id or name in the 1099 book.
        #[arg(long)]
        payer: String,
        /// Also import vendors below the threshold.
        #[arg(long)]
        all: bool,
    },
    /// Print recipient copies to a PDF.
    Print {
        book: PathBuf,
        #[arg(long)]
        year: i32,
        /// Form ids (default: every form of the year without errors).
        #[arg(long = "form")]
        forms: Vec<String>,
        /// Copies: B, 1, 2, C (comma separated).
        #[arg(long, default_value = "B")]
        copies: String,
        /// Show the recipient's full TIN instead of the last four digits.
        #[arg(long)]
        full_tin: bool,
        #[arg(long)]
        out: PathBuf,
    },
    /// Write the IRIS Taxpayer Portal upload files.
    IrisCsv {
        book: PathBuf,
        #[arg(long)]
        year: i32,
        #[arg(long, default_value = "nec")]
        kind: String,
        /// Only this payer (id or name).
        #[arg(long)]
        payer: Option<String>,
        /// The template downloaded from the IRIS portal (recommended).
        #[arg(long)]
        template: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Mark forms filed with the IRS.
    MarkFiled {
        book: PathBuf,
        #[arg(long)]
        year: i32,
        /// Form ids (default: every ready form of the year).
        #[arg(long = "form")]
        forms: Vec<String>,
        #[arg(long)]
        date: String,
        #[arg(long, default_value = "")]
        receipt: String,
    },
    /// Show recent changes.
    History {
        book: PathBuf,
        #[arg(long, default_value_t = 30)]
        limit: u32,
    },
}

#[derive(Subcommand, Debug)]
enum PayerCmd {
    Add {
        book: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long)]
        name2: Option<String>,
        #[arg(long)]
        tin: Option<String>,
        /// ein or ssn.
        #[arg(long, default_value = "ein")]
        tin_type: String,
        #[command(flatten)]
        address: AddressArgs,
        #[arg(long)]
        phone: Option<String>,
        #[arg(long)]
        email: Option<String>,
    },
    List {
        book: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum RecipientCmd {
    Add {
        book: PathBuf,
        /// Business name (or use --first and --last for a person).
        #[arg(long)]
        business: Option<String>,
        #[arg(long)]
        first: Option<String>,
        #[arg(long)]
        last: Option<String>,
        #[arg(long)]
        tin: Option<String>,
        /// ein, ssn, itin or atin.
        #[arg(long)]
        tin_type: Option<String>,
        #[command(flatten)]
        address: AddressArgs,
        #[arg(long)]
        email: Option<String>,
    },
    List {
        book: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum FormCmd {
    Add {
        book: PathBuf,
        #[arg(long)]
        payer: String,
        #[arg(long)]
        recipient: String,
        #[arg(long, default_value = "nec")]
        kind: String,
        #[arg(long)]
        year: i32,
        /// Box amounts as KEY=AMOUNT, e.g. --box 1a=2500.00 --box 4=0
        #[arg(long = "box")]
        boxes: Vec<String>,
        /// Check boxes to set (2, 7, fatca).
        #[arg(long = "check")]
        checks: Vec<String>,
        /// State line as ST,STATE-NO,WITHHELD,INCOME
        #[arg(long = "state")]
        states: Vec<String>,
        #[arg(long)]
        account: Option<String>,
    },
    List {
        book: PathBuf,
        #[arg(long)]
        year: i32,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn password(var: &str, prompt: &str) -> Result<String> {
    if let Ok(p) = std::env::var(var) {
        return Ok(p);
    }
    rpassword::prompt_password(prompt)
        .map_err(|e| Error::invalid(format!("cannot read the password: {e}")))
}

fn open(path: &Path) -> Result<Book> {
    let mut book = match mk_store::book_kind(path)? {
        mk_store::BookKind::Plain => Book::open(path, None)?,
        mk_store::BookKind::Encrypted => Book::open(
            path,
            Some(&password(
                "F1099KEEP_PASSWORD",
                "Book password or recovery key: ",
            )?),
        )?,
    };
    // security-review-1 M6: Form1099Keep's marker is written only after the user
    // confirms opening a MoneyKeep book as a 1099 book.
    if !book.is_1099_book()? {
        if !confirm_use_as_1099_book(path)? {
            return Err(Error::Rejected(format!(
                "{} is not a 1099 book; nothing was changed",
                path.display()
            )));
        }
        book.mark()?;
    }
    Ok(book)
}

/// `F1099KEEP_USE_AS_1099_BOOK=yes`, or a yes at the prompt.
fn confirm_use_as_1099_book(path: &Path) -> Result<bool> {
    if let Ok(v) = std::env::var("F1099KEEP_USE_AS_1099_BOOK") {
        return Ok(v.trim().eq_ignore_ascii_case("yes"));
    }
    eprint!(
        "{} is a MoneyKeep book that Form1099Keep hasn't used yet. Open it as a 1099 book? [y/N] ",
        path.display()
    );
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|e| Error::invalid(format!("cannot read the answer: {e}")))?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes" | "YES"))
}

fn dec(s: &str) -> Result<Decimal> {
    Decimal::from_str(s.trim().trim_start_matches('$').replace(',', "").as_str())
        .map_err(|_| Error::invalid(format!("{s:?} is not an amount")))
}

fn run(cli: Cli) -> Result<()> {
    match cli.cmd {
        Cmd::Init { book, name } => {
            let p1 = password("F1099KEEP_PASSWORD", "New password (8+ characters): ")?;
            if std::env::var("F1099KEEP_PASSWORD").is_err() {
                let p2 = password("F1099KEEP_PASSWORD", "Repeat the password: ")?;
                if p1 != p2 {
                    return Err(Error::invalid("the passwords differ"));
                }
            }
            let (_, key) = Book::create(&book, &name, &p1)?;
            println!("Created {}.", book.display());
            println!("Recovery key (keep it somewhere safe; it is shown only now):\n  {key}");
        }
        Cmd::Payer { cmd } => match cmd {
            PayerCmd::Add {
                book,
                name,
                name2,
                tin,
                tin_type,
                address,
                phone,
                email,
            } => {
                let mut b = open(&book)?;
                let p = b.save_payer(Payer {
                    name,
                    name2: name2.unwrap_or_default(),
                    tin: tin.unwrap_or_default(),
                    tin_type: TinType::parse(&tin_type)
                        .ok_or_else(|| Error::invalid("tin type is ein or ssn"))?,
                    address: address.address(),
                    phone: phone.unwrap_or_default(),
                    email: email.unwrap_or_default(),
                    ..Payer::default()
                })?;
                println!("{}", p.id);
            }
            PayerCmd::List { book } => {
                let mut b = open(&book)?;
                for p in b.payers()? {
                    println!(
                        "{}  {:<34} {:<12} {}",
                        p.id,
                        p.name,
                        p.masked_tin(),
                        p.address.city_line()
                    );
                }
            }
        },
        Cmd::Recipient { cmd } => match cmd {
            RecipientCmd::Add {
                book,
                business,
                first,
                last,
                tin,
                tin_type,
                address,
                email,
            } => {
                let mut b = open(&book)?;
                let person = business.is_none();
                let tt = match tin_type {
                    Some(t) => Some(
                        TinType::parse(&t)
                            .ok_or_else(|| Error::invalid("tin type is ein, ssn, itin or atin"))?,
                    ),
                    None if tin.is_some() => Some(if person { TinType::Ssn } else { TinType::Ein }),
                    None => None,
                };
                let r = b.save_recipient(Recipient {
                    name_type: if person {
                        NameType::Person
                    } else {
                        NameType::Business
                    },
                    business_name: business.unwrap_or_default(),
                    first: first.unwrap_or_default(),
                    last: last.unwrap_or_default(),
                    tin: tin.unwrap_or_default(),
                    tin_type: tt,
                    address: address.address(),
                    email: email.unwrap_or_default(),
                    ..Recipient::default()
                })?;
                println!("{}", r.id);
            }
            RecipientCmd::List { book } => {
                let mut b = open(&book)?;
                for r in b.recipients()? {
                    println!(
                        "{}  {:<34} {:<12} {}",
                        r.id,
                        r.display_name(),
                        r.masked_tin(),
                        r.address.city_line()
                    );
                }
            }
        },
        Cmd::Form { cmd } => match cmd {
            FormCmd::Add {
                book,
                payer,
                recipient,
                kind,
                year,
                boxes,
                checks,
                states,
                account,
            } => {
                let mut b = open(&book)?;
                let kind =
                    FormKind::parse(&kind).ok_or_else(|| Error::invalid("kind is nec or misc"))?;
                let mut f = Form {
                    payer: b.find_payer(&payer)?.id,
                    recipient: b.find_recipient(&recipient)?.id,
                    tax_year: year,
                    kind,
                    account_number: account.unwrap_or_default(),
                    ..Form::default()
                };
                for kv in boxes {
                    let (k, v) = kv
                        .split_once('=')
                        .ok_or_else(|| Error::invalid(format!("{kv:?}: use KEY=AMOUNT")))?;
                    let k = k.trim().to_ascii_lowercase();
                    if k == "1c" || k == "13b" {
                        f.ttoc = v.split(',').map(|c| c.trim().to_owned()).collect();
                    } else {
                        f.set_amount(&k, dec(v)?);
                    }
                }
                for c in checks {
                    f.set_check(c.trim(), true);
                }
                for s in states {
                    let parts: Vec<&str> = s.split(',').collect();
                    let get = |i: usize| parts.get(i).copied().unwrap_or_default();
                    f.states.push(StateRow {
                        state: get(0).trim().to_ascii_uppercase(),
                        payer_state_no: get(1).trim().to_owned(),
                        tax_withheld: if get(2).trim().is_empty() {
                            Decimal::ZERO
                        } else {
                            dec(get(2))?
                        },
                        income: if get(3).trim().is_empty() {
                            Decimal::ZERO
                        } else {
                            dec(get(3))?
                        },
                    });
                }
                let f = b.save_form(f)?;
                println!("{}", f.id);
            }
            FormCmd::List { book, year } => {
                let mut b = open(&book)?;
                let y = Year::load(&mut b, year)?;
                for f in &y.forms {
                    let errors = check::for_form(&y.findings, f)
                        .iter()
                        .filter(|x| x.severity == Severity::Error)
                        .count();
                    println!(
                        "{}  {:<9} {:<34} {:>14}  {:<6} {}",
                        f.id,
                        f.kind.label(),
                        y.label(f).split(" — ").next().unwrap_or_default(),
                        check::money(f.income_total()),
                        f.status.label(),
                        if errors == 0 {
                            String::new()
                        } else {
                            format!("{errors} to answer")
                        }
                    );
                }
            }
        },
        Cmd::Check { book, year } => {
            let mut b = open(&book)?;
            let y = Year::load(&mut b, year)?;
            if y.findings.is_empty() {
                println!("No checks to answer for {year}.");
            }
            for x in &y.findings {
                println!(
                    "{:<7} {:<16} {}",
                    format!("{:?}", x.severity).to_lowercase(),
                    x.field,
                    x.message
                );
            }
        }
        Cmd::Import {
            book,
            from,
            year,
            payer,
            all,
        } => {
            let mut b = open(&book)?;
            let payer = b.find_payer(&payer)?.id;
            let mk_password = match mk_store::book_kind(&from)? {
                mk_store::BookKind::Encrypted => Some(password(
                    "F1099KEEP_MK_PASSWORD",
                    "MoneyKeep book password: ",
                )?),
                mk_store::BookKind::Plain => None,
            };
            let r = import::import_moneykeep(
                &mut b,
                &import::Options {
                    path: from,
                    password: mk_password,
                    year,
                    payer,
                    include_below_threshold: all,
                },
            )?;
            println!("{}", r.summary());
            for n in &r.notes {
                println!("• {n}");
            }
            for (k, (mk, us)) in &r.threshold_differences {
                println!("  {k}: MoneyKeep {mk}, IRS {us}");
            }
        }
        Cmd::Print {
            book,
            year,
            forms,
            copies,
            full_tin,
            out,
        } => {
            let mut b = open(&book)?;
            let y = Year::load(&mut b, year)?;
            let copies = copies
                .split(',')
                .map(|c| {
                    pdf::Copy::parse(c)
                        .ok_or_else(|| Error::invalid(format!("copy {c:?}: use B, 1, 2 or C")))
                })
                .collect::<Result<Vec<_>>>()?;
            let p = jobs::print(
                &y,
                &forms,
                &pdf::Options {
                    copies,
                    truncate_recipient_tin: !full_tin,
                },
            )?;
            std::fs::write(&out, &p.pdf).map_err(|e| Error::io("write", &out, &e))?;
            println!(
                "Wrote {} ({} forms, {} pages).",
                out.display(),
                p.forms,
                p.pages
            );
            for s in &p.skipped {
                println!("  skipped {}: {}", s.label, s.reason);
            }
        }
        Cmd::IrisCsv {
            book,
            year,
            kind,
            payer,
            template,
            out,
        } => {
            let mut b = open(&book)?;
            let kind =
                FormKind::parse(&kind).ok_or_else(|| Error::invalid("kind is nec or misc"))?;
            let payer = payer.map(|p| b.find_payer(&p).map(|p| p.id)).transpose()?;
            let y = Year::load(&mut b, year)?;
            let tpl = template.as_deref().map(iris::read_template).transpose()?;
            let r = jobs::iris_files(&y, kind, payer.as_deref(), tpl)?;
            for p in iris::write_files(&out, &r.built.files)? {
                println!("Wrote {}", p.display());
            }
            println!("{} form(s).", r.rows);
            if r.builtin_headers {
                println!(
                    "Note: built with Form1099Keep's draft header list. The IRS accepts only its own \
                     template: download it from the IRIS portal and run again with --template."
                );
            }
            for h in &r.built.unmapped {
                println!("  left blank (unknown column): {h}");
            }
            for h in &r.built.unconfirmed {
                println!("  check this column's value: {h}");
            }
            for s in &r.skipped {
                println!("  skipped {}: {}", s.label, s.reason);
            }
        }
        Cmd::MarkFiled {
            book,
            year,
            forms,
            date,
            receipt,
        } => {
            let mut b = open(&book)?;
            let ids: Vec<String> = if forms.is_empty() {
                b.forms(Some(year))?
                    .into_iter()
                    .filter(|f| f.status == form1099keep::Status::Ready)
                    .map(|f| f.id)
                    .collect()
            } else {
                forms
            };
            let n = b.mark_filed(&ids, &date, &receipt)?;
            println!("Marked {n} form(s) filed.");
        }
        Cmd::History { book, limit } => {
            let mut b = open(&book)?;
            for h in b.history(limit)? {
                println!(
                    "{:>5}  {}  {:<28} {}",
                    h.seq,
                    h.at.get(..19).unwrap_or_default(),
                    h.command,
                    h.summary
                );
            }
        }
    }
    Ok(())
}
