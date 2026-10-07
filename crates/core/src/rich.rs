//! The rich-text model (plan cards R1–R7): a document whose text holds no
//! Markdown syntax.
//!
//! The text is a list of paragraphs, each ended by `'\n'`. The last one
//! has its `'\n'` too, like Word's final paragraph mark, so even an empty
//! last paragraph has a byte to carry its formatting.
//!
//! - What a paragraph is (heading, list item, quote, code line, table row,
//!   caption) is its `ParaFormat::kind`.
//! - Bold, links and inline code are `CharFormat`s.
//! - Images, rules and page breaks are `OBJECT_CHAR`s whose
//!   `CharFormat::object` names one of the objects.
//! - A table row's cells are separated by `CELL_SEPARATOR`.
//!
//! This module converts between that model and the export blocks (`Block`,
//! what the exporters read): `from_blocks` brings in whatever the Markdown
//! parser made, and `to_blocks` exports.

use crate::document::{
    Block, DocObject, ListItem, ParagraphStyle, SpanStyle, TableCell, TableData, TableProps,
    TextRun,
};
use crate::format::{
    CharFormat, FormatSpan, FormatSpans, ParaFormat, ParaKind, ParagraphSpans, CELL_SEPARATOR,
    OBJECT_CHAR,
};
use crate::text::snap_to_char_boundary;
use std::collections::BTreeMap;
use std::ops::Range;

/// A rich document's content: its text and what lies over it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RichText {
    pub text: String,
    pub chars: FormatSpans,
    pub paras: ParagraphSpans,
    pub objects: BTreeMap<u32, DocObject>,
}

impl RichText {
    /// The export blocks (see `to_blocks`).
    pub fn to_blocks(&self, normal: &ParagraphStyle) -> Vec<Block> {
        to_blocks(&self.text, &self.chars, &self.paras, &self.objects, normal)
    }
}

/// The content range (without its `'\n'`) of every paragraph of `text`.
/// A text missing its final `'\n'` still ends with a paragraph.
pub fn paragraphs(text: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut start = 0;
    std::iter::from_fn(move || {
        if start >= text.len() {
            return None;
        }
        let end = text[start..].find('\n').map_or(text.len(), |p| start + p);
        let range = start..end;
        start = end + 1;
        Some(range)
    })
}

/// The rich form of export `blocks`. `normal` is the Normal style: a
/// paragraph whose spacing or alignment equals it follows the style instead
/// of holding its own.
///
/// Cover pages are not text and are left out (they stay in
/// `Document::blocks`). The two structure characters are removed from
/// incoming text, so a pasted `U+FFFC` never becomes an object, and line
/// breaks inside a run become spaces.
pub fn from_blocks(blocks: &[Block], normal: &ParagraphStyle) -> RichText {
    let mut out = Builder::default();
    for block in blocks {
        match block {
            Block::Paragraph { runs, style } => {
                out.paragraph(paragraph_format(style, normal), |b| b.runs(runs));
            }
            Block::Heading {
                level,
                runs,
                alignment,
            } => {
                let format = ParaFormat {
                    kind: Some(ParaKind::Heading(*level)),
                    align: (*alignment != normal.alignment).then_some(*alignment),
                    ..Default::default()
                };
                out.paragraph(format, |b| b.runs(runs));
            }
            Block::List { items } => {
                for item in items {
                    let kind = ParaKind::ListItem {
                        ordered: item.ordered,
                        level: item.level,
                        checked: item.checked,
                    };
                    out.paragraph(of_kind(kind), |b| b.runs(&item.runs));
                }
            }
            Block::Quote { level, runs } => {
                out.paragraph(of_kind(ParaKind::Quote(*level)), |b| b.runs(runs));
            }
            Block::CodeBlock { language, text } => {
                for line in text.split('\n') {
                    let kind = ParaKind::Code {
                        language: language.clone(),
                    };
                    out.paragraph(of_kind(kind), |b| b.plain(line));
                }
            }
            Block::Table { data } => {
                if data.rows.is_empty() {
                    continue;
                }
                let table = out.add_object(DocObject::Table(TableProps {
                    column_widths: data.column_widths.clone(),
                    caption: data.caption.clone(),
                    ..Default::default()
                }));
                for row in &data.rows {
                    out.paragraph(of_kind(ParaKind::TableRow { table }), |b| {
                        for (n, cell) in row.iter().enumerate() {
                            if n > 0 {
                                b.text.push(CELL_SEPARATOR);
                            }
                            b.runs(&cell.runs);
                        }
                    });
                }
            }
            Block::Image { data } => {
                out.paragraph(ParaFormat::default(), |b| {
                    b.object(DocObject::Image(data.clone()))
                });
            }
            Block::HorizontalRule => {
                out.paragraph(ParaFormat::default(), |b| {
                    b.object(DocObject::HorizontalRule)
                });
            }
            Block::PageBreak => {
                out.paragraph(ParaFormat::default(), |b| b.object(DocObject::PageBreak));
            }
            Block::Caption { text } => {
                out.paragraph(of_kind(ParaKind::Caption), |b| b.plain(text));
            }
            Block::CoverPage { .. } => {}
        }
    }
    if out.text.is_empty() {
        // A document always has a paragraph to type in.
        out.paragraph(ParaFormat::default(), |_| {});
    }
    out.finish()
}

/// The export blocks of a rich document. Consecutive list items form one
/// list, until a top-level item switches between numbered and bulleted;
/// consecutive code lines in one language form one code block; the rows
/// of one table form that table.
pub fn to_blocks(
    text: &str,
    chars: &FormatSpans,
    paras: &ParagraphSpans,
    objects: &BTreeMap<u32, DocObject>,
    normal: &ParagraphStyle,
) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    let mut group: Option<Group> = None;
    for range in paragraphs(text) {
        let para = paras.format_at(range.start);
        match para.kind {
            Some(ParaKind::ListItem {
                ordered,
                level,
                checked,
            }) => {
                let item = ListItem {
                    level,
                    ordered,
                    checked,
                    runs: runs_of(text, chars, range),
                };
                match &mut group {
                    Some(Group::List {
                        items,
                        ordered: list_ordered,
                    }) if level > 0 || ordered == *list_ordered => items.push(item),
                    _ => {
                        flush(&mut group, &mut out, objects);
                        group = Some(Group::List {
                            items: vec![item],
                            ordered,
                        });
                    }
                }
            }
            Some(ParaKind::Code { language }) => {
                let line = plain_of(text, range);
                match &mut group {
                    Some(Group::Code {
                        language: open,
                        lines,
                    }) if *open == language => lines.push(line),
                    _ => {
                        flush(&mut group, &mut out, objects);
                        group = Some(Group::Code {
                            language,
                            lines: vec![line],
                        });
                    }
                }
            }
            Some(ParaKind::TableRow { table }) => {
                let row = cells_of(text, chars, range);
                match &mut group {
                    Some(Group::Table { id, rows }) if *id == table => rows.push(row),
                    _ => {
                        flush(&mut group, &mut out, objects);
                        group = Some(Group::Table {
                            id: table,
                            rows: vec![row],
                        });
                    }
                }
            }
            kind => {
                flush(&mut group, &mut out, objects);
                out.push(match kind {
                    Some(ParaKind::Heading(level)) => Block::Heading {
                        level,
                        runs: runs_of(text, chars, range),
                        alignment: para.align.unwrap_or(normal.alignment),
                    },
                    Some(ParaKind::Quote(level)) => Block::Quote {
                        level,
                        runs: runs_of(text, chars, range),
                    },
                    Some(ParaKind::Caption) => Block::Caption {
                        text: plain_of(text, range),
                    },
                    _ => match object_of(text, chars, range.clone(), objects) {
                        Some(block) => block,
                        None => Block::Paragraph {
                            runs: runs_of(text, chars, range),
                            style: ParagraphStyle {
                                line_spacing: para.line_spacing.unwrap_or(normal.line_spacing),
                                space_before: para.space_before.unwrap_or(normal.space_before),
                                space_after: para.space_after.unwrap_or(normal.space_after),
                                alignment: para.align.unwrap_or(normal.alignment),
                            },
                        },
                    },
                });
            }
        }
    }
    flush(&mut group, &mut out, objects);
    out
}

/// Paragraphs that export as one block together.
enum Group {
    List {
        items: Vec<ListItem>,
        ordered: bool,
    },
    Code {
        language: String,
        lines: Vec<String>,
    },
    Table {
        id: u32,
        rows: Vec<Vec<TableCell>>,
    },
}

fn flush(group: &mut Option<Group>, out: &mut Vec<Block>, objects: &BTreeMap<u32, DocObject>) {
    let Some(group) = group.take() else {
        return;
    };
    out.push(match group {
        Group::List { items, .. } => Block::List { items },
        Group::Code { language, lines } => Block::CodeBlock {
            language,
            text: lines.join("\n"),
        },
        Group::Table { id, mut rows } => {
            let props = match objects.get(&id) {
                Some(DocObject::Table(props)) => props.clone(),
                _ => TableProps::default(),
            };
            let columns = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
            for row in &mut rows {
                row.resize_with(columns, || TableCell { runs: Vec::new() });
            }
            let column_widths = if props.column_widths.len() == columns {
                props.column_widths
            } else {
                vec![100.0 / columns as f32; columns]
            };
            Block::Table {
                data: TableData {
                    rows,
                    caption: props.caption,
                    column_widths,
                },
            }
        }
    });
}

/// The block of a paragraph that is one object (image, rule, page break).
fn object_of(
    text: &str,
    chars: &FormatSpans,
    range: Range<usize>,
    objects: &BTreeMap<u32, DocObject>,
) -> Option<Block> {
    let content = text.get(range.clone())?;
    if content.len() != OBJECT_CHAR.len_utf8() || !content.starts_with(OBJECT_CHAR) {
        return None;
    }
    match objects.get(&chars.format_at(range.start).object?)? {
        DocObject::Image(data) => Some(Block::Image { data: data.clone() }),
        DocObject::HorizontalRule => Some(Block::HorizontalRule),
        DocObject::PageBreak => Some(Block::PageBreak),
        DocObject::Table(_) => None,
    }
}

/// Is `c` one of the characters that mark structure, not text?
fn is_structure(c: char) -> bool {
    c == OBJECT_CHAR || c == CELL_SEPARATOR
}

/// The export runs of `range`: one per change of formatting.
fn runs_of(text: &str, chars: &FormatSpans, range: Range<usize>) -> Vec<TextRun> {
    let mut runs: Vec<TextRun> = Vec::new();
    for (piece, format) in chars.runs(range) {
        // Span ends off a character boundary (damaged data) move back to
        // one, so the text is still all exported.
        let start = snap_to_char_boundary(text, piece.start);
        let end = snap_to_char_boundary(text, piece.end);
        let chunk: String = text[start..end.max(start)]
            .chars()
            .filter(|&c| !is_structure(c))
            .collect();
        if chunk.is_empty() {
            continue;
        }
        let run = export_run(chunk, &format);
        match runs.last_mut() {
            Some(last) if last.styles == run.styles && last.format == run.format => {
                last.text.push_str(&run.text);
            }
            _ => runs.push(run),
        }
    }
    runs
}

/// A table row's cells.
fn cells_of(text: &str, chars: &FormatSpans, range: Range<usize>) -> Vec<TableCell> {
    let mut cells = Vec::new();
    let mut start = range.start;
    for (at, c) in text[range.clone()].char_indices() {
        if c == CELL_SEPARATOR {
            cells.push(TableCell {
                runs: runs_of(text, chars, start..range.start + at),
            });
            start = range.start + at + c.len_utf8();
        }
    }
    cells.push(TableCell {
        runs: runs_of(text, chars, start..range.end),
    });
    cells
}

/// The text of `range` without structure characters.
fn plain_of(text: &str, range: Range<usize>) -> String {
    text[range].chars().filter(|&c| !is_structure(c)).collect()
}

/// An export run: on/off attributes become the style flags the exporters
/// read; size and font stay in its format.
fn export_run(text: String, format: &CharFormat) -> TextRun {
    let on = |value: Option<bool>| value == Some(true);
    let mut styles = Vec::new();
    match (on(format.bold), on(format.italic)) {
        (true, true) => styles.push(SpanStyle::BoldItalic),
        (true, false) => styles.push(SpanStyle::Bold),
        (false, true) => styles.push(SpanStyle::Italic),
        (false, false) => {}
    }
    if on(format.underline) {
        styles.push(SpanStyle::Underline);
    }
    if on(format.strike) {
        styles.push(SpanStyle::Strikethrough);
    }
    if on(format.code) {
        styles.push(SpanStyle::Code);
    }
    if let Some(url) = &format.link {
        styles.push(SpanStyle::Link(url.clone()));
    }
    TextRun {
        text,
        styles,
        format: CharFormat {
            size: format.size,
            font: format.font.clone(),
            color: format.color.clone(),
            highlight: format.highlight.clone(),
            ..Default::default()
        },
    }
}

/// An export run's character formatting: its style flags, overridden by
/// any on/off its format still holds.
fn run_format(run: &TextRun) -> CharFormat {
    let has = |style: SpanStyle| run.styles.contains(&style);
    let set = |own: Option<bool>, from_styles: bool| own.unwrap_or(from_styles).then_some(true);
    CharFormat {
        size: run.format.size,
        font: run.format.font.clone(),
        color: run.format.color.clone(),
        highlight: run.format.highlight.clone(),
        bold: set(
            run.format.bold,
            has(SpanStyle::Bold) || has(SpanStyle::BoldItalic),
        ),
        italic: set(
            run.format.italic,
            has(SpanStyle::Italic) || has(SpanStyle::BoldItalic),
        ),
        underline: set(run.format.underline, has(SpanStyle::Underline)),
        strike: set(run.format.strike, has(SpanStyle::Strikethrough)),
        code: set(run.format.code, has(SpanStyle::Code)),
        link: run.format.link.clone().or_else(|| {
            run.styles.iter().find_map(|style| match style {
                SpanStyle::Link(url) => Some(url.clone()),
                _ => None,
            })
        }),
        object: None,
    }
}

/// A Normal paragraph's own formatting: whatever differs from `normal`.
fn paragraph_format(style: &ParagraphStyle, normal: &ParagraphStyle) -> ParaFormat {
    let own = |value: f32, normal: f32| (value != normal).then_some(value);
    ParaFormat {
        kind: None,
        align: (style.alignment != normal.alignment).then_some(style.alignment),
        line_spacing: own(style.line_spacing, normal.line_spacing),
        space_before: own(style.space_before, normal.space_before),
        space_after: own(style.space_after, normal.space_after),
    }
}

fn of_kind(kind: ParaKind) -> ParaFormat {
    ParaFormat {
        kind: Some(kind),
        ..Default::default()
    }
}

/// Builds a `RichText` paragraph by paragraph, in text order.
#[derive(Default)]
struct Builder {
    text: String,
    chars: Vec<FormatSpan<CharFormat>>,
    paras: Vec<FormatSpan<ParaFormat>>,
    objects: BTreeMap<u32, DocObject>,
}

impl Builder {
    /// One paragraph: `fill` writes its content, then its `'\n'` ends it.
    fn paragraph(&mut self, format: ParaFormat, fill: impl FnOnce(&mut Self)) {
        let start = self.text.len();
        fill(self);
        self.text.push('\n');
        self.paras.push(FormatSpan {
            start,
            end: self.text.len(),
            format,
        });
    }

    fn runs(&mut self, runs: &[TextRun]) {
        for run in runs {
            let start = self.text.len();
            self.plain(&run.text);
            self.chars.push(FormatSpan {
                start,
                end: self.text.len(),
                format: run_format(run),
            });
        }
    }

    /// Text without structure characters and on one line.
    fn plain(&mut self, text: &str) {
        for c in text.chars() {
            match c {
                '\n' => self.text.push(' '),
                '\r' => {}
                c if is_structure(c) => {}
                c => self.text.push(c),
            }
        }
    }

    fn add_object(&mut self, object: DocObject) -> u32 {
        let id = self.objects.keys().next_back().map_or(1, |id| id + 1);
        self.objects.insert(id, object);
        id
    }

    /// An object at this point of the text.
    fn object(&mut self, object: DocObject) {
        let id = self.add_object(object);
        let start = self.text.len();
        self.text.push(OBJECT_CHAR);
        self.chars.push(FormatSpan {
            start,
            end: self.text.len(),
            format: CharFormat {
                object: Some(id),
                ..Default::default()
            },
        });
    }

    fn finish(self) -> RichText {
        RichText {
            text: self.text,
            chars: FormatSpans::from_ordered(self.chars),
            paras: ParagraphSpans::from_ordered(self.paras),
            objects: self.objects,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{ImageData, NORMAL_SPACE_AFTER};
    use crate::format::Alignment;

    fn normal() -> ParagraphStyle {
        ParagraphStyle {
            line_spacing: 1.15,
            space_before: 0.0,
            space_after: NORMAL_SPACE_AFTER,
            alignment: Alignment::Left,
        }
    }

    fn run(text: &str, styles: Vec<SpanStyle>) -> TextRun {
        TextRun::styled(text, styles)
    }

    fn item(level: u8, ordered: bool, checked: Option<bool>, text: &str) -> ListItem {
        ListItem {
            level,
            ordered,
            checked,
            runs: vec![TextRun::plain(text)],
        }
    }

    fn cell(runs: Vec<TextRun>) -> TableCell {
        TableCell { runs }
    }

    /// One of every block the Markdown parser and the Insert menu make.
    fn every_block() -> Vec<Block> {
        let mut sized = TextRun::plain("big");
        sized.format.size = Some(20.0);
        sized.format.font = Some("Lora".into());
        sized.format.color = Some("#c0392b".into());
        sized.format.highlight = Some("#ffff00".into());
        let mut figure = ImageData::new("figs/plot.png");
        figure.alt_text = "A plot".into();
        figure.caption = Some("Figure 1".into());
        vec![
            Block::Heading {
                level: 2,
                runs: vec![TextRun::plain("Title "), run("bold", vec![SpanStyle::Bold])],
                alignment: Alignment::Center,
            },
            Block::Paragraph {
                runs: vec![
                    TextRun::plain("Plain, "),
                    run("both", vec![SpanStyle::BoldItalic]),
                    TextRun::plain(", "),
                    sized,
                    TextRun::plain(", "),
                    run("code", vec![SpanStyle::Code]),
                    TextRun::plain(" and "),
                    run(
                        "a link",
                        vec![SpanStyle::Bold, SpanStyle::Link("https://x.y".into())],
                    ),
                ],
                style: normal(),
            },
            Block::Paragraph {
                runs: vec![run(
                    "own spacing",
                    vec![SpanStyle::Underline, SpanStyle::Strikethrough],
                )],
                style: ParagraphStyle {
                    line_spacing: 2.0,
                    space_before: 12.0,
                    space_after: 0.0,
                    alignment: Alignment::Justify,
                },
            },
            Block::List {
                items: vec![
                    item(0, false, None, "bullet"),
                    item(1, true, None, "nested number"),
                    item(0, false, None, "bullet two"),
                ],
            },
            // Right after a bulleted list: a list of its own.
            Block::List {
                items: vec![item(0, true, None, "one"), item(0, true, None, "two")],
            },
            Block::List {
                items: vec![
                    item(0, false, Some(false), "todo"),
                    item(1, false, Some(true), "done"),
                ],
            },
            Block::Quote {
                level: 2,
                runs: vec![
                    TextRun::plain("quoted "),
                    run("words", vec![SpanStyle::Italic]),
                ],
            },
            Block::CodeBlock {
                language: "python".into(),
                text: "def f():\n\n    return 1  # **not bold**".into(),
            },
            Block::CodeBlock {
                language: String::new(),
                text: "plain fence".into(),
            },
            Block::Table {
                data: TableData {
                    rows: vec![
                        vec![
                            cell(vec![TextRun::plain("Name")]),
                            cell(vec![TextRun::plain("Score")]),
                        ],
                        vec![
                            cell(vec![run("Ada", vec![SpanStyle::Bold])]),
                            cell(Vec::new()),
                        ],
                    ],
                    caption: Some("Table 1".into()),
                    column_widths: vec![70.0, 30.0],
                },
            },
            Block::Image { data: figure },
            Block::HorizontalRule,
            Block::PageBreak,
            Block::Image {
                data: ImageData::new("second.png"),
            },
            Block::Caption {
                text: "Figure 2".into(),
            },
        ]
    }

    #[test]
    fn every_block_exports_as_it_came_in() {
        let blocks = every_block();
        let rich = from_blocks(&blocks, &normal());
        assert_eq!(rich.to_blocks(&normal()), blocks);
    }

    #[test]
    fn the_text_is_what_the_page_shows() {
        let rich = from_blocks(&every_block(), &normal());
        let obj = OBJECT_CHAR;
        let sep = CELL_SEPARATOR;
        assert_eq!(
            rich.text,
            format!(
                "Title bold\n\
                 Plain, both, big, code and a link\n\
                 own spacing\n\
                 bullet\nnested number\nbullet two\none\ntwo\ntodo\ndone\n\
                 quoted words\n\
                 def f():\n\n    return 1  # **not bold**\nplain fence\n\
                 Name{sep}Score\nAda{sep}\n\
                 {obj}\n{obj}\n{obj}\n{obj}\n\
                 Figure 2\n"
            )
        );
        // What each line is lives in its paragraph formatting.
        let kind_at = |line: &str| {
            let start = rich.text.find(line).expect(line);
            rich.paras.format_at(start).kind
        };
        assert_eq!(kind_at("Title"), Some(ParaKind::Heading(2)));
        assert_eq!(kind_at("Plain"), None);
        assert_eq!(
            kind_at("nested"),
            Some(ParaKind::ListItem {
                ordered: true,
                level: 1,
                checked: None
            })
        );
        assert_eq!(kind_at("quoted"), Some(ParaKind::Quote(2)));
        assert_eq!(
            kind_at("    return"),
            Some(ParaKind::Code {
                language: "python".into()
            })
        );
        assert_eq!(kind_at("Figure 2"), Some(ParaKind::Caption));
        // Bold is formatting, not `**` in the text.
        let bold = rich.text.find("bold").unwrap();
        assert_eq!(rich.chars.format_at(bold).bold, Some(true));
        assert_eq!(rich.chars.format_at(bold - 1).bold, None);
        // Each object character names its own object, in order.
        let objects: Vec<&DocObject> = rich
            .text
            .match_indices(OBJECT_CHAR)
            .map(|(at, _)| &rich.objects[&rich.chars.format_at(at).object.unwrap()])
            .collect();
        assert!(matches!(objects[0], DocObject::Image(d) if d.path == "figs/plot.png"));
        assert!(matches!(objects[1], DocObject::HorizontalRule));
        assert!(matches!(objects[2], DocObject::PageBreak));
        assert!(matches!(objects[3], DocObject::Image(d) if d.path == "second.png"));
        assert_eq!(rich.objects.len(), 5, "four objects and the table");
    }

    #[test]
    fn pasted_structure_characters_stay_text_not_objects() {
        // Word and LibreOffice put U+FFFC where an image was in copied text.
        let blocks = vec![Block::Paragraph {
            runs: vec![TextRun::plain("a\u{FFFC}b\u{1F}c\nd")],
            style: normal(),
        }];
        let rich = from_blocks(&blocks, &normal());
        assert_eq!(rich.text, "abc d\n");
        assert!(rich.objects.is_empty());
    }

    #[test]
    fn an_empty_document_is_one_empty_paragraph() {
        let rich = from_blocks(&[], &normal());
        assert_eq!(rich.text, "\n");
        assert_eq!(
            rich.to_blocks(&normal()),
            [Block::Paragraph {
                runs: Vec::new(),
                style: normal()
            }]
        );
    }

    #[test]
    fn enter_after_the_last_list_item_makes_another_item() {
        // The last paragraph's '\n' carries its kind, so the new empty
        // paragraph Enter makes at the end of the document keeps it.
        let blocks = vec![Block::List {
            items: vec![item(0, true, None, "first")],
        }];
        let mut rich = from_blocks(&blocks, &normal());
        let end = rich.text.len() - 1;
        rich.text.insert(end, '\n');
        rich.paras.edit(end, 0, 1);
        rich.chars.edit(end, 0, 1);
        assert_eq!(
            rich.to_blocks(&normal()),
            [Block::List {
                items: vec![
                    item(0, true, None, "first"),
                    ListItem {
                        runs: Vec::new(),
                        ..item(0, true, None, "")
                    }
                ]
            }]
        );
    }

    #[test]
    fn spacing_equal_to_normal_follows_the_style() {
        let mut own = normal();
        own.space_after = 20.0;
        let blocks = vec![
            Block::Paragraph {
                runs: vec![TextRun::plain("normal")],
                style: normal(),
            },
            Block::Paragraph {
                runs: vec![TextRun::plain("own")],
                style: own,
            },
        ];
        let rich = from_blocks(&blocks, &normal());
        assert_eq!(rich.paras.format_at(0), ParaFormat::default());
        let own_at = rich.text.find("own").unwrap();
        assert_eq!(rich.paras.format_at(own_at).space_after, Some(20.0));
        // Changing Normal later changes the first paragraph only.
        let mut wider = normal();
        wider.line_spacing = 2.0;
        let exported = rich.to_blocks(&wider);
        let spacing = |b: &Block| match b {
            Block::Paragraph { style, .. } => (style.line_spacing, style.space_after),
            b => panic!("{b:?}"),
        };
        assert_eq!(spacing(&exported[0]), (2.0, NORMAL_SPACE_AFTER));
        assert_eq!(spacing(&exported[1]), (2.0, 20.0));
    }

    #[test]
    fn table_rows_cut_at_separators_and_pad_to_the_widest_row() {
        // Rows typed with fewer cells than the header still export whole.
        let text = format!("A{s}B{s}C\nx\n", s = CELL_SEPARATOR);
        let mut paras = ParagraphSpans::new();
        paras.apply(0..text.len(), |p| {
            p.kind = Some(ParaKind::TableRow { table: 7 })
        });
        let mut objects = BTreeMap::new();
        objects.insert(
            7,
            DocObject::Table(TableProps {
                column_widths: vec![50.0, 50.0],
                ..Default::default()
            }),
        );
        let blocks = to_blocks(&text, &FormatSpans::new(), &paras, &objects, &normal());
        let Block::Table { data } = &blocks[0] else {
            panic!("{blocks:?}");
        };
        let texts: Vec<Vec<String>> = data
            .rows
            .iter()
            .map(|row| row.iter().map(TableCell::text).collect())
            .collect();
        assert_eq!(texts, [["A", "B", "C"], ["x", "", ""]]);
        // Two widths for three columns no longer fit: equal columns.
        assert_eq!(data.column_widths, vec![100.0 / 3.0; 3]);
    }
}
