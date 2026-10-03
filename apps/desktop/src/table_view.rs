//! Pipe tables on the page: where each cell's text sits in a table line,
//! so the canvas can draw the table as a grid and editing stays inside a
//! cell (Tab moves between cells, as in Word).

use std::ops::Range;

/// Byte positions of the line's unescaped `|` separators.
fn pipes(line: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            // `\|` is a literal pipe inside a cell; skip the escaped byte.
            b'\\' => i += 2,
            b'|' => {
                out.push(i);
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// The content range of every cell of a table line (byte offsets in the
/// line), trimmed of its padding. An empty cell is an empty range just
/// after its opening `| `, where typing into it goes.
pub(crate) fn cell_ranges(line: &str) -> Vec<Range<usize>> {
    let pipes = pipes(line);
    let trimmed_start = line.len() - line.trim_start().len();
    let trimmed_end = line.trim_end().len();
    // Cell boundaries: the separators, plus the line's ends when the row
    // has no outer pipe there.
    let mut bounds: Vec<(usize, usize)> = Vec::new(); // (after-separator, before-separator)
    let mut start = match pipes.first() {
        Some(&p) if p == trimmed_start => p + 1,
        _ => trimmed_start,
    };
    for &p in &pipes {
        if p < start {
            continue;
        }
        bounds.push((start, p));
        start = p + 1;
    }
    if start < trimmed_end {
        bounds.push((start, trimmed_end));
    }
    bounds
        .into_iter()
        .map(|(a, b)| {
            let cell = &line[a..b];
            let lead = cell.len() - cell.trim_start().len();
            let content = cell.trim();
            if content.is_empty() {
                let at = (a + 1).min(b);
                at..at
            } else {
                a + lead..a + lead + content.len()
            }
        })
        .collect()
}

/// The cell holding byte `offset` of the line: the one whose content
/// range contains it, else the nearest one to its left.
pub(crate) fn cell_at(line: &str, offset: usize) -> Option<usize> {
    let cells = cell_ranges(line);
    cells
        .iter()
        .rposition(|c| c.start <= offset)
        .or((!cells.is_empty()).then_some(0))
}

/// A new empty row for a table whose rows have `cells` cells.
pub(crate) fn empty_row(cells: usize) -> String {
    format!("|{}", "  |".repeat(cells.max(1)))
}

/// Shown between two cells of a row's display text (never drawn): it
/// gives every cell, even an empty one, its own caret positions.
pub(crate) const CELL_SEPARATOR: &str = "\u{1f}";

/// The display line of table row `line`: each cell's text (inline
/// Markdown as the export reads it), the pipes and padding hidden, one
/// `CELL_SEPARATOR` between cells. The delimiter row shows nothing.
pub(crate) fn display_row(line: &str, header: bool, rule: bool) -> crate::DisplayLine {
    let mut b = crate::DisplayBuilder::default();
    let mut cells = Vec::new();
    if rule {
        b.hide(line.len());
    } else {
        let ranges = cell_ranges(line);
        for (k, range) in ranges.iter().enumerate() {
            let gap = range.start - b.src;
            if k == 0 {
                b.hide(gap);
            } else {
                b.replace(gap, CELL_SEPARATOR);
            }
            let start = b.text.len();
            b.inline(line, range.len());
            cells.push(start..b.text.len());
        }
        b.hide(line.len() - b.src);
    }
    let mut dl = crate::DisplayLine::identity(0, "", false);
    dl.text = b.text;
    dl.segments = b.segments;
    dl.styles = b.styles;
    dl.table = true;
    dl.cells = cells;
    dl.table_header = header;
    dl.table_rule = rule;
    dl.bold = header;
    dl
}

impl crate::TextInput {
    /// The bounds of the line holding `offset` when it is a table row on
    /// the page (Markdown on; not the `|---|` row).
    pub(crate) fn table_line_at(&self, offset: usize) -> Option<(usize, usize)> {
        if !self.markdown_mode {
            return None;
        }
        let index = self.content[..offset.min(self.content.len())]
            .matches('\n')
            .count();
        let dl = self.display_lines.get(index)?;
        (dl.table && !dl.table_rule).then(|| (self.line_start(offset), self.line_end(offset)))
    }

    /// Select cell `cell` of the table line starting at `line_start`.
    fn select_cell(&mut self, line_start: usize, cell: usize, cx: &mut gpui::Context<Self>) {
        let line_end = self.line_end(line_start);
        let cells = cell_ranges(&self.content[line_start..line_end]);
        if let Some(range) = cells.get(cell.min(cells.len().saturating_sub(1))) {
            self.move_to(line_start + range.start, cx);
            self.select_to(line_start + range.end, cx);
        }
    }

    /// The table row before (`forward == false`) or after the line at
    /// `line_start`, skipping the `|---|` row: its start, if it is one.
    fn neighbour_row(&self, line_start: usize, forward: bool) -> Option<usize> {
        let mut at = line_start;
        loop {
            at = if forward {
                let end = self.line_end(at);
                (end < self.content.len()).then_some(end + 1)?
            } else {
                (at > 0).then(|| self.line_start(at - 1))?
            };
            let index = self.content[..at].matches('\n').count();
            let dl = self.display_lines.get(index)?;
            if !dl.table {
                return None;
            }
            if !dl.table_rule {
                return Some(at);
            }
        }
    }

    /// Tab / Shift+Tab in a table: select the next / previous cell, as in
    /// Word; Tab in the last cell adds a row. `false` outside tables.
    pub(crate) fn table_tab(&mut self, forward: bool, cx: &mut gpui::Context<Self>) -> bool {
        let caret = self.cursor_offset();
        let Some((start, end)) = self.table_line_at(caret) else {
            return false;
        };
        let line = self.content[start..end].to_string();
        let cells = cell_ranges(&line).len();
        let current = cell_at(&line, caret - start).unwrap_or(0);
        if forward {
            if current + 1 < cells {
                self.select_cell(start, current + 1, cx);
            } else if let Some(next) = self.neighbour_row(start, true) {
                self.select_cell(next, 0, cx);
            } else {
                // The last cell: a new row, caret in its first cell.
                let row = format!("\n{}", empty_row(cells));
                self.replace_text_in_range(Some(end..end), &row, cx);
                self.select_cell(end + 1, 0, cx);
            }
        } else if current > 0 {
            self.select_cell(start, current - 1, cx);
        } else if let Some(previous) = self.neighbour_row(start, false) {
            self.select_cell(previous, usize::MAX, cx);
        }
        true
    }

    /// Whether Backspace (`backward`) or Delete at the caret would cross a
    /// cell's border: nothing happens then, as in Word.
    pub(crate) fn at_cell_border(&self, backward: bool) -> bool {
        if !self.selected_range.is_empty() {
            return false;
        }
        let caret = self.cursor_offset();
        let Some((start, end)) = self.table_line_at(caret) else {
            return false;
        };
        let rel = caret - start;
        cell_ranges(&self.content[start..end]).iter().any(|cell| {
            if backward {
                cell.start == rel
            } else {
                cell.end == rel
            }
        })
    }
}

/// Typed text for a table cell: a `|` would split the cell, so it goes in
/// escaped (`\|` shows and exports as a plain `|`).
pub(crate) fn escape_cell_text(text: &str) -> String {
    text.replace('|', "\\|")
}

#[cfg(test)]
mod tests {
    use super::{cell_at, cell_ranges, empty_row};

    fn texts(line: &str) -> Vec<&str> {
        cell_ranges(line).into_iter().map(|r| &line[r]).collect()
    }

    #[test]
    fn cells_are_found_with_and_without_outer_pipes() {
        assert_eq!(texts("| Field | Source |"), ["Field", "Source"]);
        assert_eq!(texts("a | b"), ["a", "b"]);
        assert_eq!(texts("  | x |y|  "), ["x", "y"]);
        assert_eq!(texts("|---|:--:|"), ["---", ":--:"]);
    }

    #[test]
    fn escaped_pipes_stay_inside_their_cell() {
        assert_eq!(texts(r"| a \| b | c |"), [r"a \| b", "c"]);
    }

    #[test]
    fn empty_cells_have_a_place_to_type() {
        let line = "| a |  | c |";
        let cells = cell_ranges(line);
        assert_eq!(cells.len(), 3);
        assert!(cells[1].is_empty());
        // Just after "| ": between the pipes, after one space.
        assert_eq!(cells[1].start, 6);
        assert_eq!(&line[cells[2].clone()], "c");
    }

    #[test]
    fn the_cell_at_an_offset() {
        let line = "| one | two |";
        assert_eq!(cell_at(line, 3), Some(0));
        assert_eq!(cell_at(line, 9), Some(1));
        assert_eq!(cell_at(line, 0), Some(0));
        assert_eq!(cell_at("", 0), None);
    }

    #[test]
    fn typed_pipes_are_escaped() {
        assert_eq!(super::escape_cell_text("a|b"), r"a\|b");
        assert_eq!(super::escape_cell_text("plain"), "plain");
    }

    #[test]
    fn new_rows_match_the_table() {
        assert_eq!(empty_row(3), "|  |  |  |");
        assert_eq!(cell_ranges(&empty_row(3)).len(), 3);
    }
}

#[cfg(test)]
mod display_tests {
    use super::{display_row, CELL_SEPARATOR};

    #[test]
    fn a_row_shows_its_cells_and_maps_back_to_the_source() {
        let line = "| **Field** |  | Source |";
        let dl = display_row(line, true, false);
        let shown: Vec<&str> = dl.cells.iter().map(|r| &dl.text[r.clone()]).collect();
        assert_eq!(shown, ["Field", "", "Source"]);
        assert_eq!(
            dl.text,
            format!("Field{CELL_SEPARATOR}{CELL_SEPARATOR}Source")
        );
        // The empty cell's caret position maps to its place in the source.
        let empty_at = dl.cells[1].start;
        assert_eq!(dl.disp_to_src(empty_at), line.find("|  |").unwrap() + 2);
        // Text maps back inside its cell.
        assert_eq!(&line[dl.disp_to_src(dl.cells[2].start)..][..6], "Source");
        assert!(dl.table_header && dl.bold);
    }

    #[test]
    fn the_delimiter_row_shows_nothing() {
        let dl = display_row("|---|---|", false, true);
        assert!(dl.text.is_empty() && dl.table_rule && dl.cells.is_empty());
    }
}
