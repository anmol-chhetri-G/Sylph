//! One canvas row whose text may mix font sizes and families (character
//! formatting on selected words). GPUI shapes a line at a single size, so
//! a row is a list of segments, one per run of equal size, shaped on their
//! own and placed side by side on a shared baseline. A row without
//! character formatting is a single segment, exactly as before.

use gpui::{point, px, App, Pixels, Point, ShapedLine, TextRun, Window};
use std::ops::Range;
use sylph_core::format::{CharFormat, SpanFormat};

/// Points to canvas pixels (96 dpi).
const PX_PER_PT: f32 = 4.0 / 3.0;

#[derive(Clone)]
struct Segment {
    /// Byte offset of the segment in the row's text.
    start: usize,
    /// Left edge within the row.
    x: Pixels,
    shaped: ShapedLine,
    /// A table cell's horizontal extent: clicks anywhere in it land in
    /// this segment, and painting is clipped to it.
    cell: Option<(Pixels, Pixels)>,
}

#[derive(Clone)]
pub(crate) struct RowText {
    segments: Vec<Segment>,
}

impl RowText {
    /// Shape `text` with `runs` (colours and fonts, covering the text) at
    /// `base_size`, where `formats` (row-relative ranges covering the text)
    /// may override size and font family.
    pub(crate) fn shape(
        window: &Window,
        text: &str,
        runs: &[TextRun],
        base_size: Pixels,
        formats: &[(Range<usize>, CharFormat)],
    ) -> Self {
        let plain = formats.iter().all(|(_, f)| f.is_empty());
        if plain || text.is_empty() {
            let shaped =
                window
                    .text_system()
                    .shape_line(text.to_string().into(), base_size, runs, None);
            return Self {
                segments: vec![Segment {
                    start: 0,
                    x: px(0.0),
                    shaped,
                    cell: None,
                }],
            };
        }
        // Split the style runs where the formatting changes, then group
        // consecutive pieces of equal size into segments.
        let pieces = split_runs(runs, formats, base_size);
        let mut segments: Vec<Segment> = Vec::new();
        let mut x = px(0.0);
        let mut i = 0;
        while i < pieces.len() {
            let size = pieces[i].1;
            let start = pieces[i].0.start;
            let mut end = pieces[i].0.end;
            let mut group_runs: Vec<TextRun> = vec![pieces[i].2.clone()];
            let mut j = i + 1;
            while j < pieces.len() && pieces[j].1 == size {
                end = pieces[j].0.end;
                group_runs.push(pieces[j].2.clone());
                j += 1;
            }
            let shaped = window.text_system().shape_line(
                text[start..end].to_string().into(),
                size,
                &group_runs,
                None,
            );
            let width = shaped.width;
            segments.push(Segment {
                start,
                x,
                shaped,
                cell: None,
            });
            x += width;
            i = j;
        }
        Self { segments }
    }

    /// The row moved right by `dx` (alignment). Positions reported by
    /// `x_for_index` and taken by `closest_index_for_x` include it.
    pub(crate) fn offset_by(mut self, dx: Pixels) -> Self {
        for seg in &mut self.segments {
            seg.x += dx;
        }
        self
    }

    /// Shape a justified row: words spread so the row's last visible
    /// character reaches `target` (Word's justify; not used for a
    /// paragraph's last row). Falls back to a normal row with one word.
    pub(crate) fn shape_justified(
        window: &Window,
        text: &str,
        runs: &[TextRun],
        base_size: Pixels,
        formats: &[(Range<usize>, CharFormat)],
        target: Pixels,
    ) -> Self {
        // Word pieces: each ends after its run of spaces.
        let mut cuts = vec![0];
        let bytes = text.as_bytes();
        for i in 0..bytes.len() {
            if bytes[i] == b' ' && bytes.get(i + 1).is_some_and(|&b| b != b' ') {
                cuts.push(i + 1);
            }
        }
        cuts.push(text.len());
        if cuts.len() < 3 {
            return Self::shape(window, text, runs, base_size, formats);
        }
        let pieces: Vec<RowText> = cuts
            .windows(2)
            .map(|w| {
                let range = w[0]..w[1];
                Self::shape(
                    window,
                    &text[range.clone()],
                    &slice_runs(runs, range.clone()),
                    base_size,
                    &slice_formats(formats, range),
                )
            })
            .collect();
        // The last piece's trailing spaces are invisible: justify to its
        // last visible character.
        let last = pieces.last().expect("at least two pieces");
        let last_text = &text[cuts[cuts.len() - 2]..];
        let natural: Pixels = pieces[..pieces.len() - 1]
            .iter()
            .map(RowText::width)
            .fold(px(0.0), |a, b| a + b)
            + last.x_for_index(last_text.trim_end().len());
        let gaps = (pieces.len() - 1) as f32;
        let extra = ((target - natural) / gaps).max(px(0.0));
        let mut segments = Vec::new();
        let mut x = px(0.0);
        for (k, piece) in pieces.into_iter().enumerate() {
            let width = piece.width();
            for mut seg in piece.segments {
                seg.start += cuts[k];
                seg.x += x;
                segments.push(seg);
            }
            x += width + extra;
        }
        Self { segments }
    }

    /// A table row: each cell's display range shaped on its own and placed
    /// at its column (`left`, `width`), text inset by `pad`. Separators
    /// between cells are not drawn.
    pub(crate) fn shape_cells(
        window: &Window,
        text: &str,
        runs: &[TextRun],
        base_size: Pixels,
        formats: &[(Range<usize>, CharFormat)],
        cells: &[(Range<usize>, Pixels, Pixels)],
        pad: Pixels,
    ) -> Self {
        let mut segments = Vec::new();
        for (range, left, width) in cells {
            let piece = Self::shape(
                window,
                &text[range.clone()],
                &slice_runs(runs, range.clone()),
                base_size,
                &slice_formats(formats, range.clone()),
            );
            for mut seg in piece.segments {
                seg.start += range.start;
                seg.x += *left + pad;
                seg.cell = Some((*left, *left + *width));
                segments.push(seg);
            }
        }
        if segments.is_empty() {
            return Self::shape(window, text, runs, base_size, formats);
        }
        Self { segments }
    }

    pub(crate) fn width(&self) -> Pixels {
        self.segments
            .last()
            .map_or(px(0.0), |s| s.x + s.shaped.width)
    }

    /// The largest font size in the row (for its height).
    pub(crate) fn max_font_size(&self) -> Pixels {
        self.segments
            .iter()
            .map(|s| s.shaped.font_size)
            .fold(px(0.0), |a, b| a.max(b))
    }

    fn segment_for_index(&self, index: usize) -> &Segment {
        self.segments
            .iter()
            .rev()
            .find(|s| s.start <= index)
            .unwrap_or(&self.segments[0])
    }

    fn segment_for_x(&self, x: Pixels) -> &Segment {
        self.segments
            .iter()
            .find(|s| x < s.cell.map_or(s.x + s.shaped.width, |(_, right)| right))
            .unwrap_or_else(|| self.segments.last().expect("a row has a segment"))
    }

    pub(crate) fn x_for_index(&self, index: usize) -> Pixels {
        let seg = self.segment_for_index(index);
        seg.x + seg.shaped.x_for_index(index - seg.start)
    }

    pub(crate) fn closest_index_for_x(&self, x: Pixels) -> usize {
        let seg = self.segment_for_x(x);
        seg.start + seg.shaped.closest_index_for_x(x - seg.x)
    }

    pub(crate) fn index_for_x(&self, x: Pixels) -> Option<usize> {
        let seg = self.segment_for_x(x);
        seg.shaped
            .index_for_x(x - seg.x)
            .map(|index| seg.start + index)
    }

    /// Paint in a box of `line_height` at `origin`, every segment on the
    /// row's common baseline (GPUI centres each line's own glyph box).
    pub(crate) fn paint(
        &self,
        origin: Point<Pixels>,
        line_height: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) {
        let ascent = self
            .segments
            .iter()
            .map(|s| s.shaped.ascent)
            .fold(px(0.0), |a, b| a.max(b));
        let descent = self
            .segments
            .iter()
            .map(|s| s.shaped.descent)
            .fold(px(0.0), |a, b| a.max(b));
        let baseline = (line_height - ascent - descent) / 2.0 + ascent;
        for seg in &self.segments {
            let own =
                (line_height - seg.shaped.ascent - seg.shaped.descent) / 2.0 + seg.shaped.ascent;
            let at = point(origin.x + seg.x, origin.y + baseline - own);
            match seg.cell {
                // A table cell's text never spills into its neighbour.
                Some((left, right)) => {
                    let mask = gpui::ContentMask {
                        bounds: gpui::Bounds::from_corners(
                            point(origin.x + left, origin.y),
                            point(origin.x + right, origin.y + line_height),
                        ),
                    };
                    window.with_content_mask(Some(mask), |window| {
                        let _ = seg.shaped.paint(at, line_height, window, cx);
                    });
                }
                None => {
                    let _ = seg.shaped.paint(at, line_height, window, cx);
                }
            }
        }
    }
}

/// `runs` split at the `formats` boundaries: each piece's byte range, its
/// font size in px (the format's, else `base_size`), and the run with the
/// format's font family applied.
fn split_runs(
    runs: &[TextRun],
    formats: &[(Range<usize>, CharFormat)],
    base_size: Pixels,
) -> Vec<(Range<usize>, Pixels, TextRun)> {
    let mut out = Vec::new();
    let mut pos = 0;
    for run in runs {
        let range = pos..pos + run.len;
        pos += run.len;
        for (fmt_range, format) in formats {
            let start = range.start.max(fmt_range.start);
            let end = range.end.min(fmt_range.end);
            if start >= end {
                continue;
            }
            let mut piece = run.clone();
            piece.len = end - start;
            if let Some(family) = &format.font {
                piece.font.family = crate::fonts::render_family(family).to_string().into();
            }
            apply_emphasis(&mut piece, format);
            let size = format.size.map_or(base_size, |pt| px(pt * PX_PER_PT));
            out.push((start..end, size, piece));
        }
    }
    out
}

/// The character formatting over display bytes `range` of `dl`, as
/// ranges relative to `range.start` covering it. Formatting is kept on
/// source offsets, so each display character looks up its source byte.
pub(crate) fn line_formats(
    dl: &crate::DisplayLine,
    range: Range<usize>,
    formats: &sylph_core::format::FormatSpans,
) -> Vec<(Range<usize>, CharFormat)> {
    let whole = vec![(0..range.len(), CharFormat::default())];
    if formats.is_empty() || range.is_empty() {
        return whole;
    }
    let src_start = dl.src_offset + dl.disp_to_src(range.start);
    let src_end = dl.src_offset + dl.disp_to_src(range.end);
    if !formats
        .spans()
        .iter()
        .any(|s| s.start < src_end.max(src_start + 1) && s.end > src_start)
    {
        return whole;
    }
    let text = &dl.text[range.clone()];
    let mut out: Vec<(Range<usize>, CharFormat)> = Vec::new();
    for (i, _) in text.char_indices() {
        // The character's own source byte (not the caret position, which
        // after hidden `**` would read the marker's formatting).
        let src = dl.src_offset + dl.char_src(range.start + i);
        let format = formats.format_at(src);
        match out.last_mut() {
            Some((r, f)) if *f == format => r.end = i,
            _ => out.push((i..i, format)),
        }
        // Each piece ends where the next character starts.
        if let Some((r, _)) = out.last_mut() {
            r.end = text[i..].chars().next().map_or(i, |c| i + c.len_utf8());
        }
    }
    out
}

/// The runs covering `range` of the text, cut to it.
fn slice_runs(runs: &[TextRun], range: Range<usize>) -> Vec<TextRun> {
    let mut out = Vec::new();
    let mut pos = 0;
    for run in runs {
        let (start, end) = (pos.max(range.start), (pos + run.len).min(range.end));
        if start < end {
            let mut piece = run.clone();
            piece.len = end - start;
            out.push(piece);
        }
        pos += run.len;
    }
    out
}

/// `formats` (row-relative) cut to `range` and made relative to it.
fn slice_formats(
    formats: &[(Range<usize>, CharFormat)],
    range: Range<usize>,
) -> Vec<(Range<usize>, CharFormat)> {
    formats
        .iter()
        .filter_map(|(r, f)| {
            let (start, end) = (r.start.max(range.start), r.end.min(range.end));
            (start < end).then(|| (start - range.start..end - range.start, f.clone()))
        })
        .collect()
}

/// Bold / italic / underline / strikethrough from character formatting on
/// a run; `Some(false)` turns off what the style or Markdown gave it.
fn apply_emphasis(run: &mut TextRun, format: &CharFormat) {
    match format.bold {
        Some(true) => run.font.weight = gpui::FontWeight::BOLD,
        Some(false) => run.font.weight = gpui::FontWeight::NORMAL,
        None => {}
    }
    match format.italic {
        Some(true) => run.font.style = gpui::FontStyle::Italic,
        Some(false) => run.font.style = gpui::FontStyle::Normal,
        None => {}
    }
    match format.underline {
        Some(true) => {
            run.underline = Some(gpui::UnderlineStyle {
                thickness: px(1.0),
                color: Some(run.color),
                wavy: false,
            })
        }
        Some(false) => run.underline = None,
        None => {}
    }
    match format.strike {
        Some(true) => {
            run.strikethrough = Some(gpui::StrikethroughStyle {
                thickness: px(1.0),
                color: Some(run.color),
            })
        }
        Some(false) => run.strikethrough = None,
        None => {}
    }
}
