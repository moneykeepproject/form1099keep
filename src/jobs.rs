//! The year-end jobs both front ends run: checking a year, printing recipient
//! copies and building the IRIS files. Forms with errors are skipped, never
//! half-printed.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::check::{self, Finding};
use crate::iris;
use crate::model::{Form, FormKind, Payer, Recipient};
use crate::pdf;
use crate::{Book, Error, Result};

/// A year's forms with everything needed to check, print or export them.
#[derive(Debug, Clone)]
pub struct Year {
    pub year: i32,
    pub forms: Vec<Form>,
    pub payers: BTreeMap<String, Payer>,
    pub recipients: BTreeMap<String, Recipient>,
    pub findings: Vec<Finding>,
}

impl Year {
    pub fn load(book: &mut Book, year: i32) -> Result<Self> {
        let mut forms = book.forms(Some(year))?;
        let payers: BTreeMap<String, Payer> = book
            .payers()?
            .into_iter()
            .map(|p| (p.id.clone(), p))
            .collect();
        let recipients: BTreeMap<String, Recipient> = book
            .recipients()?
            .into_iter()
            .map(|r| (r.id.clone(), r))
            .collect();
        forms.sort_by_key(|f| {
            (
                f.kind,
                recipients
                    .get(&f.recipient)
                    .map(Recipient::sort_key)
                    .unwrap_or_default(),
            )
        });
        let t = book.thresholds(year)?;
        let p: Vec<Payer> = payers.values().cloned().collect();
        let r: Vec<Recipient> = recipients.values().cloned().collect();
        let findings = check::year(year, &forms, &p, &r, &t);
        Ok(Self {
            year,
            forms,
            payers,
            recipients,
            findings,
        })
    }

    /// The forms chosen (all when `ids` is empty), with their parties.
    fn chosen<'a>(
        &'a self,
        ids: &[String],
    ) -> Vec<(&'a Form, Option<&'a Payer>, Option<&'a Recipient>)> {
        self.forms
            .iter()
            .filter(|f| ids.is_empty() || ids.contains(&f.id))
            .map(|f| {
                (
                    f,
                    self.payers.get(&f.payer),
                    self.recipients.get(&f.recipient),
                )
            })
            .collect()
    }

    pub fn label(&self, f: &Form) -> String {
        format!(
            "{} — {}",
            self.recipients
                .get(&f.recipient)
                .map(Recipient::display_name)
                .unwrap_or_else(|| "?".into()),
            f.kind.label()
        )
    }
}

/// Forms left out of a job, and why.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Skipped {
    pub form: String,
    pub label: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct Printed {
    pub pdf: Vec<u8>,
    pub forms: usize,
    pub pages: usize,
    pub skipped: Vec<Skipped>,
}

/// Recipient copies for the chosen forms of a year (all when `ids` is empty).
pub fn print(y: &Year, ids: &[String], opts: &pdf::Options) -> Result<Printed> {
    let mut items = Vec::new();
    let mut skipped = Vec::new();
    for (f, p, r) in y.chosen(ids) {
        match (p, r) {
            (Some(p), Some(r)) if !check::blocks(&y.findings, f) => items.push(pdf::Item {
                form: f,
                payer: p,
                recipient: r,
            }),
            _ => skipped.push(skip(y, f)),
        }
    }
    if items.is_empty() {
        return Err(Error::Rejected(if skipped.is_empty() {
            "there are no forms to print".into()
        } else {
            format!(
                "none of the {} form(s) can be printed until their checks are answered",
                skipped.len()
            )
        }));
    }
    let pages = items.len().saturating_mul(opts.copies.len());
    Ok(Printed {
        pdf: pdf::render(&items, opts),
        forms: items.len(),
        pages,
        skipped,
    })
}

#[derive(Debug, Clone, Default)]
pub struct IrisFiles {
    pub built: iris::Built,
    pub rows: usize,
    pub skipped: Vec<Skipped>,
    /// True when the built-in header list was used instead of the portal's template.
    pub builtin_headers: bool,
}

/// IRIS upload files for one kind of form in a year (all payers, or one).
pub fn iris_files(
    y: &Year,
    kind: FormKind,
    payer: Option<&str>,
    template: Option<(Vec<String>, bool)>,
) -> Result<IrisFiles> {
    let mut rows = Vec::new();
    let mut skipped = Vec::new();
    for (f, p, r) in y.chosen(&[]) {
        if f.kind != kind || payer.is_some_and(|id| id != f.payer) {
            continue;
        }
        match (p, r) {
            (Some(p), Some(r)) if !check::blocks(&y.findings, f) => {
                rows.push(iris::Row {
                    form: f,
                    payer: p,
                    recipient: r,
                });
            }
            _ => skipped.push(skip(y, f)),
        }
    }
    if rows.is_empty() {
        return Err(Error::Rejected(if skipped.is_empty() {
            format!("there are no {} forms for {}", kind.label(), y.year)
        } else {
            format!(
                "none of the {} {} form(s) are ready: answer their checks first",
                skipped.len(),
                kind.label()
            )
        }));
    }
    let builtin = template.is_none();
    let (headers, bom) = template.unwrap_or_else(|| (iris::builtin_headers(kind), false));
    let built = iris::build(y.year, kind, &rows, &headers, bom);
    Ok(IrisFiles {
        rows: rows.len(),
        built,
        skipped,
        builtin_headers: builtin,
    })
}

fn skip(y: &Year, f: &Form) -> Skipped {
    let first_error = check::for_form(&y.findings, f)
        .into_iter()
        .find(|x| x.severity == check::Severity::Error)
        .map(|x| x.message.clone())
        .unwrap_or_else(|| "its payer or recipient is missing".into());
    Skipped {
        form: f.id.clone(),
        label: y.label(f),
        reason: first_error,
    }
}
