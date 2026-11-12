use std::fmt;

/// Everything that can go wrong in Form1099Keep. Messages are shown to the user.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Store(#[from] mk_store::StoreError),

    #[error("MoneyKeep book: {0}")]
    MoneyKeep(String),

    #[error(
        "this book is not password protected, so it cannot hold taxpayer id numbers; \
         protect it with a password first"
    )]
    TinRequiresPassword,

    /// The book is in an older MoneyKeep format (security-review-1 M6): Form1099Keep
    /// never updates a book's format.
    #[error("Open this book in MoneyKeep first, so it can update it safely.")]
    BookTooOld,

    /// The book is in a format newer than the MoneyKeep this build uses (M6).
    #[error("This book was made by a newer MoneyKeep. Update Form1099Keep.")]
    BookTooNew,

    #[error("{0}")]
    Invalid(String),

    #[error("{what} not found: {id}")]
    NotFound { what: &'static str, id: String },

    #[error("{0}")]
    Rejected(String),

    #[error("cannot {action} {path}: {message}")]
    Io {
        action: &'static str,
        path: String,
        message: String,
    },

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

impl Error {
    pub fn invalid(msg: impl fmt::Display) -> Self {
        Self::Invalid(msg.to_string())
    }

    pub fn io(action: &'static str, path: &std::path::Path, e: &std::io::Error) -> Self {
        Self::Io {
            action,
            path: path.display().to_string(),
            message: e.to_string(),
        }
    }
}

impl From<mk_core::CoreError> for Error {
    fn from(e: mk_core::CoreError) -> Self {
        Self::Invalid(e.to_string())
    }
}

impl From<mk_commands::CommandError> for Error {
    fn from(e: mk_commands::CommandError) -> Self {
        Self::MoneyKeep(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
