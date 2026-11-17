//! Native file dialogs.
//!
//! macOS: a dialog must not run a modal loop of its own from inside a winit event
//! handler. `rfd::FileDialog` calls `NSSavePanel runModal`, which delivers events
//! while the click that opened it is still being handled; winit then panics
//! ("tried to handle event while another event is currently being handled") inside
//! an Objective-C callback, where the panic cannot unwind, and the app aborts. So on
//! macOS the dialog opens as a sheet on the window (`rfd::AsyncFileDialog`, which
//! returns at once) and its answer arrives on a later turn of the event loop.
//!
//! Windows and Linux keep the blocking dialog: it answers in the same call, exactly
//! as before.

use std::path::PathBuf;

/// Whether dialogs answer later (macOS) or in the same call (elsewhere).
pub const ANSWER_LATER: bool = cfg!(target_os = "macos");

/// Which dialog to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Choose where to save one file.
    Save,
    /// Choose one existing file.
    File,
    /// Choose a folder.
    Folder,
}

/// A file dialog to show: what to choose, its title, the file types and a suggested
/// name.
#[derive(Clone, Debug)]
pub struct Ask {
    pub kind: Kind,
    title: Option<String>,
    filters: Vec<(String, Vec<String>)>,
    file_name: Option<String>,
}

impl Ask {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            title: None,
            filters: Vec::new(),
            file_name: None,
        }
    }

    /// Choose where to save one file.
    pub fn save() -> Self {
        Self::new(Kind::Save)
    }

    /// Choose one existing file.
    pub fn file() -> Self {
        Self::new(Kind::File)
    }

    /// Choose a folder.
    pub fn folder() -> Self {
        Self::new(Kind::Folder)
    }

    /// The dialog's title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Offer files with these extensions under this name.
    pub fn filter(mut self, name: impl Into<String>, extensions: &[&str]) -> Self {
        self.filters.push((
            name.into(),
            extensions.iter().map(|e| (*e).to_owned()).collect(),
        ));
        self
    }

    /// The suggested file name.
    pub fn file_name(mut self, name: impl Into<String>) -> Self {
        self.file_name = Some(name.into());
        self
    }

    /// Show the dialog and wait for the answer (Windows and Linux). Cancelled: `None`.
    pub fn run_blocking(self) -> Option<PathBuf> {
        let mut d = rfd::FileDialog::new();
        if let Some(t) = &self.title {
            d = d.set_title(t);
        }
        for (name, exts) in &self.filters {
            d = d.add_filter(name, exts);
        }
        if let Some(n) = &self.file_name {
            d = d.set_file_name(n);
        }
        match self.kind {
            Kind::Save => d.save_file(),
            Kind::File => d.pick_file(),
            Kind::Folder => d.pick_folder(),
        }
    }

    /// Show the dialog without a modal loop (macOS: a sheet on the window); the
    /// future completes with the answer. Cancelled: `None`.
    pub fn run_later(self) -> impl Future<Output = Option<PathBuf>> + 'static {
        let mut d = rfd::AsyncFileDialog::new();
        if let Some(t) = &self.title {
            d = d.set_title(t);
        }
        for (name, exts) in &self.filters {
            d = d.add_filter(name, exts);
        }
        if let Some(n) = &self.file_name {
            d = d.set_file_name(n);
        }
        let kind = self.kind;
        async move {
            let handle = match kind {
                Kind::Save => d.save_file().await,
                Kind::File => d.pick_file().await,
                Kind::Folder => d.pick_folder().await,
            };
            handle.map(|h| h.path().to_path_buf())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_keeps_title_filters_and_name() {
        let a = Ask::save()
            .title("Save the new 1099 book")
            .filter("MoneyKeep book", &["moneykeep"])
            .file_name("Payers 2026.moneykeep");
        assert_eq!(a.kind, Kind::Save);
        assert_eq!(a.title.as_deref(), Some("Save the new 1099 book"));
        assert_eq!(
            a.filters,
            vec![("MoneyKeep book".to_owned(), vec!["moneykeep".to_owned()])]
        );
        assert_eq!(a.file_name.as_deref(), Some("Payers 2026.moneykeep"));
    }

    #[test]
    fn only_macos_answers_later() {
        assert_eq!(ANSWER_LATER, cfg!(target_os = "macos"));
    }

    #[test]
    fn folder_is_its_own_kind() {
        assert_eq!(Ask::folder().kind, Kind::Folder);
    }

    /// Every dialog goes through `Ask`: a blocking `rfd::FileDialog` called from a UI
    /// callback aborts Form1099Keep on macOS (see the module notes).
    #[test]
    fn the_app_opens_no_blocking_dialog() {
        assert!(
            !include_str!("main.rs").contains("rfd::"),
            "main.rs uses rfd directly; use dialogs::Ask"
        );
    }
}
