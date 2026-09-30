//! Where the text stands relative to storage, as the status bar shows it.

use crate::normalize_newlines;
use sylph_storage::Storage;

/// Where the typed text stands relative to storage. The status bar shows
/// this instead of assuming every save worked.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SaveState {
    Saved,
    /// Edited; waiting for the autosave debounce or the write itself.
    Saving,
    /// The last write failed. `last_ok_at` is the last one that worked.
    Failed {
        error: String,
        last_ok_at: Option<std::time::Instant>,
    },
    /// The database could not open, so edits live in memory only and are
    /// lost on exit. Permanent for the session: no write can change it.
    Unpersisted(String),
    /// The document could not be read. Editing and saving are off, so an
    /// empty buffer can never overwrite the real content.
    ReadOnly(String),
}

impl SaveState {
    /// The state after a write finished with `result`. In a session with
    /// no database (`unpersisted`), even a good write only reached memory.
    pub(crate) fn after_write(
        result: Result<(), Box<dyn std::error::Error>>,
        unpersisted: Option<&str>,
        last_ok_at: Option<std::time::Instant>,
    ) -> Self {
        match (unpersisted, result) {
            (Some(reason), _) => Self::Unpersisted(reason.to_string()),
            (None, Ok(())) => Self::Saved,
            (None, Err(e)) => Self::Failed {
                error: e.to_string(),
                last_ok_at,
            },
        }
    }

    /// Status-bar wording (Word/Docs style); a failure says why.
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Saved => "All changes saved".to_string(),
            Self::Saving => "Saving…".to_string(),
            Self::Failed { error, last_ok_at } => match last_ok_at {
                Some(at) => format!(
                    "Save failed: {error} (last saved {})",
                    ago(at.elapsed().as_secs())
                ),
                None => format!("Save failed: {error}"),
            },
            Self::Unpersisted(reason) => format!("NOT SAVING — {reason}"),
            Self::ReadOnly(reason) => format!("Read-only — {reason}"),
        }
    }

    /// States that are errors, shown in the danger color.
    pub(crate) fn is_problem(&self) -> bool {
        matches!(
            self,
            Self::Failed { .. } | Self::Unpersisted(_) | Self::ReadOnly(_)
        )
    }
}

/// "just now", "1 min ago", "5 min ago", "2 h ago".
pub(crate) fn ago(secs: u64) -> String {
    match secs {
        0..60 => "just now".to_string(),
        60..3600 => format!("{} min ago", secs / 60),
        _ => format!("{} h ago", secs / 3600),
    }
}

/// The document's saved text (empty for one never saved), or an empty
/// buffer plus the reason when it cannot be read. Such a document must
/// open read-only: saving the empty buffer would destroy the real text.
pub(crate) fn load_text_or_read_only(storage: &Storage, doc_id: i64) -> (String, Option<String>) {
    match storage.load_text(doc_id) {
        Ok(text) => (
            text.map(|text| normalize_newlines(&text).into_owned())
                .unwrap_or_default(),
            None,
        ),
        Err(e) => (
            String::new(),
            Some(format!("This document could not be read ({e})")),
        ),
    }
}

/// The save state of a document just opened.
pub(crate) fn opened_state(read_only: &Option<String>, unpersisted: &Option<String>) -> SaveState {
    match (read_only, unpersisted) {
        (Some(reason), _) => SaveState::ReadOnly(reason.clone()),
        (None, Some(reason)) => SaveState::Unpersisted(reason.clone()),
        (None, None) => SaveState::Saved,
    }
}

#[cfg(test)]
mod save_state_tests {
    use super::{ago, load_text_or_read_only, opened_state, SaveState};
    use sylph_storage::Storage;

    #[test]
    fn save_state_reports_the_real_result() {
        assert_eq!(SaveState::after_write(Ok(()), None, None), SaveState::Saved);
        let failed = SaveState::after_write(Err("disk full".into()), None, None);
        assert_eq!(
            failed,
            SaveState::Failed {
                error: "disk full".to_string(),
                last_ok_at: None
            }
        );
        // A failure says why instead of claiming the changes were saved.
        assert_eq!(failed.label(), "Save failed: disk full");
        let since = SaveState::after_write(
            Err("disk full".into()),
            None,
            Some(std::time::Instant::now()),
        );
        assert_eq!(
            since.label(),
            "Save failed: disk full (last saved just now)"
        );
    }

    #[test]
    fn a_session_without_a_database_never_claims_to_save() {
        for result in [Ok(()), Err("x".into())] {
            let state = SaveState::after_write(result, Some("no database"), None);
            assert_eq!(state, SaveState::Unpersisted("no database".to_string()));
            assert_eq!(state.label(), "NOT SAVING — no database");
            assert!(state.is_problem());
        }
    }

    #[test]
    fn opened_documents_start_in_the_honest_state() {
        let none = None;
        let reason = Some("r".to_string());
        assert_eq!(opened_state(&none, &none), SaveState::Saved);
        assert_eq!(
            opened_state(&none, &reason),
            SaveState::Unpersisted("r".into())
        );
        assert_eq!(
            opened_state(&reason, &none),
            SaveState::ReadOnly("r".into())
        );
        assert_eq!(
            opened_state(&reason, &reason),
            SaveState::ReadOnly("r".into())
        );
    }

    #[test]
    fn an_unreadable_document_opens_read_only_and_empty() {
        let dir = std::env::temp_dir().join(format!("sylph-ro-doc-test-{}", std::process::id()));
        let storage = Storage::open_at(&dir).unwrap();
        let good = storage.create_document("Good").unwrap();
        storage.save_text(good, "a\r\nb").unwrap();
        assert_eq!(
            load_text_or_read_only(&storage, good),
            ("a\nb".to_string(), None)
        );
        let bad = storage.create_document("Bad").unwrap();
        storage.save_document(bad, &[0xff, 0xfe, 0x00]).unwrap();
        let (text, read_only) = load_text_or_read_only(&storage, bad);
        assert_eq!(text, "");
        assert!(read_only
            .unwrap()
            .starts_with("This document could not be read"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ago_reads_naturally() {
        assert_eq!(ago(5), "just now");
        assert_eq!(ago(61), "1 min ago");
        assert_eq!(ago(7200), "2 h ago");
    }

    #[test]
    fn save_state_labels_use_word_docs_wording() {
        assert_eq!(SaveState::Saved.label(), "All changes saved");
        assert_eq!(SaveState::Saving.label(), "Saving…");
    }
}
