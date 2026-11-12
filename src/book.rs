//! A 1099 book: a MoneyKeep book holding payers, recipients and forms.

use std::path::{Path, PathBuf};

use mk_core::{CommodityCode, Decimal};
use mk_store::{AuditRecord, Doc, DocTable, KdfParams, Secret, Store, StoreError, WriteTx};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::boxes::Thresholds;
use crate::model::{Filed, Form, Payer, Recipient, Status};
use crate::{Error, Result, tin};

/// The audit-log actor for every change Form1099Keep makes.
pub const ACTOR: &str = "form1099keep";
/// Book setting (`form1099keep / book_kind`) that marks a 1099 book.
pub const BOOK_KIND: &str = "form1099";
const MODULE: &str = "form1099keep";

/// One History row.
#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    pub seq: i64,
    pub at: String,
    pub actor: String,
    pub command: String,
    pub summary: String,
}

/// An open 1099 book.
#[derive(Debug)]
pub struct Book {
    store: Store,
    path: Option<PathBuf>,
    kdf: KdfParams,
}

impl Book {
    /// Create a new password-protected 1099 book. Returns the book and its
    /// recovery key (shown once).
    pub fn create(path: &Path, name: &str, password: &str) -> Result<(Self, String)> {
        Self::create_with(path, name, password, KdfParams::DEFAULT)
    }

    /// As [`Book::create`] with chosen key-derivation parameters (tests).
    pub fn create_with(
        path: &Path,
        name: &str,
        password: &str,
        kdf: KdfParams,
    ) -> Result<(Self, String)> {
        let usd = CommodityCode::new("USD")?;
        let (store, key) = Store::create_encrypted(path, name.trim(), &usd, password, true, kdf)?;
        let mut book = Self {
            store,
            path: Some(path.to_path_buf()),
            kdf,
        };
        book.write(
            "book.create",
            json!({"name": name.trim(), "encrypted": true}),
            |tx| {
                tx.set_setting(MODULE, "book_kind", &json!(BOOK_KIND), ACTOR)?;
                Ok(json!({}))
            },
        )?;
        Ok((book, key.unwrap_or_default()))
    }

    /// Open a book; `password` (or a recovery key) is needed when it is encrypted.
    ///
    /// Form1099Keep never updates a book's format (security-review-1 M6): a book in an
    /// older MoneyKeep format is refused before anything in its file changes
    /// ([`Error::BookTooOld`]), and one in a newer format with [`Error::BookTooNew`].
    /// Opening writes nothing: a MoneyKeep book that is not yet a 1099 book
    /// ([`Book::is_1099_book`]) is marked only by [`Book::mark`], after the user
    /// confirms.
    pub fn open(path: &Path, password: Option<&str>) -> Result<Self> {
        let secret = password.map(secret_of);
        let store = Store::open_with_hook(path, secret.as_ref(), |_| Err(Error::BookTooOld))
            .map_err(|e| match e {
                Error::Store(StoreError::TooNew { .. }) => Error::BookTooNew,
                other => other,
            })?;
        Ok(Self {
            store,
            path: Some(path.to_path_buf()),
            kdf: KdfParams::DEFAULT,
        })
    }

    /// Whether the book carries Form1099Keep's marker (`form1099keep / book_kind`).
    pub fn is_1099_book(&self) -> Result<bool> {
        Ok(self.store.get_setting(MODULE, "book_kind")?.is_some())
    }

    /// Mark the book as a 1099 book, once the user has confirmed opening it as one
    /// (security-review-1 M6). The book is at the format this build understands, since
    /// [`Book::open`] refuses any other. Nothing is written when it is marked already.
    pub fn mark(&mut self) -> Result<()> {
        if self.is_1099_book()? {
            return Ok(());
        }
        self.write("book.mark", json!({}), |tx| {
            tx.set_setting(MODULE, "book_kind", &json!(BOOK_KIND), ACTOR)?;
            Ok(json!({}))
        })?;
        Ok(())
    }

    /// An in-memory book (tests and previews). Nothing reaches a disk, so it takes
    /// TINs without a password.
    pub fn in_memory(name: &str) -> Result<Self> {
        let usd = CommodityCode::new("USD")?;
        Ok(Self {
            store: Store::in_memory(name, &usd)?,
            path: None,
            kdf: KdfParams::TEST,
        })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn name(&self) -> String {
        self.store.info().map(|i| i.name).unwrap_or_default()
    }

    pub fn id(&self) -> String {
        self.store.info().map(|i| i.id).unwrap_or_default()
    }

    pub fn is_encrypted(&self) -> bool {
        self.store.is_encrypted()
    }

    /// Protect a plain book with a password, in place. Returns the recovery key.
    pub fn protect(&mut self, password: &str) -> Result<String> {
        let key = self.store.encrypt(password, true, self.kdf, ACTOR)?;
        Ok(key.unwrap_or_default())
    }

    /// Change the password (the current password or the recovery key).
    pub fn change_password(&mut self, current: &str, new: &str) -> Result<()> {
        self.store
            .change_password(&secret_of(current), new, self.kdf, ACTOR)?;
        Ok(())
    }

    // ----- reading -------------------------------------------------------------------

    pub fn payers(&mut self) -> Result<Vec<Payer>> {
        let mut v: Vec<Payer> = self.load(DocTable::F1099Payer, None)?;
        v.sort_by_key(|p| p.name.to_lowercase());
        Ok(v)
    }

    pub fn recipients(&mut self) -> Result<Vec<Recipient>> {
        let mut v: Vec<Recipient> = self.load(DocTable::F1099Recipient, None)?;
        v.sort_by_key(Recipient::sort_key);
        Ok(v)
    }

    /// Forms of one tax year, or all.
    pub fn forms(&mut self, year: Option<i32>) -> Result<Vec<Form>> {
        let grp = year.map(|y| y.to_string());
        self.load(DocTable::F1099Form, grp.as_deref())
    }

    pub fn payer(&mut self, id: &str) -> Result<Payer> {
        self.get(DocTable::F1099Payer, id, "payer")
    }

    pub fn recipient(&mut self, id: &str) -> Result<Recipient> {
        self.get(DocTable::F1099Recipient, id, "recipient")
    }

    pub fn form(&mut self, id: &str) -> Result<Form> {
        self.get(DocTable::F1099Form, id, "form")
    }

    /// Tax years that have forms, newest first.
    pub fn years(&mut self) -> Result<Vec<i32>> {
        let mut v: Vec<i32> = self.forms(None)?.iter().map(|f| f.tax_year).collect();
        v.sort_unstable_by(|a, b| b.cmp(a));
        v.dedup();
        Ok(v)
    }

    /// Thresholds for a year: the 2026 amounts with the book's overrides
    /// (setting `form1099keep / thresholds`: `{"2027": {"nec:1a": "2050.00"}}`).
    pub fn thresholds(&mut self, year: i32) -> Result<Thresholds> {
        let mut t = Thresholds::defaults(year);
        if let Some(v) = self.store.get_setting(MODULE, "thresholds")?
            && let Some(map) = v.get(year.to_string()).and_then(Value::as_object)
        {
            for (k, amount) in map {
                if let Some(d) = amount.as_str().and_then(|s| s.parse::<Decimal>().ok()) {
                    t.amounts.insert(k.clone(), d);
                    t.copied_from_2026 = false;
                }
            }
        }
        Ok(t)
    }

    /// Set threshold overrides for one year (replacing that year's overrides).
    pub fn set_thresholds(
        &mut self,
        year: i32,
        overrides: &std::collections::BTreeMap<String, Decimal>,
    ) -> Result<()> {
        let mut all = self
            .store
            .get_setting(MODULE, "thresholds")?
            .unwrap_or_else(|| json!({}));
        let entry: serde_json::Map<String, Value> = overrides
            .iter()
            .map(|(k, v)| (k.clone(), json!(v.to_string())))
            .collect();
        if let Some(o) = all.as_object_mut() {
            o.insert(year.to_string(), Value::Object(entry));
        }
        self.write(
            "f1099.thresholds.set",
            json!({"year": year, "thresholds": all.get(year.to_string())}),
            |tx| {
                tx.set_setting(MODULE, "thresholds", &all, ACTOR)?;
                Ok(json!({}))
            },
        )
    }

    /// Recent audit entries, newest first.
    pub fn history(&mut self, limit: u32) -> Result<Vec<HistoryEntry>> {
        let entries = self.store.audit_entries(limit)?;
        Ok(entries
            .into_iter()
            .map(|e| HistoryEntry {
                summary: summarise(&e.command, &e.args),
                seq: e.seq,
                at: e.at,
                actor: e.actor,
                command: e.command,
            })
            .collect())
    }

    /// A payer by id, or by name (case-insensitive) when exactly one matches.
    pub fn find_payer(&mut self, key: &str) -> Result<Payer> {
        let all = self.payers()?;
        pick(all, key, "payer", |p| p.id.clone(), |p| p.name.clone())
    }

    /// A recipient by id, or by name (case-insensitive) when exactly one matches.
    pub fn find_recipient(&mut self, key: &str) -> Result<Recipient> {
        let all = self.recipients()?;
        pick(
            all,
            key,
            "recipient",
            |r| r.id.clone(),
            Recipient::display_name,
        )
    }

    // ----- changing --------------------------------------------------------------------

    /// Create (empty id) or update a payer. Returns the stored payer.
    pub fn save_payer(&mut self, mut p: Payer) -> Result<Payer> {
        p.name = p.name.trim().to_owned();
        if p.name.is_empty() {
            return Err(Error::invalid("a payer needs a name"));
        }
        p.tin = normalise_tin(&p.tin)?;
        self.require_password_for(&p.tin)?;
        let creating = p.id.is_empty();
        if creating {
            p.id = new_id();
            p.created_at = now();
        } else {
            self.payer(&p.id)?;
        }
        let verb = if creating { "create" } else { "update" };
        let audit = json!(p.redacted());
        let stored = p.clone();
        self.write(&format!("f1099.payer.{verb}"), audit, |tx| {
            put(tx, DocTable::F1099Payer, &stored.id, None, &stored)?;
            Ok(json!({"id": stored.id}))
        })?;
        Ok(p)
    }

    pub fn delete_payer(&mut self, id: &str) -> Result<()> {
        let p = self.payer(id)?;
        if self.forms(None)?.iter().any(|f| f.payer == id) {
            return Err(Error::Rejected(format!(
                "{} has forms; delete or move them first",
                p.name
            )));
        }
        self.write(
            "f1099.payer.delete",
            json!({"id": id, "name": p.name}),
            |tx| {
                tx.delete_doc(DocTable::F1099Payer, id)?;
                Ok(json!({}))
            },
        )
    }

    /// Create (empty id) or update a recipient.
    pub fn save_recipient(&mut self, mut r: Recipient) -> Result<Recipient> {
        if r.display_name().is_empty() {
            return Err(Error::invalid("a recipient needs a name"));
        }
        r.tin = normalise_tin(&r.tin)?;
        self.require_password_for(&r.tin)?;
        let creating = r.id.is_empty();
        if creating {
            r.id = new_id();
            r.created_at = now();
        } else {
            self.recipient(&r.id)?;
        }
        let verb = if creating { "create" } else { "update" };
        let stored = r.clone();
        self.write(
            &format!("f1099.recipient.{verb}"),
            json!(r.redacted()),
            |tx| {
                put(tx, DocTable::F1099Recipient, &stored.id, None, &stored)?;
                Ok(json!({"id": stored.id}))
            },
        )?;
        Ok(r)
    }

    pub fn delete_recipient(&mut self, id: &str) -> Result<()> {
        let r = self.recipient(id)?;
        if self.forms(None)?.iter().any(|f| f.recipient == id) {
            return Err(Error::Rejected(format!(
                "{} has forms; delete them first",
                r.display_name()
            )));
        }
        self.write(
            "f1099.recipient.delete",
            json!({"id": id, "name": r.display_name()}),
            |tx| {
                tx.delete_doc(DocTable::F1099Recipient, id)?;
                Ok(json!({}))
            },
        )
    }

    /// Create (empty id) or update a form. A filed form cannot be changed (use
    /// [`Book::correct`]); its status cannot be set to filed here (use
    /// [`Book::mark_filed`]).
    pub fn save_form(&mut self, mut f: Form) -> Result<Form> {
        if f.tax_year < crate::FIRST_YEAR || f.tax_year > 2100 {
            return Err(Error::invalid(format!(
                "this version makes forms for tax year {} and later",
                crate::FIRST_YEAR
            )));
        }
        self.payer(&f.payer)?;
        self.recipient(&f.recipient)?;
        for (k, v) in &f.amounts {
            if v.is_sign_negative() {
                return Err(Error::invalid(format!("box {k} cannot be negative")));
            }
            if crate::boxes::find(f.kind, k).is_none() {
                return Err(Error::invalid(format!("{} has no box {k}", f.kind.label())));
            }
        }
        f.amounts.retain(|_, v| !v.is_zero());
        for v in f.amounts.values_mut() {
            *v = v.round_dp(2);
        }
        f.ttoc.retain(|c| !c.trim().is_empty());
        f.ttoc.truncate(2);
        let creating = f.id.is_empty();
        if creating {
            f.id = new_id();
            f.created_at = now();
            if f.status == Status::Filed {
                f.status = Status::Draft;
            }
        } else {
            let old = self.form(&f.id)?;
            if old.is_filed() {
                return Err(Error::Rejected(
                    "this form is filed; choose Correct to change it".into(),
                ));
            }
            if f.status == Status::Filed {
                f.status = old.status;
            }
            f.created_at = old.created_at;
        }
        f.updated_at = now();
        let verb = if creating { "create" } else { "update" };
        let stored = f.clone();
        self.write(&format!("f1099.form.{verb}"), json!(f), |tx| {
            put(
                tx,
                DocTable::F1099Form,
                &stored.id,
                Some(stored.tax_year.to_string()),
                &stored,
            )?;
            Ok(json!({"id": stored.id}))
        })?;
        Ok(f)
    }

    pub fn delete_form(&mut self, id: &str) -> Result<()> {
        let f = self.form(id)?;
        if f.is_filed() {
            return Err(Error::Rejected("a filed form cannot be deleted".into()));
        }
        self.write("f1099.form.delete", json!({"id": id}), |tx| {
            tx.delete_doc(DocTable::F1099Form, id)?;
            Ok(json!({}))
        })
    }

    /// Set draft or ready on forms that are not filed.
    pub fn set_status(&mut self, ids: &[String], status: Status) -> Result<usize> {
        if status == Status::Filed {
            return Err(Error::invalid("use mark_filed to mark forms filed"));
        }
        self.update_forms(
            "f1099.form.set_status",
            ids,
            json!({"status": status}),
            |f| {
                if f.is_filed() {
                    return false;
                }
                f.status = status;
                true
            },
        )
    }

    /// Mark forms filed with the IRS (date, IRIS receipt id).
    pub fn mark_filed(&mut self, ids: &[String], date: &str, receipt_id: &str) -> Result<usize> {
        let filed = Filed {
            date: date.trim().to_owned(),
            receipt_id: receipt_id.trim().to_owned(),
        };
        self.update_forms(
            "f1099.form.mark_filed",
            ids,
            json!({"date": filed.date, "receipt_id": filed.receipt_id}),
            |f| {
                if f.is_filed() {
                    return false;
                }
                f.status = Status::Filed;
                f.filed = Some(filed.clone());
                true
            },
        )
    }

    /// Reopen a filed form as a correction: it becomes a draft marked CORRECTED,
    /// and the earlier filing is kept in the notes.
    pub fn correct(&mut self, id: &str) -> Result<Form> {
        let mut f = self.form(id)?;
        if !f.is_filed() {
            return Err(Error::Rejected("only a filed form is corrected".into()));
        }
        if let Some(prev) = f.filed.take() {
            let line = format!(
                "Filed {}{}; reopened as a correction {}.",
                prev.date,
                if prev.receipt_id.is_empty() {
                    String::new()
                } else {
                    format!(" (receipt {})", prev.receipt_id)
                },
                now().get(..10).unwrap_or_default()
            );
            f.notes = if f.notes.trim().is_empty() {
                line
            } else {
                format!("{}\n{line}", f.notes.trim_end())
            };
        }
        f.status = Status::Draft;
        f.corrected = true;
        f.updated_at = now();
        let stored = f.clone();
        self.write("f1099.form.correct", json!({"id": id}), |tx| {
            put(
                tx,
                DocTable::F1099Form,
                &stored.id,
                Some(stored.tax_year.to_string()),
                &stored,
            )?;
            Ok(json!({}))
        })?;
        Ok(f)
    }

    /// Store imported recipients and forms in one write, with one audit entry.
    pub(crate) fn import_batch(
        &mut self,
        recipients: &[Recipient],
        forms: &[Form],
        summary: Value,
    ) -> Result<()> {
        if recipients.is_empty() && forms.is_empty() {
            return Ok(());
        }
        for f in forms {
            if f.tax_year < crate::FIRST_YEAR {
                return Err(Error::invalid(format!(
                    "this version makes forms for tax year {} and later",
                    crate::FIRST_YEAR
                )));
            }
        }
        self.write("f1099.import_moneykeep", summary, |tx| {
            for r in recipients {
                put(tx, DocTable::F1099Recipient, &r.id, None, r)?;
            }
            for f in forms {
                put(
                    tx,
                    DocTable::F1099Form,
                    &f.id,
                    Some(f.tax_year.to_string()),
                    f,
                )?;
            }
            Ok(json!({"recipients": recipients.len(), "forms": forms.len()}))
        })
    }

    // ----- internals -------------------------------------------------------------------

    /// Store several changed forms in one write (one audit entry).
    fn update_forms(
        &mut self,
        command: &str,
        ids: &[String],
        args: Value,
        mut change: impl FnMut(&mut Form) -> bool,
    ) -> Result<usize> {
        let mut changed = Vec::new();
        for id in ids {
            let mut f = self.form(id)?;
            if change(&mut f) {
                f.updated_at = now();
                changed.push(f);
            }
        }
        if changed.is_empty() {
            return Ok(0);
        }
        let n = changed.len();
        let mut a = args;
        if let Some(o) = a.as_object_mut() {
            o.insert(
                "ids".into(),
                json!(changed.iter().map(|f| f.id.clone()).collect::<Vec<_>>()),
            );
        }
        self.write(command, a, |tx| {
            for f in &changed {
                put(
                    tx,
                    DocTable::F1099Form,
                    &f.id,
                    Some(f.tax_year.to_string()),
                    f,
                )?;
            }
            Ok(json!({"changed": n}))
        })?;
        Ok(n)
    }

    /// Run `f` in one write with its audit entry (no TINs, no snapshots).
    pub(crate) fn write(
        &mut self,
        command: &str,
        args: Value,
        f: impl FnOnce(&WriteTx<'_>) -> Result<Value>,
    ) -> Result<()> {
        self.store.write(|tx| -> Result<()> {
            let result = f(tx)?;
            tx.append_audit_record(&AuditRecord {
                actor: ACTOR,
                command,
                args,
                result,
                ..AuditRecord::default()
            })?;
            Ok(())
        })
    }

    fn load<T: DeserializeOwned>(&mut self, t: DocTable, grp: Option<&str>) -> Result<Vec<T>> {
        self.store.read(|tx| -> Result<Vec<T>> {
            tx.docs(t, grp)?
                .into_iter()
                .map(|d| serde_json::from_value(d.json).map_err(Error::from))
                .collect()
        })
    }

    fn get<T: DeserializeOwned>(&mut self, t: DocTable, id: &str, what: &'static str) -> Result<T> {
        let doc = self
            .store
            .read(|tx| -> Result<Option<Doc>> { Ok(tx.get_doc(t, id)?) })?;
        let doc = doc.ok_or_else(|| Error::NotFound {
            what,
            id: id.to_owned(),
        })?;
        Ok(serde_json::from_value(doc.json)?)
    }

    fn require_password_for(&self, tin: &str) -> Result<()> {
        if !tin.is_empty() && !self.store.is_encrypted() && self.path.is_some() {
            return Err(Error::TinRequiresPassword);
        }
        Ok(())
    }
}

fn pick<T>(
    all: Vec<T>,
    key: &str,
    what: &'static str,
    id: impl Fn(&T) -> String,
    name: impl Fn(&T) -> String,
) -> Result<T> {
    let k = key.trim();
    let mut by_name = Vec::new();
    for x in all {
        if id(&x) == k {
            return Ok(x);
        }
        if name(&x).eq_ignore_ascii_case(k) {
            by_name.push(x);
        }
    }
    match by_name.len() {
        1 => by_name.pop().ok_or_else(|| Error::NotFound {
            what,
            id: k.to_owned(),
        }),
        0 => Err(Error::NotFound {
            what,
            id: k.to_owned(),
        }),
        n => Err(Error::invalid(format!(
            "{n} {what}s are named {k:?}; use the id instead"
        ))),
    }
}

fn put<T: Serialize>(
    tx: &WriteTx<'_>,
    t: DocTable,
    id: &str,
    grp: Option<String>,
    v: &T,
) -> Result<()> {
    tx.put_doc(
        t,
        &Doc {
            id: id.to_owned(),
            grp,
            ord: 0,
            json: serde_json::to_value(v)?,
        },
    )?;
    Ok(())
}

/// A password or, when it looks like one (`MCRK-…`), a recovery key.
pub(crate) fn secret_of(s: &str) -> Secret {
    let t = s.trim();
    if t.len() > 5 && t.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("MCRK-")) {
        Secret::RecoveryKey(t.to_owned())
    } else {
        Secret::Password(s.to_owned())
    }
}

fn normalise_tin(t: &str) -> Result<String> {
    tin::normalise(t).ok_or_else(|| {
        Error::invalid("a taxpayer id number has only digits (hyphens and spaces are fine)")
    })
}

pub(crate) fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

pub(crate) fn now() -> String {
    mk_store::now_rfc3339().unwrap_or_default()
}

/// One line for History.
fn summarise(command: &str, args: &Value) -> String {
    let s = |k: &str| {
        args.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    match command {
        "f1099.payer.create" | "f1099.payer.update" => format!("Payer {}", s("name")),
        "f1099.payer.delete" => format!("Deleted payer {}", s("name")),
        "f1099.recipient.create" | "f1099.recipient.update" => {
            let name = if s("name_type") == "person" {
                format!("{} {}", s("first"), s("last"))
            } else {
                s("business_name")
            };
            format!("Recipient {}", name.trim())
        }
        "f1099.recipient.delete" => format!("Deleted recipient {}", s("name")),
        "f1099.form.create" | "f1099.form.update" => {
            let kind = args
                .get("kind")
                .and_then(Value::as_str)
                .and_then(crate::FormKind::parse)
                .map(|k| k.label())
                .unwrap_or("Form");
            format!(
                "{kind} for {}",
                args.get("tax_year")
                    .and_then(Value::as_i64)
                    .unwrap_or_default()
            )
        }
        "f1099.form.mark_filed" => format!(
            "Marked {} form(s) filed",
            args.get("ids")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        ),
        "f1099.import_moneykeep" => format!("Imported from {}", s("book")),
        _ => String::new(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::model::{FormKind, NameType, TinType};
    use std::str::FromStr;

    fn new_book(dir: &Path) -> Book {
        Book::create_with(
            &dir.join("t.moneykeep"),
            "Test 1099s",
            "correct horse battery",
            KdfParams::TEST,
        )
        .unwrap()
        .0
    }

    fn payer() -> Payer {
        Payer {
            name: "Grace Chapel".into(),
            tin: "12-3456789".into(),
            phone: "555-0100".into(),
            ..Payer::default()
        }
    }

    fn recipient() -> Recipient {
        Recipient {
            name_type: NameType::Person,
            first: "Ada".into(),
            last: "Lovelace".into(),
            tin_type: Some(TinType::Ssn),
            tin: "123-45-6789".into(),
            ..Recipient::default()
        }
    }

    #[test]
    fn create_save_reopen_and_no_tins_in_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.moneykeep");
        let (mut b, key) = Book::create_with(
            &path,
            "Test 1099s",
            "correct horse battery",
            KdfParams::TEST,
        )
        .unwrap();
        assert!(key.starts_with("MCRK-"));
        assert!(b.is_encrypted());
        let p = b.save_payer(payer()).unwrap();
        assert_eq!(p.tin, "123456789");
        let r = b.save_recipient(recipient()).unwrap();
        let mut f = Form {
            payer: p.id.clone(),
            recipient: r.id.clone(),
            ..Form::default()
        };
        f.set_amount("1a", Decimal::from_str("2500.505").unwrap());
        let f = b.save_form(f).unwrap();
        assert_eq!(f.amount("1a"), Decimal::from_str("2500.50").unwrap());
        drop(b);

        // Wrong password, then right one; then the recovery key.
        assert!(Book::open(&path, Some("nope nope nope")).is_err());
        let mut b = Book::open(&path, Some("correct horse battery")).unwrap();
        assert_eq!(b.forms(Some(2026)).unwrap().len(), 1);
        assert_eq!(b.recipient(&r.id).unwrap().tin, "123456789");
        let log = serde_json::to_string(&b.store.audit_entries(100).unwrap()).unwrap();
        assert!(!log.contains("123456789"), "a TIN reached the audit log");
        assert!(log.contains("***-**-6789"));
        drop(b);
        let mut b = Book::open(&path, Some(&key)).unwrap();
        assert_eq!(b.payers().unwrap().len(), 1);
        assert_eq!(
            b.history(50).unwrap().first().unwrap().command,
            "f1099.form.create"
        );
    }

    #[test]
    fn plain_book_refuses_tins() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plain.moneykeep");
        drop(Store::create(&path, "Plain", &CommodityCode::new("USD").unwrap()).unwrap());
        let mut b = Book::open(&path, None).unwrap();
        assert!(matches!(
            b.save_payer(payer()),
            Err(Error::TinRequiresPassword)
        ));
        // Without a TIN it is fine; protecting the book then allows TINs.
        let mut p = payer();
        p.tin.clear();
        let p = b.save_payer(p).unwrap();
        let key = b.protect("a long enough password").unwrap();
        assert!(key.starts_with("MCRK-"));
        let mut p2 = p.clone();
        p2.tin = "123456789".into();
        b.save_payer(p2).unwrap();
    }

    #[test]
    fn filed_forms_are_locked_until_corrected() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = new_book(dir.path());
        let p = b.save_payer(payer()).unwrap();
        let r = b.save_recipient(recipient()).unwrap();
        let mut f = Form {
            payer: p.id,
            recipient: r.id,
            kind: FormKind::Misc,
            ..Form::default()
        };
        f.set_amount("1", Decimal::from(3000));
        let f = b.save_form(f).unwrap();
        assert_eq!(
            b.mark_filed(std::slice::from_ref(&f.id), "2027-01-20", "1099-123")
                .unwrap(),
            1
        );
        let mut g = b.form(&f.id).unwrap();
        g.set_amount("1", Decimal::from(3100));
        assert!(matches!(b.save_form(g.clone()), Err(Error::Rejected(_))));
        assert!(b.delete_form(&f.id).is_err());
        let c = b.correct(&f.id).unwrap();
        assert!(c.corrected && c.status == Status::Draft);
        assert!(c.notes.contains("1099-123"));
        g.status = Status::Draft;
        g.corrected = true;
        g.notes = c.notes;
        b.save_form(g).unwrap();
        // Bad boxes are refused.
        let mut bad = b.form(&f.id).unwrap();
        bad.set_amount("1a", Decimal::ONE);
        assert!(b.save_form(bad).is_err());
    }

    #[test]
    fn thresholds_override() {
        let mut b = Book::in_memory("x").unwrap();
        let mut o = std::collections::BTreeMap::new();
        o.insert("nec:1a".to_owned(), Decimal::from(2050));
        b.set_thresholds(2027, &o).unwrap();
        let t = b.thresholds(2027).unwrap();
        assert_eq!(t.get(FormKind::Nec, "1a"), Some(Decimal::from(2050)));
        assert!(!t.copied_from_2026);
        assert!(b.thresholds(2028).unwrap().copied_from_2026);
    }

    /// security-review-1 M6: an older book is refused before anything in its file
    /// changes, a newer one is refused, and opening a current MoneyKeep book writes no
    /// marker until `mark` (after the user confirms).
    #[test]
    fn m6_never_updates_a_books_format_and_marks_only_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = |p: &Path| std::fs::read(p).unwrap();

        // Older: refused with the message; the file is byte for byte the same.
        let old = dir.path().join("old.moneykeep");
        mk_store::create_old_book_for_tests(&old, "Old", mk_store::latest_schema_version() - 1)
            .unwrap();
        let before = bytes(&old);
        let e = Book::open(&old, None).unwrap_err();
        assert!(matches!(e, Error::BookTooOld));
        assert_eq!(
            e.to_string(),
            "Open this book in MoneyKeep first, so it can update it safely."
        );
        assert_eq!(bytes(&old), before, "the older book is not changed");

        // Newer: refused with the message.
        let new = dir.path().join("new.moneykeep");
        mk_store::create_old_book_for_tests(&new, "New", mk_store::latest_schema_version())
            .unwrap();
        {
            let conn = rusqlite::Connection::open(&new).unwrap();
            conn.pragma_update(None, "user_version", mk_store::latest_schema_version() + 1)
                .unwrap();
        }
        let e = Book::open(&new, None).unwrap_err();
        assert!(matches!(e, Error::BookTooNew));
        assert_eq!(
            e.to_string(),
            "This book was made by a newer MoneyKeep. Update Form1099Keep."
        );

        // Current format, not yet a 1099 book: opening writes nothing.
        let cur = dir.path().join("cur.moneykeep");
        let usd = CommodityCode::new("USD").unwrap();
        drop(Store::create(&cur, "Household", &usd).unwrap());
        {
            let b = Book::open(&cur, None).unwrap();
            assert!(!b.is_1099_book().unwrap());
        }
        let mut b = Book::open(&cur, None).unwrap();
        assert!(!b.is_1099_book().unwrap(), "no marker without confirming");
        assert!(
            b.history(10)
                .unwrap()
                .iter()
                .all(|h| h.command != "book.mark")
        );
        // The user confirmed: marked once, then left alone.
        b.mark().unwrap();
        b.mark().unwrap();
        assert!(b.is_1099_book().unwrap());
        drop(b);
        let mut b = Book::open(&cur, None).unwrap();
        assert!(b.is_1099_book().unwrap());
        let marks = b
            .history(10)
            .unwrap()
            .iter()
            .filter(|h| h.command == "book.mark")
            .count();
        assert_eq!(marks, 1);
    }
}
