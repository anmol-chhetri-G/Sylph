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

/// List indentation step for Tab / Shift+Tab (the export nests by it).
pub(crate) const LIST_INDENT: &str = "  ";

/// What Enter does on a list line, as in Word.
#[derive(Debug, PartialEq)]
pub(crate) enum ListEnter {
    /// Insert this at the caret: a newline and the next item's marker.
    Continue(String),
    /// The item is empty: end the list by replacing the line's first
    /// `marker_len` bytes with `replacement` (outdent one level if nested,
    /// else remove the marker).
    End {
        marker_len: usize,
        replacement: String,
    },
}

/// Enter on `line` with the caret `caret` bytes into it. `None` when the
/// line is not a list item or the caret is still inside its marker.
pub(crate) fn list_enter(line: &str, caret: usize) -> Option<ListEnter> {
    let (kind, marker_len) = list_marker(line)?;
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    let after_marker = &line[marker_len..];
    // A task item's checkbox is part of its marker.
    let checkbox = ["[ ] ", "[x] ", "[X] "]
        .iter()
        .find(|b| after_marker.starts_with(**b))
        .map_or(0, |b| b.len());
    let full_marker = marker_len + checkbox;
    if caret < full_marker {
        return None;
    }
    if line[full_marker..].trim().is_empty() {
        let replacement = match indent.strip_suffix(LIST_INDENT) {
            // Nested: an empty item moves up a level.
            Some(outer) => format!("{outer}{}", &line[indent.len()..marker_len]),
            None => String::new(),
        };
        return Some(ListEnter::End {
            marker_len: full_marker,
            replacement,
        });
    }
    let marker = match kind {
        ListKind::Bullet => trimmed[..2].to_string(),
        ListKind::Numbered => {
            let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
            let number: u64 = digits.parse().unwrap_or(0);
            format!("{}. ", number + 1)
        }
    };
    let checkbox = if checkbox > 0 { "[ ] " } else { "" };
    Some(ListEnter::Continue(format!("\n{indent}{marker}{checkbox}")))
}

/// Byte length of a displayed list line's prefix (indent, `• ` or `12. `,
/// and a `☐ `/`☑ ` box): where its text, and its wrapped lines, start.
pub(crate) fn list_hang_prefix(display: &str) -> usize {
    let trimmed = display.trim_start();
    let mut prefix = display.len() - trimmed.len();
    let rest = &display[prefix..];
    if let Some(after) = rest.strip_prefix("• ") {
        prefix = display.len() - after.len();
    } else {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && rest[digits..].starts_with(". ") {
            prefix += digits + 2;
        }
    }
    for box_ in ["☐ ", "☑ "] {
        if display[prefix..].starts_with(box_) {
            prefix += box_.len();
        }
    }
    prefix
}

/// The line with one more list level (Tab on a list item).
pub(crate) fn indent_list_line(line: &str) -> String {
    format!("{LIST_INDENT}{line}")
}

/// The line with one level less, if it is indented (Shift+Tab).
pub(crate) fn outdent_list_line(line: &str) -> Option<String> {
    line.strip_prefix(LIST_INDENT)
        .or_else(|| line.strip_prefix(' '))
        .map(str::to_string)
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

#[cfg(test)]
mod list_enter_tests {
    use super::{indent_list_line, list_enter, outdent_list_line, ListEnter};

    #[test]
    fn enter_continues_a_list() {
        assert_eq!(
            list_enter("- milk", 6),
            Some(ListEnter::Continue("\n- ".into()))
        );
        assert_eq!(
            list_enter("  * eggs", 8),
            Some(ListEnter::Continue("\n  * ".into()))
        );
        assert_eq!(
            list_enter("9. nine", 7),
            Some(ListEnter::Continue("\n10. ".into()))
        );
        assert_eq!(
            list_enter("- [x] done", 10),
            Some(ListEnter::Continue("\n- [ ] ".into()))
        );
        // Mid-item Enter still continues (the rest moves to the new item).
        assert_eq!(
            list_enter("- milk", 3),
            Some(ListEnter::Continue("\n- ".into()))
        );
    }

    #[test]
    fn enter_on_an_empty_item_ends_or_outdents_the_list() {
        assert_eq!(
            list_enter("- ", 2),
            Some(ListEnter::End {
                marker_len: 2,
                replacement: String::new()
            })
        );
        assert_eq!(
            list_enter("  1. ", 5),
            Some(ListEnter::End {
                marker_len: 5,
                replacement: "1. ".into()
            })
        );
        assert_eq!(
            list_enter("- [ ] ", 6),
            Some(ListEnter::End {
                marker_len: 6,
                replacement: String::new()
            })
        );
    }

    #[test]
    fn plain_lines_and_carets_in_the_marker_are_ordinary() {
        assert_eq!(list_enter("plain", 5), None);
        assert_eq!(list_enter("- milk", 1), None);
        assert_eq!(list_enter("नेपाल", 3), None);
    }

    #[test]
    fn wrapped_items_hang_under_their_text() {
        use super::list_hang_prefix;
        assert_eq!(list_hang_prefix("• item"), "• ".len());
        assert_eq!(list_hang_prefix("  • item"), 2 + "• ".len());
        assert_eq!(list_hang_prefix("12. item"), 4);
        assert_eq!(list_hang_prefix("• ☐ task"), "• ☐ ".len());
        assert_eq!(list_hang_prefix("plain"), 0);
    }

    #[test]
    fn tab_and_shift_tab_change_the_level() {
        assert_eq!(indent_list_line("- a"), "  - a");
        assert_eq!(outdent_list_line("  - a").as_deref(), Some("- a"));
        assert_eq!(outdent_list_line(" - a").as_deref(), Some("- a"));
        assert_eq!(outdent_list_line("- a"), None);
    }
}
