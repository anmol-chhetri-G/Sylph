//! Page flow for the print-layout canvas. The editor lays its rows out in
//! one column; these place each row on a page, like Word's print layout:
//! a row that does not fit on the rest of a page moves to the top of the
//! next one, and a page-break line ends its page. Positions are in the
//! editor's own coordinates, where page `n`'s text area starts at
//! `n * stride`.

/// The page geometry the editor flows into.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PageFlow {
    /// Height of a page's text area (page height minus both margins).
    pub(crate) content_height: f32,
    /// From one page's text-area bottom to the next page's text-area top:
    /// bottom margin, the gap between pages, top margin.
    pub(crate) gap: f32,
}

impl PageFlow {
    /// Distance from one page's text-area top to the next one's.
    pub(crate) fn stride(&self) -> f32 {
        self.content_height + self.gap
    }

    /// The page (0-based) that position `y` is on. A position in the gap
    /// below a page belongs to that page.
    pub(crate) fn page_of(&self, y: f32) -> usize {
        if y <= 0.0 || self.stride() <= 0.0 {
            return 0;
        }
        (y / self.stride()).floor() as usize
    }

    /// The top of the text area of the page after the one `y` is on.
    pub(crate) fn next_page_top(&self, y: f32) -> f32 {
        (self.page_of(y) + 1) as f32 * self.stride()
    }

    /// Where a row box of `height` that would start at `y` goes: at `y`
    /// when it fits on the rest of that page, else at the top of the next
    /// page. A box taller than a whole page stays where it starts (it
    /// cannot fit anywhere), and so does one already at a page's top.
    pub(crate) fn place(&self, y: f32, height: f32) -> f32 {
        let page_top = self.page_of(y) as f32 * self.stride();
        let offset = y - page_top;
        if offset > self.content_height {
            // In the gap below a page: start on the next one.
            return self.next_page_top(y);
        }
        if offset > 0.0 && offset + height > self.content_height + 0.01 {
            return self.next_page_top(y);
        }
        y
    }

    /// Pages needed for text that ends at `bottom`.
    pub(crate) fn page_count(&self, bottom: f32) -> usize {
        // The end of a page's last row is still that page.
        self.page_of((bottom - 0.01).max(0.0)) + 1
    }
}

/// A line that ends its page: Sylph's `\newpage` marker (what Ctrl+Enter
/// inserts and what the Markdown export writes for a page break).
pub(crate) fn is_page_break_line(line: &str) -> bool {
    line.trim() == "\\newpage"
}

/// The canvas scroll offset (negative downwards, as GPUI keeps it) that
/// brings a caret spanning `top..bottom` (window coordinates) into the
/// `viewport` with a small margin, or `None` when it is already visible
/// or the viewport is unknown.
pub(crate) fn caret_scroll_offset(
    offset: gpui::Pixels,
    viewport: gpui::Bounds<gpui::Pixels>,
    top: gpui::Pixels,
    bottom: gpui::Pixels,
) -> Option<gpui::Pixels> {
    let margin = gpui::px(48.0);
    if viewport.size.height <= margin * 2.0 {
        return None;
    }
    if bottom > viewport.bottom() - margin {
        Some(offset - (bottom - (viewport.bottom() - margin)))
    } else if top < viewport.top() + margin {
        Some((offset + (viewport.top() + margin - top)).min(gpui::px(0.0)))
    } else {
        None
    }
}

/// What Ctrl+Enter types at the caret: the page-break marker on a line of
/// its own (after a newline unless the caret is at a line start) and a new
/// line for the text that continues on the next page.
pub(crate) fn page_break_insertion(at_line_start: bool) -> String {
    let lead = if at_line_start { "" } else { "\n" };
    format!("{lead}\\newpage\n")
}

/// The bounds `[start, end)` of the line holding `offset` (without its
/// newline).
fn line_bounds(content: &str, offset: usize) -> (usize, usize) {
    let start = content[..offset].rfind('\n').map_or(0, |p| p + 1);
    let end = content[offset..]
        .find('\n')
        .map_or(content.len(), |p| offset + p);
    (start, end)
}

/// The whole page-break line starting at `start` and ending at `end`, as
/// one removable range: with its own newline, or with the newline before
/// it when it is the last line.
fn break_line_range(content: &str, start: usize, end: usize) -> std::ops::Range<usize> {
    if end < content.len() {
        start..end + 1
    } else {
        start.saturating_sub(1)..end
    }
}

/// What Backspace removes at `cursor` when a page break is involved, as
/// in Word, where a break is one character: at the start of the line after
/// a break, or anywhere on the break line, the whole break goes (the text
/// after it moves up to the previous page). `None` when no break is near.
pub(crate) fn backspace_page_break(content: &str, cursor: usize) -> Option<std::ops::Range<usize>> {
    let (start, end) = line_bounds(content, cursor);
    if is_page_break_line(&content[start..end]) {
        return Some(break_line_range(content, start, end));
    }
    if cursor == start && start > 0 {
        let (prev_start, prev_end) = line_bounds(content, start - 1);
        if is_page_break_line(&content[prev_start..prev_end]) {
            return Some(prev_start..start);
        }
    }
    None
}

/// What Delete removes at `cursor`: anywhere on a break line, or at the
/// end of the line before one, the whole break goes (the text after it
/// moves up). `None` when no break is near.
pub(crate) fn delete_page_break(content: &str, cursor: usize) -> Option<std::ops::Range<usize>> {
    let (start, end) = line_bounds(content, cursor);
    if is_page_break_line(&content[start..end]) {
        return Some(break_line_range(content, start, end));
    }
    if cursor == end && end < content.len() {
        let (next_start, next_end) = line_bounds(content, end + 1);
        if is_page_break_line(&content[next_start..next_end]) {
            return Some(break_line_range(content, next_start, next_end));
        }
    }
    None
}

/// Move page breaks stored as model blocks into the text as `\newpage`
/// lines. Earlier versions inserted Ctrl+Enter breaks as blocks after the
/// text, where the caret could not reach them and Backspace could not
/// remove them, so their pages could be neither entered nor deleted. They
/// already came after all typed text, so appending them keeps the order.
/// Returns how many moved.
pub(crate) fn move_page_breaks_into_text(
    model: &mut sylph_core::document::Document,
    text: &mut String,
) -> usize {
    use sylph_core::document::Block;
    let before = model.blocks.len();
    model
        .blocks
        .retain(|block| !matches!(block, Block::PageBreak));
    let moved = before - model.blocks.len();
    for _ in 0..moved {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str("\\newpage\n");
    }
    moved
}

/// The text offset of the first row on `page` (0-based), from the rows'
/// (box top, source offset) pairs in layout order; `None` for a page with
/// no text row.
pub(crate) fn first_row_on_page(
    flow: &PageFlow,
    rows: impl IntoIterator<Item = (f32, usize)>,
    page: usize,
) -> Option<usize> {
    rows.into_iter()
        .find(|&(top, _)| flow.page_of(top) == page)
        .map(|(_, offset)| offset)
}

#[cfg(test)]
mod tests {
    use super::{is_page_break_line, PageFlow};

    const FLOW: PageFlow = PageFlow {
        content_height: 100.0,
        gap: 20.0,
    };

    #[test]
    fn rows_that_fit_stay_and_others_move_to_the_next_page() {
        assert_eq!(FLOW.place(0.0, 30.0), 0.0);
        assert_eq!(FLOW.place(70.0, 30.0), 70.0, "fits exactly");
        assert_eq!(FLOW.place(80.0, 30.0), 120.0, "overflows: next page");
        assert_eq!(FLOW.place(110.0, 10.0), 120.0, "in the gap: next page");
        assert_eq!(FLOW.place(120.0, 30.0), 120.0);
        assert_eq!(FLOW.place(0.0, 500.0), 0.0, "taller than a page: stays");
        assert_eq!(FLOW.place(120.0, 500.0), 120.0);
    }

    #[test]
    fn pages_are_counted_from_positions() {
        assert_eq!(FLOW.page_of(0.0), 0);
        assert_eq!(FLOW.page_of(119.0), 0);
        assert_eq!(FLOW.page_of(120.0), 1);
        assert_eq!(FLOW.next_page_top(50.0), 120.0);
        assert_eq!(FLOW.next_page_top(130.0), 240.0);
        assert_eq!(FLOW.page_count(0.0), 1);
        assert_eq!(FLOW.page_count(100.0), 1);
        assert_eq!(FLOW.page_count(150.0), 2);
    }

    #[test]
    fn a_page_is_entered_at_its_first_row() {
        use super::first_row_on_page;
        let rows = [(0.0, 0), (30.0, 10), (120.0, 25), (150.0, 40)];
        assert_eq!(first_row_on_page(&FLOW, rows, 0), Some(0));
        assert_eq!(first_row_on_page(&FLOW, rows, 1), Some(25));
        assert_eq!(first_row_on_page(&FLOW, rows, 2), None);
    }

    #[test]
    fn only_the_marker_line_breaks_a_page() {
        assert!(is_page_break_line("\\newpage"));
        assert!(is_page_break_line("  \\newpage  "));
        assert!(!is_page_break_line("see \\newpage"));
        assert!(!is_page_break_line(""));
    }
}

#[cfg(test)]
mod caret_scroll_tests {
    use super::caret_scroll_offset;
    use gpui::{point, px, size, Bounds};

    #[test]
    fn the_canvas_scrolls_only_as_far_as_the_caret_needs() {
        let viewport = Bounds::new(point(px(0.0), px(100.0)), size(px(800.0), px(600.0)));
        // Visible: no scroll.
        assert_eq!(
            caret_scroll_offset(px(0.0), viewport, px(300.0), px(320.0)),
            None
        );
        // Below the view (typing onto the next page): scroll down so the
        // caret sits 48 px above the bottom.
        assert_eq!(
            caret_scroll_offset(px(0.0), viewport, px(900.0), px(920.0)),
            Some(px(-268.0))
        );
        // Above the view: scroll up, never past the top.
        assert_eq!(
            caret_scroll_offset(px(-500.0), viewport, px(40.0), px(60.0)),
            Some(px(-392.0))
        );
        assert_eq!(
            caret_scroll_offset(px(-10.0), viewport, px(40.0), px(60.0)),
            Some(px(0.0))
        );
        // Unknown (zero-size) viewport: leave it.
        assert_eq!(
            caret_scroll_offset(px(0.0), Bounds::default(), px(900.0), px(920.0)),
            None
        );
    }
}

#[cfg(test)]
mod page_break_tests {
    use super::page_break_insertion;

    #[test]
    fn a_page_break_is_a_line_of_its_own() {
        assert_eq!(page_break_insertion(true), "\\newpage\n");
        assert_eq!(page_break_insertion(false), "\n\\newpage\n");
    }
}

#[cfg(test)]
mod page_break_editing_tests {
    use super::{backspace_page_break, delete_page_break};

    fn apply(content: &str, range: Option<std::ops::Range<usize>>) -> String {
        let range = range.expect("a page break to remove");
        format!("{}{}", &content[..range.start], &content[range.end..])
    }

    #[test]
    fn backspace_at_the_top_of_a_page_removes_the_break() {
        let text = "abc\n\\newpage\ndef";
        let top_of_page_2 = text.find("def").unwrap();
        assert_eq!(
            apply(text, backspace_page_break(text, top_of_page_2)),
            "abc\ndef"
        );
        // Anywhere on the break line itself.
        assert_eq!(apply(text, backspace_page_break(text, 6)), "abc\ndef");
        // A break as the last line.
        assert_eq!(
            apply("abc\n\\newpage", backspace_page_break("abc\n\\newpage", 12)),
            "abc"
        );
        // Ordinary text is left to the normal Backspace.
        assert_eq!(backspace_page_break(text, 2), None);
        assert_eq!(backspace_page_break(text, text.len()), None);
        assert_eq!(backspace_page_break("abc", 0), None);
    }

    #[test]
    fn delete_before_a_break_removes_it() {
        let text = "abc\n\\newpage\ndef";
        assert_eq!(apply(text, delete_page_break(text, 3)), "abc\ndef");
        assert_eq!(apply(text, delete_page_break(text, 4)), "abc\ndef");
        assert_eq!(delete_page_break(text, 1), None);
        assert_eq!(delete_page_break(text, text.len()), None);
        // An empty page after a break at the end of the text.
        let trailing = "abc\n\\newpage\n";
        assert_eq!(apply(trailing, delete_page_break(trailing, 3)), "abc\n");
    }
}

#[cfg(test)]
mod migration_tests {
    use super::move_page_breaks_into_text;
    use sylph_core::document::{Block, Document};

    #[test]
    fn stored_breaks_become_editable_break_lines() {
        let mut model = Document::new();
        model.push_block(Block::page_break());
        model.push_block(Block::paragraph("kept"));
        model.push_block(Block::page_break());
        let mut text = "abc".to_string();
        assert_eq!(move_page_breaks_into_text(&mut model, &mut text), 2);
        assert_eq!(text, "abc\n\\newpage\n\\newpage\n");
        assert!(!model.blocks.iter().any(|b| matches!(b, Block::PageBreak)));
        assert!(model.blocks.iter().any(|b| b.plain_text() == "kept"));
        // Nothing to move: nothing changes.
        let mut again = text.clone();
        assert_eq!(move_page_breaks_into_text(&mut model, &mut again), 0);
        assert_eq!(again, text);
    }
}
