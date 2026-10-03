//! Word count details, as Word's Word Count dialog shows them (click the
//! count in the status bar): words, characters with and without spaces,
//! paragraphs and pages, for the document or the selection.

use gpui::prelude::*;
use gpui::{div, px, Context, Div};

use crate::ui::{count_words, label};
use crate::{doc, SylphApp};

/// The counts for some text.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Counts {
    pub(crate) words: usize,
    pub(crate) characters: usize,
    pub(crate) characters_no_spaces: usize,
    pub(crate) paragraphs: usize,
}

/// Counts over the paragraphs' visible text (Markdown syntax excluded, as
/// the export sees it), with `source` for words so the dialog agrees with
/// the status bar.
pub(crate) fn counts(source: &str, blocks: &[doc::Block]) -> Counts {
    let texts: Vec<String> = blocks
        .iter()
        .map(doc::Block::plain_text)
        .filter(|t| !t.trim().is_empty())
        .collect();
    let characters: usize = texts.iter().map(|t| t.chars().count()).sum();
    let spaces: usize = texts
        .iter()
        .map(|t| t.chars().filter(|c| c.is_whitespace()).count())
        .sum();
    Counts {
        words: count_words(source),
        characters,
        characters_no_spaces: characters - spaces,
        paragraphs: texts.len(),
    }
}

impl SylphApp {
    /// The Word count dialog's body: the selection's counts when text is
    /// selected, else the document's, plus its pages.
    pub(crate) fn word_count_dialog(&self, cx: &mut Context<Self>) -> Div {
        let (text, muted) = (self.ui_text(), self.ui_muted());
        let editor = self.editor.read(cx);
        let range = editor.selected_range.clone();
        let selection = !range.is_empty();
        let source = if selection {
            editor.content[range].to_string()
        } else {
            editor.content.clone()
        };
        let blocks = if self.markdown_mode {
            crate::parse_content_blocks(&source, 1.0)
        } else {
            source
                .lines()
                .map(|line| doc::Block::paragraph(line.to_string()))
                .collect()
        };
        let counts = counts(&source, &blocks);
        let mut rows: Vec<(&str, String)> = vec![
            ("Words", counts.words.to_string()),
            (
                "Characters (no spaces)",
                counts.characters_no_spaces.to_string(),
            ),
            ("Characters (with spaces)", counts.characters.to_string()),
            ("Paragraphs", counts.paragraphs.to_string()),
        ];
        if !selection {
            rows.insert(0, ("Pages", self.page_count(cx).to_string()));
        }
        let mut body = div().flex().flex_col().gap(px(8.0)).child(label(
            if selection {
                "Selected text"
            } else {
                "Whole document"
            },
            muted,
            12.0,
        ));
        for (name, value) in rows {
            body = body.child(
                div()
                    .flex()
                    .justify_between()
                    .child(label(name, text, 13.0))
                    .child(label(value, text, 13.0).font_weight(gpui::FontWeight(600.0))),
            );
        }
        self.modal("Word count", 340.0, body, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::{counts, Counts};
    use crate::parse_content_blocks;

    #[test]
    fn counts_skip_markdown_syntax_and_blank_lines() {
        let text = "# Title\n\nSome **bold** text.\n\n- one item";
        let c = counts(text, &parse_content_blocks(text, 1.0));
        assert_eq!(
            c,
            Counts {
                words: 6,
                // "Title" + "Some bold text." + "one item"
                characters: 5 + 15 + 8,
                characters_no_spaces: 5 + 13 + 7,
                paragraphs: 3,
            }
        );
    }

    #[test]
    fn empty_text_counts_nothing() {
        assert_eq!(
            counts("", &parse_content_blocks("", 1.0)),
            Counts::default()
        );
    }
}
