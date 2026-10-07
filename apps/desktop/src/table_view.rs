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

/// Wrap one cell's display `text` into lines (T3). `base` is the text's
/// offset in the row's display text, so the ranges returned are row ranges.
/// `break_at(rest)` says where `rest` stops fitting the column: `None` when
/// all of it fits, else the byte where the first character that does not fit
/// starts. A line ends after the last space before that (a word is never cut
/// unless it alone is wider than the column) and always holds a character,
/// so wrapping ends. An empty cell is one empty line.
pub(crate) fn wrap_cell(
    text: &str,
    base: usize,
    mut break_at: impl FnMut(&str) -> Option<usize>,
) -> Vec<std::ops::Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let rest = &text[start..];
        let first = rest.chars().next().map_or(1, char::len_utf8);
        let cut = match break_at(rest) {
            Some(at) if rest.len() > first => {
                let mut at = at.min(rest.len());
                while !rest.is_char_boundary(at) {
                    at -= 1;
                }
                let cut = match at {
                    0 => first,
                    _ => rest[..at].rfind(' ').map_or(at, |space| space + 1),
                };
                cut.max(first)
            }
            _ => rest.len(),
        };
        lines.push(base + start..base + start + cut);
        start += cut;
    }
    if lines.is_empty() {
        lines.push(base..base);
    }
    lines
}

/// The visual row (index into `rows`) that shows display byte `disp` of
/// logical line `line_idx`. `rows` gives each row's line, its first display
/// byte, and, for a table row, the pieces of cells it shows (T3). A text row
/// is the last one starting at or before `disp`; a table row is the last one
/// with a piece containing `disp` (a wrap point belongs to the later line,
/// as in text), else the line's first row (a separator byte).
pub(crate) fn visual_row_of<'a>(
    rows: impl Iterator<Item = (usize, usize, &'a [std::ops::Range<usize>])>,
    line_idx: usize,
    disp: usize,
) -> usize {
    let mut first = None;
    let mut found = None;
    for (ri, (line, disp_start, pieces)) in rows.enumerate() {
        if line != line_idx {
            continue;
        }
        first.get_or_insert(ri);
        let here = if pieces.is_empty() {
            disp >= disp_start
        } else {
            pieces.iter().any(|p| p.start <= disp && disp <= p.end)
        };
        if here {
            found = Some(ri);
        }
    }
    found.or(first).unwrap_or(0)
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

/// Each column's share of the table width, from the delimiter row's dash
/// counts (`|------|---|` → 2/3, 1/3): column widths kept in plain
/// Markdown, so the canvas and both exporters agree.
pub(crate) fn column_fractions(delimiter: &str) -> Vec<f32> {
    let lengths: Vec<f32> = cell_ranges(delimiter)
        .iter()
        .map(|r| delimiter[r.clone()].trim_matches(':').len().max(1) as f32)
        .collect();
    let total: f32 = lengths.iter().sum();
    if total <= 0.0 {
        return Vec::new();
    }
    lengths.into_iter().map(|l| l / total).collect()
}

/// The delimiter row for column shares `fractions`, keeping each column's
/// `:` alignment markers from `old`.
pub(crate) fn delimiter_with_fractions(old: &str, fractions: &[f32]) -> String {
    let old_cells: Vec<&str> = cell_ranges(old).into_iter().map(|r| &old[r]).collect();
    let cells: Vec<String> = fractions
        .iter()
        .enumerate()
        .map(|(k, f)| {
            let dashes = "-".repeat(((f * 60.0).round() as usize).max(3));
            let cell = old_cells.get(k).copied().unwrap_or("---");
            let left = if cell.starts_with(':') { ":" } else { "" };
            let right = if cell.len() > 1 && cell.ends_with(':') {
                ":"
            } else {
                ""
            };
            format!("{left}{dashes}{right}")
        })
        .collect();
    format!("|{}|", cells.join("|"))
}

/// A table row from cell texts (already escaped Markdown).
pub(crate) fn row_from_cells(cells: &[String]) -> String {
    format!("| {} |", cells.join(" | "))
}

/// The cells' texts of a table line.
pub(crate) fn cell_texts(line: &str) -> Vec<String> {
    cell_ranges(line)
        .into_iter()
        .map(|r| line[r].to_string())
        .collect()
}

/// Table lines (header, delimiter, rows) with a column inserted at
/// `column` (shifting the rest right). The new column is empty, `---` in
/// the delimiter, and takes an equal share of the width.
pub(crate) fn insert_column(lines: &[String], column: usize) -> Vec<String> {
    let columns = lines.first().map_or(0, |l| cell_ranges(l).len());
    lines
        .iter()
        .enumerate()
        .map(|(n, line)| {
            if n == 1 {
                let mut fractions = column_fractions(line);
                let share = 1.0 / (columns + 1) as f32;
                fractions.iter_mut().for_each(|f| *f *= 1.0 - share);
                fractions.insert(column.min(fractions.len()), share);
                let mut cells = cell_texts(line);
                cells.insert(column.min(cells.len()), "---".to_string());
                return delimiter_with_fractions(&row_from_cells(&cells), &fractions);
            }
            let mut cells = cell_texts(line);
            cells.resize(columns.max(cells.len()), String::new());
            cells.insert(column.min(cells.len()), String::new());
            row_from_cells(&cells)
        })
        .collect()
}

/// Table lines with column `column` removed (its width goes to the rest).
/// `None` when it is the only column (delete the table instead).
pub(crate) fn delete_column(lines: &[String], column: usize) -> Option<Vec<String>> {
    let columns = lines.first().map_or(0, |l| cell_ranges(l).len());
    if columns <= 1 || column >= columns {
        return None;
    }
    Some(
        lines
            .iter()
            .enumerate()
            .map(|(n, line)| {
                let mut cells = cell_texts(line);
                if column < cells.len() {
                    cells.remove(column);
                }
                if n == 1 {
                    let mut fractions = column_fractions(line);
                    if column < fractions.len() {
                        fractions.remove(column);
                    }
                    let total: f32 = fractions.iter().sum();
                    fractions
                        .iter_mut()
                        .for_each(|f| *f /= total.max(f32::EPSILON));
                    return delimiter_with_fractions(&row_from_cells(&cells), &fractions);
                }
                row_from_cells(&cells)
            })
            .collect(),
    )
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

/// A column border being dragged on the page: the table (by its header
/// line), which inner border, and the column edges as they now are.
#[derive(Clone, Debug)]
pub(crate) struct ColumnDrag {
    pub(crate) header_line: usize,
    boundary: usize,
    edges: Vec<f32>,
}

/// Narrowest a column may be dragged, in px.
const MIN_COLUMN: f32 = 24.0;

impl ColumnDrag {
    /// Each column's share of the table width now.
    pub(crate) fn fractions(&self) -> Vec<f32> {
        let total = self.edges.last().copied().unwrap_or(1.0).max(1.0);
        self.edges
            .windows(2)
            .map(|w| (w[1] - w[0]) / total)
            .collect()
    }

    /// Move the dragged border to `x` (px from the table's left), keeping
    /// both neighbouring columns at least `MIN_COLUMN` wide.
    pub(crate) fn drag_to(&mut self, x: f32) {
        let k = self.boundary;
        let low = self.edges[k - 1] + MIN_COLUMN;
        let high = self.edges[k + 1] - MIN_COLUMN;
        if low <= high {
            self.edges[k] = x.clamp(low, high);
        }
    }
}

impl crate::TextInput {
    /// A table column border under `position` (window coordinates), as a
    /// new drag, from the last layout.
    pub(crate) fn column_border_at(
        &self,
        position: gpui::Point<gpui::Pixels>,
    ) -> Option<ColumnDrag> {
        let bounds = self.last_bounds?;
        let local_y = position.y - bounds.top() + self.scroll_offset_y;
        let local_x = f32::from(position.x - bounds.left());
        let row = self.row_metas.iter().rev().find(|m| {
            m.box_top <= local_y && local_y < m.text_top + m.text_height + gpui::px(8.0)
        })?;
        let (header_line, edges) = row.table.as_ref()?;
        let edges: Vec<f32> = edges.iter().map(|&e| f32::from(e)).collect();
        let boundary =
            (1..edges.len().saturating_sub(1)).find(|&k| (edges[k] - local_x).abs() <= 4.0)?;
        Some(ColumnDrag {
            header_line: *header_line,
            boundary,
            edges,
        })
    }

    pub(crate) fn drag_column_to(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(bounds) = self.last_bounds else {
            return;
        };
        if let Some(drag) = &mut self.column_drag {
            drag.drag_to(f32::from(position.x - bounds.left()));
            cx.notify();
        }
    }

    /// Write the dragged widths into the table's delimiter row (one undo
    /// step), so they are saved and exported.
    pub(crate) fn finish_column_drag(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(drag) = self.column_drag.take() else {
            return;
        };
        let delimiter_line = drag.header_line + 1;
        let Some(start) = line_start_of(&self.content, delimiter_line) else {
            return cx.notify();
        };
        let end = self.line_end(start);
        let old = self.content[start..end].to_string();
        let new = delimiter_with_fractions(&old, &drag.fractions());
        if new != old {
            let caret = self.selected_range.clone();
            self.replace_text_in_range(Some(start..end), &new, cx);
            // Resizing leaves the caret where it was.
            let shift = |o: usize| {
                if o > end {
                    o + new.len() - old.len()
                } else {
                    o
                }
            };
            self.selected_range = shift(caret.start)..shift(caret.end);
        }
        cx.notify();
    }
}

/// A table edit from the right-click menu.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TableCommand {
    InsertRowAbove,
    InsertRowBelow,
    InsertColumnLeft,
    InsertColumnRight,
    DeleteRow,
    DeleteColumn,
    DeleteTable,
}

impl crate::TextInput {
    /// The table around the caret: its first and last line indices and
    /// the caret's line index. Uses the last layout's table rows.
    fn table_extent(&self) -> Option<(usize, usize, usize)> {
        let caret = self.cursor_offset();
        self.table_line_at(caret)?;
        let line = self.content[..caret].matches('\n').count();
        let is_table = |i: usize| self.display_lines.get(i).is_some_and(|dl| dl.table);
        let mut first = line;
        while first > 0 && is_table(first - 1) && !self.display_lines[first].table_header {
            first -= 1;
        }
        let mut last = line;
        while is_table(last + 1) && !self.display_lines[last + 1].table_header {
            last += 1;
        }
        Some((first, last, line))
    }

    /// Run a table command at the caret as one undoable edit. `false` when
    /// the caret is not in a table.
    pub(crate) fn table_command(
        &mut self,
        command: TableCommand,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let Some((first, last, line)) = self.table_extent() else {
            return false;
        };
        let (Some(start), Some(last_start)) = (
            line_start_of(&self.content, first),
            line_start_of(&self.content, last),
        ) else {
            return false;
        };
        let end = self.line_end(last_start);
        let mut lines: Vec<String> = self.content[start..end]
            .split('\n')
            .map(String::from)
            .collect();
        let row = line - first;
        let caret_line_start = line_start_of(&self.content, line).unwrap_or(start);
        let column = cell_at(
            &self.content[caret_line_start..self.line_end(caret_line_start)],
            self.cursor_offset() - caret_line_start,
        )
        .unwrap_or(0);
        let columns = cell_ranges(&lines[0]).len().max(1);
        // Where the caret goes afterwards: (row, column) in the new table.
        let focus;
        match command {
            TableCommand::InsertRowAbove | TableCommand::InsertRowBelow => {
                // The header stays first: "above" the header adds below it.
                let at = if command == TableCommand::InsertRowAbove && row > 1 {
                    row
                } else {
                    row.max(1) + 1
                };
                lines.insert(at, empty_row(columns));
                focus = Some((at, 0));
            }
            TableCommand::InsertColumnLeft | TableCommand::InsertColumnRight => {
                let at = if command == TableCommand::InsertColumnLeft {
                    column
                } else {
                    column + 1
                };
                lines = insert_column(&lines, at);
                focus = Some((row, at));
            }
            TableCommand::DeleteRow => {
                // Only the header and its `|---|` row left: no table.
                if lines.len() <= 2 {
                    return self.table_command(TableCommand::DeleteTable, cx);
                }
                if row == 0 {
                    // The first body row becomes the header.
                    lines[0] = lines.remove(2);
                } else {
                    lines.remove(row);
                }
                // The row that moved up into its place, else the last one.
                let landing = if row == 0 {
                    0
                } else {
                    row.min(lines.len() - 1)
                };
                focus = Some((if landing == 1 { 0 } else { landing }, 0));
            }
            TableCommand::DeleteColumn => match delete_column(&lines, column) {
                Some(new) => {
                    lines = new;
                    focus = Some((row, column.saturating_sub(1)));
                }
                None => return self.table_command(TableCommand::DeleteTable, cx),
            },
            TableCommand::DeleteTable => {
                let remove_end = if end < self.content.len() {
                    end + 1
                } else {
                    end
                };
                self.replace_text_in_range(Some(start..remove_end), "", cx);
                return true;
            }
        }
        let new = lines.join("\n");
        self.replace_text_in_range(Some(start..end), &new, cx);
        if let Some((row, column)) = focus {
            let row_start = start
                + new
                    .split('\n')
                    .take(row)
                    .map(|l| l.len() + 1)
                    .sum::<usize>();
            let row_end = self.line_end(row_start);
            let cells = cell_ranges(&self.content[row_start..row_end]);
            if let Some(cell) = cells.get(column.min(cells.len().saturating_sub(1))) {
                self.move_to(row_start + cell.start, cx);
            }
        }
        true
    }
}

/// Byte offset where line `index` (0-based) starts.
pub(crate) fn line_start_of(content: &str, index: usize) -> Option<usize> {
    if index == 0 {
        return Some(0);
    }
    content
        .match_indices('\n')
        .nth(index - 1)
        .map(|(i, _)| i + 1)
}

/// Typed text for a table cell: a `|` would split the cell, so it goes in
/// escaped (`\|` shows and exports as a plain `|`).
pub(crate) fn escape_cell_text(text: &str) -> String {
    text.replace('|', "\\|")
}

#[cfg(test)]
mod tests {
    use super::{cell_at, cell_ranges, empty_row, visual_row_of, wrap_cell};

    #[test]
    fn the_caret_is_on_the_visual_line_that_shows_its_cell_text() {
        // One table row (line 3) wrapped into three visual lines over two
        // cells: cell A = bytes 0..9 in lines 0 and 1, cell B = 10..30 in
        // lines 0, 1 and 2. A text line (line 4) follows.
        let a: Vec<std::ops::Range<usize>> = vec![0..5, 5..9];
        let b: Vec<std::ops::Range<usize>> = vec![10..16, 16..23, 23..30];
        let line0 = vec![a[0].clone(), b[0].clone()];
        let line1 = vec![a[1].clone(), b[1].clone()];
        let line2 = vec![b[2].clone()];
        let rows = [
            (3, 0, line0.as_slice()),
            (3, 0, line1.as_slice()),
            (3, 0, line2.as_slice()),
            (4, 0, &[][..]),
        ];
        let at = |disp| visual_row_of(rows.iter().copied(), 3, disp);
        assert_eq!(at(2), 0, "inside A's first line");
        assert_eq!(at(7), 1, "inside A's second line");
        assert_eq!(at(9), 1, "the end of A, where it stopped");
        assert_eq!(at(12), 0, "inside B's first line");
        assert_eq!(at(20), 1);
        assert_eq!(at(28), 2, "B's third line, which A has no piece in");
        assert_eq!(at(30), 2, "the end of B");
        // A wrap point belongs to the later line, as for text.
        assert_eq!(at(5), 1);
        assert_eq!(at(16), 1);
        // A byte no piece shows (past the cells) falls back to the first line.
        assert_eq!(at(31), 0);
        // Other lines still pick the last row starting at or before it.
        assert_eq!(visual_row_of(rows.iter().copied(), 4, 5), 3);
    }

    /// Every character is 10 wide; a column holds `chars` characters.
    fn wrapped(text: &str, base: usize, chars: usize) -> Vec<std::ops::Range<usize>> {
        wrap_cell(text, base, |rest| {
            let mut width = 0;
            for (i, _) in rest.char_indices() {
                width += 10;
                if width > chars * 10 {
                    return Some(i);
                }
            }
            None
        })
    }

    fn lines<'a>(text: &'a str, ranges: &[std::ops::Range<usize>], base: usize) -> Vec<&'a str> {
        ranges
            .iter()
            .map(|r| &text[r.start - base..r.end - base])
            .collect()
    }

    #[test]
    fn a_long_cell_wraps_at_spaces_and_covers_every_byte() {
        let text = "Decoded payload, the loaded implant, keys";
        let ranges = wrapped(text, 0, 14);
        assert_eq!(
            lines(text, &ranges, 0),
            ["Decoded ", "payload, the ", "loaded ", "implant, keys"]
        );
        // The lines are contiguous and end where the text ends.
        assert_eq!(ranges.first().unwrap().start, 0);
        assert!(ranges.windows(2).all(|w| w[0].end == w[1].start));
        assert_eq!(ranges.last().unwrap().end, text.len());
    }

    #[test]
    fn ranges_are_offsets_in_the_row_not_the_cell() {
        let ranges = wrapped("aaa bbb", 40, 4);
        assert_eq!(ranges, vec![40..44, 44..47]);
    }

    #[test]
    fn short_empty_and_unbreakable_cells() {
        assert_eq!(wrapped("fits", 0, 10), vec![0..4]);
        // An empty cell is one empty line, so it still has a place to click.
        assert_eq!(wrapped("", 7, 10), vec![7..7]);
        // A word wider than the column is cut, not looped on.
        let ranges = wrapped("abcdefghij", 0, 4);
        assert_eq!(lines("abcdefghij", &ranges, 0), ["abcd", "efgh", "ij"]);
        // A column narrower than one character still moves forward.
        let ranges = wrap_cell("abc", 0, |_| Some(0));
        assert_eq!(ranges, vec![0..1, 1..2, 2..3]);
    }

    #[test]
    fn wrapping_never_splits_a_character() {
        let text = "नमस्ते दुनिया café 😀😀😀";
        for chars in 1..12 {
            let ranges = wrapped(text, 0, chars);
            assert_eq!(ranges.first().unwrap().start, 0);
            assert!(ranges.windows(2).all(|w| w[0].end == w[1].start));
            assert_eq!(ranges.last().unwrap().end, text.len());
            for r in &ranges {
                assert!(text.is_char_boundary(r.start) && text.is_char_boundary(r.end));
                assert!(r.end > r.start, "a line holds a character");
            }
        }
    }

    #[test]
    fn a_break_index_inside_a_character_is_snapped_back() {
        // The measure may point into the middle of a multibyte character.
        let ranges = wrap_cell("é é é", 0, |rest| (rest.len() > 3).then_some(1));
        assert!(ranges.iter().all(|r| "é é é".is_char_boundary(r.start)));
        assert_eq!(ranges.last().unwrap().end, "é é é".len());
    }

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
    fn column_widths_live_in_the_delimiter_row() {
        use super::{column_fractions, delimiter_with_fractions};
        assert_eq!(column_fractions("|------|---|"), [6.0 / 9.0, 3.0 / 9.0]);
        assert_eq!(column_fractions("|:---:|---:|"), [0.5, 0.5]);
        let row = delimiter_with_fractions("|:---|---:|", &[0.25, 0.75]);
        assert_eq!(row, format!("|:{}|{}:|", "-".repeat(15), "-".repeat(45)));
        let back = column_fractions(&row);
        assert!((back[0] - 0.25).abs() < 0.01 && (back[1] - 0.75).abs() < 0.01);
    }

    #[test]
    fn columns_are_inserted_and_deleted() {
        use super::{cell_texts, column_fractions, delete_column, insert_column};
        let table: Vec<String> = ["| A | B |", "|---|---|", "| 1 | 2 |"]
            .map(String::from)
            .to_vec();
        let wider = insert_column(&table, 1);
        assert_eq!(cell_texts(&wider[0]), ["A", "", "B"]);
        assert_eq!(cell_texts(&wider[2]), ["1", "", "2"]);
        let fractions = column_fractions(&wider[1]);
        assert_eq!(fractions.len(), 3);
        assert!(fractions.iter().all(|f| (f - 1.0 / 3.0).abs() < 0.02));
        let narrower = delete_column(&wider, 0).unwrap();
        assert_eq!(cell_texts(&narrower[0]), ["", "B"]);
        assert_eq!(cell_texts(&narrower[2]), ["", "2"]);
        assert_eq!(column_fractions(&narrower[1]).len(), 2);
        assert!(delete_column(&delete_column(&narrower, 0).unwrap(), 0).is_none());
    }

    #[test]
    fn dragging_a_border_moves_width_between_neighbours() {
        let mut drag = super::ColumnDrag {
            header_line: 0,
            boundary: 1,
            edges: vec![0.0, 100.0, 300.0],
        };
        drag.drag_to(150.0);
        let f = drag.fractions();
        assert!((f[0] - 0.5).abs() < 1e-6 && (f[1] - 0.5).abs() < 1e-6);
        // Never narrower than the minimum on either side.
        drag.drag_to(5.0);
        assert_eq!(drag.edges[1], 24.0);
        drag.drag_to(1000.0);
        assert_eq!(drag.edges[1], 276.0);
        assert_eq!(super::line_start_of("a\nbb\nc", 2), Some(5));
        assert_eq!(super::line_start_of("a", 3), None);
    }

    #[test]
    fn resized_widths_reach_the_export() {
        let blocks = crate::parse_content_blocks("| A | B |\n|------|---|\n| 1 | 2 |", 1.0);
        match &blocks[0] {
            sylph_core::document::Block::Table { data } => {
                let w = &data.column_widths;
                assert!((w[0] - 200.0 / 3.0).abs() < 0.01 && (w[1] - 100.0 / 3.0).abs() < 0.01);
            }
            b => panic!("{b:?}"),
        }
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
