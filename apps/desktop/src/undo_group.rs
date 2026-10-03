//! Undo that groups typing, as Word and Docs do: Ctrl+Z takes back a typed
//! word (or a run of Backspaces), not one character at a time. A group
//! ends at a word boundary, a caret jump, a newline, or a pause.

use std::time::{Duration, Instant};

/// Typing after this long a pause starts a new undo step.
pub(crate) const PAUSE: Duration = Duration::from_secs(1);

/// How a new edit joins the previous undo step.
#[derive(Debug, PartialEq)]
pub(crate) enum Merge {
    /// Typed right after the previous typing: append to its text.
    Typing,
    /// Backspace right before the previous deletion: prepend what went.
    Backspace,
    /// Delete at the same place as the previous deletion: append what went.
    ForwardDelete,
    /// A separate step.
    No,
}

/// One text edit: `old` at `start` replaced by `new`.
#[derive(Clone, Copy)]
pub(crate) struct Edit<'a> {
    pub(crate) start: usize,
    pub(crate) old: &'a str,
    pub(crate) new: &'a str,
}

fn single_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c != '\n' => Some(c),
        _ => None,
    }
}

/// Whether `edit`, made `since` after the previous step `last`, joins it.
pub(crate) fn merge(last: Edit, edit: Edit, since: Duration) -> Merge {
    if since > PAUSE {
        return Merge::No;
    }
    // Typing: one character inserted where the previous typing ended.
    if let Some(c) = single_char(edit.new) {
        let typing_before = edit.old.is_empty()
            && last.old.is_empty()
            && !last.new.is_empty()
            && !last.new.contains('\n');
        if typing_before && edit.start == last.start + last.new.len() {
            // A word after a space starts a new step ("undo one word").
            let after_space = last.new.ends_with(char::is_whitespace);
            if after_space && !c.is_whitespace() {
                return Merge::No;
            }
            return Merge::Typing;
        }
        return Merge::No;
    }
    // Deleting: one character removed next to the previous deletion.
    if edit.new.is_empty() && single_char(edit.old).is_some() && last.new.is_empty() {
        if !last.old.is_empty() && edit.start + edit.old.len() == last.start {
            return Merge::Backspace;
        }
        if !last.old.is_empty() && edit.start == last.start {
            return Merge::ForwardDelete;
        }
    }
    Merge::No
}

/// When `last` was recorded, for the pause rule.
pub(crate) fn elapsed(last_at: Instant, now: Instant) -> Duration {
    now.saturating_duration_since(last_at)
}

#[cfg(test)]
mod tests {
    use super::{merge, Edit, Merge};
    use std::time::Duration;

    const QUICK: Duration = Duration::from_millis(100);

    fn typed(start: usize, text: &str) -> Edit<'_> {
        Edit {
            start,
            old: "",
            new: text,
        }
    }

    fn removed(start: usize, text: &str) -> Edit<'_> {
        Edit {
            start,
            old: text,
            new: "",
        }
    }

    #[test]
    fn typing_a_word_is_one_step() {
        assert_eq!(merge(typed(0, "h"), typed(1, "e"), QUICK), Merge::Typing);
        assert_eq!(merge(typed(0, "hel"), typed(3, "l"), QUICK), Merge::Typing);
        // The space ends the word; the next word is a new step.
        assert_eq!(
            merge(typed(0, "hello"), typed(5, " "), QUICK),
            Merge::Typing
        );
        assert_eq!(merge(typed(0, "hello "), typed(6, "w"), QUICK), Merge::No);
        // Multi-byte characters advance by their byte length.
        assert_eq!(merge(typed(0, "é"), typed(2, "x"), QUICK), Merge::Typing);
    }

    #[test]
    fn jumps_newlines_pauses_and_pastes_start_new_steps() {
        assert_eq!(merge(typed(0, "ab"), typed(7, "c"), QUICK), Merge::No);
        assert_eq!(merge(typed(0, "ab"), typed(2, "\n"), QUICK), Merge::No);
        assert_eq!(merge(typed(0, "a\n"), typed(2, "b"), QUICK), Merge::No);
        assert_eq!(
            merge(typed(0, "ab"), typed(2, "c"), Duration::from_secs(2)),
            Merge::No
        );
        assert_eq!(merge(typed(0, "ab"), typed(2, "pasted"), QUICK), Merge::No);
        // Typing over a selection is its own step.
        let replace = Edit {
            start: 2,
            old: "x",
            new: "c",
        };
        assert_eq!(merge(typed(0, "ab"), replace, QUICK), Merge::No);
    }

    #[test]
    fn backspaces_and_deletes_group() {
        // Backspace at 4 removed "d"; the next Backspace removes "c" at 3.
        assert_eq!(
            merge(removed(3, "d"), removed(2, "c"), QUICK),
            Merge::Backspace
        );
        assert_eq!(
            merge(removed(3, "d"), removed(3, "e"), QUICK),
            Merge::ForwardDelete
        );
        assert_eq!(merge(removed(3, "d"), removed(0, "a"), QUICK), Merge::No);
        // Typing then deleting are separate steps.
        assert_eq!(merge(typed(0, "ab"), removed(1, "b"), QUICK), Merge::No);
        assert_eq!(merge(removed(3, "d"), typed(3, "x"), QUICK), Merge::No);
    }
}
