//! Markdown source edits and structure: list markers and the outline.

use crate::heading_level_and_text;

/// A heading in the Document Map: level, text and the byte offset of its
/// line (where a click puts the caret).
pub(crate) type OutlineEntry = (u8, String, usize);

/// The headings typed in Markdown `content`, by the export parser's rules:
/// nothing inside ``` fences is a heading, and `# ` with no text is not
/// one either.
pub(crate) fn text_outline(content: &str) -> Vec<OutlineEntry> {
    let mut headings = Vec::new();
    let mut in_fence = false;
    let mut offset = 0;
    for line in content.split('\n') {
        let start = offset;
        offset += line.len() + 1;
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some((level, text)) = heading_level_and_text(line.trim_start()) {
            headings.push((level, text.to_string(), start));
        }
    }
    headings
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ListKind {
    Bullet,
    Numbered,
}

/// The list marker a line starts with (after its indent): its kind and
/// byte length including the space, e.g. `- ` or `12. `.
pub(crate) fn list_marker(line: &str) -> Option<(ListKind, usize)> {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    if ["- ", "* ", "+ "].iter().any(|m| trimmed.starts_with(m)) {
        return Some((ListKind::Bullet, indent + 2));
    }
    let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
    (digits > 0 && trimmed[digits..].starts_with(". "))
        .then_some((ListKind::Numbered, indent + digits + 2))
}

/// `line` as a `kind` list item (the `number`th for a numbered list), or
/// as plain text for `None`; any list marker it had is replaced. Returns
/// the new line and the marker lengths before and after, like
/// `with_heading_level`. The indent (the list level) is kept.
pub(crate) fn with_list_marker(
    line: &str,
    kind: Option<ListKind>,
    number: usize,
) -> (String, usize, usize) {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    let before = list_marker(line).map_or(indent.len(), |(_, len)| len);
    let text = &line[before..];
    let marker = match kind {
        None => String::new(),
        Some(ListKind::Bullet) => "- ".to_string(),
        Some(ListKind::Numbered) => format!("{number}. "),
    };
    let new_line = format!("{indent}{marker}{text}");
    let after = new_line.len() - text.len();
    (new_line, before, after)
}

#[cfg(test)]
mod outline_tests {
    use super::text_outline;

    #[test]
    fn headings_carry_their_line_offsets() {
        let text = "# Title\nbody\n```\n# not a heading\n```\n## नेपाल\n#\n### Last";
        let outline = text_outline(text);
        assert_eq!(
            outline,
            [
                (1, "Title".to_string(), 0),
                (2, "नेपाल".to_string(), text.find("## नेपाल").unwrap()),
                (3, "Last".to_string(), text.find("### Last").unwrap()),
            ]
        );
        for (_, _, offset) in outline {
            assert!(text.is_char_boundary(offset));
        }
    }
}

#[cfg(test)]
mod list_marker_tests {
    use super::{list_marker, with_list_marker, ListKind};

    #[test]
    fn markers_are_recognised() {
        assert_eq!(list_marker("- a"), Some((ListKind::Bullet, 2)));
        assert_eq!(list_marker("  * a"), Some((ListKind::Bullet, 4)));
        assert_eq!(list_marker("12. a"), Some((ListKind::Numbered, 4)));
        assert_eq!(list_marker("12.a"), None);
        assert_eq!(list_marker("-a"), None);
        assert_eq!(list_marker("नेपाल"), None);
    }

    #[test]
    fn lists_toggle_convert_and_keep_the_indent() {
        let bullet = Some(ListKind::Bullet);
        let numbered = Some(ListKind::Numbered);
        assert_eq!(with_list_marker("text", bullet, 1), ("- text".into(), 0, 2));
        assert_eq!(
            with_list_marker("- text", numbered, 3),
            ("3. text".into(), 2, 3)
        );
        assert_eq!(
            with_list_marker("  10. text", None, 1),
            ("  text".into(), 6, 2)
        );
        assert_eq!(
            with_list_marker("  नेपाल", bullet, 1),
            ("  - नेपाल".into(), 2, 4)
        );
        assert_eq!(with_list_marker("", bullet, 1), ("- ".into(), 0, 2));
    }
}
