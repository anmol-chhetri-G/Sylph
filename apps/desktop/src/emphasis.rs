//! Bold, italic, underline and strikethrough as Word does them: Ctrl+B on
//! a selection turns bold on, or off when all of it is already bold (from
//! formatting, `**` Markdown or a heading's style); with only a caret it
//! formats the word under it, or what is typed next. Stored as character
//! formatting, so it works with Markdown on or off and across lines.

use gpui::Context;
use std::ops::Range;
use sylph_core::document::SpanStyle;
use sylph_core::format::Emphasis;

use crate::TextInput;

/// Whether the Markdown or the style of display line `dl` makes display
/// byte `disp` bold / italic / struck through (underline has no Markdown).
pub(crate) fn base_emphasis(dl: &crate::DisplayLine, disp: usize, emphasis: Emphasis) -> bool {
    let styled = |wanted: &[SpanStyle]| {
        dl.styles.iter().any(|(range, styles)| {
            range.contains(&disp) && styles.iter().any(|s| wanted.contains(s))
        })
    };
    match emphasis {
        Emphasis::Bold => dl.bold || styled(&[SpanStyle::Bold, SpanStyle::BoldItalic]),
        Emphasis::Italic => dl.italic || styled(&[SpanStyle::Italic, SpanStyle::BoldItalic]),
        Emphasis::Strike => styled(&[SpanStyle::Strikethrough]),
        Emphasis::Underline => false,
    }
}

/// The word around `offset` (letters, digits, `_`, `'`), if the caret is
/// inside one rather than at its edge.
pub(crate) fn word_at(text: &str, offset: usize) -> Option<Range<usize>> {
    // Not alphanumeric alone: Devanagari vowel signs are marks.
    let is_word = |c: char| {
        !c.is_whitespace()
            && !(c.is_ascii_punctuation() && c != '_' && c != '\'')
            && !matches!(c, '—' | '–' | '“' | '”' | '…' | '«' | '»')
    };
    let before = text[..offset].chars().next_back().is_some_and(is_word);
    let after = text[offset..].chars().next().is_some_and(is_word);
    if !(before && after) {
        return None;
    }
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| is_word(c))
        .last()
        .map_or(offset, |(i, _)| i);
    let end = text[offset..]
        .char_indices()
        .find(|&(_, c)| !is_word(c))
        .map_or(text.len(), |(i, _)| offset + i);
    Some(start..end)
}

/// What the B / I / U / S decisions read: the text, its character
/// formatting, and the last layout's display lines (Markdown on).
pub(crate) struct TextView<'a> {
    pub(crate) content: &'a str,
    pub(crate) formats: &'a sylph_core::format::FormatSpans,
    pub(crate) lines: &'a [crate::DisplayLine],
    pub(crate) markdown: bool,
}

impl TextView<'_> {
    /// Whether the character starting at `offset` shows `emphasis`: its
    /// own formatting, else its Markdown / style.
    pub(crate) fn emphasis_at(&self, offset: usize, emphasis: Emphasis) -> bool {
        if let Some(value) = self.formats.format_at(offset).emphasis(emphasis) {
            return value;
        }
        let index = self.content[..offset.min(self.content.len())]
            .matches('\n')
            .count();
        match self.lines.get(index) {
            Some(dl) if self.markdown => {
                let disp = dl.src_to_disp(offset.saturating_sub(dl.src_offset));
                base_emphasis(dl, disp, emphasis)
            }
            _ => false,
        }
    }

    /// Whether source byte `offset` is shown on the page (hidden Markdown
    /// syntax such as `**` is not; with Markdown off everything is).
    fn is_shown(&self, offset: usize) -> bool {
        if !self.markdown {
            return true;
        }
        let index = self.content[..offset.min(self.content.len())]
            .matches('\n')
            .count();
        self.lines
            .get(index)
            .is_none_or(|dl| dl.shows_src(offset.saturating_sub(dl.src_offset)))
    }

    /// Whether every visible character of `range` shows `emphasis`: spaces
    /// and hidden Markdown syntax don't count, as the user can't see them.
    pub(crate) fn all_have(&self, range: Range<usize>, emphasis: Emphasis) -> bool {
        let text = &self.content[range.clone()];
        let mut any = false;
        for (i, c) in text.char_indices() {
            if c.is_whitespace() || !self.is_shown(range.start + i) {
                continue;
            }
            any = true;
            if !self.emphasis_at(range.start + i, emphasis) {
                return false;
            }
        }
        any
    }

    /// Ctrl+B / I / U on `range`: whether it turns `emphasis` on (off when
    /// all of it already has it), and the formatting afterwards.
    pub(crate) fn toggled(
        &self,
        range: Range<usize>,
        emphasis: Emphasis,
    ) -> (bool, sylph_core::format::FormatSpans) {
        let on = !self.all_have(range.clone(), emphasis);
        let mut formats = self.formats.clone();
        formats.apply(range, |f| f.set_emphasis(emphasis, Some(on)));
        (on, formats)
    }
}

impl TextInput {
    pub(crate) fn text_view(&self) -> TextView<'_> {
        TextView {
            content: &self.content,
            formats: &self.formats,
            lines: &self.display_lines,
            markdown: self.markdown_mode,
        }
    }

    pub(crate) fn emphasis_at(&self, offset: usize, emphasis: Emphasis) -> bool {
        self.text_view().emphasis_at(offset, emphasis)
    }

    fn all_have(&self, range: Range<usize>, emphasis: Emphasis) -> bool {
        self.text_view().all_have(range, emphasis)
    }

    /// Whether `emphasis` is on where the controls look: all of the
    /// selection, or the pending choice / the character before the caret.
    pub(crate) fn emphasis_active(&self, emphasis: Emphasis) -> bool {
        let range = self.selected_range.clone();
        if !range.is_empty() {
            return self.all_have(range, emphasis);
        }
        if let Some(&(_, on)) = self.pending_emphasis.iter().find(|(e, _)| *e == emphasis) {
            return on;
        }
        self.content[..range.start]
            .chars()
            .next_back()
            .is_some_and(|c| self.emphasis_at(range.start - c.len_utf8(), emphasis))
    }

    /// The B / I / U / S toggle (Ctrl+B / I / U): see the module docs.
    /// Returns whether it is now on.
    pub(crate) fn toggle_emphasis(&mut self, emphasis: Emphasis, cx: &mut Context<Self>) -> bool {
        if self.read_only.is_some() {
            return false;
        }
        let mut range = self.selected_range.clone();
        let caret_only = range.is_empty();
        if caret_only {
            match word_at(&self.content, range.start) {
                Some(word) => range = word,
                None => {
                    // Formats what is typed next, until the caret moves.
                    let on = !self
                        .pending_emphasis
                        .iter()
                        .any(|&(e, v)| e == emphasis && v)
                        && !self.emphasis_at(
                            self.content[..range.start]
                                .chars()
                                .next_back()
                                .map_or(range.start, |c| range.start - c.len_utf8()),
                            emphasis,
                        );
                    self.pending_emphasis.retain(|&(e, _)| e != emphasis);
                    self.pending_emphasis.push((emphasis, on));
                    cx.notify();
                    return on;
                }
            }
        }
        let (on, formats) = self.text_view().toggled(range, emphasis);
        let formats_before = std::mem::replace(&mut self.formats, formats);
        let selection = self.selected_range.clone();
        self.push_format_step(selection, formats_before, self.para_formats.clone());
        self.redo_stack.clear();
        self.content_rev += 1;
        self.schedule_save(cx);
        cx.notify();
        on
    }

    /// Give just-typed text `inserted` the pending B / I / U / S choices.
    pub(crate) fn apply_pending_emphasis(&mut self, inserted: Range<usize>) {
        if inserted.is_empty() || self.pending_emphasis.is_empty() {
            return;
        }
        let pending = self.pending_emphasis.clone();
        self.formats.apply(inserted, |f| {
            for &(emphasis, on) in &pending {
                f.set_emphasis(emphasis, Some(on));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::word_at;

    #[test]
    fn the_word_under_the_caret() {
        let text = "hello wor_ld, it's";
        assert_eq!(word_at(text, 2), Some(0..5));
        assert_eq!(word_at(text, 8), Some(6..12));
        assert_eq!(word_at(text, 16), Some(14..18));
        // At a word's edge or between words: no word.
        assert_eq!(word_at(text, 0), None);
        assert_eq!(word_at(text, 5), None);
        assert_eq!(word_at(text, 12), None);
        assert_eq!(word_at("नेपाली", 3), Some(0..18));
    }
}

#[cfg(test)]
mod run_coverage {
    //! The renderer splits these runs where formatting changes; text a run
    //! does not cover would lose its formatting on screen.
    #[test]
    fn markdown_off_runs_cover_every_byte() {
        let font = gpui::font("EB Garamond");
        let color = gpui::hsla(0.0, 0.0, 0.0, 1.0);
        for line in [
            "plain words here",
            "  indented words",
            "# Heading words",
            "- list item words",
            "1. numbered item",
            "> quoted words",
            "**bold** and *italic* and `code`",
            "a [link](http://x.y) b",
            "| a | b |",
            "---",
            "",
            "नेपाली शब्द",
        ] {
            let runs = crate::TextInput::markdown_runs(line, font.clone(), color, false);
            let covered: usize = runs.iter().map(|r| r.len).sum();
            assert_eq!(
                covered,
                line.len(),
                "line {line:?}: runs cover {covered} of {} bytes",
                line.len()
            );
        }
    }
}

#[cfg(test)]
mod bold_scenarios {
    //! What the user sees: select text, press Ctrl+B, and which displayed
    //! characters render bold on the page. Rendering follows the canvas:
    //! a character's own bold setting wins, else its Markdown / style.
    use super::{base_emphasis, TextView};
    use sylph_core::format::{Emphasis, FormatSpans};

    struct Doc {
        text: String,
        formats: FormatSpans,
        markdown: bool,
    }

    impl Doc {
        fn new(text: &str, markdown: bool) -> Self {
            Self {
                text: text.into(),
                formats: FormatSpans::new(),
                markdown,
            }
        }

        fn lines(&self) -> Vec<crate::DisplayLine> {
            let lines: Vec<String> = self.text.split('\n').map(String::from).collect();
            crate::build_display_lines(&lines, self.markdown)
        }

        /// Select the first `selected` in the text and press Ctrl+B.
        fn bold(&mut self, selected: &str) -> &mut Self {
            let start = self.text.find(selected).expect("selection in text");
            let lines = self.lines();
            let view = TextView {
                content: &self.text,
                formats: &self.formats,
                lines: &lines,
                markdown: self.markdown,
            };
            let (_, formats) = view.toggled(start..start + selected.len(), Emphasis::Bold);
            self.formats = formats;
            self
        }

        /// The displayed characters that render bold, runs joined by "|".
        fn shown_bold(&self) -> String {
            let mut out = Vec::new();
            for dl in self.lines() {
                let mut run = String::new();
                for (range, format) in
                    crate::rich_row::line_formats(&dl, 0..dl.text.len(), &self.formats)
                {
                    for (i, c) in dl.text[range.clone()].char_indices() {
                        let bold = format.bold.unwrap_or_else(|| {
                            self.markdown && base_emphasis(&dl, range.start + i, Emphasis::Bold)
                        });
                        if bold {
                            run.push(c);
                        } else if !run.is_empty() {
                            out.push(std::mem::take(&mut run));
                        }
                    }
                }
                if !run.is_empty() {
                    out.push(run);
                }
            }
            out.join("|")
        }
    }

    #[test]
    fn part_of_a_word() {
        for markdown in [true, false] {
            assert_eq!(
                Doc::new("hello world", markdown).bold("ell").shown_bold(),
                "ell"
            );
        }
    }

    #[test]
    fn spaces_at_the_selection_edges() {
        let mut doc = Doc::new("say hello world now", true);
        assert_eq!(doc.bold(" hello ").shown_bold(), " hello ");
        // Pressing again turns it off; the spaces do not count.
        assert_eq!(doc.bold(" hello ").shown_bold(), "");
    }

    #[test]
    fn a_mixed_selection_becomes_all_bold_then_plain() {
        let mut doc = Doc::new("hello world", true);
        doc.bold("ell");
        assert_eq!(doc.bold("hello").shown_bold(), "hello");
        assert_eq!(doc.bold("hello").shown_bold(), "");
    }

    #[test]
    fn the_letter_right_after_markdown_syntax() {
        // "rest" follows the hidden `**`: all four letters must be bold.
        assert_eq!(
            Doc::new("**bold**rest", true).bold("rest").shown_bold(),
            "boldrest"
        );
        assert_eq!(Doc::new("*it*rest", true).bold("rest").shown_bold(), "rest");
    }

    #[test]
    fn unbolding_markdown_bold_text() {
        // Everything visible is bold already: Ctrl+B turns it off, even
        // though the selection contains the hidden `**` markers.
        let mut doc = Doc::new("**one** **two**", true);
        assert_eq!(doc.shown_bold(), "one|two");
        assert_eq!(doc.bold("**one** **two**").shown_bold(), "");
    }

    #[test]
    fn unbolding_part_of_a_heading() {
        let mut doc = Doc::new("# Title words", true);
        assert_eq!(doc.bold("Title").shown_bold(), " words");
    }

    #[test]
    fn literal_markdown_when_markdown_is_off() {
        assert_eq!(Doc::new("**x** y", false).bold("x").shown_bold(), "x");
    }
}
