//! Character formatting as ranges over the text, the way Google Docs keeps
//! it: the text stays one string, and each span says "these bytes are
//! 14 pt Lora". Spans move with the text as it is edited, so formatting
//! stays on the words it was applied to.

use serde::{Deserialize, Serialize};
use std::ops::Range;

/// Formatting on a run of characters. `None` fields follow the paragraph
/// style (Normal, Heading 1…), as direct formatting does in Word.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CharFormat {
    /// Font size in points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    /// Font family.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// Bold, italic, underline, strikethrough: `Some(true)` on,
    /// `Some(false)` explicitly off (e.g. un-bolding a heading's words or
    /// `**`-bold text), `None` as the style / Markdown says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strike: Option<bool>,
}

/// The on/off character attributes that the B / I / U / S buttons toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    Bold,
    Italic,
    Underline,
    Strike,
}

impl CharFormat {
    /// This format's setting for `emphasis` (`None`: not set here).
    pub fn emphasis(&self, emphasis: Emphasis) -> Option<bool> {
        match emphasis {
            Emphasis::Bold => self.bold,
            Emphasis::Italic => self.italic,
            Emphasis::Underline => self.underline,
            Emphasis::Strike => self.strike,
        }
    }

    pub fn set_emphasis(&mut self, emphasis: Emphasis, value: Option<bool>) {
        match emphasis {
            Emphasis::Bold => self.bold = value,
            Emphasis::Italic => self.italic = value,
            Emphasis::Underline => self.underline = value,
            Emphasis::Strike => self.strike = value,
        }
    }
}

impl SpanFormat for CharFormat {
    fn is_empty(&self) -> bool {
        self.size.is_none()
            && self.font.is_none()
            && self.bold.is_none()
            && self.italic.is_none()
            && self.underline.is_none()
            && self.strike.is_none()
    }
}

/// Horizontal alignment of a paragraph.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

/// Formatting of one paragraph (one line of the text) over its style's:
/// Word's paragraph formatting. `None` fields follow the style.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParaFormat {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align: Option<Alignment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_spacing: Option<f32>,
    /// Points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_before: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_after: Option<f32>,
}

impl SpanFormat for ParaFormat {
    fn is_empty(&self) -> bool {
        self.align.is_none()
            && self.line_spacing.is_none()
            && self.space_before.is_none()
            && self.space_after.is_none()
    }
}

/// What a span can carry: a format with an "unformatted" default.
pub trait SpanFormat: Clone + Default + PartialEq {
    fn is_empty(&self) -> bool;
}

/// One formatted range of the text, in byte offsets `[start, end)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormatSpan<F = CharFormat> {
    pub start: usize,
    pub end: usize,
    pub format: F,
}

/// Formatting as ranges over the text: sorted, non-overlapping, non-empty
/// spans; unformatted text has no span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    transparent,
    bound(serialize = "F: Serialize", deserialize = "F: Deserialize<'de>")
)]
pub struct Spans<F> {
    spans: Vec<FormatSpan<F>>,
}

impl<F> Default for Spans<F> {
    fn default() -> Self {
        Self { spans: Vec::new() }
    }
}

/// Character formatting (size, font on words).
pub type FormatSpans = Spans<CharFormat>;

/// Paragraph formatting: each span covers whole lines, newline included
/// (see `paragraph_range`), so Enter at a paragraph's end carries its
/// formatting into the new paragraph, as in Word.
pub type ParagraphSpans = Spans<ParaFormat>;

/// The range paragraph formatting is stored over for the lines holding
/// `start..end` of `text`: from the first line's start to the last line's
/// end, plus its newline when there is one (so an empty line still has a
/// byte to carry formatting).
pub fn paragraph_range(text: &str, start: usize, end: usize) -> Range<usize> {
    let first = text[..start.min(text.len())]
        .rfind('\n')
        .map_or(0, |p| p + 1);
    let end = end.max(start).min(text.len());
    let last_end = text[end..].find('\n').map_or(text.len(), |p| end + p);
    first..(last_end + 1).min(text.len())
}

impl<F: SpanFormat> Spans<F> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    pub fn spans(&self) -> &[FormatSpan<F>] {
        &self.spans
    }

    /// The formatting of the character starting at byte `offset`.
    pub fn format_at(&self, offset: usize) -> F {
        self.spans
            .iter()
            .find(|s| s.start <= offset && offset < s.end)
            .map(|s| s.format.clone())
            .unwrap_or_default()
    }

    /// `range` split where the formatting changes, each piece with its
    /// formatting (unformatted pieces get `CharFormat::default()`).
    pub fn runs(&self, range: Range<usize>) -> Vec<(Range<usize>, F)> {
        let mut out: Vec<(Range<usize>, F)> = Vec::new();
        let mut pos = range.start;
        for span in &self.spans {
            if span.end <= pos {
                continue;
            }
            if span.start >= range.end {
                break;
            }
            if span.start > pos {
                out.push((pos..span.start, F::default()));
                pos = span.start;
            }
            let end = span.end.min(range.end);
            out.push((pos..end, span.format.clone()));
            pos = end;
        }
        if pos < range.end {
            out.push((pos..range.end, F::default()));
        }
        out
    }

    /// Change the formatting of `range` with `change` (e.g. set the size),
    /// keeping whatever else each part already had.
    pub fn apply(&mut self, range: Range<usize>, change: impl Fn(&mut F)) {
        if range.is_empty() {
            return;
        }
        let mut spans: Vec<FormatSpan<F>> = Vec::new();
        for span in &self.spans {
            // Keep the parts outside `range` as they are.
            if span.start < range.start {
                spans.push(FormatSpan {
                    start: span.start,
                    end: span.end.min(range.start),
                    format: span.format.clone(),
                });
            }
            if span.end > range.end {
                spans.push(FormatSpan {
                    start: span.start.max(range.end),
                    end: span.end,
                    format: span.format.clone(),
                });
            }
        }
        for (piece, mut format) in self.runs(range) {
            change(&mut format);
            spans.push(FormatSpan {
                start: piece.start,
                end: piece.end,
                format,
            });
        }
        spans.sort_by_key(|s| s.start);
        self.spans = spans;
        self.normalize();
    }

    /// Follow a text edit: `removed` bytes at `start` were replaced by
    /// `inserted` bytes. Spans after the edit shift; spans inside the
    /// removed text shrink. Inserted text takes the formatting of the
    /// character before it, as typing does in Word (typing at the end of
    /// 14 pt text continues in 14 pt).
    pub fn edit(&mut self, start: usize, removed: usize, inserted: usize) {
        let end = start + removed;
        let mut spans: Vec<FormatSpan<F>> = Vec::new();
        for span in &self.spans {
            let mut s = span.clone();
            // Remove [start, end).
            let cut = |x: usize| {
                if x <= start {
                    x
                } else if x >= end {
                    x - removed
                } else {
                    start
                }
            };
            s.start = cut(s.start);
            s.end = cut(s.end);
            // Insert `inserted` bytes at `start`: a span that ends at or
            // covers the insertion point grows (its character is before
            // the new text); spans after it shift.
            if inserted > 0 {
                if s.start < start && s.end >= start {
                    s.end += inserted;
                } else if s.start >= start {
                    s.start += inserted;
                    s.end += inserted;
                }
            }
            spans.push(s);
        }
        self.spans = spans;
        self.normalize();
    }

    /// Drop spans outside `len` or off character boundaries of `text`
    /// (after a failed save left text and formatting out of step).
    pub fn clamp_to(&mut self, text: &str) {
        let fix = |mut x: usize| {
            x = x.min(text.len());
            while !text.is_char_boundary(x) {
                x -= 1;
            }
            x
        };
        for span in &mut self.spans {
            span.start = fix(span.start);
            span.end = fix(span.end);
        }
        self.normalize();
    }

    /// Sorted, no empty or formatless spans, adjacent equal spans merged.
    fn normalize(&mut self) {
        self.spans
            .retain(|s| s.start < s.end && !s.format.is_empty());
        self.spans.sort_by_key(|s| s.start);
        let mut merged: Vec<FormatSpan<F>> = Vec::with_capacity(self.spans.len());
        for span in self.spans.drain(..) {
            match merged.last_mut() {
                Some(last) if last.end == span.start && last.format == span.format => {
                    last.end = span.end;
                }
                _ => merged.push(span),
            }
        }
        self.spans = merged;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(pt: f32) -> impl Fn(&mut CharFormat) {
        move |f: &mut CharFormat| f.size = Some(pt)
    }

    fn sizes(spans: &FormatSpans) -> Vec<(usize, usize, Option<f32>)> {
        spans
            .spans()
            .iter()
            .map(|s| (s.start, s.end, s.format.size))
            .collect()
    }

    #[test]
    fn applying_splits_and_merges_ranges() {
        let mut spans = FormatSpans::new();
        spans.apply(2..6, size(14.0));
        assert_eq!(sizes(&spans), [(2, 6, Some(14.0))]);
        // Overlapping change splits the old span.
        spans.apply(4..8, size(20.0));
        assert_eq!(sizes(&spans), [(2, 4, Some(14.0)), (4, 8, Some(20.0))]);
        // The same size again over both merges them.
        spans.apply(2..8, size(20.0));
        assert_eq!(sizes(&spans), [(2, 8, Some(20.0))]);
        // A font change keeps the size.
        spans.apply(3..5, |f| f.font = Some("Lora".into()));
        assert_eq!(spans.format_at(4).size, Some(20.0));
        assert_eq!(spans.format_at(4).font.as_deref(), Some("Lora"));
        assert_eq!(spans.format_at(6).font, None);
        // Clearing makes the text plain again.
        spans.apply(0..10, |f| *f = CharFormat::default());
        assert!(spans.is_empty());
    }

    #[test]
    fn runs_cover_the_range_with_gaps_as_plain() {
        let mut spans = FormatSpans::new();
        spans.apply(2..4, size(14.0));
        let runs: Vec<(Range<usize>, Option<f32>)> = spans
            .runs(0..6)
            .into_iter()
            .map(|(r, f)| (r, f.size))
            .collect();
        assert_eq!(runs, [(0..2, None), (2..4, Some(14.0)), (4..6, None)]);
        assert_eq!(spans.runs(2..3)[0].1.size, Some(14.0));
    }

    #[test]
    fn formatting_follows_the_text_through_edits() {
        let mut spans = FormatSpans::new();
        spans.apply(5..10, size(14.0)); // "hello [world]"
                                        // Typing before it shifts it.
        spans.edit(0, 0, 3);
        assert_eq!(sizes(&spans), [(8, 13, Some(14.0))]);
        // Typing at its end continues the formatting.
        spans.edit(13, 0, 2);
        assert_eq!(sizes(&spans), [(8, 15, Some(14.0))]);
        // Typing at its start does not (the character before is plain).
        spans.edit(8, 0, 1);
        assert_eq!(sizes(&spans), [(9, 16, Some(14.0))]);
        // Deleting inside shrinks it; deleting across its start trims it.
        spans.edit(10, 2, 0);
        assert_eq!(sizes(&spans), [(9, 14, Some(14.0))]);
        spans.edit(7, 4, 0);
        assert_eq!(sizes(&spans), [(7, 10, Some(14.0))]);
        // Deleting all of it removes it.
        spans.edit(6, 5, 0);
        assert!(spans.is_empty());
    }

    #[test]
    fn replacing_formatted_text_keeps_its_formatting() {
        // Select formatted text and type over it: the new text keeps it.
        let mut spans = FormatSpans::new();
        spans.apply(0..5, size(14.0));
        spans.edit(1, 3, 2);
        assert_eq!(sizes(&spans), [(0, 4, Some(14.0))]);
    }

    #[test]
    fn clamping_repairs_out_of_step_spans() {
        let mut spans = FormatSpans::new();
        spans.apply(1..50, size(14.0));
        spans.clamp_to("aé");
        assert_eq!(sizes(&spans), [(1, 3, Some(14.0))]);
        spans.clamp_to("a");
        assert!(spans.is_empty());
    }

    #[test]
    fn paragraph_ranges_cover_whole_lines_with_their_newline() {
        let text = "one\ntwo\n\nfour";
        assert_eq!(paragraph_range(text, 5, 5), 4..8);
        assert_eq!(paragraph_range(text, 1, 6), 0..8);
        assert_eq!(
            paragraph_range(text, 8, 8),
            8..9,
            "an empty line keeps its newline"
        );
        assert_eq!(
            paragraph_range(text, 12, 12),
            9..13,
            "the last line has none"
        );
    }

    #[test]
    fn enter_at_a_paragraph_end_keeps_its_formatting() {
        let text = "centred\nplain";
        let mut paras = ParagraphSpans::new();
        paras.apply(paragraph_range(text, 0, 0), |p| {
            p.align = Some(Alignment::Center)
        });
        // Enter at the end of "centred" inserts "\n" at 7, inside the span.
        paras.edit(7, 0, 1);
        assert_eq!(paras.format_at(0).align, Some(Alignment::Center));
        assert_eq!(
            paras.format_at(8).align,
            Some(Alignment::Center),
            "the new paragraph"
        );
        assert_eq!(paras.format_at(9).align, None, "plain stays plain");
    }

    #[test]
    fn spans_round_trip_as_json() {
        let mut spans = FormatSpans::new();
        spans.apply(0..3, size(14.0));
        let json = serde_json::to_string(&spans).unwrap();
        assert_eq!(json, r#"[{"start":0,"end":3,"format":{"size":14.0}}]"#);
        assert_eq!(serde_json::from_str::<FormatSpans>(&json).unwrap(), spans);
    }
}
