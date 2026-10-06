//! Rich documents in the app (plan cards R2–R7, model in
//! `sylph_core::rich`): exporting them, and turning a Markdown-model
//! document into one.

use sylph_core::document as doc;
use sylph_core::rich;

/// The export blocks of a rich document's text `content`.
pub(crate) fn text_blocks(model: &doc::Document, content: &str) -> Vec<doc::Block> {
    rich::to_blocks(
        content,
        &model.char_formats,
        &model.para_formats,
        &model.objects,
        &model.normal_paragraph_style(),
    )
}

/// `model` with its editor text `content`, in the rich model: what its
/// export holds (the typed text, read as Markdown when `markdown_on`, else
/// as literal lines; then the blocks inserted after it), with no Markdown
/// syntax left in the text. The cover page stays in the model's blocks.
/// Returns the rich model and its text; nothing is saved here.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "card R7 opens old documents with it")
)]
pub(crate) fn to_rich(
    model: &doc::Document,
    content: &str,
    markdown_on: bool,
) -> (doc::Document, String) {
    let exported = crate::export_model(model, content, markdown_on);
    let (covers, body): (Vec<doc::Block>, Vec<doc::Block>) = exported
        .blocks
        .into_iter()
        .partition(|b| matches!(b, doc::Block::CoverPage { .. }));
    // An empty document exports one empty paragraph in the default style;
    // it is a stand-in, not text (the rich text has its own empty one).
    let body: Vec<doc::Block> = body
        .into_iter()
        .filter(|b| !matches!(b, doc::Block::Paragraph { runs, .. } if runs.iter().all(|r| r.text.is_empty())))
        .collect();
    let content = rich::from_blocks(&body, &model.normal_paragraph_style());
    let mut out = model.clone();
    out.blocks = covers;
    out.rich = true;
    out.char_formats = content.chars;
    out.para_formats = content.paras;
    out.objects = content.objects;
    (out, content.text)
}

#[cfg(test)]
mod tests {
    use super::to_rich;
    use crate::export_model;
    use sylph_core::document::{
        Block, CoverPageData, DocObject, Document, ListItem, TableCell, TableData, TextRun,
    };
    use sylph_core::format::{paragraph_range, Alignment, ParaKind, CELL_SEPARATOR, OBJECT_CHAR};

    const KITCHEN_SINK: &str = include_str!("../../../fixtures/kitchen-sink.md");

    /// Blocks as the exporters use them. They read a run's style flags as a
    /// set and draw neighbouring runs of equal formatting as one, so style
    /// order, empty runs and a split between equal runs make no difference
    /// to the page.
    fn as_exported(blocks: Vec<Block>) -> Vec<Block> {
        fn runs(runs: Vec<TextRun>) -> Vec<TextRun> {
            let mut out: Vec<TextRun> = Vec::new();
            for mut run in runs {
                if run.text.is_empty() {
                    continue;
                }
                run.styles.sort_by_key(|s| format!("{s:?}"));
                match out.last_mut() {
                    Some(last) if last.styles == run.styles && last.format == run.format => {
                        last.text.push_str(&run.text);
                    }
                    _ => out.push(run),
                }
            }
            out
        }
        blocks
            .into_iter()
            .map(|block| match block {
                Block::Paragraph { runs: r, style } => Block::Paragraph {
                    runs: runs(r),
                    style,
                },
                Block::Heading {
                    level,
                    runs: r,
                    alignment,
                } => Block::Heading {
                    level,
                    runs: runs(r),
                    alignment,
                },
                Block::Quote { level, runs: r } => Block::Quote {
                    level,
                    runs: runs(r),
                },
                Block::List { items } => Block::List {
                    items: items
                        .into_iter()
                        .map(|item| ListItem {
                            runs: runs(item.runs),
                            ..item
                        })
                        .collect(),
                },
                Block::Table { mut data } => {
                    for cell in data.rows.iter_mut().flatten() {
                        cell.runs = runs(std::mem::take(&mut cell.runs));
                    }
                    Block::Table { data }
                }
                other => other,
            })
            .collect()
    }

    /// Converts the document and checks that its export did not change.
    fn convert_and_compare(
        model: &Document,
        content: &str,
        markdown_on: bool,
    ) -> (Document, String) {
        let (rich_model, text) = to_rich(model, content, markdown_on);
        assert!(rich_model.rich);
        let before = as_exported(export_model(model, content, markdown_on).blocks);
        let after = as_exported(export_model(&rich_model, &text, markdown_on).blocks);
        for (n, (a, b)) in after.iter().zip(&before).enumerate() {
            assert_eq!(a, b, "block {n} of the export changed");
        }
        assert_eq!(after.len(), before.len(), "{after:#?}");
        (rich_model, text)
    }

    /// The paragraph kind of the line of `text` that starts with `line`.
    fn kind_of(model: &Document, text: &str, line: &str) -> Option<ParaKind> {
        let start = text
            .split_inclusive('\n')
            .scan(0, |offset, l| {
                let at = *offset;
                *offset += l.len();
                Some((at, l))
            })
            .find(|(_, l)| l.starts_with(line))
            .unwrap_or_else(|| panic!("no line starts with {line:?} in {text:?}"))
            .0;
        model.para_formats.format_at(start).kind
    }

    #[test]
    fn kitchen_sink_exports_the_same_from_the_rich_model() {
        let (rich, text) = convert_and_compare(&Document::new(), KITCHEN_SINK, true);
        assert!(text.ends_with('\n'), "every paragraph ends with its mark");

        // No Markdown syntax is left outside code blocks.
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            let kind = rich.para_formats.format_at(offset).kind;
            offset += line.len();
            if matches!(kind, Some(ParaKind::Code { .. })) {
                continue;
            }
            for syntax in ["#", "- ", "> ", "```", "|", "1. "] {
                assert!(!line.starts_with(syntax), "{line:?} kept {syntax:?}");
            }
            for syntax in ["**", "~~", "`", "](", "<http"] {
                assert!(!line.contains(syntax), "{line:?} kept {syntax:?}");
            }
        }

        // What the syntax said is now paragraph kinds and formatting.
        assert_eq!(
            kind_of(&rich, &text, "Inline styles\n"),
            Some(ParaKind::Heading(2))
        );
        assert_eq!(
            kind_of(&rich, &text, "Level six"),
            Some(ParaKind::Heading(6))
        );
        assert_eq!(
            kind_of(&rich, &text, "Deeper nesting"),
            Some(ParaKind::ListItem {
                ordered: false,
                level: 2,
                checked: None
            })
        );
        assert_eq!(
            kind_of(&rich, &text, "Checked task"),
            Some(ParaKind::ListItem {
                ordered: false,
                level: 0,
                checked: Some(true)
            })
        );
        assert_eq!(
            kind_of(&rich, &text, "A second-level quote"),
            Some(ParaKind::Quote(2))
        );
        assert_eq!(
            kind_of(&rich, &text, "def greet(name):"),
            Some(ParaKind::Code {
                language: "python".into()
            })
        );
        let header = format!("Feature{s}Status{s}Notes\n", s = CELL_SEPARATOR);
        assert!(matches!(
            kind_of(&rich, &text, &header),
            Some(ParaKind::TableRow { .. })
        ));
        let plain = "Plain text with italic, bold, bold-italic, inline code, and \
                     strikethrough. Snake_case stays plain, as does *escaped asterisks*.\n";
        assert!(text.contains(plain), "{text}");
        let format_of = |word: &str| {
            let at = text.find(plain).unwrap() + plain.find(word).unwrap();
            rich.char_formats.format_at(at)
        };
        assert_eq!(format_of("bold,").bold, Some(true));
        assert_eq!(format_of("bold,").italic, None);
        assert_eq!(format_of("inline code").code, Some(true));
        assert_eq!(format_of("strikethrough").strike, Some(true));
        let link_at = text.find("two flavors").unwrap();
        assert_eq!(
            rich.char_formats.format_at(link_at).link.as_deref(),
            Some("https://example.com/docs")
        );

        // The image and the rule are objects on lines of their own.
        let objects: Vec<&DocObject> = text
            .match_indices(&format!("\n{OBJECT_CHAR}\n"))
            .map(|(at, _)| {
                let id = rich.char_formats.format_at(at + 1).object.unwrap();
                &rich.objects[&id]
            })
            .collect();
        assert!(matches!(objects[0], DocObject::HorizontalRule));
        assert!(matches!(
            objects[1],
            DocObject::Image(image)
                if image.path == "fixtures/proof.png" && image.alt_text == "A proof image"
        ));
    }

    #[test]
    fn formatting_on_markdown_text_exports_the_same() {
        let content = "# Report title\n\
                       Some **bold** words and a sized word.\n\
                       - item one\n\
                       Plain closing line.";
        let mut model = Document::new();
        let at = |s: &str| content.find(s).unwrap();
        let span = |s: &str| at(s)..at(s) + s.len();
        model
            .char_formats
            .apply(span("Report"), |f| f.font = Some("Lora".into()));
        model
            .char_formats
            .apply(span("sized"), |f| f.size = Some(18.0));
        // Un-bolding Markdown bold, as Ctrl+B on `**bold**` does.
        model
            .char_formats
            .apply(span("bold"), |f| f.bold = Some(false));
        model
            .char_formats
            .apply(span("item"), |f| f.underline = Some(true));
        model
            .para_formats
            .apply(paragraph_range(content, 0, 0), |p| {
                p.align = Some(Alignment::Center)
            });
        model
            .para_formats
            .apply(paragraph_range(content, at("Some"), at("Some")), |p| {
                p.align = Some(Alignment::Justify);
                p.line_spacing = Some(2.0);
                p.space_after = Some(20.0);
            });
        let space_after = model.space_after;
        model
            .para_formats
            .apply(paragraph_range(content, at("Plain"), at("Plain")), |p| {
                p.space_after = Some(space_after)
            });

        let (rich, text) = convert_and_compare(&model, content, true);
        assert_eq!(
            text,
            "Report title\nSome bold words and a sized word.\nitem one\nPlain closing line.\n"
        );
        assert_eq!(kind_of(&rich, &text, "Report"), Some(ParaKind::Heading(1)));
        let some = rich.para_formats.format_at(text.find("Some").unwrap());
        assert_eq!(
            (some.align, some.line_spacing, some.space_after),
            (Some(Alignment::Justify), Some(2.0), Some(20.0))
        );
        // Spacing equal to Normal's follows the style from now on.
        let plain = rich.para_formats.format_at(text.find("Plain").unwrap());
        assert_eq!(plain.space_after, None);
    }

    #[test]
    fn markdown_off_text_stays_literal() {
        let content = "# not a heading **raw**\n\\newpage\n- not a list";
        let mut model = Document::new();
        model.char_formats.apply(2..5, |f| f.size = Some(14.0));
        let (rich, text) = convert_and_compare(&model, content, false);
        assert_eq!(
            text,
            format!("# not a heading **raw**\n{OBJECT_CHAR}\n- not a list\n")
        );
        assert_eq!(kind_of(&rich, &text, "# not"), None);
        assert_eq!(rich.char_formats.format_at(2).size, Some(14.0));
        let id = rich
            .char_formats
            .format_at(text.find(OBJECT_CHAR).unwrap())
            .object
            .unwrap();
        assert_eq!(rich.objects[&id], DocObject::PageBreak);
    }

    #[test]
    fn inserted_blocks_join_the_text_and_the_cover_stays() {
        let mut model = Document::new();
        let mut table = TableData::new(2, 2);
        table.rows[0][0] = TableCell::plain("Name");
        table.rows[1][1] = TableCell::plain("42");
        model.blocks = vec![
            Block::CoverPage {
                data: CoverPageData::new(),
            },
            Block::Table { data: table },
            Block::caption("Table 1: results"),
        ];
        let (rich, text) = convert_and_compare(&model, "Intro paragraph", true);
        assert!(matches!(rich.blocks[..], [Block::CoverPage { .. }]));
        assert_eq!(
            text,
            format!(
                "Intro paragraph\nName{s}\n{s}42\nTable 1: results\n",
                s = CELL_SEPARATOR
            )
        );
        assert_eq!(
            kind_of(&rich, &text, "Table 1: results"),
            Some(ParaKind::Caption)
        );
    }

    #[test]
    fn an_empty_document_becomes_one_plain_paragraph() {
        // Not the export's stand-in paragraph, whose default spacing would
        // otherwise stick to the paragraph as its own.
        let (rich, text) = to_rich(&Document::new(), "", true);
        assert_eq!(text, "\n");
        assert!(rich.para_formats.is_empty() && rich.char_formats.is_empty());
    }
}
