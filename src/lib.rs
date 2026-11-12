//! Form1099Keep: Forms 1099-NEC and 1099-MISC kept in a MoneyKeep book.
//!
//! The book is an ordinary MoneyKeep book (`mk-store`): the same file format, the
//! same password protection and the same append-only audit log. Payers, recipients
//! and forms live in the `f1099_*` tables (MoneyKeep migration v15). Front ends (the
//! desktop app and the CLI) call [`Book`] and never touch SQLite.
//!
//! Spec: `docs/specs/form1099keep.md`.

mod book;
pub mod boxes;
pub mod check;
mod error;
pub mod import;
pub mod iris;
pub mod jobs;
pub mod model;
pub mod pdf;
pub mod tin;

pub use book::{ACTOR, BOOK_KIND, Book, HistoryEntry};
pub use error::{Error, Result};
pub use mk_core::Decimal;
pub use model::{
    Address, Filed, Form, FormKind, NameType, Payer, Recipient, Source, StateRow, Status, TinType,
};

/// The program's name, as shown in window titles and the About box.
pub const PRODUCT: &str = "Form1099Keep";
/// The family tag line (MoneyKeep `branding.md`).
pub const FAMILY: &str = "A MoneyKeep app";
/// The per-user config folder name.
pub const FOLDER: &str = "Form1099Keep";
/// The book file extension: 1099 books are MoneyKeep books.
pub const BOOK_EXTENSION: &str = mk_core::brand::BOOK_EXTENSION;
/// The first tax year with the layout this build knows (Rev. December 2026).
pub const FIRST_YEAR: i32 = 2026;
