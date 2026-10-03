//! Character formatting (size, font on selected words) into the export's
//! text runs. The Markdown parser builds runs from slices of the editor
//! text; formatting is kept as ranges over that same text, so each run is
//! split where the formatting changes and carries it in `TextRun::format`.

use crate::{doc, inline_pieces, parse_inline_runs, InlinePiece};
use sylph_core::format::{CharFormat, FormatSpans, ParaFormat, ParagraphSpans};

/// Where the parser's text came from, and the formatting over it.
pub(crate) struct RunFormat<'a> {
    base: &'a str,
    formats: Option<&'a FormatSpans>,
    paras: Option<&'a ParagraphSpans>,
    /// The Normal style's space before/after (points), for paragraphs
    /// without their own.
    normal_spacing: (f32, f32),
}

impl<'a> RunFormat<'a> {
    /// No character formatting: runs exactly as `parse_inline_runs` makes.
    pub(crate) fn none(base: &'a str) -> Self {
        Self {
            base,
            formats: None,
            paras: None,
            normal_spacing: (0.0, sylph_core::document::NORMAL_SPACE_AFTER),
        }
    }

    /// `formats` are ranges over `base`, the text being parsed.
    pub(crate) fn with(base: &'a str, formats: &'a FormatSpans) -> Self {
        Self {
            base,
            formats: (!formats.is_empty()).then_some(formats),
            paras: None,
            normal_spacing: (0.0, sylph_core::document::NORMAL_SPACE_AFTER),
        }
    }

    /// Also carry paragraph formatting (`paras`, over `base`) and the
    /// Normal style's spacing into the blocks.
    pub(crate) fn with_paragraphs(
        mut self,
        paras: &'a ParagraphSpans,
        normal_spacing: (f32, f32),
    ) -> Self {
        self.paras = (!paras.is_empty()).then_some(paras);
        self.normal_spacing = normal_spacing;
        self
    }

    /// The paragraph formatting of the line `chunk` (a slice of the text)
    /// starts on.
    pub(crate) fn para_at(&self, chunk: &str) -> ParaFormat {
        match (self.paras, self.offset_of(chunk)) {
            (Some(paras), Some(offset)) => {
                let line_start = self.base[..offset].rfind('\n').map_or(0, |p| p + 1);
                paras.format_at(line_start)
            }
            _ => ParaFormat::default(),
        }
    }

    /// A paragraph's style: its own formatting over the Normal style's.
    pub(crate) fn paragraph_style(&self, chunk: &str, line_spacing: f32) -> doc::ParagraphStyle {
        let para = self.para_at(chunk);
        doc::ParagraphStyle {
            line_spacing: para.line_spacing.unwrap_or(line_spacing),
            space_before: para.space_before.unwrap_or(self.normal_spacing.0),
            space_after: para.space_after.unwrap_or(self.normal_spacing.1),
            alignment: para.align.unwrap_or_default(),
        }
    }

    /// The byte offset of `chunk` in `base`, when it is a slice of it.
    fn offset_of(&self, chunk: &str) -> Option<usize> {
        let offset = (chunk.as_ptr() as usize).checked_sub(self.base.as_ptr() as usize)?;
        (offset + chunk.len() <= self.base.len()).then_some(offset)
    }

    /// Inline-Markdown runs of `chunk` (a slice of the text).
    pub(crate) fn runs(&self, chunk: &str) -> Vec<doc::TextRun> {
        match (self.formats, self.offset_of(chunk)) {
            (Some(formats), Some(offset)) => runs_mapped(chunk, |i| offset + i, formats),
            _ => parse_inline_runs(chunk),
        }
    }

    /// Runs of `lines` (slices of the text) joined with spaces, as a
    /// paragraph or quote soft-wraps them.
    pub(crate) fn joined_runs(&self, lines: &[&str]) -> Vec<doc::TextRun> {
        let joined = lines.join(" ");
        let Some(formats) = self.formats else {
            return parse_inline_runs(&joined);
        };
        // Source offset of every byte of `joined` (a joining space takes
        // the offset of the end of the line before it).
        let mut map: Vec<usize> = Vec::with_capacity(joined.len());
        for (n, line) in lines.iter().enumerate() {
            let Some(offset) = self.offset_of(line) else {
                return parse_inline_runs(&joined);
            };
            if n > 0 {
                map.push(map.last().map_or(offset, |&last| last + 1));
            }
            map.extend(offset..offset + line.len());
        }
        runs_mapped(&joined, |i| map.get(i).copied().unwrap_or(0), formats)
    }

    /// One literal run per formatting change of `line` (Markdown off: no
    /// inline parsing).
    pub(crate) fn literal_runs(&self, line: &str) -> Vec<doc::TextRun> {
        match (self.formats, self.offset_of(line)) {
            (Some(formats), Some(offset)) => formats
                .runs(offset..offset + line.len())
                .into_iter()
                .map(|(range, format)| doc::TextRun {
                    text: line[range.start - offset..range.end - offset].to_string(),
                    styles: Vec::new(),
                    format,
                })
                .collect(),
            _ => vec![doc::TextRun::plain(line)],
        }
    }
}

/// `parse_inline_runs`, with each run also split where the formatting of
/// its source bytes (`src_of` maps a byte of `text` to the source) changes.
fn runs_mapped(
    text: &str,
    src_of: impl Fn(usize) -> usize,
    formats: &FormatSpans,
) -> Vec<doc::TextRun> {
    let mut runs: Vec<doc::TextRun> = Vec::new();
    // The inline piece each run came from: text of the same piece and the
    // same formatting continues one run, as in `parse_inline_runs`.
    let mut run_ids: Vec<usize> = Vec::new();
    let mut pos = 0;
    for piece in inline_pieces(text) {
        match piece {
            InlinePiece::Syntax(len) => pos += len,
            InlinePiece::Text { len, run, styles } => {
                let chunk = &text[pos..pos + len];
                let mut pieces: Vec<(usize, usize, CharFormat)> = Vec::new();
                for (i, c) in chunk.char_indices() {
                    let format = formats.format_at(src_of(pos + i));
                    let end = i + c.len_utf8();
                    match pieces.last_mut() {
                        Some((_, e, f)) if *f == format => *e = end,
                        _ => pieces.push((i, end, format)),
                    }
                }
                for (start, end, format) in pieces {
                    let part = &chunk[start..end];
                    match (runs.last_mut(), run_ids.last()) {
                        (Some(last), Some(&id)) if id == run && last.format == format => {
                            last.text.push_str(part);
                        }
                        _ => {
                            let mut new_run = doc::TextRun::styled(part, styles.clone());
                            new_run.format = format;
                            runs.push(new_run);
                            run_ids.push(run);
                        }
                    }
                }
                pos += len;
            }
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::RunFormat;
    use crate::parse_inline_runs;
    use sylph_core::format::FormatSpans;

    fn sized(text: &str, range: std::ops::Range<usize>, pt: f32) -> FormatSpans {
        let mut spans = FormatSpans::new();
        spans.apply(range, |f| f.size = Some(pt));
        assert!(text.len() >= spans.spans()[0].end);
        spans
    }

    #[test]
    fn without_formatting_runs_are_unchanged() {
        let text = "Plain **bold** and *it* [link](https://x.y)";
        let spans = FormatSpans::new();
        assert_eq!(
            RunFormat::with(text, &spans).runs(text),
            parse_inline_runs(text)
        );
        assert_eq!(RunFormat::none(text).runs(text), parse_inline_runs(text));
    }

    #[test]
    fn formatted_words_become_their_own_runs() {
        let text = "one **two three** four";
        // "three" (inside the bold) and " fo" are 20 pt.
        let start = text.find("three").unwrap();
        let spans = sized(text, start..text.find("ur").unwrap(), 20.0);
        let runs = RunFormat::with(text, &spans).runs(text);
        let described: Vec<(String, Option<f32>, bool)> = runs
            .iter()
            .map(|r| {
                (
                    r.text.clone(),
                    r.format.size,
                    r.styles.contains(&sylph_core::document::SpanStyle::Bold),
                )
            })
            .collect();
        assert_eq!(
            described,
            [
                ("one ".to_string(), None, false),
                ("two ".to_string(), None, true),
                ("three".to_string(), Some(20.0), true),
                (" fo".to_string(), Some(20.0), false),
                ("ur".to_string(), None, false),
            ]
        );
    }

    #[test]
    fn joined_lines_map_back_to_their_source() {
        let text = "first line\nsecond line";
        let spans = sized(text, text.find("second").unwrap()..text.len(), 18.0);
        let lines: Vec<&str> = text.split('\n').collect();
        let runs = RunFormat::with(text, &spans).joined_runs(&lines);
        let described: Vec<(&str, Option<f32>)> = runs
            .iter()
            .map(|r| (r.text.as_str(), r.format.size))
            .collect();
        assert_eq!(
            described,
            [("first line ", None), ("second line", Some(18.0))]
        );
    }

    #[test]
    fn literal_lines_split_by_formatting_only() {
        let text = "# not a heading **raw**";
        let spans = sized(text, 2..5, 14.0);
        let runs = RunFormat::with(text, &spans).literal_runs(text);
        let described: Vec<(&str, Option<f32>)> = runs
            .iter()
            .map(|r| (r.text.as_str(), r.format.size))
            .collect();
        assert_eq!(
            described,
            [
                ("# ", None),
                ("not", Some(14.0)),
                (" a heading **raw**", None)
            ]
        );
    }

    #[test]
    fn paragraph_formatting_reaches_the_blocks() {
        use sylph_core::document::{Block, Document};
        use sylph_core::format::{paragraph_range, Alignment};
        let text = "# Title\nCentred line\nPlain line";
        let mut model = Document::new();
        model.para_formats.apply(paragraph_range(text, 0, 0), |p| {
            p.align = Some(Alignment::Center)
        });
        let second = text.find("Centred").unwrap();
        model
            .para_formats
            .apply(paragraph_range(text, second, second), |p| {
                p.align = Some(Alignment::Justify);
                p.line_spacing = Some(2.0);
                p.space_after = Some(20.0);
            });
        for markdown in [true, false] {
            let blocks = crate::export_model(&model, text, markdown).blocks;
            let plain = blocks.last().unwrap();
            match plain {
                Block::Paragraph { style, .. } => {
                    assert_eq!(style.alignment, Alignment::Left);
                    assert_eq!(style.space_after, model.space_after, "Normal's spacing");
                }
                b => panic!("{b:?}"),
            }
            let centred = blocks
                .iter()
                .find(|b| b.plain_text() == "Centred line")
                .unwrap();
            match centred {
                Block::Paragraph { style, .. } => {
                    assert_eq!(style.alignment, Alignment::Justify);
                    assert_eq!((style.line_spacing, style.space_after), (2.0, 20.0));
                }
                b => panic!("{b:?}"),
            }
            if markdown {
                assert!(matches!(
                    blocks[0],
                    Block::Heading {
                        alignment: Alignment::Center,
                        ..
                    }
                ));
            }
        }
    }

    #[test]
    fn text_that_is_not_a_slice_of_the_source_is_left_plain() {
        let text = "abc";
        let spans = sized(text, 0..3, 30.0);
        let copy = String::from("abc");
        assert_eq!(
            RunFormat::with(text, &spans).runs(&copy),
            parse_inline_runs("abc")
        );
    }
}
