use std::{ops::Range, path::PathBuf};

use gpui::{
    actions, anchored, div, fill, hsla, img, point, prelude::*, px, rgb, rgba, size, App,
    Application, AsyncApp, Bounds, ClipboardEntry, ClipboardItem, Context, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable, GlobalElementId,
    KeyBinding, KeystrokeEvent, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PaintQuad, PathPromptOptions, Point, ScrollWheelEvent, ShapedLine, Style,
    Subscription, Task, TextRun, TitlebarOptions, UTF16Selection, WeakEntity, Window, WindowBounds,
    WindowDecorations, WindowOptions,
};

use sylph_core::{
    byte_offset_from_utf16, next_grapheme_boundary, next_word_boundary, previous_grapheme_boundary,
    previous_word_boundary, snap_to_char_boundary, utf16_offset_from_byte, utf8_range_from_utf16,
};
use sylph_storage::Storage;

mod ui;

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        Enter,
        DeleteWordLeft,
        DeleteWordRight,
        DeleteToLineStart,
        DeleteToLineEnd,
        Dedent,
    ]
);

actions!(editor, [Save, Summarize]);

#[derive(Clone)]
struct EditAction {
    start: usize,
    old_text: String,
    new_text: String,
    selection_before: Range<usize>,
}

/// Where the typed text stands relative to storage. The status bar shows
/// this instead of assuming every save worked.
#[derive(Clone, Debug, PartialEq)]
enum SaveState {
    Saved,
    /// Edited; waiting for the autosave debounce or the write itself.
    Saving,
    Failed(String),
}

impl SaveState {
    fn from_result(result: Result<(), Box<dyn std::error::Error>>) -> Self {
        match result {
            Ok(()) => Self::Saved,
            Err(e) => Self::Failed(e.to_string()),
        }
    }

    /// Status-bar wording (Word/Docs style); a failure says why.
    fn label(&self) -> String {
        match self {
            Self::Saved => "All changes saved".to_string(),
            Self::Saving => "Saving…".to_string(),
            Self::Failed(reason) => format!("Save failed: {reason}"),
        }
    }
}

struct TextInput {
    focus_handle: FocusHandle,
    content: String,
    placeholder: String,
    selected_range: Range<usize>,
    selection_reversed: bool,
    preferred_column: Option<usize>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<gpui::Pixels>>,
    all_lines: Vec<ShapedLine>,
    line_char_offsets: Vec<usize>,
    /// Markdown mode ON: the canvas shows the WYSIWYG transform of
    /// `content` (block syntax hidden, heading type scale applied); OFF
    /// renders source literally. Mirrors `SylphApp::markdown_mode`.
    markdown_mode: bool,
    /// Per-row layout metrics from the last prepaint — rows have variable
    /// heights (headings are taller and carry space before/after).
    row_metas: Vec<RowMeta>,
    /// Total laid-out content height from the last prepaint.
    content_height: gpui::Pixels,
    /// Source→display maps from the last prepaint (identity when OFF).
    display_lines: Vec<DisplayLine>,
    /// Row the caret was laid out in, for scroll-into-view.
    cursor_row: usize,
    line_height: gpui::Pixels,
    scroll_offset_y: gpui::Pixels,
    is_selecting: bool,
    storage: Storage,
    doc_id: i64,
    undo_stack: Vec<EditAction>,
    redo_stack: Vec<EditAction>,
    show_line_numbers: bool,
    word_wrap: bool,
    save_task: Option<Task<()>>,
    save_state: SaveState,
}

impl TextInput {
    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = snap_to_char_boundary(&self.content, offset.min(self.content.len()));
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.preferred_column = None;
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = snap_to_char_boundary(&self.content, offset.min(self.content.len()));
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.preferred_column = None;
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn ensure_cursor_visible(&mut self) {
        let Some(bounds) = self.last_bounds else {
            return;
        };
        // Rows carry their own box metrics (headings are taller), so use
        // the stored layout instead of assuming a uniform line height.
        let Some(row) = self.row_metas.get(self.cursor_row) else {
            return;
        };
        let (row_top, text_height) = (row.text_top, row.text_height);
        if text_height <= px(0.0) || self.row_metas.is_empty() {
            return;
        }

        let visible_height = bounds.size.height;
        let max_scroll = (self.content_height - visible_height).max(px(0.0));
        let mut scroll = self.scroll_offset_y;

        if row_top < scroll {
            scroll = row_top;
        } else if row_top + text_height > scroll + visible_height {
            scroll = row_top + text_height - visible_height;
        }

        self.scroll_offset_y = scroll.max(px(0.0)).min(max_scroll);
    }

    fn index_for_mouse_position(&self, position: Point<gpui::Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let Some(bounds) = self.last_bounds.as_ref() else {
            return 0;
        };
        if self.row_metas.is_empty() || self.all_lines.is_empty() {
            return 0;
        }
        let local_y = position.y - bounds.top() + self.scroll_offset_y;
        if local_y < px(0.0) {
            return 0;
        }
        // Row containing the click — rows have variable heights, so walk
        // the box ranges instead of dividing by a uniform line height.
        let mut row_idx = self.row_metas.len() - 1;
        for (i, m) in self.row_metas.iter().enumerate() {
            if local_y < m.box_top + m.box_height {
                row_idx = i;
                break;
            }
        }
        let meta = self.row_metas[row_idx];
        let local_x = (position.x - bounds.left()).max(px(0.0));
        // The shaped line is display text; map back through the source→
        // display transform so clicks land on real source offsets.
        let disp = meta.disp_start + self.all_lines[row_idx].closest_index_for_x(local_x);
        let src = match self.display_lines.get(meta.line_idx) {
            Some(dl) => dl.src_offset + dl.disp_to_src(disp),
            None => meta.src_start,
        };
        snap_to_char_boundary(&self.content, src.min(self.content.len()))
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let prev = if self.selected_range.is_empty() {
            self.previous_boundary(self.cursor_offset())
        } else {
            self.selected_range.start
        };
        self.move_to(prev, cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let next = if self.selected_range.is_empty() {
            self.next_boundary(self.cursor_offset())
        } else {
            self.selected_range.end
        };
        self.move_to(next, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn move_word_left(&mut self, _: &MoveWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let new_pos = self.previous_word_boundary(self.cursor_offset());
        self.move_to(new_pos, cx);
    }

    fn move_word_right(&mut self, _: &MoveWordRight, _: &mut Window, cx: &mut Context<Self>) {
        let new_pos = self.next_word_boundary(self.cursor_offset());
        self.move_to(new_pos, cx);
    }

    fn page_up(&mut self, _: &PageUp, _: &mut Window, cx: &mut Context<Self>) {
        let visible_height: f32 = self
            .last_bounds
            .map(|b| b.size.height)
            .unwrap_or(px(0.))
            .into();
        let line_height: f32 = self.line_height.into();
        if line_height <= 0. || self.line_char_offsets.is_empty() {
            return;
        }

        let lines_per_page = ((visible_height / line_height) as usize).max(1);
        let scroll_delta = px(line_height * lines_per_page as f32);

        self.scroll_offset_y = (self.scroll_offset_y - scroll_delta).max(px(0.));

        let old_pos = self.cursor_offset();
        let current_line_idx = self
            .line_char_offsets
            .iter()
            .position(|&offset| offset > old_pos)
            .unwrap_or(self.line_char_offsets.len())
            .saturating_sub(1);
        if current_line_idx >= self.line_char_offsets.len() {
            return;
        }
        let old_col = old_pos.saturating_sub(self.line_char_offsets[current_line_idx]);

        let new_line_idx = current_line_idx.saturating_sub(lines_per_page);
        if new_line_idx >= self.line_char_offsets.len() {
            return;
        }
        let target_line_len = if new_line_idx + 1 < self.line_char_offsets.len() {
            self.line_char_offsets[new_line_idx + 1]
                .saturating_sub(self.line_char_offsets[new_line_idx])
                .saturating_sub(1)
        } else {
            self.content
                .len()
                .saturating_sub(self.line_char_offsets[new_line_idx])
        };

        let new_col = old_col.min(target_line_len);
        let new_pos = self.line_char_offsets[new_line_idx] + new_col;

        self.move_to(new_pos, cx);
    }

    fn page_down(&mut self, _: &PageDown, _: &mut Window, cx: &mut Context<Self>) {
        let visible_height: f32 = self
            .last_bounds
            .map(|b| b.size.height)
            .unwrap_or(px(0.))
            .into();
        let line_height: f32 = self.line_height.into();
        if line_height <= 0. || self.line_char_offsets.is_empty() {
            return;
        }

        let lines_per_page = ((visible_height / line_height) as usize).max(1);
        let scroll_delta = px(line_height * lines_per_page as f32);

        let total_content_height = self.content_height;
        let max_scroll = (total_content_height - px(visible_height)).max(px(0.));
        self.scroll_offset_y = (self.scroll_offset_y + scroll_delta).min(max_scroll);

        let old_pos = self.cursor_offset();
        let current_line_idx = self
            .line_char_offsets
            .iter()
            .position(|&offset| offset > old_pos)
            .unwrap_or(self.line_char_offsets.len())
            .saturating_sub(1);
        if current_line_idx >= self.line_char_offsets.len() {
            return;
        }
        let old_col = old_pos.saturating_sub(self.line_char_offsets[current_line_idx]);

        let new_line_idx =
            (current_line_idx + lines_per_page).min(self.line_char_offsets.len().saturating_sub(1));
        let target_line_len = if new_line_idx + 1 < self.line_char_offsets.len() {
            self.line_char_offsets[new_line_idx + 1]
                .saturating_sub(self.line_char_offsets[new_line_idx])
                .saturating_sub(1)
        } else {
            self.content
                .len()
                .saturating_sub(self.line_char_offsets[new_line_idx])
        };

        let new_col = old_col.min(target_line_len);
        let new_pos = self.line_char_offsets[new_line_idx] + new_col;

        self.move_to(new_pos, cx);
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(-1, cx);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(1, cx);
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.select_vertically(-1, cx);
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.select_vertically(1, cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.line_start(self.cursor_offset());
        self.move_to(offset, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.line_end(self.cursor_offset());
        self.move_to(offset, cx);
    }

    fn vertically_navigate(&mut self, direction: i32, select: bool, cx: &mut Context<Self>) {
        let cursor = snap_to_char_boundary(&self.content, self.cursor_offset());
        let (line_idx, byte_col) = self.cursor_line_and_column(cursor);
        let desired_column = self.preferred_column.unwrap_or(byte_col);
        let new_line_idx = (line_idx as i32 + direction).max(0) as usize;
        // Use a line counting method that includes trailing empty lines
        let line_count = self.content.matches('\n').count() + 1;
        if new_line_idx >= line_count {
            return;
        }
        let lines: Vec<&str> = self.content.split('\n').collect();
        let new_line_start: usize = lines
            .iter()
            .take(new_line_idx)
            .map(|line| line.len() + 1)
            .sum();
        let target_line_len = lines.get(new_line_idx).map(|line| line.len()).unwrap_or(0);
        let new_col = desired_column.min(target_line_len);
        let new_offset = snap_to_char_boundary(&self.content, new_line_start + new_col);
        if select {
            self.select_to(new_offset, cx);
        } else {
            self.move_to(new_offset, cx);
            self.preferred_column = Some(desired_column);
        }
    }

    fn move_vertically(&mut self, direction: i32, cx: &mut Context<Self>) {
        self.vertically_navigate(direction, false, cx);
    }

    fn select_vertically(&mut self, direction: i32, cx: &mut Context<Self>) {
        self.vertically_navigate(direction, true, cx);
    }

    fn cursor_line_and_column(&self, offset: usize) -> (usize, usize) {
        let offset = snap_to_char_boundary(&self.content, offset);
        let before = &self.content[..offset];
        // Count lines including trailing empty line (lines() doesn't count trailing \n)
        let line_count = before.matches('\n').count();
        let line_idx = line_count;
        let last_newline = before.rfind('\n').map(|p| p + 1).unwrap_or(0);
        let byte_col = offset - last_newline;
        (line_idx, byte_col)
    }

    fn line_start(&self, offset: usize) -> usize {
        let offset = snap_to_char_boundary(&self.content, offset);
        self.content[..offset]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0)
    }

    fn line_end(&self, offset: usize) -> usize {
        let offset = snap_to_char_boundary(&self.content, offset);
        self.content[offset..]
            .find('\n')
            .map(|p| offset + p)
            .unwrap_or(self.content.len())
    }

    /// Inline markdown styling for one run of text: emphasis, code spans,
    /// strikethrough, links/autolinks (whole span colored, matching
    /// export's Link style), inline images (plain, matching export's alt
    /// text), and backslash escapes. Marker tokens become explicit runs,
    /// so the runs always cover `text` exactly and paint boundaries stay
    /// aligned with what the export renders.
    fn inline_runs(text: &str, font: gpui::Font, base_color: gpui::Hsla) -> Vec<TextRun> {
        let color_bold = hsla(0.0, 0.0, 0.15, 1.0);
        let color_italic = hsla(0.0, 0.0, 0.35, 1.0);
        let color_code = hsla(120.0 / 360.0, 0.5, 0.35, 1.0);
        let color_link = hsla(210.0 / 360.0, 0.8, 0.45, 1.0);
        let color_strike = hsla(0.0, 0.6, 0.45, 1.0);

        let state_color = |in_code: bool, in_strike: bool, in_bold: bool, in_italic: bool| {
            if in_code {
                color_code
            } else if in_strike {
                color_strike
            } else if in_bold {
                color_bold
            } else if in_italic {
                color_italic
            } else {
                base_color
            }
        };
        let mut runs: Vec<TextRun> = Vec::new();
        // Struck runs get a real strike line (plus the strike color), so
        // the canvas shows what the export renders for `~~…~~`.
        let mut push = |len: usize, color: gpui::Hsla, struck: bool| {
            runs.push(TextRun {
                len,
                font: font.clone(),
                color,
                background_color: None,
                underline: None,
                strikethrough: struck.then(|| gpui::StrikethroughStyle {
                    thickness: px(1.0),
                    color: None,
                }),
            });
        };

        let mut chars = text.char_indices().peekable();
        let mut seg_start = 0;
        let mut in_bold = false;
        let mut in_italic = false;
        let mut in_code = false;
        let mut in_strike = false;

        // Flush [seg_start, end) with the current emphasis-state color.
        macro_rules! flush {
            ($end:expr) => {
                if $end > seg_start {
                    let color = state_color(in_code, in_strike, in_bold, in_italic);
                    push($end - seg_start, color, in_strike);
                }
            };
        }

        while let Some((i, ch)) = chars.next() {
            // 1. Backslash escape: ASCII punctuation after `\` is literal
            //    (export drops the backslash and styles nothing extra).
            if !in_code
                && ch == '\\'
                && chars
                    .peek()
                    .is_some_and(|(_, next)| next.is_ascii_punctuation())
            {
                chars.next();
                continue;
            }

            // 2. Link/image/autolink spans: color exactly what export
            //    treats as a link; image alt text stays plain, like export.
            let span_end = if in_code {
                None
            } else if ch == '!' {
                image_span_end(text, i)
            } else if ch == '[' {
                link_span_end(text, i)
            } else if ch == '<' {
                autolink_span_end(text, i)
            } else {
                None
            };
            if let Some(end) = span_end {
                flush!(i);
                push(
                    end - i,
                    if ch == '!' { base_color } else { color_link },
                    in_strike,
                );
                seg_start = end;
                while chars.peek().is_some_and(|(b, _)| *b < end) {
                    chars.next();
                }
                continue;
            }

            if ch == '`' {
                flush!(i);
                in_code = !in_code;
                seg_start = i;
                if !in_code {
                    push(1, color_code, in_strike);
                    seg_start = i + 1;
                }
            } else if !in_code && ch == '~' && chars.peek().map(|(_, c)| *c) == Some('~') {
                // `~~` pair: markers stay plain, content gets strike color
                // and a strike line.
                flush!(i);
                push(2, base_color, false);
                in_strike = !in_strike;
                chars.next();
                seg_start = i + 2;
            } else if !in_code && ch == '*' {
                // `*` only — `_..._` is not a marker (snake_case stays
                // plain), exactly like the export parser.
                if chars.peek().map(|(_, c)| *c) == Some(ch) {
                    flush!(i);
                    push(2, base_color, in_strike);
                    in_bold = !in_bold;
                    chars.next();
                    seg_start = i + 2;
                } else if !in_bold {
                    flush!(i);
                    push(1, base_color, in_strike);
                    in_italic = !in_italic;
                    seg_start = i + 1;
                }
                // Lone `*` inside bold stays literal (as before).
            } else if !in_code && (ch == '[' || ch == ']' || ch == '(' || ch == ')' || ch == '|') {
                flush!(i);
                push(
                    ch.len_utf8(),
                    if ch == '|' { base_color } else { color_link },
                    in_strike,
                );
                seg_start = i + ch.len_utf8();
            }
        }

        if seg_start < text.len() {
            let color = state_color(in_code, in_strike, in_bold, in_italic);
            push(text.len() - seg_start, color, in_strike);
        }

        runs
    }

    /// Style one editor line for display. Mirrors the export parser's
    /// decisions — same block order as `parse_content_blocks`, same inline
    /// rules as `parse_inline_runs` — so the editor never claims styling
    /// the export will not render. `in_fence` marks a line inside ``` where
    /// export keeps text verbatim.
    fn markdown_runs(
        line: &str,
        font: gpui::Font,
        base_color: gpui::Hsla,
        in_fence: bool,
    ) -> Vec<TextRun> {
        let mut runs = Vec::new();

        // Fenced-code body: export stores it verbatim — flat code color.
        if in_fence {
            runs.push(TextRun {
                len: line.len(),
                font,
                color: hsla(120.0 / 360.0, 0.5, 0.35, 1.0),
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            return runs;
        }

        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        if indent > 0 {
            runs.push(TextRun {
                len: indent,
                font: font.clone(),
                color: base_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }

        let color_header = hsla(210.0 / 360.0, 0.8, 0.4, 1.0);
        let color_code = hsla(120.0 / 360.0, 0.5, 0.35, 1.0);
        let color_list = hsla(30.0 / 360.0, 0.7, 0.45, 1.0);
        let color_quote = hsla(0.0, 0.0, 0.5, 1.0);

        // Block order mirrors parse_content_blocks: fence → quote →
        // heading → rule → list → paragraph.

        // ── Fence marker (```lang): flat code color.
        if trimmed.starts_with("```") {
            runs.push(TextRun {
                len: trimmed.len(),
                font: font.clone(),
                color: color_code,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            return runs;
        }

        // ── Blockquote: same >-stripping as export (any depth, space optional).
        if trimmed.starts_with('>') {
            let mut rest = trimmed;
            while let Some(r) = rest.strip_prefix('>') {
                rest = r.strip_prefix(' ').unwrap_or(r);
            }
            let marker_len = trimmed.len() - rest.len();
            runs.push(TextRun {
                len: marker_len,
                font: font.clone(),
                color: color_quote,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            runs.extend(Self::inline_runs(rest, font, color_quote));
            return runs;
        }

        // ── Heading: hashes + space, like export's heading_level_and_text.
        let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&hash_count) && trimmed[hash_count..].starts_with(' ') {
            let prefix_len = hash_count + 1;
            runs.push(TextRun {
                len: prefix_len,
                font: font.clone(),
                color: color_header,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            runs.extend(Self::inline_runs(
                &trimmed[prefix_len..],
                font,
                color_header,
            ));
            return runs;
        }

        // ── Thematic break, checked before list (like export: `* * *` is
        //    a rule, not a bullet whose content is `* *`).
        if is_horizontal_rule(trimmed) {
            runs.push(TextRun {
                len: trimmed.len(),
                font: font.clone(),
                color: color_list,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            return runs;
        }

        // ── List item: marker colored like the bullet color, plus export's
        //    task box as metadata; item text gets inline styling.
        if let Some((marker_len, content)) = list_marker_and_content(trimmed) {
            runs.push(TextRun {
                len: marker_len,
                font: font.clone(),
                color: color_list,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            let box_len = if content.starts_with("[ ] ")
                || content.starts_with("[x] ")
                || content.starts_with("[X] ")
            {
                4
            } else if matches!(content, "[ ]" | "[x]" | "[X]") {
                3
            } else {
                0
            };
            if box_len > 0 {
                runs.push(TextRun {
                    len: box_len,
                    font: font.clone(),
                    color: color_list,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                });
            }
            runs.extend(Self::inline_runs(&content[box_len..], font, base_color));
            return runs;
        }

        // ── Everything else (paragraphs and table rows): export parses
        //    those runs inline, so style them the same way.
        runs.extend(Self::inline_runs(trimmed, font.clone(), base_color));

        if runs.is_empty() {
            runs.push(TextRun {
                len: line.len(),
                font,
                color: base_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }

        runs
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let prev = self.previous_boundary(self.cursor_offset());
            if self.cursor_offset() == prev {
                return;
            }
            self.select_to(prev, cx);
        }
        self.replace_text_in_range(None, "", cx);
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.cursor_offset());
            if self.cursor_offset() == next {
                return;
            }
            self.select_to(next, cx);
        }
        self.replace_text_in_range(None, "", cx);
    }

    fn delete_word_left(&mut self, _: &DeleteWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let boundary = self.previous_word_boundary(self.cursor_offset());
            self.selected_range = boundary..self.cursor_offset();
            self.selection_reversed = true;
        }
        self.replace_text_in_range(None, "", cx);
    }

    fn delete_word_right(&mut self, _: &DeleteWordRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let boundary = self.next_word_boundary(self.cursor_offset());
            self.selected_range = self.cursor_offset()..boundary;
        }
        self.replace_text_in_range(None, "", cx);
    }

    fn delete_to_line_start(
        &mut self,
        _: &DeleteToLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            let start = self.line_start(self.cursor_offset());
            self.selected_range = start..self.cursor_offset();
            self.selection_reversed = true;
        }
        self.replace_text_in_range(None, "", cx);
    }

    fn delete_to_line_end(&mut self, _: &DeleteToLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let end = self.line_end(self.cursor_offset());
            self.selected_range = self.cursor_offset()..end;
        }
        self.replace_text_in_range(None, "", cx);
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        let pos = self.index_for_mouse_position(event.position);
        if event.click_count == 2 {
            let start = self.previous_word_boundary(pos);
            let end = self.next_word_boundary(pos);
            if start == end {
                self.selected_range = pos..self.next_boundary(pos);
            } else {
                self.selected_range = start..end;
            }
            self.selection_reversed = false;
            self.preferred_column = None;
            self.is_selecting = true;
            cx.notify();
            return;
        }

        if event.click_count == 3 {
            let line_start = self.content[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let line_end = self.content[pos..]
                .find('\n')
                .map(|i| pos + i)
                .unwrap_or(self.content.len());
            self.selected_range = line_start..line_end;
            self.selection_reversed = false;
            self.preferred_column = None;
            self.is_selecting = false;
            cx.notify();
            return;
        }

        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(pos, cx);
        } else {
            self.move_to(pos, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(self.line_height);
        let visible_height: f32 = self
            .last_bounds
            .map(|b| b.size.height)
            .unwrap_or(px(0.))
            .into();
        let total_content_height: f32 = self.content_height.into();
        let max_scroll = (total_content_height - visible_height).max(0.);
        let current: f32 = self.scroll_offset_y.into();
        let delta_f: f32 = delta.y.into();
        let new_scroll = (current - delta_f).clamp(0., max_scroll);
        self.scroll_offset_y = px(new_scroll);
        cx.notify();
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", cx);
        }
    }

    fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(None, "\n", cx);
    }

    fn indent(&mut self, _: &Indent, _: &mut Window, cx: &mut Context<Self>) {
        let indent_str = "    ";
        let start = self.selected_range.start;
        let end = self.selected_range.end;

        if start == end {
            self.replace_text_in_range(None, indent_str, cx);
            return;
        }

        let old_text = self.content[start..end].to_string();
        let lines: Vec<&str> = old_text.split('\n').collect();
        let indented_lines: Vec<String> = lines
            .iter()
            .map(|line| format!("{}{}", indent_str, line))
            .collect();
        let final_new_text = indented_lines.join("\n");

        let new_start = start + indent_str.len();
        let new_end = end + (lines.len() * indent_str.len());

        self.replace_text_in_range(Some(start..end), &final_new_text, cx);
        self.selected_range = new_start..new_end;
        cx.notify();
    }

    fn dedent(&mut self, _: &Dedent, _: &mut Window, cx: &mut Context<Self>) {
        let indent_str = "    ";
        let start = self.selected_range.start;
        let end = self.selected_range.end;
        let line_start = self.line_start(start);
        let range_start = line_start;

        let old_text = self.content[range_start..end].to_string();
        let lines: Vec<&str> = old_text.split('\n').collect();
        let dedented_lines: Vec<String> = lines
            .iter()
            .map(|line| {
                if let Some(stripped) = line.strip_prefix(indent_str) {
                    stripped.to_string()
                } else if let Some(stripped) = line.strip_prefix('\t') {
                    stripped.to_string()
                } else if line.starts_with(' ') {
                    let trim = line.len().min(4);
                    let spaces = line.chars().take_while(|c| *c == ' ').count().min(trim);
                    line[spaces..].to_string()
                } else {
                    line.to_string()
                }
            })
            .collect();
        let final_new_text = dedented_lines.join("\n");

        let removed: usize = lines
            .iter()
            .zip(dedented_lines.iter())
            .map(|(old, new)| old.len().saturating_sub(new.len()))
            .sum();

        self.replace_text_in_range(Some(range_start..end), &final_new_text, cx);
        self.selected_range = start..end.saturating_sub(removed);
        cx.notify();
    }

    fn save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        self.save_now(cx);
    }

    /// Write the text immediately and record whether it worked. Supersedes
    /// a pending autosave, which would only write the same text again.
    fn save_now(&mut self, cx: &mut Context<Self>) -> bool {
        self.save_task.take();
        let text = self.content.clone();
        let _ = std::fs::write(data_path("document.txt"), &text);
        self.save_state = SaveState::from_result(self.storage.save_text(self.doc_id, &text));
        cx.notify();
        self.save_state == SaveState::Saved
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        self.save_task.take();
        self.save_state = SaveState::Saving;
        let doc_id = self.doc_id;
        let text = self.content.clone();
        self.save_task = Some(cx.spawn(
            async move |this: WeakEntity<TextInput>, cx: &mut gpui::AsyncApp| {
                // Keep writes out of the typing path while saving shortly after
                // the user pauses. Dropping the previous task debounces bursts.
                gpui::Timer::after(std::time::Duration::from_millis(750)).await;
                let _ = std::fs::write(data_path("document.txt"), &text);
                let state = SaveState::from_result(
                    sylph_storage::Storage::open()
                        .and_then(|storage| storage.save_text(doc_id, &text)),
                );
                let _ = this.update(cx, |this, cx| {
                    this.save_state = state;
                    cx.notify();
                });
            },
        ));
    }

    fn summarize(&mut self, _: &Summarize, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.content.clone();
        let summary = sylph_py_bridge::summarize_text(&text);
        let _ = std::fs::write(data_path("summary.txt"), &summary);
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16.unwrap_or(self.selected_range.clone());
        let start = snap_to_char_boundary(&self.content, range.start);
        let end = snap_to_char_boundary(&self.content, range.end).max(start);

        let old_text = self.content[start..end].to_string();
        let selection_before = self.selected_range.clone();

        self.undo_stack.push(EditAction {
            start,
            old_text,
            new_text: new_text.to_string(),
            selection_before,
        });
        self.redo_stack.clear();

        self.apply_edit(start, end, new_text, cx);
    }

    fn apply_edit(&mut self, start: usize, end: usize, new_text: &str, cx: &mut Context<Self>) {
        let start = snap_to_char_boundary(&self.content, start);
        let end = snap_to_char_boundary(&self.content, end)
            .max(start)
            .min(self.content.len());
        if start > end {
            return;
        }

        self.content.replace_range(start..end, new_text);
        self.selected_range = start + new_text.len()..start + new_text.len();
        self.selection_reversed = false;
        self.preferred_column = None;
        self.ensure_cursor_visible();
        self.schedule_save(cx);
        cx.notify();
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        if let Some(action) = self.undo_stack.pop() {
            let replace_end = action.start + action.new_text.len();
            self.apply_edit(action.start, replace_end, &action.old_text, cx);
            self.selected_range = action.selection_before.clone();
            self.redo_stack.push(action);
            cx.notify();
        }
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        if let Some(action) = self.redo_stack.pop() {
            let replace_end = action.start + action.old_text.len();
            self.apply_edit(action.start, replace_end, &action.new_text, cx);
            self.selected_range =
                action.start + action.new_text.len()..action.start + action.new_text.len();
            self.undo_stack.push(action);
            cx.notify();
        }
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        previous_grapheme_boundary(&self.content, offset)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        next_grapheme_boundary(&self.content, offset)
    }

    fn previous_word_boundary(&self, pos: usize) -> usize {
        previous_word_boundary(&self.content, pos)
    }

    fn next_word_boundary(&self, pos: usize) -> usize {
        next_word_boundary(&self.content, pos)
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = utf8_range_from_utf16(&self.content, range_utf16);
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: utf16_offset_from_byte(&self.content, self.selected_range.start)
                ..utf16_offset_from_byte(&self.content, self.selected_range.end),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16.map(|range| utf8_range_from_utf16(&self.content, range));
        TextInput::replace_text_in_range(self, range, new_text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let replacement_range = range_utf16
            .clone()
            .map(|range| utf8_range_from_utf16(&self.content, range));
        let replacement_start = replacement_range
            .as_ref()
            .map(|range| range.start)
            .unwrap_or(self.selected_range.start);
        TextInput::replace_text_in_range(self, replacement_range, new_text, cx);
        if let Some(sel) = new_selected_range_utf16 {
            let start = replacement_start + byte_offset_from_utf16(new_text, sel.start);
            let end = replacement_start + byte_offset_from_utf16(new_text, sel.end);
            self.selected_range = start..end;
            self.selection_reversed = false;
        }
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<gpui::Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<gpui::Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = utf8_range_from_utf16(&self.content, range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(range.start),
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(range.end),
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<gpui::Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        last_layout
            .index_for_x(point.x - line_point.x)
            .map(|byte_offset| utf16_offset_from_byte(&self.content, byte_offset))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

/// One laid-out visual row (a wrapped segment of a logical line). Rows
/// have their own box/text metrics so headings can be taller than body
/// text and carry space before/after.
struct PrepRow {
    shaped: ShapedLine,
    /// Top of the row's full box, relative to the element (space-before
    /// included); rows are stacked contiguously.
    box_top: gpui::Pixels,
    /// Top of the text within the box.
    text_top: gpui::Pixels,
    text_height: gpui::Pixels,
    box_height: gpui::Pixels,
    /// Absolute source byte offset of the row start.
    src_start: usize,
    /// Display byte offset of the row start within its logical line.
    disp_start: usize,
    /// Logical line this row belongs to.
    line_idx: usize,
    /// Draw the row as a horizontal rule (Markdown mode ON).
    rule_color: Option<gpui::Hsla>,
}

/// Persisted half of `PrepRow` — layout metrics the input needs for
/// hit-testing, scroll clamping and caret visibility between frames.
#[derive(Clone, Copy)]
struct RowMeta {
    box_top: gpui::Pixels,
    text_top: gpui::Pixels,
    text_height: gpui::Pixels,
    box_height: gpui::Pixels,
    src_start: usize,
    disp_start: usize,
    line_idx: usize,
}

struct PrepaintState {
    rows: Vec<PrepRow>,
    /// (y, height, shaped number) per logical line, for the optional gutter.
    line_numbers: Vec<(gpui::Pixels, gpui::Pixels, ShapedLine)>,
    gutter_width: gpui::Pixels,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let _input = self.input.read(cx);
        let mut style = Style::default();
        style.size.width = gpui::relative(1.).into();
        // The custom element is the editor viewport, not a content-sized child.
        // Keeping this at the parent's height makes scrolling and caret visibility
        // use the actual visible area instead of a one-line/content-sized bounds.
        style.size.height = gpui::relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<gpui::Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> PrepaintState {
        let input = self.input.read(cx);
        let content = &input.content;
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let scroll_offset_y = input.scroll_offset_y;
        let show_line_numbers = input.show_line_numbers;
        let word_wrap = input.word_wrap;
        let markdown_on = input.markdown_mode;
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let base_font = style.font();

        let line_count = if content.is_empty() {
            1
        } else {
            content.matches('\n').count() + 1
        };
        let gutter_width = if show_line_numbers {
            let digits = line_count.to_string().len();
            px((digits as f32 * 10.0) + 20.0)
        } else {
            px(0.0)
        };

        let display_text = if content.is_empty() {
            input.placeholder.clone()
        } else {
            content.clone()
        };

        let lines: Vec<String> = if display_text.is_empty() {
            vec![String::new()]
        } else {
            let mut lines: Vec<String> = display_text.lines().map(String::from).collect();
            // Include trailing empty line if content ends with newline
            if display_text.ends_with('\n') {
                lines.push(String::new());
            }
            lines
        };

        let text_color = if content.is_empty() {
            hsla(0., 0., 0., 0.3)
        } else {
            style.color
        };

        // Source→display transform: identity when Markdown mode is OFF.
        let display_lines = build_display_lines(&lines, markdown_on);

        let available_width = bounds.size.width - gutter_width;
        let mut rows: Vec<PrepRow> = Vec::new();
        let mut y = px(0.0);

        for (i, dl) in display_lines.iter().enumerate() {
            let row_font_size = dl.font_size.map(px).unwrap_or(font_size);
            // Body rows keep the layout's line height; rows with an
            // explicit size (headings) get a box that fits their glyphs.
            let text_height = match dl.font_size {
                Some(size) => line_height.max(px(size * 1.4)),
                None => line_height,
            };
            let mut font = if dl.mono {
                gpui::font(ui::MONO_FONT)
            } else {
                base_font.clone()
            };
            if dl.bold {
                font = font.bold();
            }
            if dl.italic {
                font = font.italic();
            }

            // Wrap the *display* text (Markdown ON wraps rendered text,
            // not source), recording each row's display range.
            let mut ranges: Vec<(usize, usize)> = Vec::new();
            if word_wrap && available_width > px(0.0) && !dl.text.is_empty() {
                let mut d0 = 0usize;
                while d0 < dl.text.len() {
                    let remaining = &dl.text[d0..];
                    let check_run = TextRun {
                        len: remaining.len(),
                        font: font.clone(),
                        color: text_color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let measured = window.text_system().shape_line(
                        remaining.to_string().into(),
                        row_font_size,
                        &[check_run],
                        None,
                    );
                    if measured.width > available_width && remaining.len() > 1 {
                        let break_idx = measured.closest_index_for_x(available_width);
                        let mut actual_break = if break_idx > 0 {
                            remaining[..break_idx]
                                .rfind(' ')
                                .map(|p| p + 1)
                                .unwrap_or(break_idx)
                        } else {
                            // First char boundary — never split UTF-8.
                            remaining
                                .char_indices()
                                .nth(1)
                                .map(|(p, _)| p)
                                .unwrap_or(remaining.len())
                        };
                        while actual_break > 0
                            && actual_break < remaining.len()
                            && !remaining.is_char_boundary(actual_break)
                        {
                            actual_break -= 1;
                        }
                        actual_break = actual_break.max(1);
                        ranges.push((d0, d0 + actual_break));
                        d0 += actual_break;
                    } else {
                        ranges.push((d0, dl.text.len()));
                        d0 = dl.text.len();
                    }
                }
            } else {
                ranges.push((0, dl.text.len()));
            }
            if ranges.is_empty() {
                ranges.push((0, 0));
            }

            let row_count = ranges.len();
            for (r, &(d0, d1)) in ranges.iter().enumerate() {
                let space_before = if r == 0 { dl.space_before } else { 0.0 };
                let space_after = if r + 1 == row_count {
                    dl.space_after
                } else {
                    0.0
                };
                let box_top = y;
                let text_top = y + px(space_before);
                let box_height = px(space_before) + text_height + px(space_after);
                y = box_top + box_height;

                let slice = &dl.text[d0..d1];
                let runs = if markdown_on {
                    display_runs(dl, d0..d1, font.clone(), text_color)
                } else {
                    TextInput::markdown_runs(slice, font.clone(), text_color, dl.fenced)
                };
                let shaped = window.text_system().shape_line(
                    slice.to_string().into(),
                    row_font_size,
                    &runs,
                    None,
                );
                let rule_color = match dl.kind {
                    DisplayKind::Rule => {
                        let mut color = text_color;
                        color.a = 0.35;
                        Some(color)
                    }
                    // Page breaks reuse the rule painter, but stay visibly
                    // distinct from ordinary `---` rules.
                    DisplayKind::PageBreak => Some(hsla(210.0 / 360.0, 0.8, 0.4, 0.6)),
                    _ => None,
                };
                rows.push(PrepRow {
                    shaped,
                    box_top,
                    text_top,
                    text_height,
                    box_height,
                    src_start: dl.src_offset + dl.disp_to_src(d0),
                    disp_start: d0,
                    line_idx: i,
                    rule_color,
                });
            }
        }
        let content_height = y;

        // Caret and selection live in source space; map them through the
        // display transform onto the row that shows them.
        let line_of = |offset: usize| -> usize {
            let mut idx = 0;
            for (i, dl) in display_lines.iter().enumerate() {
                if dl.src_offset <= offset {
                    idx = i;
                } else {
                    break;
                }
            }
            idx
        };
        let disp_of = |offset: usize| -> usize {
            let li = line_of(offset);
            let dl = &display_lines[li];
            dl.src_to_disp(offset.saturating_sub(dl.src_offset))
        };
        let row_of = |line_idx: usize, disp: usize| -> usize {
            let mut found = rows
                .iter()
                .position(|r| r.line_idx == line_idx)
                .unwrap_or(0);
            for (ri, r) in rows.iter().enumerate() {
                if r.line_idx == line_idx && disp >= r.disp_start {
                    found = ri;
                }
            }
            found
        };
        let x_of = |ri: usize, disp: usize| -> gpui::Pixels {
            let r = &rows[ri];
            r.shaped.x_for_index(disp.saturating_sub(r.disp_start))
        };

        let cursor_disp = disp_of(cursor);
        let cursor_row = row_of(line_of(cursor), cursor_disp);
        let cursor_x = x_of(cursor_row, cursor_disp);

        let (selection, cursor_quad) = if selected_range.is_empty() {
            (
                Vec::new(),
                Some(fill(
                    Bounds::new(
                        point(
                            bounds.left() + gutter_width + cursor_x,
                            bounds.top() + rows[cursor_row].text_top - scroll_offset_y,
                        ),
                        size(px(1.), rows[cursor_row].text_height),
                    ),
                    gpui::blue(),
                )),
            )
        } else {
            let start_disp = disp_of(selected_range.start);
            let end_disp = disp_of(selected_range.end);
            let start_row = row_of(line_of(selected_range.start), start_disp);
            let end_row = row_of(line_of(selected_range.end), end_disp);
            let start_x = x_of(start_row, start_disp);
            let end_x = x_of(end_row, end_disp);

            let top_ri = start_row.min(end_row);
            let bot_ri = start_row.max(end_row);
            let (left_x, right_x) = if start_row <= end_row {
                (start_x, end_x)
            } else {
                (end_x, start_x)
            };
            let row_left = bounds.left() + gutter_width;
            let mut quads = Vec::new();
            let single = top_ri == bot_ri;
            for (ri, r) in rows.iter().enumerate().take(bot_ri + 1).skip(top_ri) {
                // Selection covers each row's text box; multi-row spans
                // stitch together through the space between boxes.
                let y0 = if single || ri == top_ri {
                    r.text_top
                } else {
                    r.box_top
                };
                let y1 = if single || ri == bot_ri {
                    r.text_top + r.text_height
                } else {
                    r.box_top + r.box_height
                };
                let x0 = if ri == top_ri { left_x } else { px(0.0) };
                let x1 = if ri == bot_ri {
                    right_x
                } else {
                    available_width
                };
                quads.push(fill(
                    Bounds::from_corners(
                        point(row_left + x0, bounds.top() + y0 - scroll_offset_y),
                        point(row_left + x1, bounds.top() + y1 - scroll_offset_y),
                    ),
                    rgba(0x3311ff30),
                ));
            }
            (quads, None)
        };

        // Line numbers: one per logical line, at its first row.
        let line_numbers = if show_line_numbers {
            let number_color = hsla(0., 0., 0.4, 0.5);
            let mut seen_lines = std::collections::HashSet::new();
            let mut nums = Vec::new();
            for r in rows.iter() {
                if !seen_lines.insert(r.line_idx) {
                    continue;
                }
                let num_str = format!("{}", r.line_idx + 1);
                let run = TextRun {
                    len: num_str.len(),
                    font: base_font.clone(),
                    color: number_color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let shaped =
                    window
                        .text_system()
                        .shape_line(num_str.into(), font_size, &[run], None);
                nums.push((r.text_top, r.text_height, shaped));
            }
            nums
        } else {
            Vec::new()
        };

        let last_layout = rows.last().map(|r| r.shaped.clone());
        let all_lines: Vec<ShapedLine> = rows.iter().map(|r| r.shaped.clone()).collect();
        let line_char_offsets: Vec<usize> = rows.iter().map(|r| r.src_start).collect();
        let row_metas: Vec<RowMeta> = rows
            .iter()
            .map(|r| RowMeta {
                box_top: r.box_top,
                text_top: r.text_top,
                text_height: r.text_height,
                box_height: r.box_height,
                src_start: r.src_start,
                disp_start: r.disp_start,
                line_idx: r.line_idx,
            })
            .collect();
        let cursor_row_idx = cursor_row;
        self.input.update(cx, |input, _cx| {
            input.last_layout = last_layout;
            input.all_lines = all_lines;
            input.line_char_offsets = line_char_offsets;
            input.row_metas = row_metas;
            input.display_lines = display_lines;
            input.content_height = content_height;
            input.cursor_row = cursor_row_idx;
            input.line_height = line_height;
        });

        PrepaintState {
            rows,
            line_numbers,
            gutter_width,
            cursor: cursor_quad,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<gpui::Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        let scroll_offset_y = self.input.read(cx).scroll_offset_y;
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        for quad in prepaint.selection.drain(..) {
            window.paint_quad(quad);
        }
        let gutter_width = prepaint.gutter_width;

        for (y, h, num) in prepaint.line_numbers.iter() {
            let num_w = num.width;
            let x = bounds.left() + gutter_width - num_w - px(8.0);
            num.paint(
                point(x, bounds.top() + *y - scroll_offset_y),
                *h,
                window,
                cx,
            )
            .ok();
        }

        for row in prepaint.rows.iter() {
            if let Some(color) = row.rule_color {
                // Markdown mode ON hides `---` and draws the rule instead.
                let mid_y = bounds.top() + row.text_top + px(f32::from(row.text_height) / 2.0)
                    - scroll_offset_y;
                window.paint_quad(fill(
                    Bounds::new(
                        point(bounds.left() + gutter_width + px(4.0), mid_y - px(1.0)),
                        size(
                            (bounds.size.width - gutter_width - px(8.0)).max(px(0.0)),
                            px(1.5),
                        ),
                    ),
                    color,
                ));
            }
            row.shaped
                .paint(
                    point(
                        bounds.left() + gutter_width,
                        bounds.top() + row.text_top - scroll_offset_y,
                    ),
                    row.text_height,
                    window,
                    cx,
                )
                .ok();
        }
        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }
        let last_layout = prepaint.rows.last().map(|r| r.shaped.clone());
        let last_bounds = Bounds::new(
            point(bounds.left() + gutter_width, bounds.top()),
            size(bounds.size.width - gutter_width, bounds.size.height),
        );
        self.input.update(cx, |input, _cx| {
            input.last_layout = last_layout;
            input.last_bounds = Some(last_bounds);
        });
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .h_full()
            .flex()
            .key_context("TextInput")
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(Self::delete_to_line_start))
            .on_action(cx.listener(Self::delete_to_line_end))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::summarize))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::dedent))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel({
                let entity = cx.entity();
                move |event, window, cx| {
                    entity.update(cx, |input, cx| {
                        input.on_scroll_wheel(event, window, cx);
                    });
                }
            })
            .child(TextElement { input: cx.entity() })
    }
}

struct FindReplaceState {
    visible: bool,
    query: String,
    replacement: String,
    matches: Vec<usize>,
    current_match: usize,
}

struct ContextMenuState {
    visible: bool,
    position: Point<gpui::Pixels>,
}

struct AiPanelState {
    visible: bool,
    query: String,
    response: String,
    history: Vec<(String, String)>,
}

#[derive(Clone, PartialEq)]
enum EditingField {
    None,
    CoverTitle,
    CoverSubtitle,
    CoverAuthor,
    ImageCaption(usize),
    TableCaption(usize),
    ParagraphSpacing,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NavigatorTab {
    Outline,
    Pages,
    Assets,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InspectorMode {
    Paragraph,
    Image,
    History,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WorkspaceOverlay {
    None,
    CommandPalette,
    ModalShowcase,
}

/// Where `export_document` writes: same merged model, three renderers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    Pdf,
    Docx,
    Markdown,
}

impl ExportFormat {
    fn ext(self) -> &'static str {
        match self {
            ExportFormat::Pdf => "pdf",
            ExportFormat::Docx => "docx",
            ExportFormat::Markdown => "md",
        }
    }
}

/// How long transient status-bar feedback ("Table inserted") stays up.
const STATUS_MESSAGE_TTL: std::time::Duration = std::time::Duration::from_secs(4);

struct SylphApp {
    editor: Entity<TextInput>,
    document: sylph_core::document::Document,
    /// The copy of `document` last written to storage; any difference is
    /// saved by `persist_model_if_changed` after the next notify.
    persisted_model: sylph_core::document::Document,
    /// Why the structured model failed to save, if it did. The save
    /// indicator shows it, so a good text save cannot hide it.
    model_save_error: Option<String>,
    focus_handle: FocusHandle,
    sidebar_visible: bool,
    /// Canvas layout: `false` = Print (fixed page canvas), `true` = Web
    /// (continuous full-width flow, no page box or ruler). Focus is not a
    /// field — it is derived from both side panels being hidden.
    web_layout: bool,
    preview_visible: bool,
    find: FindReplaceState,
    context_menu: ContextMenuState,
    doc_title: String,
    editing_title: bool,
    /// Every document for the Recent Files list, newest first.
    documents: Vec<sylph_storage::DocumentSummary>,
    dark_mode: bool,
    ai_panel: AiPanelState,
    /// Transient action feedback; set it with `set_status` so it clears
    /// itself. The save indicator is separate (`TextInput::save_state`).
    status_message: Option<String>,
    status_clear_task: Option<Task<()>>,
    editing_field: EditingField,
    field_input: String,
    paragraph_spacing: f32,
    navigator_tab: NavigatorTab,
    inspector_mode: InspectorMode,
    inspector_visible: bool,
    overlay: WorkspaceOverlay,
    markdown_mode: bool,
    ruler_visible: bool,
    zoom_percent: u16,
    image_picker_task: Option<Task<()>>,
    _keystroke_subscription: Subscription,
    _model_observer: Subscription,
}

actions!(
    app,
    [
        SaveDoc,
        SummarizeDoc,
        ToggleSidebar,
        Undo,
        Redo,
        MoveWordLeft,
        MoveWordRight,
        PageUp,
        PageDown,
        Indent,
        OpenFindBar,
        CloseFindBar,
        FindNext,
        FindPrev,
        ReplaceCurrent,
        ReplaceAll,
        ContextMenuCopy,
        ContextMenuCut,
        ContextMenuPaste,
        ContextMenuSelectAll,
        DismissContextMenu,
        ConfirmTitle,
        CancelTitle,
        NewDocument,
        ToggleDarkMode,
        ExportDocx,
        ExportPdf,
        ExportMarkdown,
        TogglePreview,
        AddCoverPage,
        PasteImage,
        BoldText,
        ItalicText,
        StrikethroughText,
        InsertTable,
        RewriteText,
        OpenAiPanel,
        CloseAiPanel,
        AiSubmit,
        SetImageCaption,
        SetTableCaption,
        SetParagraphSpacing,
        EditingCoverTitle,
        EditingCoverSubtitle,
        EditingCoverAuthor,
        InsertPageBreak,
        SetPageSize,
        SetPageMargins,
        OpenCommandPalette,
        CloseOverlay,
        OpenModalShowcase,
        ShowParagraphInspector,
        ShowImageInspector,
        ShowVersionHistory,
        ToggleInspector,
        ToggleMarkdownMode,
        ToggleRuler,
        CycleHeading,
        CycleBodyFont,
        SetOrientation,
    ]
);

impl SylphApp {
    fn save_doc(&mut self, _: &SaveDoc, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| editor.save_now(cx));
    }

    fn summarize_doc(&mut self, _: &SummarizeDoc, _window: &mut Window, cx: &mut Context<Self>) {
        let text = self.editor.read(cx).content.clone();
        let summary = sylph_py_bridge::summarize_text(&text);
        let _ = std::fs::write(data_path("summary.txt"), &summary);
        cx.notify();
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_visible = !self.sidebar_visible;
        cx.notify();
    }

    fn undo(&mut self, _: &Undo, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            editor.undo(cx);
        });
    }

    fn redo(&mut self, _: &Redo, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            editor.redo(cx);
        });
    }

    fn move_word_left(&mut self, _: &MoveWordLeft, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            editor.move_word_left(&MoveWordLeft, _window, cx);
        });
    }

    fn move_word_right(&mut self, _: &MoveWordRight, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            editor.move_word_right(&MoveWordRight, _window, cx);
        });
    }

    fn page_up(&mut self, _: &PageUp, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            editor.page_up(&PageUp, _window, cx);
        });
    }

    fn page_down(&mut self, _: &PageDown, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            editor.page_down(&PageDown, _window, cx);
        });
    }

    fn indent(&mut self, _: &Indent, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            editor.indent(&Indent, _window, cx);
        });
    }

    fn dedent(&mut self, _: &Dedent, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            editor.dedent(&Dedent, _window, cx);
        });
    }

    fn open_find_bar(&mut self, _: &OpenFindBar, _window: &mut Window, cx: &mut Context<Self>) {
        self.find.visible = !self.find.visible;
        if self.find.visible {
            self.find.query.clear();
            self.find.replacement.clear();
            self.find.matches.clear();
            self.find.current_match = 0;
        }
        cx.notify();
    }

    fn close_find_bar(&mut self, _: &CloseFindBar, _window: &mut Window, cx: &mut Context<Self>) {
        self.find.visible = false;
        cx.notify();
    }

    fn find_navigate(&mut self, forward: bool, cx: &mut Context<Self>) {
        if self.find.matches.is_empty() {
            return;
        }
        if forward {
            self.find.current_match = (self.find.current_match + 1) % self.find.matches.len();
        } else {
            self.find.current_match = if self.find.current_match == 0 {
                self.find.matches.len() - 1
            } else {
                self.find.current_match - 1
            };
        }
        let pos = self.find.matches[self.find.current_match];
        let query_len = self.find.query.len();
        self.editor.update(cx, |editor, cx| {
            editor.selected_range = pos..pos + query_len;
            editor.move_to(pos, cx);
        });
        cx.notify();
    }

    fn find_next(&mut self, _: &FindNext, _window: &mut Window, cx: &mut Context<Self>) {
        self.find_navigate(true, cx);
    }

    fn find_prev(&mut self, _: &FindPrev, _window: &mut Window, cx: &mut Context<Self>) {
        self.find_navigate(false, cx);
    }

    fn replace_current(
        &mut self,
        _: &ReplaceCurrent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.find.matches.is_empty() || self.find.query.is_empty() {
            return;
        }
        let pos = self.find.matches[self.find.current_match];
        let query_len = self.find.query.len();
        self.editor.update(cx, |editor, cx| {
            editor.selected_range = pos..pos + query_len;
            editor.replace_text_in_range(None, &self.find.replacement, cx);
        });
        self.update_matches(cx);
        cx.notify();
    }

    fn replace_all(&mut self, _: &ReplaceAll, _window: &mut Window, cx: &mut Context<Self>) {
        if self.find.query.is_empty() {
            return;
        }
        let content = self.editor.read(cx).content.clone();
        let query = self.find.query.clone();
        let replacement = self.find.replacement.clone();
        let new_content = content.replace(&query, &replacement);
        if new_content != content {
            self.editor.update(cx, |editor, cx| {
                editor.selected_range = 0..content.len();
                editor.replace_text_in_range(None, &new_content, cx);
            });
        }
        self.update_matches(cx);
        cx.notify();
    }

    fn on_find_keystroke(
        &mut self,
        event: &KeystrokeEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.find.visible {
            return;
        }
        let key = event.keystroke.key.as_str();
        if key == "escape" {
            self.find.visible = false;
            cx.notify();
            return;
        }
        if key == "enter" {
            if event.keystroke.modifiers.shift {
                self.find_prev(&FindPrev, _window, cx);
            } else {
                self.find_next(&FindNext, _window, cx);
            }
            return;
        }
        if key == "backspace" {
            self.find.query.pop();
            self.update_matches(cx);
            cx.notify();
            return;
        }
        if let Some(ch) = &event.keystroke.key_char {
            if !event.keystroke.modifiers.platform && !event.keystroke.modifiers.control {
                self.find.query.push_str(ch);
                self.update_matches(cx);
                cx.notify();
            }
        }
    }

    fn update_matches(&mut self, cx: &mut Context<Self>) {
        self.find.matches.clear();
        self.find.current_match = 0;
        if self.find.query.is_empty() {
            return;
        }
        let content = self.editor.read(cx).content.clone();
        let query = self.find.query.clone();
        let mut start = 0;
        while let Some(pos) = content[start..].find(&query) {
            self.find.matches.push(start + pos);
            start += pos + 1;
        }
    }

    fn on_editor_right_click(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.context_menu.visible = true;
        self.context_menu.position = event.position;
        cx.notify();
    }

    fn dismiss_context_menu(
        &mut self,
        _: &DismissContextMenu,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.context_menu.visible = false;
        cx.notify();
    }

    fn context_menu_copy(
        &mut self,
        _: &ContextMenuCopy,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.context_menu.visible = false;
        self.editor.update(cx, |editor, cx| {
            editor.copy(&Copy, _window, cx);
        });
        cx.notify();
    }

    fn context_menu_cut(
        &mut self,
        _: &ContextMenuCut,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.context_menu.visible = false;
        self.editor.update(cx, |editor, cx| {
            editor.cut(&Cut, _window, cx);
        });
        cx.notify();
    }

    fn context_menu_paste(
        &mut self,
        _: &ContextMenuPaste,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.context_menu.visible = false;
        self.editor.update(cx, |editor, cx| {
            editor.paste(&Paste, _window, cx);
        });
        cx.notify();
    }

    fn context_menu_select_all(
        &mut self,
        _: &ContextMenuSelectAll,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.context_menu.visible = false;
        self.editor.update(cx, |editor, cx| {
            editor.select_all(&SelectAll, _window, cx);
        });
        cx.notify();
    }

    fn confirm_title(&mut self, _: &ConfirmTitle, _window: &mut Window, cx: &mut Context<Self>) {
        if self.editing_title {
            self.editing_title = false;
            let title = self.doc_title.clone();
            let doc_id = self.editor.read(cx).doc_id;
            let _ = self.editor.read(cx).storage.update_title(doc_id, &title);
            cx.notify();
        }
    }

    fn cancel_title(&mut self, _: &CancelTitle, _window: &mut Window, cx: &mut Context<Self>) {
        self.editing_title = false;
        cx.notify();
    }

    fn on_title_keystroke(
        &mut self,
        event: &KeystrokeEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editing_title {
            return;
        }
        let key = event.keystroke.key.as_str();
        if key == "enter" {
            self.confirm_title(&ConfirmTitle, _window, cx);
            return;
        }
        if key == "escape" {
            self.cancel_title(&CancelTitle, _window, cx);
            return;
        }
        if key == "backspace" {
            self.doc_title.pop();
            cx.notify();
            return;
        }
        if let Some(ch) = &event.keystroke.key_char {
            if !event.keystroke.modifiers.platform && !event.keystroke.modifiers.control {
                self.doc_title.push_str(ch);
                cx.notify();
            }
        }
    }

    fn load_documents(&mut self, cx: &mut Context<Self>) {
        self.documents = self
            .editor
            .read(cx)
            .storage
            .list_document_summaries()
            .unwrap_or_default();
    }

    /// Save the structured model (page setup, cover page, inserted blocks)
    /// whenever it differs from the last saved copy. Registered with
    /// `observe_self`, so it runs after every notify and no handler can
    /// forget it. It never notifies itself: a failed save retries on the
    /// next notify instead of looping.
    fn persist_model_if_changed(&mut self, cx: &mut Context<Self>) {
        if self.document == self.persisted_model {
            return;
        }
        let result = {
            let editor = self.editor.read(cx);
            serde_json::to_string(&self.document)
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
                .and_then(|json| editor.storage.save_model(editor.doc_id, &json))
        };
        match result {
            Ok(()) => {
                self.persisted_model = self.document.clone();
                self.model_save_error = None;
            }
            Err(e) => self.model_save_error = Some(e.to_string()),
        }
    }

    /// `false` when the text or the structured model could not be written.
    /// Callers about to replace the editor content must then stay put, or
    /// the unsaved work is lost.
    fn save_current_document(&mut self, cx: &mut Context<Self>) -> bool {
        self.persist_model_if_changed(cx);
        let saved = self.editor.update(cx, |editor, cx| editor.save_now(cx))
            && self.model_save_error.is_none();
        if !saved {
            self.set_status("Could not save this document, so it stays open", cx);
        }
        saved
    }

    fn load_document_by_id(&mut self, doc_id: i64, cx: &mut Context<Self>) {
        let saved = self
            .editor
            .read(cx)
            .storage
            .load_text(doc_id)
            .unwrap_or(None)
            .unwrap_or_default();
        let doc_title = self
            .editor
            .read(cx)
            .storage
            .get_title(doc_id)
            .unwrap_or_else(|_| "Untitled".to_string());
        // New and switched-to documents are what the next launch reopens.
        let _ = self.editor.read(cx).storage.set_last_opened(doc_id);

        self.editor.update(cx, |editor, cx| {
            editor.doc_id = doc_id;
            editor.content = saved.clone();
            editor.selected_range = 0..0;
            editor.selection_reversed = false;
            editor.preferred_column = None;
            editor.scroll_offset_y = px(0.0);
            editor.undo_stack.clear();
            editor.redo_stack.clear();
            // The text just came from storage, so there is nothing to save.
            editor.save_state = SaveState::Saved;
            cx.notify();
        });
        // Each document keeps its own page setup, cover page and inserted
        // blocks, so nothing leaks from the previous document.
        let (model, warning) =
            load_model_or_default(&self.editor.read(cx).storage, doc_id, &recovered_dir());
        self.persisted_model = model.clone();
        self.document = model;
        self.model_save_error = None;
        if let Some(warning) = warning {
            self.set_status(warning, cx);
        }
        self.doc_title = doc_title;
    }

    fn new_document(&mut self, _: &NewDocument, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.save_current_document(cx) {
            return;
        }
        let created = self.editor.read(cx).storage.create_document("Untitled");
        let Ok(doc_id) = created else {
            // Never fall back to some other document id: say so and stay.
            self.set_status("Could not create a new document", cx);
            return;
        };
        self.load_document_by_id(doc_id, cx);
        self.load_documents(cx);
        cx.notify();
    }

    fn switch_document(&mut self, doc_id: i64, _window: &mut Window, cx: &mut Context<Self>) {
        let current_id = self.editor.read(cx).doc_id;
        if doc_id == current_id {
            return;
        }
        if !self.save_current_document(cx) {
            return;
        }
        self.load_document_by_id(doc_id, cx);
        // The save above moved the old document in the recency order.
        self.load_documents(cx);
        cx.notify();
    }

    fn toggle_dark_mode(
        &mut self,
        _: &ToggleDarkMode,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dark_mode = !self.dark_mode;
        cx.notify();
    }

    fn bg_color(&self) -> gpui::Rgba {
        if self.dark_mode {
            rgb(0x0b1220)
        } else {
            rgb(0xf8f9ff)
        }
    }
    fn surface_color(&self) -> gpui::Rgba {
        if self.dark_mode {
            rgb(0x111c2e)
        } else {
            rgb(0xffffff)
        }
    }
    fn sidebar_color(&self) -> gpui::Rgba {
        if self.dark_mode {
            rgb(0x111c2e)
        } else {
            rgb(0xeff4ff)
        }
    }
    fn border_color(&self) -> gpui::Rgba {
        if self.dark_mode {
            rgb(0x1e293b)
        } else {
            rgb(0xc4c5d7)
        }
    }
    fn editor_bg(&self) -> gpui::Rgba {
        if self.dark_mode {
            rgb(0x0b1220)
        } else {
            rgb(0xffffff)
        }
    }
    fn hover_color(&self) -> gpui::Rgba {
        if self.dark_mode {
            rgb(0x1e293b)
        } else {
            rgb(0xe5eeff)
        }
    }
    fn active_doc_color(&self) -> gpui::Rgba {
        if self.dark_mode {
            rgb(0x1e3b73)
        } else {
            rgb(0xdce9ff)
        }
    }

    /// The document exactly as export will see it — the one truth for chrome.
    fn export_view(&self, cx: &mut Context<Self>) -> doc::Document {
        let editor = self.editor.read(cx);
        export_model(&self.document, &editor.content, self.markdown_mode)
    }

    fn page_count(&self, cx: &mut Context<Self>) -> usize {
        export_page_count(&self.export_view(cx))
    }

    fn export_document(&mut self, format: ExportFormat, cx: &mut Context<Self>) {
        let ext = format.ext();
        // Keep the file inside the data dir: doc_title is user-editable, so
        // strip path separators and other unsafe filename characters.
        let safe_title: String = self
            .doc_title
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let file_name = format!("{}.{}", safe_title.replace(' ', "_"), ext);
        let path = data_path(&file_name).to_string_lossy().into_owned();
        let content = self.editor.read(cx).content.clone();
        // Neither buffer alone is the document yet: typed text lives in the
        // editor, cover/tables/images/page breaks live in self.document.
        // Merge both so export never silently drops content.
        let model = self.export_view(cx);
        let result = match serde_json::to_string(&model) {
            Ok(json) => match format {
                ExportFormat::Pdf => sylph_py_bridge::export_rich_pdf(&json, &path),
                ExportFormat::Docx => sylph_py_bridge::export_rich_docx(&json, &path),
                ExportFormat::Markdown => sylph_py_bridge::export_rich_markdown(&json, &path),
            },
            Err(e) => match format {
                ExportFormat::Pdf => {
                    // Never write DOCX bytes into a .pdf path.
                    format!("Export failed to serialize document: {e}")
                }
                ExportFormat::Docx => {
                    let fallback = sylph_py_bridge::export_to_docx(&content, &path);
                    format!("Export failed to serialize document: {e}. Fallback: {fallback}")
                }
                ExportFormat::Markdown => format!("Export failed to serialize document: {e}"),
            },
        };
        // The status bar shows the file name, never the internal
        // storage path (Google Docs / Word say "Exported · x").
        let message = if result.starts_with("Exported to ") {
            format!("Exported · {}", file_name)
        } else {
            result.replace(&sylph_storage::data_dir().display().to_string(), "")
        };
        self.set_status(message, cx);
    }

    fn export_docx(&mut self, _: &ExportDocx, _window: &mut Window, cx: &mut Context<Self>) {
        self.export_document(ExportFormat::Docx, cx);
    }

    fn export_pdf(&mut self, _: &ExportPdf, _window: &mut Window, cx: &mut Context<Self>) {
        self.export_document(ExportFormat::Pdf, cx);
    }

    fn export_markdown(
        &mut self,
        _: &ExportMarkdown,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.export_document(ExportFormat::Markdown, cx);
    }

    fn toggle_preview(&mut self, _: &TogglePreview, _window: &mut Window, cx: &mut Context<Self>) {
        self.preview_visible = !self.preview_visible;
        cx.notify();
    }

    fn add_cover_page(&mut self, _: &AddCoverPage, _window: &mut Window, cx: &mut Context<Self>) {
        if self.document.has_cover_page() {
            self.document.remove_cover_page();
            self.set_status("Cover page removed", cx);
        } else {
            let mut cp = sylph_core::document::CoverPageData::with_template(
                sylph_core::document::CoverTemplate::Classic,
            );
            cp.title = self.doc_title.clone();
            self.document.set_cover_page(cp);
            self.set_status("Cover page added (Classic)", cx);
        }
        cx.notify();
    }

    fn add_image_asset(&mut self, bytes: &[u8], extension: &str, cx: &mut Context<Self>) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let filename = format!("image_{}.{}", timestamp, extension);
        let stored = data_path(&format!("images/{}", filename));
        let path = stored.to_string_lossy().into_owned();
        if std::fs::write(&path, bytes).is_ok() {
            self.document
                .push_block(sylph_core::document::Block::image(&path));
            self.set_status(format!("Image added: {}", filename), cx);
        } else {
            self.set_status("Could not store the selected image", cx);
        }
        cx.notify();
    }

    fn add_image_file(&mut self, source: PathBuf, cx: &mut Context<Self>) {
        let extension = source
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("png")
            .to_ascii_lowercase();
        let allowed = [
            "png", "jpg", "jpeg", "webp", "gif", "svg", "bmp", "tif", "tiff",
        ];
        if !allowed.contains(&extension.as_str()) {
            self.set_status(format!("Unsupported image type: .{}", extension), cx);
            cx.notify();
            return;
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let filename = format!("image_{}.{}", timestamp, extension);
        let destination = data_path(&format!("images/{}", filename));
        if std::fs::copy(&source, &destination).is_ok() {
            let path = destination.to_string_lossy().into_owned();
            self.document
                .push_block(sylph_core::document::Block::image(&path));
            self.set_status(format!("Image added: {}", filename), cx);
        } else {
            self.set_status("Could not copy the selected image", cx);
        }
        cx.notify();
    }

    fn open_image_picker(&mut self, cx: &mut Context<Self>) {
        self.set_status("Choose an image file…", cx);
        let options = PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Insert image".into()),
        };
        let receiver = cx.prompt_for_paths(options);
        let task = cx.spawn(async move |this: WeakEntity<SylphApp>, cx: &mut AsyncApp| {
            match receiver.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        let _ = this.update(cx, |this, cx| this.add_image_file(path, cx));
                    }
                }
                Ok(Err(error)) => {
                    let message = format!("Image picker unavailable: {}", error);
                    let _ = this.update(cx, |this, cx| this.set_status(message, cx));
                }
                _ => {}
            }
        });
        self.image_picker_task = Some(task);
        cx.notify();
    }

    fn paste_image(&mut self, _: &PasteImage, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(item) = cx.read_from_clipboard() {
            for entry in item.entries() {
                if let ClipboardEntry::Image(image) = entry {
                    let extension = match image.format {
                        gpui::ImageFormat::Png => "png",
                        gpui::ImageFormat::Jpeg => "jpg",
                        gpui::ImageFormat::Webp => "webp",
                        gpui::ImageFormat::Gif => "gif",
                        gpui::ImageFormat::Svg => "svg",
                        gpui::ImageFormat::Bmp => "bmp",
                        gpui::ImageFormat::Tiff => "tiff",
                    };
                    self.add_image_asset(&image.bytes, extension, cx);
                    return;
                }
            }
        }
        self.open_image_picker(cx);
    }

    /// Show transient feedback in the status bar; it clears itself after
    /// `STATUS_MESSAGE_TTL`. A newer message replaces the pending clear.
    fn set_status(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        let message = message.into();
        self.status_message = Some(message.clone());
        self.status_clear_task = Some(cx.spawn(
            async move |this: WeakEntity<SylphApp>, cx: &mut AsyncApp| {
                gpui::Timer::after(STATUS_MESSAGE_TTL).await;
                let _ = this.update(cx, |this, cx| {
                    if this.status_message.as_deref() == Some(message.as_str()) {
                        this.status_message = None;
                        cx.notify();
                    }
                });
            },
        ));
        cx.notify();
    }

    /// Why a formatting command must refuse, or `None` when it may run.
    /// Markdown OFF means literal text: inserting `# ` or `**` would only
    /// add raw markers that stay literal on the canvas *and* in the export,
    /// so the user would get a stray character instead of a heading.
    fn markdown_required_message(markdown_on: bool) -> Option<&'static str> {
        if markdown_on {
            None
        } else {
            Some("Turn on Markdown to use formatting")
        }
    }

    /// `true` when the command may proceed; otherwise reports why not.
    /// Every Markdown-driven formatting command starts with this guard.
    fn require_markdown(&mut self, cx: &mut Context<Self>) -> bool {
        match Self::markdown_required_message(self.markdown_mode) {
            None => true,
            Some(message) => {
                self.set_status(message, cx);
                false
            }
        }
    }

    /// Wrap the selection in `marker` markdown (the same source-level
    /// syntax the export parser reads back).
    fn wrap_selection(&mut self, marker: &str, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let text = editor.content.clone();
            let sel = editor.selected_range.clone();
            if sel.start < sel.end && sel.end <= text.len() {
                let selected = &text[sel.clone()];
                let new_text = format!("{marker}{}{marker}", selected);
                editor.replace_text_in_range(Some(sel), &new_text, cx);
            }
        });
    }

    fn bold_text(&mut self, _: &BoldText, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.require_markdown(cx) {
            return;
        }
        self.wrap_selection("**", cx);
        cx.notify();
    }

    fn italic_text(&mut self, _: &ItalicText, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.require_markdown(cx) {
            return;
        }
        self.wrap_selection("*", cx);
        cx.notify();
    }

    fn strikethrough_text(
        &mut self,
        _: &StrikethroughText,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.require_markdown(cx) {
            return;
        }
        self.wrap_selection("~~", cx);
        cx.notify();
    }

    fn set_heading(&mut self, level: u8, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.require_markdown(cx) {
            return;
        }
        let prefix = "#".repeat(level as usize);
        self.editor.update(cx, |editor, cx| {
            let text = editor.content.clone();
            let sel = editor.selected_range.clone();
            if sel.start < sel.end && sel.end <= text.len() {
                let selected = &text[sel.clone()];
                let new_text = format!("{} {}", prefix, selected);
                editor.replace_text_in_range(Some(sel), &new_text, cx);
            } else {
                // No selection — insert heading prefix at cursor
                editor.replace_text_in_range(None, &format!("{} ", prefix), cx);
            }
        });
        cx.notify();
    }

    fn insert_table(&mut self, _: &InsertTable, _window: &mut Window, cx: &mut Context<Self>) {
        // Insert a 3x3 table
        let table_block = sylph_core::document::Block::table(3, 3);
        self.document.push_block(table_block);
        self.set_status("Table inserted (3×3)", cx);
        cx.notify();
    }

    fn open_ai_panel(&mut self, _: &OpenAiPanel, _window: &mut Window, cx: &mut Context<Self>) {
        self.ai_panel.visible = !self.ai_panel.visible;
        cx.notify();
    }

    fn close_ai_panel(&mut self, _: &CloseAiPanel, _window: &mut Window, cx: &mut Context<Self>) {
        self.ai_panel.visible = false;
        cx.notify();
    }

    fn ai_submit(&mut self, _: &AiSubmit, _window: &mut Window, cx: &mut Context<Self>) {
        if self.ai_panel.query.is_empty() {
            return;
        }
        let query = self.ai_panel.query.clone();
        let doc_text = self.editor.read(cx).content.clone();
        let response = sylph_py_bridge::chat_with_doc(&query, &doc_text);
        self.ai_panel.history.push((query, response.clone()));
        self.ai_panel.response = response;
        self.ai_panel.query.clear();
        cx.notify();
    }

    fn set_image_caption(
        &mut self,
        _: &SetImageCaption,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let EditingField::ImageCaption(idx) = &self.editing_field {
            let idx = *idx;
            let text = self.field_input.clone();
            if let Some(sylph_core::document::Block::Image { data }) =
                self.document.blocks.get_mut(idx)
            {
                data.caption = Some(text);
            }
        }
        self.editing_field = EditingField::None;
        self.field_input.clear();
        cx.notify();
    }

    fn set_table_caption(
        &mut self,
        _: &SetTableCaption,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let EditingField::TableCaption(idx) = &self.editing_field {
            let idx = *idx;
            let text = self.field_input.clone();
            if let Some(sylph_core::document::Block::Table { data }) =
                self.document.blocks.get_mut(idx)
            {
                data.caption = Some(text);
            }
        }
        self.editing_field = EditingField::None;
        self.field_input.clear();
        cx.notify();
    }

    fn set_paragraph_spacing(
        &mut self,
        _: &SetParagraphSpacing,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Ok(val) = self.field_input.parse::<f32>() {
            self.paragraph_spacing = val.clamp(0.0, 48.0);
        }
        self.editing_field = EditingField::None;
        self.field_input.clear();
        cx.notify();
    }

    fn insert_page_break(
        &mut self,
        _: &InsertPageBreak,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The structured block is the source for pagination. The current visual
        // scaffold appends it to the active report flow until block positions are
        // unified with TextInput in the editor-kernel phase.
        self.document
            .push_block(sylph_core::document::Block::page_break());
        self.set_status("Page break inserted", cx);
        cx.notify();
    }

    fn set_page_size(&mut self, _: &SetPageSize, _window: &mut Window, cx: &mut Context<Self>) {
        // Toggle between A4 and Letter
        self.document.set_page_size(match self.document.page_size {
            sylph_core::document::PageSize::A4 => sylph_core::document::PageSize::Letter,
            sylph_core::document::PageSize::Letter => sylph_core::document::PageSize::A4,
        });
        self.set_status(format!("Page size: {}", self.document.page_size.name()), cx);
        cx.notify();
    }

    /// Portrait or landscape for the whole document: the canvas, the ruler
    /// and both exporters read `landscape`.
    fn set_orientation(&mut self, landscape: bool, cx: &mut Context<Self>) {
        self.document.set_landscape(landscape);
        let name = if landscape { "Landscape" } else { "Portrait" };
        self.set_status(format!("Orientation: {name}"), cx);
        cx.notify();
    }

    fn toggle_orientation(
        &mut self,
        _: &SetOrientation,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_orientation(!self.document.landscape, cx);
    }

    fn set_page_margins(
        &mut self,
        _: &SetPageMargins,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Cycle through margin presets: Normal -> Narrow -> Wide -> Normal
        let m = &self.document.page_margins;
        let new_margins = if (m.top - 72.0).abs() < 1.0 {
            // Normal (72pt = 1in) -> Narrow (36pt = 0.5in)
            sylph_core::document::PageMargins {
                top: 36.0,
                bottom: 36.0,
                left: 36.0,
                right: 36.0,
            }
        } else if (m.top - 36.0).abs() < 1.0 {
            // Narrow -> Wide (144pt = 2in)
            sylph_core::document::PageMargins {
                top: 144.0,
                bottom: 144.0,
                left: 144.0,
                right: 144.0,
            }
        } else {
            // Wide or other -> Normal
            sylph_core::document::PageMargins::default()
        };
        self.document.set_margins(new_margins);
        self.set_status(
            format!("Margins: {:.1}pt", self.document.page_margins.top),
            cx,
        );
        cx.notify();
    }

    fn cycle_line_spacing(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let current = self.document.line_spacing;
        let next = if (current - 1.0).abs() < 0.01 {
            1.15
        } else if (current - 1.15).abs() < 0.01 {
            1.5
        } else if (current - 1.5).abs() < 0.01 {
            2.0
        } else {
            1.0
        };
        self.document.set_line_spacing(next);
        self.set_status(format!("Line spacing: {:.2}", next), cx);
        cx.notify();
    }

    fn set_line_spacing_value(&mut self, value: f32, _window: &mut Window, cx: &mut Context<Self>) {
        self.document.set_line_spacing(value);
        self.set_status(format!("Line spacing: {:.2}", value), cx);
        cx.notify();
    }

    fn cycle_heading(&mut self, _: &CycleHeading, _window: &mut Window, cx: &mut Context<Self>) {
        let level = self.current_heading_level(cx);
        let new_level = match level {
            0 => 1,
            6 => 0,
            l => l + 1,
        };
        // Style controls never change the Markdown toggle: when it is OFF,
        // the user asked for literal source, so say so instead of changing
        // modes behind their back.
        if !self.require_markdown(cx) {
            return;
        }

        // Modify the current line in the editor
        self.editor.update(cx, |editor, cx| {
            let cursor = editor.cursor_offset();
            let content = editor.content.clone();
            let line_start = content[..cursor].rfind('\n').map(|p| p + 1).unwrap_or(0);
            let line_end = content[cursor..]
                .find('\n')
                .map(|p| cursor + p)
                .unwrap_or(content.len());
            let line = &content[line_start..line_end];
            let trimmed = line.trim_start();
            let indent = &line[..line.len() - trimmed.len()];

            // Strip the existing heading prefix: `#{1,6} ` — all six
            // levels, including `# ` with no text (a marker that is not
            // yet a heading). `#nospace` is not a marker, so it stays.
            let hashes = trimmed.chars().take_while(|&c| c == '#').count();
            let stripped = if (1..=6).contains(&hashes) {
                trimmed[hashes..].strip_prefix(' ').unwrap_or(trimmed)
            } else {
                trimmed
            };

            let new_line = if new_level == 0 {
                format!("{}{}", indent, stripped)
            } else {
                let prefix = "#".repeat(new_level as usize);
                format!("{}{} {}", indent, prefix, stripped)
            };

            editor.selected_range = line_start..line_end;
            editor.replace_text_in_range(Some(line_start..line_end), &new_line, cx);
        });

        let label = match new_level {
            0 => "Normal",
            1 => "Heading 1",
            2 => "Heading 2",
            3 => "Heading 3",
            4 => "Heading 4",
            5 => "Heading 5",
            6 => "Heading 6",
            _ => "Normal",
        };
        self.set_status(format!("Style: {}", label), cx);
        cx.notify();
    }

    fn cycle_body_font(&mut self, _: &CycleBodyFont, _window: &mut Window, cx: &mut Context<Self>) {
        let fonts = [
            "Noto Serif",
            "Noto Sans",
            "Liberation Serif",
            "Liberation Sans",
            "DejaVu Serif",
            "DejaVu Sans",
        ];
        let current = &self.document.body_font;
        let next_idx = fonts
            .iter()
            .position(|f| *f == current.as_str())
            .map(|i| (i + 1) % fonts.len())
            .unwrap_or(0);
        self.document.set_body_font(fonts[next_idx]);
        self.set_status(format!("Font: {}", fonts[next_idx]), cx);
        cx.notify();
    }

    fn adjust_body_font_size(&mut self, delta: i8, _window: &mut Window, cx: &mut Context<Self>) {
        let new_size = (self.document.body_font_size + delta as f32).clamp(8.0, 72.0);
        self.document.set_body_font_size(new_size);
        self.set_status(format!("Font size: {}", new_size.round() as i32), cx);
        cx.notify();
    }

    /// Heading level at the caret — `0` (Normal) unless Markdown mode is
    /// ON, which is the single source of truth for what counts as a
    /// heading. Uses the export parser's own `heading_level_and_text`, so
    /// `# ` with no text stays Normal, levels 4–6 work, and levels match
    /// what the Document Map and export will parse.
    fn current_heading_level(&self, cx: &mut Context<Self>) -> u8 {
        if !self.markdown_mode {
            return 0;
        }
        let content = self.editor.read(cx).content.clone();
        let cursor = self.editor.read(cx).cursor_offset();
        let line_start = content[..cursor].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let line_end = content[cursor..]
            .find('\n')
            .map(|p| cursor + p)
            .unwrap_or(content.len());
        let line = &content[line_start..line_end];
        heading_level_and_text(line.trim_start())
            .map(|(level, _)| level)
            .unwrap_or(0)
    }

    fn editing_cover_title(
        &mut self,
        _: &EditingCoverTitle,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editing_field = EditingField::CoverTitle;
        if let Some(cp) = self.document.cover_page() {
            self.field_input = cp.title.clone();
        }
        cx.notify();
    }

    fn editing_cover_subtitle(
        &mut self,
        _: &EditingCoverSubtitle,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editing_field = EditingField::CoverSubtitle;
        if let Some(cp) = self.document.cover_page() {
            self.field_input = cp.subtitle.clone();
        }
        cx.notify();
    }

    fn editing_cover_author(
        &mut self,
        _: &EditingCoverAuthor,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editing_field = EditingField::CoverAuthor;
        if let Some(cp) = self.document.cover_page() {
            self.field_input = cp.author.clone();
        }
        cx.notify();
    }

    fn commit_field_edit(&mut self, cx: &mut Context<Self>) {
        match &self.editing_field {
            EditingField::CoverTitle => {
                let text = self.field_input.clone();
                self.document.set_cover_title(text);
            }
            EditingField::CoverSubtitle => {
                let text = self.field_input.clone();
                self.document.set_cover_subtitle(text);
            }
            EditingField::CoverAuthor => {
                let text = self.field_input.clone();
                self.document.set_cover_author(text);
            }
            EditingField::ImageCaption(idx) => {
                let idx = *idx;
                let text = self.field_input.clone();
                if let Some(sylph_core::document::Block::Image { data }) =
                    self.document.blocks.get_mut(idx)
                {
                    data.caption = Some(text);
                }
            }
            EditingField::TableCaption(idx) => {
                let idx = *idx;
                let text = self.field_input.clone();
                if let Some(sylph_core::document::Block::Table { data }) =
                    self.document.blocks.get_mut(idx)
                {
                    data.caption = Some(text);
                }
            }
            _ => {}
        }
        self.editing_field = EditingField::None;
        self.field_input.clear();
        cx.notify();
    }

    fn cancel_field_edit(&mut self, cx: &mut Context<Self>) {
        self.editing_field = EditingField::None;
        self.field_input.clear();
        cx.notify();
    }

    #[allow(dead_code)]
    fn render_field_input(
        &self,
        value: &str,
        _border: gpui::Rgba,
        editor_bg: gpui::Rgba,
    ) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded(px(4.0))
            .border_1()
            .border_color(rgb(0x2196f3))
            .bg(editor_bg)
            .min_w_48()
            .child(format!("{}█", value))
    }

    fn on_ai_keystroke(
        &mut self,
        event: &KeystrokeEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ai_panel.visible {
            return;
        }
        let key = event.keystroke.key.as_str();
        if key == "enter" && !event.keystroke.modifiers.shift {
            self.ai_submit(&AiSubmit, _window, cx);
            return;
        }
        if key == "escape" {
            self.close_ai_panel(&CloseAiPanel, _window, cx);
            return;
        }
        if key == "backspace" {
            self.ai_panel.query.pop();
            cx.notify();
            return;
        }
        if let Some(ch) = &event.keystroke.key_char {
            if !event.keystroke.modifiers.platform && !event.keystroke.modifiers.control {
                self.ai_panel.query.push_str(ch);
                cx.notify();
            }
        }
    }

    fn on_field_keystroke(
        &mut self,
        event: &KeystrokeEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editing_field == EditingField::None {
            return;
        }
        let key = event.keystroke.key.as_str();
        if key == "enter" && !event.keystroke.modifiers.shift {
            self.commit_field_edit(cx);
            return;
        }
        if key == "escape" {
            self.cancel_field_edit(cx);
            return;
        }
        if key == "backspace" {
            self.field_input.pop();
            cx.notify();
            return;
        }
        if let Some(ch) = &event.keystroke.key_char {
            if !event.keystroke.modifiers.platform && !event.keystroke.modifiers.control {
                self.field_input.push_str(ch);
                cx.notify();
            }
        }
    }
}

impl Focusable for SylphApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl SylphApp {
    #[allow(dead_code)]
    fn legacy_render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bg = self.bg_color();
        let surface = self.surface_color();
        let sidebar_bg = self.sidebar_color();
        let border = self.border_color();
        let editor_bg = self.editor_bg();
        let hover = self.hover_color();
        let active_doc = self.active_doc_color();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(bg)
            .track_focus(&self.focus_handle(cx))
            .key_context("SylphApp")
            .on_action(cx.listener(Self::save_doc))
            .on_action(cx.listener(Self::summarize_doc))
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::move_word_left))
            .on_action(cx.listener(Self::move_word_right))
            .on_action(cx.listener(Self::page_up))
            .on_action(cx.listener(Self::page_down))
            .on_action(cx.listener(Self::indent))
            .on_action(cx.listener(Self::dedent))
            .on_action(cx.listener(Self::open_find_bar))
            .on_action(cx.listener(Self::close_find_bar))
            .on_action(cx.listener(Self::find_next))
            .on_action(cx.listener(Self::find_prev))
            .on_action(cx.listener(Self::replace_current))
            .on_action(cx.listener(Self::replace_all))
            .on_action(cx.listener(Self::context_menu_copy))
            .on_action(cx.listener(Self::context_menu_cut))
            .on_action(cx.listener(Self::context_menu_paste))
            .on_action(cx.listener(Self::context_menu_select_all))
            .on_action(cx.listener(Self::dismiss_context_menu))
            .on_action(cx.listener(Self::confirm_title))
            .on_action(cx.listener(Self::cancel_title))
            .on_action(cx.listener(Self::new_document))
            .on_action(cx.listener(Self::toggle_dark_mode))
            .on_action(cx.listener(Self::export_docx))
            .on_action(cx.listener(Self::export_pdf))
            .on_action(cx.listener(Self::toggle_preview))
            .on_action(cx.listener(Self::add_cover_page))
            .on_action(cx.listener(Self::paste_image))
            .on_action(cx.listener(Self::bold_text))
            .on_action(cx.listener(Self::italic_text))
            .on_action(cx.listener(Self::strikethrough_text))
            .on_action(cx.listener(Self::insert_table))
            .on_action(cx.listener(Self::open_ai_panel))
            .on_action(cx.listener(Self::close_ai_panel))
            .on_action(cx.listener(Self::ai_submit))
            .on_action(cx.listener(Self::insert_page_break))
            .on_action(cx.listener(Self::set_page_size))
            .on_action(cx.listener(Self::set_image_caption))
            .on_action(cx.listener(Self::set_table_caption))
            .on_action(cx.listener(Self::set_paragraph_spacing))
            .on_action(cx.listener(Self::editing_cover_title))
            .on_action(cx.listener(Self::editing_cover_subtitle))
            .on_action(cx.listener(Self::editing_cover_author))
            .child(
                // ── Title Bar ──────────────────────────────────────
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .py_2()
                    .bg(surface)
                    .border_b_1()
                    .border_color(border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                img("assets/icons/sylph-logo.png")
                                    .w(px(18.0))
                                    .h(px(18.0))
                                    .rounded_full(),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight(600.0))
                                    .child("Sylph"),
                            )
                            .child(
                                div()
                                    .w_px()
                                    .h_4()
                                    .bg(border),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .child(self.doc_title.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w_5()
                                    .h_5()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_md()
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.toggle_sidebar(&ToggleSidebar, _window, cx);
                                        }),
                                    )
                                    .child(if self.sidebar_visible { "◀" } else { "▶" }),
                            ),
                    ),
            )
            .child(
                // ── Ribbon Toolbar ─────────────────────────────────
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_3()
                    .py_1()
                    .bg(surface)
                    .border_b_1()
                    .border_color(border)
                    // ── File Group ──
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.new_document(&NewDocument, _window, cx);
                                        }),
                                    )
                                    .child("New"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.save_doc(&SaveDoc, _window, cx);
                                        }),
                                    )
                                    .child("Save"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.export_docx(&ExportDocx, _window, cx);
                                        }),
                                    )
                                    .child("DOCX"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.export_pdf(&ExportPdf, _window, cx);
                                        }),
                                    )
                                    .child("PDF"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.export_markdown(&ExportMarkdown, _window, cx);
                                        }),
                                    )
                                    .child("MD"),
                            ),
                    )
                    // ── Divider ──
                    .child(div().w_px().h_5().bg(border).mx_1())
                    // ── Edit Group ──
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.undo(&Undo, _window, cx);
                                        }),
                                    )
                                    .child("Undo"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.redo(&Redo, _window, cx);
                                        }),
                                    )
                                    .child("Redo"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.context_menu_copy(&ContextMenuCopy, _window, cx);
                                        }),
                                    )
                                    .child("Copy"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.context_menu_cut(&ContextMenuCut, _window, cx);
                                        }),
                                    )
                                    .child("Cut"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.context_menu_paste(&ContextMenuPaste, _window, cx);
                                        }),
                                    )
                                    .child("Paste"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.open_find_bar(&OpenFindBar, _window, cx);
                                        }),
                                    )
                                    .child("Find"),
                            ),
                    )
                    // ── Divider ──
                    .child(div().w_px().h_5().bg(border).mx_1())
                    // ── Format Group ──
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(4.0))
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.bold_text(&BoldText, _window, cx);
                                        }),
                                    )
                                    .child(
                                        div().font_weight(gpui::FontWeight(700.0)).child("B"),
                                    ),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(4.0))
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.italic_text(&ItalicText, _window, cx);
                                        }),
                                    )
                                    .child(div().italic().child("I")),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(4.0))
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.set_heading(1, _window, cx);
                                        }),
                                    )
                                    .child("H1"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(4.0))
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.set_heading(2, _window, cx);
                                        }),
                                    )
                                    .child("H2"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(4.0))
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.set_heading(3, _window, cx);
                                        }),
                                    )
                                    .child("H3"),
                            )
                            // ── Paragraph Spacing ──
                            .child(div().w_px().h_5().bg(border).mx_1())
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(4.0))
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            if this.editing_field == EditingField::ParagraphSpacing {
                                                this.commit_field_edit(cx);
                                            } else {
                                                this.editing_field = EditingField::ParagraphSpacing;
                                                this.field_input = format!("{:.1}", this.paragraph_spacing);
                                                cx.notify();
                                            }
                                        }),
                                    )
.child(format!("Gap: {:.0}px", self.paragraph_spacing)),
                            )
                            // ── Page Setup ──
                            .child(div().w_px().h_5().bg(border).mx_1())
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(4.0))
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.set_page_size(&SetPageSize, _window, cx);
                                        }),
                                    )
                                    .child(self.document.page_size.name().to_string()),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(4.0))
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            // Toggle margins: Normal (1") <-> Narrow (0.5")
                                            let new_margins = if this.document.page_margins.top > 54.0 {
                                                sylph_core::document::PageMargins { top: 36.0, bottom: 36.0, left: 36.0, right: 36.0 }
                                            } else {
                                                sylph_core::document::PageMargins::default()
                                            };
                                            this.document.set_margins(new_margins);
                                            cx.notify();
                                        }),
                                    )
                                    .child(if self.document.page_margins.top > 54.0 { "Normal" } else { "Narrow" }),
                            ),
                    )
                    // ── Divider ──
                    .child(div().w_px().h_5().bg(border).mx_1())
                    // ── Insert Group ──
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(4.0))
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.paste_image(&PasteImage, _window, cx);
                                        }),
                                    )
                                    .child("Image"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(4.0))
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.insert_table(&InsertTable, _window, cx);
                                        }),
                                    )
                                    .child("Table"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(4.0))
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.add_cover_page(&AddCoverPage, _window, cx);
                                        }),
                                    )
                                    .child("Cover"),
                            ),
                    )
                    // ── Divider ──
                    .child(div().w_px().h_5().bg(border).mx_1())
                    // ── View Group ──
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(4.0))
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.toggle_preview(&TogglePreview, _window, cx);
                                        }),
                                    )
                                    .child("Preview"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(4.0))
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.toggle_dark_mode(&ToggleDarkMode, _window, cx);
                                        }),
                                    )
                                    .child("Theme"),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(4.0))
                                    .hover(|s| s.bg(hover))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.open_ai_panel(&OpenAiPanel, _window, cx);
                                        }),
                                    )
                                    .child("AI"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .overflow_hidden()
                    .when(self.sidebar_visible, |this| {
                        this.child(
                            div()
                                .w_48()
                                .h_full()
                                .flex()
                                .flex_col()
                                .bg(sidebar_bg)
                                .border_r_1()
                                .border_color(border)
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .px_3()
                                        .py_2()
                                        .border_b_1()
                                        .border_color(border)
                                        .child("Documents")
                                        .child(
                                            div()
                                                .px_2()
                                                .py(px(5.))
                                                .rounded(px(4.0))
                                                .bg(rgb(0x2196f3))
                                                .hover(|s| s.bg(rgb(0x1976d2)))
                                                .text_color(rgb(0xffffff))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _window, cx| {
                                                        this.new_document(
                                                            &NewDocument,
                                                            _window,
                                                            cx,
                                                        );
                                                    }),
                                                )
                                                .child("+"),
                                        ),
                                )
                                .child({
                                    let docs: Vec<_> = self
                                        .documents
                                        .iter()
                                        .map(|doc| {
                                            let (id, title) = (&doc.id, &doc.title);
                                            let is_active = *id == self.editor.read(cx).doc_id;
                                            let doc_id = *id;
                                            div()
                                                .px_3()
                                                .py(px(2.))
                                                .rounded(px(4.0))
                                                .when(is_active, |s| s.bg(active_doc))
                                                .when(!is_active, |s| s.hover(|s| s.bg(hover)))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(move |this, _, _window, cx| {
                                                        this.switch_document(doc_id, _window, cx);
                                                    }),
                                                )
                                                .child(title.clone())
                                        })
                                        .collect();
                                    div().flex_1().overflow_hidden().p_2().children(docs)
                                }),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .px_4()
                                    .py_2()
                                    .border_b_1()
                                    .border_color(border)
                                    .child(
                                        div().flex().items_center().gap_2().child(
                                            div()
                                                .px_2()
                                                .py_1()
                                                .rounded(px(4.0))
                                                .when(self.editing_title, |s| {
                                                    s.border_1().border_color(rgb(0x2196f3))
                                                })
                                                .when(!self.editing_title, |s| {
                                                    s.hover(|s| s.bg(hover))
                                                })
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _window, cx| {
                                                        if !this.editing_title {
                                                            this.editing_title = true;
                                                            cx.notify();
                                                        }
                                                    }),
                                                )
                                                .child(format!(
                                                    "{}{}",
                                                    self.doc_title,
                                                    if self.editing_title { "█" } else { "" }
                                                )),
                                        ),
                                    ),
                            )
                            .when(self.find.visible, |this| {
                                this.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .px_4()
                                        .py_2()
                                        .border_b_1()
                                        .border_color(border)
                                        .bg(surface)
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .child("Find:")
                                                .child(
                                                    div()
                                                        .px_2()
                                                        .py_1()
                                                        .rounded_md()
                                                        .border_1()
                                                        .border_color(border)
                                                        .bg(editor_bg)
                                                        .min_w_48()
                                                        .child(format!("{}█", self.find.query)),
                                                )
                                                .child(format!(
                                                    "{}/{}",
                                                    self.find.current_match + 1,
                                                    self.find.matches.len().max(1)
                                                )),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .child("Replace:")
                                                .child(
                                                    div()
                                                        .px_2()
                                                        .py_1()
                                                        .rounded_md()
                                                        .border_1()
                                                        .border_color(border)
                                                        .bg(editor_bg)
                                                        .min_w_48()
                                                        .child(self.find.replacement.clone()),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .px_3()
                                                .py_1()
                                                .rounded_md()
                                                .bg(rgb(0x2196f3))
                                                .hover(|s| s.bg(rgb(0x1976d2)))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _window, cx| {
                                                        this.find_next(&FindNext, _window, cx);
                                                    }),
                                                )
                                                .child("Next"),
                                        )
                                        .child(
                                            div()
                                                .px_3()
                                                .py_1()
                                                .rounded_md()
                                                .bg(rgb(0xff9800))
                                                .hover(|s| s.bg(rgb(0xf57c00)))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _window, cx| {
                                                        this.replace_current(
                                                            &ReplaceCurrent,
                                                            _window,
                                                            cx,
                                                        );
                                                    }),
                                                )
                                                .child("Replace"),
                                        )
                                        .child(
                                            div()
                                                .px_3()
                                                .py_1()
                                                .rounded_md()
                                                .bg(rgb(0xf44336))
                                                .hover(|s| s.bg(rgb(0xd32f2f)))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _window, cx| {
                                                        this.replace_all(&ReplaceAll, _window, cx);
                                                    }),
                                                )
                                                .child("All"),
                                        )
                                        .child(
                                            div()
                                                .px_2()
                                                .py_1()
                                                .rounded_md()
                                                .hover(|s| s.bg(border))
                                                .cursor_pointer()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _window, cx| {
                                                        this.close_find_bar(
                                                            &CloseFindBar,
                                                            _window,
                                                            cx,
                                                        );
                                                    }),
                                                )
                                                .child("✕"),
                                        ),
                                )
                            })
                            .child({
                                let editor_content = div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .p_8()
                                    .bg(editor_bg)
                                    .on_mouse_down(
                                        MouseButton::Right,
                                        cx.listener(Self::on_editor_right_click),
                                    );

                                let editor_content = if let Some(cp) = self.document.cover_page().cloned() {
                                    let cp_title = cp.title.clone();
                                    let cp_subtitle = cp.subtitle.clone();
                                    let cp_author = cp.author.clone();
                                    let is_editing_title = self.editing_field == EditingField::CoverTitle;
                                    let is_editing_subtitle = self.editing_field == EditingField::CoverSubtitle;
                                    let is_editing_author = self.editing_field == EditingField::CoverAuthor;
                                    let field_val = self.field_input.clone();
                                    let hover_c = hover;

                                    let cover_card = div()
                                        .mb_6()
                                        .p_6()
                                        .rounded(px(8.0))
                                        .border_1()
                                        .border_color(border)
                                        .bg(rgb(0xf8f9fa))
                                        .child(
                                            div()
                                                .text_2xl()
                                                .font_weight(gpui::FontWeight(700.0))
                                                .mb_2()
                                                .child(if is_editing_title {
                                                    div()
                                                        .px_2()
                                                        .py_1()
                                                        .rounded(px(4.0))
                                                        .border_1()
                                                        .border_color(rgb(0x2196f3))
                                                        .bg(editor_bg)
                                                        .child(format!("{}█", field_val))
                                                } else {
                                                    div()
                                                        .px_2()
                                                        .py_1()
                                                        .rounded(px(4.0))
                                                        .hover(|s| s.bg(hover_c))
                                                        .cursor_pointer()
                                                        .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _window, cx| {
                                                            this.editing_cover_title(&EditingCoverTitle, _window, cx);
                                                        }))
                                                        .child(cp_title)
                                                }),
                                        )
                                        .child(
                                            div()
                                                .text_lg()
                                                .text_color(rgb(0x666666))
                                                .mb_2()
                                                .child(if is_editing_subtitle {
                                                    div()
                                                        .px_2()
                                                        .py_1()
                                                        .rounded(px(4.0))
                                                        .border_1()
                                                        .border_color(rgb(0x2196f3))
                                                        .bg(editor_bg)
                                                        .child(format!("{}█", field_val))
                                                } else {
                                                    div()
                                                        .px_2()
                                                        .py_1()
                                                        .rounded(px(4.0))
                                                        .hover(|s| s.bg(hover_c))
                                                        .cursor_pointer()
                                                        .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _window, cx| {
                                                            this.editing_cover_subtitle(&EditingCoverSubtitle, _window, cx);
                                                        }))
                                                        .child(cp_subtitle)
                                                }),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(0x888888))
                                                .child(if is_editing_author {
                                                    div()
                                                        .px_2()
                                                        .py_1()
                                                        .rounded(px(4.0))
                                                        .border_1()
                                                        .border_color(rgb(0x2196f3))
                                                        .bg(editor_bg)
                                                        .child(format!("{}█", field_val))
                                                } else {
                                                    div()
                                                        .px_2()
                                                        .py_1()
                                                        .rounded(px(4.0))
                                                        .hover(|s| s.bg(hover_c))
                                                        .cursor_pointer()
                                                        .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _window, cx| {
                                                            this.editing_cover_author(&EditingCoverAuthor, _window, cx);
                                                        }))
                                                        .child(cp_author)
                                                }),
                                        );
                                    editor_content.child(cover_card)
                                } else {
                                    editor_content
                                };

                                // Render non-text blocks
                                let mut editor_content = editor_content;
                                for (idx, block) in self.document.blocks.iter().enumerate() {
                                    match block {
                                        sylph_core::document::Block::Image { data } => {
                                            let path = data.path.clone();
                                            let caption = data.caption.clone().unwrap_or_default();
                                            let is_editing = self.editing_field == EditingField::ImageCaption(idx);
                                            let field_val = self.field_input.clone();
                                            let hover_c = hover;

                                            let img_block = div()
                                                .mb_4()
                                                .flex()
                                                .flex_col()
                                                .items_center()
                                                .child(
                                                    img(path)
                                                        .max_w(px(500.0))
                                                        .max_h(px(400.0))
                                                        .rounded(px(4.0))
                                                        .border_1()
                                                        .border_color(border),
                                                )
                                                .child(
                                                    div()
                                                        .mt_2()
                                                        .text_sm()
                                                        .text_color(rgb(0x666666))
                                                        .text_center()
                                                        .child(if is_editing {
                                                            div()
                                                                .px_2()
                                                                .py_1()
                                                                .rounded(px(4.0))
                                                                .border_1()
                                                                .border_color(rgb(0x2196f3))
                                                                .bg(editor_bg)
                                                                .min_w_48()
                                                                .child(format!("{}█", field_val))
                                                        } else {
                                                            let caption_clone = caption.clone();
                                                            div()
                                                                .px_2()
                                                                .py_1()
                                                                .rounded(px(4.0))
                                                                .hover(|s| s.bg(hover_c))
                                                                .cursor_pointer()
                                                                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _window, cx| {
                                                                    this.editing_field = EditingField::ImageCaption(idx);
                                                                    this.field_input = caption_clone.clone();
                                                                    cx.notify();
                                                                }))
                                                                .child(if caption.is_empty() { "Add caption...".to_string() } else { caption })
                                                        }),
                                                );
                                            editor_content = editor_content.child(img_block);
                                        }
                                        sylph_core::document::Block::Table { data } => {
                                            let is_editing_caption = self.editing_field == EditingField::TableCaption(idx);
                                            let caption = data.caption.clone().unwrap_or_default();
                                            let field_val = self.field_input.clone();
                                            let hover_c = hover;
                                            let rows = data.rows.clone();

                                            let mut table_grid = div()
                                                .mb_4()
                                                .border_1()
                                                .border_color(border)
                                                .rounded(px(4.0))
                                                .overflow_hidden();

                                            for row in rows.iter() {
                                                let mut row_el = div().flex().border_b_1().border_color(border);
                                                for cell in row.iter() {
                                                    row_el = row_el.child(
                                                        div()
                                                            .px_3()
                                                            .py_2()
                                                            .border_r_1()
                                                            .border_color(border)
                                                            .min_w_24()
                                                            .text_sm()
                                                            .child(cell.text()),
                                                    );
                                                }
                                                table_grid = table_grid.child(row_el);
                                            }

                                            let table_with_caption = table_grid.child(
                                                div()
                                                    .mt_2()
                                                    .text_sm()
                                                    .text_color(rgb(0x666666))
                                                    .text_center()
                                                    .child(if is_editing_caption {
                                                        div()
                                                            .px_2()
                                                            .py_1()
                                                            .rounded(px(4.0))
                                                            .border_1()
                                                            .border_color(rgb(0x2196f3))
                                                            .bg(editor_bg)
                                                            .min_w_48()
                                                            .child(format!("{}█", field_val))
                                                    } else {
                                                        let caption_clone = caption.clone();
                                                        div()
                                                            .px_2()
                                                            .py_1()
                                                            .rounded(px(4.0))
                                                            .hover(|s| s.bg(hover_c))
                                                            .cursor_pointer()
                                                            .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _window, cx| {
                                                                this.editing_field = EditingField::TableCaption(idx);
                                                                this.field_input = caption_clone.clone();
                                                                cx.notify();
                                                            }))
                                                            .child(if caption.is_empty() { "Add caption...".to_string() } else { caption })
                                                    }),
                                            );
                                            editor_content = editor_content.child(table_with_caption);
                                        }
                                        _ => {} // Text blocks handled by TextInput
                                    }
                                }

                                // Add the main text editor
                                editor_content.child(self.editor.clone())
                                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _window, cx| {
                                        this.commit_field_edit(cx);
                                    }))
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .py_1()
                    .bg(surface)
                    .border_t_1()
                    .border_color(border)
                    .child("v0.1.0")
                    .child(
                        self.status_message
                            .clone()
                            .unwrap_or_else(|| "UTF-8".to_string()),
                    ),
            )
            .when(self.ai_panel.visible, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .h_64()
                        .border_t_1()
                        .border_color(border)
                        .bg(surface)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .px_3()
                                .py_1()
                                .border_b_1()
                                .border_color(border)
                                .child("AI Assistant")
                                .child(
                                    div()
                                        .px_2()
                                        .py(px(5.))
                                        .rounded(px(4.0))
                                        .hover(|s| s.bg(hover))
                                        .cursor_pointer()
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _window, cx| {
                                                this.close_ai_panel(&CloseAiPanel, _window, cx);
                                            }),
                                        )
                                        .child("✕"),
                                ),
                        )
                        .child(div().flex_1().overflow_hidden().p_3().children(
                            self.ai_panel.history.iter().map(|(q, a)| {
                                div()
                                    .mb_2()
                                    .child(div().text_sm().child(format!("Q: {}", q)))
                                    .child(div().text_sm().child(format!("A: {}", a)))
                            }),
                        ))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .px_3()
                                .py_2()
                                .border_t_1()
                                .border_color(border)
                                .child(format!("{}█", self.ai_panel.query)),
                        ),
                )
            })
            .when(self.context_menu.visible, |this| {
                this.child(
                    anchored()
                        .position(self.context_menu.position)
                        .snap_to_window()
                        .child(
                            div()
                                .bg(editor_bg)
                                .rounded(px(4.))
                                .border_1()
                                .border_color(border)
                                .shadow_md()
                                .p_1()
                                .min_w_32()
                                .child(
                                    div()
                                        .px_3()
                                        .py_1()
                                        .rounded(px(4.))
                                        .hover(|s| s.bg(rgb(0x0078d4)))
                                        .hover(|s| s.text_color(rgb(0xffffff)))
                                        .cursor_pointer()
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _window, cx| {
                                                this.context_menu_copy(
                                                    &ContextMenuCopy,
                                                    _window,
                                                    cx,
                                                );
                                            }),
                                        )
                                        .child("Copy"),
                                )
                                .child(
                                    div()
                                        .px_3()
                                        .py_1()
                                        .rounded(px(4.))
                                        .hover(|s| s.bg(rgb(0x0078d4)))
                                        .hover(|s| s.text_color(rgb(0xffffff)))
                                        .cursor_pointer()
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _window, cx| {
                                                this.context_menu_cut(&ContextMenuCut, _window, cx);
                                            }),
                                        )
                                        .child("Cut"),
                                )
                                .child(
                                    div()
                                        .px_3()
                                        .py_1()
                                        .rounded(px(4.))
                                        .hover(|s| s.bg(rgb(0x0078d4)))
                                        .hover(|s| s.text_color(rgb(0xffffff)))
                                        .cursor_pointer()
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _window, cx| {
                                                this.context_menu_paste(
                                                    &ContextMenuPaste,
                                                    _window,
                                                    cx,
                                                );
                                            }),
                                        )
                                        .child("Paste"),
                                )
                                .child(div().h(px(1.0)).mx_1().my_1().bg(rgb(0xdddddd)))
                                .child(
                                    div()
                                        .px_3()
                                        .py_1()
                                        .rounded(px(4.))
                                        .hover(|s| s.bg(rgb(0x0078d4)))
                                        .hover(|s| s.text_color(rgb(0xffffff)))
                                        .cursor_pointer()
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _window, cx| {
                                                this.context_menu_select_all(
                                                    &ContextMenuSelectAll,
                                                    _window,
                                                    cx,
                                                );
                                            }),
                                        )
                                        .child("Select All"),
                                ),
                        ),
                )
            })
    }
}

// ── Headless export support ─────────────────────────────────────────────
// The GUI keeps typed text in the editor buffer while cover/tables/images/
// page breaks live in `self.document`. These helpers merge both into the
// single rich model that export serializes, and parse editor content
// (markdown-shaped text) into structured blocks.

use sylph_core::document as doc;

/// Parse one line's leading `#` markers. Byte index equals char count here
/// because `#` is ASCII; a non-`#` start yields count 0 → None.
fn heading_level_and_text(line: &str) -> Option<(u8, &str)> {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) {
        if let Some(stripped) = line[hashes..].strip_prefix(' ') {
            let text = stripped.trim();
            if !text.is_empty() {
                return Some((hashes as u8, text));
            }
        }
    }
    None
}

/// Inline link/autolink targets must look like a URL, not any `<...>`.
fn looks_like_link_target(url: &str) -> bool {
    !url.contains(char::is_whitespace)
        && (url.starts_with("http://")
            || url.starts_with("https://")
            || url.starts_with("ftp://")
            || url.starts_with("mailto:"))
}

/// Split inline markdown markers into styled runs. Markers are ASCII, so
/// byte scanning never splits a UTF-8 char; `_..._` is deliberately not a
/// marker (snake_case words must stay plain). Handles emphasis, code,
/// strikethrough, links `[t](u)`, inline images `![a](p)` (alt as plain
/// text — block-level images are detected by the line scanner), autolinks
/// `<https://...>`, and backslash escapes `\*`.
/// One piece of an inline Markdown source string, in source order.
#[derive(Clone, Debug, PartialEq)]
enum InlinePiece {
    /// Source bytes the export drops: emphasis markers, the `\` of an
    /// escape, and the syntax around a link's text or an image's alt text.
    Syntax(usize),
    /// Source bytes the export keeps verbatim, as part of output run `run`.
    Text {
        len: usize,
        run: usize,
        styles: Vec<doc::SpanStyle>,
    },
}

/// The one inline scanner behind both the export (`parse_inline_runs`)
/// and the Markdown-mode canvas. It says, byte for byte, which source text
/// is kept (and in which styled run) and which is syntax, so the canvas
/// hides exactly what the export drops and styles exactly what it styles.
fn inline_pieces(text: &str) -> Vec<InlinePiece> {
    let markers: [(&str, Vec<doc::SpanStyle>); 5] = [
        ("***", vec![doc::SpanStyle::BoldItalic]),
        ("**", vec![doc::SpanStyle::Bold]),
        ("~~", vec![doc::SpanStyle::Strikethrough]),
        ("`", vec![doc::SpanStyle::Code]),
        ("*", vec![doc::SpanStyle::Italic]),
    ];
    let mut pieces: Vec<InlinePiece> = Vec::new();
    let mut next_run = 0usize;
    // The open run of plain text, if any; a styled run closes it.
    let mut plain_run: Option<usize> = None;
    let bytes = text.as_bytes();
    let mut i = 0;
    let n = text.len();
    let push_plain = |len: usize,
                      pieces: &mut Vec<InlinePiece>,
                      plain_run: &mut Option<usize>,
                      next_run: &mut usize| {
        let run = *plain_run.get_or_insert_with(|| {
            *next_run += 1;
            *next_run - 1
        });
        // Plain text of one run is one piece, not one piece per character.
        if let Some(InlinePiece::Text {
            len: last,
            run: last_run,
            ..
        }) = pieces.last_mut()
        {
            if *last_run == run {
                *last += len;
                return;
            }
        }
        pieces.push(InlinePiece::Text {
            len,
            run,
            styles: Vec::new(),
        });
    };
    while i < n {
        let c = bytes[i];
        let rest = &text[i..];

        // 1. Backslash escape: ASCII punctuation after `\` is literal.
        if c == b'\\' && i + 1 < n && bytes[i + 1].is_ascii_punctuation() {
            pieces.push(InlinePiece::Syntax(1));
            push_plain(1, &mut pieces, &mut plain_run, &mut next_run);
            i += 2;
            continue;
        }

        // 2. Inline link `[text](url)`: the inner text is scanned
        // recursively so emphasis inside links survives; every run gets
        // the Link style.
        if c == b'[' {
            if let Some(rel) = rest.find("](") {
                let inner_start = i + 1;
                let inner_end = i + rel; // index of ']'
                let url_start = inner_end + 2;
                if let Some(rp) = text[url_start..].find(')') {
                    let url_end = url_start + rp;
                    let inner = &text[inner_start..inner_end];
                    let url = text[url_start..url_end].trim();
                    if !inner.is_empty() && looks_like_link_target(url) {
                        let inner_pieces = inline_pieces(inner);
                        if inner_pieces
                            .iter()
                            .any(|p| matches!(p, InlinePiece::Text { .. }))
                        {
                            plain_run = None;
                            pieces.push(InlinePiece::Syntax(1));
                            let base = next_run;
                            for piece in inner_pieces {
                                pieces.push(match piece {
                                    InlinePiece::Text {
                                        len,
                                        run,
                                        mut styles,
                                    } => {
                                        styles.push(doc::SpanStyle::Link(url.to_string()));
                                        next_run = next_run.max(base + run + 1);
                                        InlinePiece::Text {
                                            len,
                                            run: base + run,
                                            styles,
                                        }
                                    }
                                    syntax => syntax,
                                });
                            }
                            pieces.push(InlinePiece::Syntax(url_end + 1 - inner_end));
                            i = url_end + 1;
                            continue;
                        }
                    }
                }
            }
        }

        // 3. Inline image `![alt](path)`: the alt text is kept as plain
        // text (the block scanner turns standalone image lines into Image
        // blocks). It starts a new plain run that following text joins.
        if c == b'!' && rest.starts_with("![") {
            if let Some(rel) = rest.find("](") {
                let inner_start = i + 2;
                let inner_end = i + rel;
                let url_start = inner_end + 2;
                if let Some(rp) = text[url_start..].find(')') {
                    let url_end = url_start + rp;
                    let alt = &text[inner_start..inner_end];
                    if !alt.is_empty() {
                        plain_run = None;
                        pieces.push(InlinePiece::Syntax(2));
                        push_plain(alt.len(), &mut pieces, &mut plain_run, &mut next_run);
                        pieces.push(InlinePiece::Syntax(url_end + 1 - inner_end));
                        i = url_end + 1;
                        continue;
                    }
                }
            }
        }

        // 4. Autolink `<https://...>`.
        if c == b'<' {
            if let Some(rel) = rest[1..].find('>') {
                let url = &rest[1..1 + rel];
                if looks_like_link_target(url) {
                    plain_run = None;
                    pieces.push(InlinePiece::Syntax(1));
                    pieces.push(InlinePiece::Text {
                        len: url.len(),
                        run: next_run,
                        styles: vec![doc::SpanStyle::Link(url.to_string())],
                    });
                    next_run += 1;
                    pieces.push(InlinePiece::Syntax(1));
                    i += rel + 2;
                    continue;
                }
            }
        }

        // 5. Emphasis markers: longest match first. The inner text is
        // taken verbatim (no nested emphasis, no escapes).
        if c == b'*' || c == b'`' || c == b'~' {
            let found = markers.iter().find_map(|(m, s)| {
                // `m: &&str`; deref so `starts_with` gets a plain `&str`.
                rest.starts_with(*m).then(|| (*m, s.clone()))
            });
            if let Some((marker, styles)) = found {
                let start = i + marker.len();
                if let Some(rel) = text[start..].find(marker) {
                    let inner = &text[start..start + rel];
                    if !inner.is_empty() {
                        plain_run = None;
                        pieces.push(InlinePiece::Syntax(marker.len()));
                        pieces.push(InlinePiece::Text {
                            len: inner.len(),
                            run: next_run,
                            styles,
                        });
                        next_run += 1;
                        pieces.push(InlinePiece::Syntax(marker.len()));
                        i = start + rel + marker.len();
                        continue;
                    }
                }
            }
        }

        let ch_len = text[i..].chars().next().map_or(1, char::len_utf8);
        push_plain(ch_len, &mut pieces, &mut plain_run, &mut next_run);
        i += ch_len;
    }
    pieces
}

/// Inline Markdown as export runs: the kept text of `inline_pieces`,
/// grouped into the runs it names (markers, escapes' `\` and link syntax
/// dropped).
fn parse_inline_runs(text: &str) -> Vec<doc::TextRun> {
    let mut runs: Vec<doc::TextRun> = Vec::new();
    let mut pos = 0;
    for piece in inline_pieces(text) {
        match piece {
            InlinePiece::Syntax(len) => pos += len,
            InlinePiece::Text { len, run, styles } => {
                let chunk = &text[pos..pos + len];
                pos += len;
                if run == runs.len() {
                    runs.push(doc::TextRun::styled(chunk, styles));
                } else {
                    runs[run].text.push_str(chunk);
                }
            }
        }
    }
    runs
}

/// `---`, `***`, or `___` alone (≥3 markers, optional inner spaces).
fn is_horizontal_rule(line: &str) -> bool {
    let compact: String = line.trim().chars().filter(|c| !c.is_whitespace()).collect();
    if compact.chars().count() < 3 {
        return false;
    }
    ['-', '*', '_']
        .iter()
        .any(|&m| compact.chars().all(|c| c == m))
}

/// End byte (exclusive) of a `[text](url)` link starting at `i` (the
/// `[`). Mirrors the export parser's link rules: non-empty text and a
/// target that looks like a URL.
fn link_span_end(text: &str, i: usize) -> Option<usize> {
    let rel = text[i..].find("](")?;
    let inner_end = i + rel;
    let url_start = inner_end + 2;
    let rp = text[url_start..].find(')')?;
    let url_end = url_start + rp;
    let inner = &text[i + 1..inner_end];
    let url = text[url_start..url_end].trim();
    if !inner.is_empty() && looks_like_link_target(url) {
        Some(url_end + 1)
    } else {
        None
    }
}

/// End byte (exclusive) of an `![alt](path)` image span starting at `i`
/// (the `!`). Export keeps alt text as plain text, so the editor must not
/// color it as a link either.
fn image_span_end(text: &str, i: usize) -> Option<usize> {
    if !text[i..].starts_with("![") {
        return None;
    }
    let rel = text[i..].find("](")?;
    let inner_end = i + rel;
    let url_start = inner_end + 2;
    let rp = text[url_start..].find(')')?;
    if inner_end == i + 2 {
        return None; // empty alt — export leaves the span as plain text
    }
    Some(url_start + rp + 1)
}

/// End byte (exclusive) of an `<autolink>` span starting at `i` (the `<`).
fn autolink_span_end(text: &str, i: usize) -> Option<usize> {
    let rel = text[i + 1..].find('>')?;
    let url = &text[i + 1..i + 1 + rel];
    if looks_like_link_target(url) {
        Some(i + rel + 2)
    } else {
        None
    }
}

/// Marker byte length and item text of a bullet (`-`/`*`/`+`) or ordered
/// (`1. `/`1)`) line — the same two separators the export parser accepts.
fn list_marker_and_content(trimmed: &str) -> Option<(usize, &str)> {
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
        return Some((2, &trimmed[2..]));
    }
    let digits = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 && (trimmed[digits..].starts_with(". ") || trimmed[digits..].starts_with(") ")) {
        return Some((digits + 2, &trimmed[digits + 2..]));
    }
    None
}

/// Strip `>` quote markers: returns (depth, remaining text).
fn strip_quote(line: &str) -> Option<(u8, &str)> {
    let t = line.trim_start();
    if !t.starts_with('>') {
        return None;
    }
    let mut level = 0u8;
    let mut rest = t;
    while let Some(r) = rest.strip_prefix('>') {
        level += 1;
        rest = r.strip_prefix(' ').unwrap_or(r);
    }
    Some((level.min(4), rest.trim_end()))
}

/// Strip a leading task checkbox `[ ]`/`[x]`/`[X]` if present.
/// Returns (checked flag, remaining content).
fn take_checkbox_prefix(s: &str) -> (Option<bool>, &str) {
    for box_lit in ["[ ]", "[x]", "[X]"] {
        if let Some(after) = s.strip_prefix(box_lit) {
            if after.is_empty() || after.starts_with(' ') {
                let checked = box_lit != "[ ]";
                let content = if after.is_empty() { "" } else { &after[1..] };
                return (Some(checked), content);
            }
        }
    }
    (None, s)
}

// ── Markdown presentation transform (Markdown mode ON) ─────────────────────
//
// The toggle is the single source of truth for what the canvas shows:
// ON turns each logical source line into a WYSIWYG display line (block
// syntax disappears — headings lose `# `, quotes lose `> `, bullets render
// as `• `, fence delimiters hide) with the type scale the export renders;
// OFF is the identity transform, so display == source and nothing is
// parsed. `segments` keep a byte-exact source↔display map either way, so
// the caret, selection and mouse keep addressing the real source text.

/// What a line renders as when Markdown mode is ON.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DisplayKind {
    Paragraph,
    Heading,
    List,
    Quote,
    Rule,
    PageBreak,
    Code,
    Fence,
}

/// One source→display mapping segment: source bytes `[src_start, src_end)`
/// render as display bytes `[disp_start, disp_end)` of the line's display
/// text. Identity segments copy bytes verbatim; non-identity segments are
/// markers — hidden (`## ` → nothing) or replaced (`- ` → `• `).
#[derive(Clone, Copy, Debug)]
struct DisplaySegment {
    src_start: usize,
    src_end: usize,
    disp_start: usize,
    disp_end: usize,
    identity: bool,
}

/// A logical source line, transformed for canvas display.
#[derive(Clone, Debug)]
struct DisplayLine {
    /// Absolute source offset of this line's first byte.
    src_offset: usize,
    /// Display text (block syntax stripped when Markdown mode is ON).
    text: String,
    /// Monotonic source↔display mapping covering the whole line.
    segments: Vec<DisplaySegment>,
    /// Inline styles over display byte ranges (Markdown mode ON), from the
    /// export's own scanner: bold, italic, code, strikethrough, links.
    styles: Vec<(Range<usize>, Vec<doc::SpanStyle>)>,
    kind: DisplayKind,
    /// Explicit font size in px (headings); `None` inherits the editor's.
    font_size: Option<f32>,
    /// Space before/after the line box in px (headings).
    space_before: f32,
    space_after: f32,
    /// Font overrides for the line.
    bold: bool,
    italic: bool,
    mono: bool,
    /// The line is inside an open ``` fence (used by the OFF-mode
    /// highlighter path, matching the legacy `in_fence` flag).
    fenced: bool,
}

impl DisplayLine {
    /// The identity line: display == source, no parsing.
    fn identity(src_offset: usize, text: &str, fenced: bool) -> Self {
        let mut segments = Vec::new();
        if !text.is_empty() {
            segments.push(DisplaySegment {
                src_start: 0,
                src_end: text.len(),
                disp_start: 0,
                disp_end: text.len(),
                identity: true,
            });
        }
        DisplayLine {
            src_offset,
            text: text.to_string(),
            segments,
            styles: Vec::new(),
            kind: DisplayKind::Paragraph,
            font_size: None,
            space_before: 0.0,
            space_after: 0.0,
            bold: false,
            italic: false,
            mono: false,
            fenced,
        }
    }

    /// Source byte offset (line-local) → display byte offset. Bytes inside
    /// a hidden marker clamp to the marker's display start, so a caret in
    /// `# ` sits at the visible text start instead of a phantom position.
    fn src_to_disp(&self, src: usize) -> usize {
        for seg in &self.segments {
            if src <= seg.src_end {
                return if seg.identity {
                    seg.disp_start + (src - seg.src_start).min(seg.disp_end - seg.disp_start)
                } else {
                    seg.disp_start
                };
            }
        }
        self.text.len()
    }

    /// Display byte offset (line-local) → source byte offset. Clicks on a
    /// replaced marker (`• `) land on its source (`- `).
    fn disp_to_src(&self, disp: usize) -> usize {
        for seg in &self.segments {
            if disp <= seg.disp_end {
                return if seg.identity {
                    seg.src_start + (disp - seg.disp_start).min(seg.src_end - seg.src_start)
                } else {
                    seg.src_start
                };
            }
        }
        self.segments.last().map(|s| s.src_end).unwrap_or(0)
    }
}

/// Accumulates one line's display text and source↔display segments.
#[derive(Default)]
struct DisplayBuilder {
    text: String,
    segments: Vec<DisplaySegment>,
    styles: Vec<(Range<usize>, Vec<doc::SpanStyle>)>,
    src: usize,
}

impl DisplayBuilder {
    /// Consume `n` source bytes without rendering them (hidden marker).
    fn hide(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        let disp = self.text.len();
        self.segments.push(DisplaySegment {
            src_start: self.src,
            src_end: self.src + n,
            disp_start: disp,
            disp_end: disp,
            identity: false,
        });
        self.src += n;
    }

    /// Render the next `n` source bytes verbatim.
    fn ident(&mut self, line: &str, n: usize) {
        if n == 0 {
            return;
        }
        let disp = self.text.len();
        self.text.push_str(&line[self.src..self.src + n]);
        self.segments.push(DisplaySegment {
            src_start: self.src,
            src_end: self.src + n,
            disp_start: disp,
            disp_end: self.text.len(),
            identity: true,
        });
        self.src += n;
    }

    /// Render the next `n` source bytes as inline Markdown with the export's
    /// own scanner: syntax is hidden, kept text is copied and its styles
    /// recorded, so the canvas shows exactly the text the export keeps.
    fn inline(&mut self, line: &str, n: usize) {
        let content = &line[self.src..self.src + n];
        for piece in inline_pieces(content) {
            match piece {
                InlinePiece::Syntax(len) => self.hide(len),
                InlinePiece::Text { len, styles, .. } => {
                    let start = self.text.len();
                    self.ident(line, len);
                    let end = self.text.len();
                    match self.styles.last_mut() {
                        // Contiguous text with the same styles is one range.
                        Some((range, last)) if range.end == start && *last == styles => {
                            range.end = end;
                        }
                        _ if !styles.is_empty() => self.styles.push((start..end, styles)),
                        _ => {}
                    }
                }
            }
        }
    }

    /// Consume `n` source bytes and render `disp` in their place.
    fn replace(&mut self, n: usize, disp: &str) {
        if n == 0 {
            return;
        }
        let disp_start = self.text.len();
        self.text.push_str(disp);
        self.segments.push(DisplaySegment {
            src_start: self.src,
            src_end: self.src + n,
            disp_start,
            disp_end: self.text.len(),
            identity: false,
        });
        self.src += n;
    }
}

/// Heading type scale in px: pt at 96dpi (1pt = 4/3px). The spec pins H1
/// at 28pt bold with 12pt before / 6pt after; the rest follow Word/Docs
/// proportions (each level ~10–20% smaller, tighter spacing as level ↑).
fn heading_metrics_pt(level: u8) -> (f32, f32, f32) {
    match level {
        1 => (28.0, 12.0, 6.0),
        2 => (22.0, 10.0, 6.0),
        3 => (18.0, 8.0, 4.0),
        4 => (16.0, 6.0, 4.0),
        5 => (14.0, 4.0, 4.0),
        _ => (12.0, 4.0, 4.0),
    }
}

/// The spec's heading scale in canvas pixels (pt → px at 96dpi). The
/// inspector shows the same numbers in points via `heading_metrics_pt`.
fn heading_metrics(level: u8) -> (f32, f32, f32) {
    let (size_pt, before_pt, after_pt) = heading_metrics_pt(level);
    const PT: f32 = 4.0 / 3.0;
    (size_pt * PT, before_pt * PT, after_pt * PT)
}

/// Transform one logical source line for the canvas, with the export
/// parser's block order: fence → page break → quote → heading → rule →
/// list → paragraph. With `markdown_on == false` this is the identity
/// transform (display == source) so nothing is parsed or hidden.
fn display_line(line: &str, in_fence: bool, markdown_on: bool) -> DisplayLine {
    if !markdown_on {
        return DisplayLine::identity(0, line, in_fence);
    }

    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    let mut b = DisplayBuilder::default();
    let mut kind = DisplayKind::Paragraph;
    let (mut font_size, mut space_before, mut space_after) = (None, 0.0, 0.0);
    let (mut bold, mut italic, mut mono) = (false, false, false);

    if trimmed.starts_with("```") {
        // Fence delimiter (opening or closing): hidden entirely.
        b.hide(line.len());
        kind = DisplayKind::Fence;
        mono = true;
    } else if in_fence {
        // Fenced-code body: verbatim, monospace — export keeps it literal.
        b.ident(line, line.len());
        kind = DisplayKind::Code;
        mono = true;
    } else if line.trim() == "\\newpage" {
        // Typed page-break directive: hidden source, drawn as a page-break
        // line by the painter (matching `Block::PageBreak` in export).
        b.hide(line.len());
        kind = DisplayKind::PageBreak;
    } else if trimmed.starts_with('>') {
        // Quote: strip `>` markers with the export's own loop.
        b.ident(line, indent);
        let mut rest = trimmed;
        while let Some(r) = rest.strip_prefix('>') {
            rest = r.strip_prefix(' ').unwrap_or(r);
        }
        b.hide(trimmed.len() - rest.len());
        b.inline(line, rest.trim_end().len());
        b.hide(rest.len() - rest.trim_end().len());
        kind = DisplayKind::Quote;
        italic = true;
    } else if let Some((level, _)) = heading_level_and_text(trimmed) {
        // Heading: `# `…`###### ` hidden; the spec's `line.slice(2).trim()`
        // generalizes to "drop indent + hashes + spaces, trim the ends".
        let hashes = trimmed.chars().take_while(|&c| c == '#').count();
        let after_hashes = &trimmed[hashes..];
        let ws = after_hashes.len() - after_hashes.trim_start().len();
        let content = &line[indent + hashes + ws..];
        b.ident(line, indent);
        b.hide(hashes + ws);
        b.inline(line, content.trim_end().len());
        b.hide(content.len() - content.trim_end().len());
        kind = DisplayKind::Heading;
        let (size, before, after) = heading_metrics(level);
        font_size = Some(size);
        space_before = before;
        space_after = after;
        bold = true;
    } else if is_horizontal_rule(trimmed) {
        // Rule: hidden source, drawn as a line by the painter.
        b.hide(line.len());
        kind = DisplayKind::Rule;
    } else if let Some((marker_len, item)) = list_marker_and_content(trimmed) {
        // List: keep indent (nesting), render the bullet as `• ` and keep
        // ordered markers verbatim; task boxes become ☐/☑.
        b.ident(line, indent);
        if trimmed.as_bytes()[indent].is_ascii_digit() {
            b.ident(line, marker_len);
        } else {
            b.replace(marker_len, "• ");
        }
        let mut item_rest = item;
        if let (Some(checked), rest) = take_checkbox_prefix(item) {
            b.replace(item.len() - rest.len(), if checked { "☑ " } else { "☐ " });
            item_rest = rest;
        }
        b.inline(line, item_rest.len());
        b.hide(line.len() - b.src);
        kind = DisplayKind::List;
    } else {
        // Paragraph: inline Markdown (build_display_lines puts pipe-table
        // rows back to raw text).
        b.inline(line, line.len());
    }

    DisplayLine {
        src_offset: 0,
        text: b.text,
        segments: b.segments,
        styles: b.styles,
        kind,
        font_size,
        space_before,
        space_after,
        bold,
        italic,
        mono,
        fenced: false,
    }
}

/// Transform every logical line, tracking ``` fence state exactly like the
/// highlighter and the export parser. Source offsets are absolute.
fn build_display_lines(lines: &[String], markdown_on: bool) -> Vec<DisplayLine> {
    let mut out = Vec::with_capacity(lines.len());
    let mut offset = 0usize;
    let mut in_fence = false;
    for line in lines {
        let line_in_fence = in_fence;
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        let mut dl = display_line(line, line_in_fence, markdown_on);
        dl.src_offset = offset;
        offset += line.len() + 1;
        out.push(dl);
    }
    if markdown_on {
        for (i, row) in table_rows(lines, &out).into_iter().enumerate() {
            if row && out[i].kind == DisplayKind::Paragraph {
                // Pipe tables stay raw text until the canvas draws real
                // grids: the export reads their cells one by one, so
                // styling a whole row could hide markers the export keeps.
                out[i] = DisplayLine::identity(out[i].src_offset, &lines[i], false);
            }
        }
    }
    out
}

/// Which lines belong to pipe tables, by the export parser's own loop: a
/// line with `|` followed by a delimiter row starts a table (header and
/// delimiter), and the table continues while lines are non-blank and
/// contain `|`. Only a line that reaches the parser's table check can start
/// one: headings, quotes, list items, rules and images are checked first
/// (`dls` holds each line's display kind). Fenced lines never count.
fn table_rows(lines: &[String], dls: &[DisplayLine]) -> Vec<bool> {
    let mut rows = vec![false; lines.len()];
    let mut in_fence = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim_end();
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        } else if !in_fence
            && dls[i].kind == DisplayKind::Paragraph
            && standalone_image(line).is_none()
            && line.contains('|')
            && lines
                .get(i + 1)
                .is_some_and(|next| is_table_delimiter(next))
        {
            rows[i] = true;
            rows[i + 1] = true;
            i += 2;
            while i < lines.len() {
                let body = lines[i].trim_end();
                if body.trim().is_empty() || !body.contains('|') {
                    break;
                }
                rows[i] = true;
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    rows
}

/// Runs for display bytes `range` of `dl` with Markdown mode ON. The block
/// kind sets the base (headings bold and heading-coloured, quotes italic and
/// muted, code in the code colour); the line's inline styles, from the
/// export's own scanner, sit on top: real bold and italic faces, monospace
/// for code, a strike line, underlined links. Markers are already hidden.
fn display_runs(
    dl: &DisplayLine,
    range: Range<usize>,
    font: gpui::Font,
    base: gpui::Hsla,
) -> Vec<TextRun> {
    let color_code = hsla(120.0 / 360.0, 0.5, 0.35, 1.0);
    let color_link = hsla(210.0 / 360.0, 0.8, 0.45, 1.0);
    let (font, base) = match dl.kind {
        DisplayKind::Rule | DisplayKind::Fence | DisplayKind::PageBreak => return Vec::new(),
        DisplayKind::Code => {
            return vec![TextRun {
                len: range.len(),
                font,
                color: color_code,
                background_color: None,
                underline: None,
                strikethrough: None,
            }]
        }
        DisplayKind::Heading => (font.bold(), hsla(210.0 / 360.0, 0.8, 0.4, 1.0)),
        DisplayKind::Quote => (font.italic(), hsla(0.0, 0.0, 0.5, 1.0)),
        DisplayKind::List | DisplayKind::Paragraph => (font, base),
    };
    // Cut the row at every style boundary inside it; each piece is one run.
    let mut cuts = vec![range.start, range.end];
    for (r, _) in &dl.styles {
        cuts.extend([r.start, r.end].into_iter().filter(|p| range.contains(p)));
    }
    cuts.sort_unstable();
    cuts.dedup();
    cuts.windows(2)
        .map(|w| {
            let (start, end) = (w[0], w[1]);
            let (mut run_font, mut color) = (font.clone(), base);
            let (mut underline, mut strikethrough) = (None, None);
            let styles = dl
                .styles
                .iter()
                .filter(|(r, _)| r.start <= start && end <= r.end)
                .flat_map(|(_, s)| s);
            for style in styles {
                match style {
                    doc::SpanStyle::Bold => run_font = run_font.bold(),
                    doc::SpanStyle::Italic => run_font = run_font.italic(),
                    doc::SpanStyle::BoldItalic => run_font = run_font.bold().italic(),
                    doc::SpanStyle::Code => {
                        run_font.family = ui::MONO_FONT.into();
                        color = color_code;
                    }
                    doc::SpanStyle::Strikethrough => {
                        strikethrough = Some(gpui::StrikethroughStyle {
                            thickness: px(1.0),
                            color: None,
                        })
                    }
                    doc::SpanStyle::Underline => {
                        underline = Some(gpui::UnderlineStyle {
                            thickness: px(1.0),
                            color: None,
                            wavy: false,
                        })
                    }
                    doc::SpanStyle::Link(_) => {
                        color = color_link;
                        underline = Some(gpui::UnderlineStyle {
                            thickness: px(1.0),
                            color: Some(color_link),
                            wavy: false,
                        });
                    }
                }
            }
            TextRun {
                len: end - start,
                font: run_font,
                color,
                background_color: None,
                underline,
                strikethrough,
            }
        })
        .collect()
}

/// One bullet (`-`/`*`/`+`) or ordered (`1.`/`1)`) item, with task-box
/// detection (`[ ]`/`[x]`). Returns (raw_indent, ordered, checked, runs);
/// the scanner converts indent into a nesting level relative to the list
/// base so both 2-space and 4-space nesting styles work.
fn parse_list_item(line: &str) -> Option<(usize, bool, Option<bool>, Vec<doc::TextRun>)> {
    let b = line.as_bytes();
    let mut idx = 0usize;
    let mut indent = 0usize;
    while idx < b.len() {
        if b[idx] == b' ' {
            indent += 1;
            idx += 1;
        } else if b[idx] == b'\t' {
            indent += 4;
            idx += 1;
        } else {
            break;
        }
    }
    let rest = &line[idx..];
    if rest.is_empty() {
        return None;
    }

    // Bullet: marker followed by a space.
    if rest.len() >= 2
        && matches!(rest.as_bytes()[0], b'-' | b'*' | b'+')
        && rest.as_bytes()[1] == b' '
    {
        let (checked, content) = take_checkbox_prefix(&rest[2..]);
        if content.trim().is_empty() && checked.is_none() {
            return None; // "- " with nothing: not an item yet
        }
        return Some((indent, false, checked, parse_inline_runs(content.trim())));
    }

    // Ordered: digits + `.` or `)` + space.
    let digits = rest.bytes().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && digits <= 9 && digits < rest.len() {
        let sep = rest.as_bytes()[digits];
        if (sep == b'.' || sep == b')')
            && digits + 1 < rest.len()
            && rest.as_bytes()[digits + 1] == b' '
        {
            let (checked, content) = take_checkbox_prefix(&rest[digits + 2..]);
            if content.trim().is_empty() && checked.is_none() {
                return None;
            }
            return Some((indent, true, checked, parse_inline_runs(content.trim())));
        }
    }
    None
}

/// GFM table delimiter row (`| :--- | ---: |`).
fn is_table_delimiter(line: &str) -> bool {
    let t = line.trim();
    if !t.contains('-') {
        return false;
    }
    let cells = split_table_row(t);
    if cells.iter().all(|c| c.is_empty()) {
        return false;
    }
    cells
        .iter()
        .filter(|c| !c.is_empty())
        .all(|c| c.chars().all(|ch| ch == '-' || ch == ':'))
}

/// Split a pipe row into trimmed cells, ignoring outer pipes.
fn split_table_row(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    // Only a real (unescaped) trailing pipe closes the row: `| a\|` ends
    // with content, not a separator.
    let t = match t.strip_suffix('|') {
        Some(rest) if !tail_is_escaped(rest) => rest,
        _ => t,
    };
    let mut cells: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            // Keep escape pairs intact: `\\|` is an escaped backslash
            // followed by a real separator, `\|` is a literal pipe.
            cur.push('\\');
            if let Some(&next) = chars.peek() {
                if next.is_ascii_punctuation() {
                    cur.push(next);
                    chars.next();
                }
            }
            continue;
        }
        if c == '|' {
            cells.push(cur.trim().to_string());
            cur.clear();
            continue;
        }
        cur.push(c);
    }
    cells.push(cur.trim().to_string());
    cells
}

/// Does the char before `s.len()` (i.e. the stripped position) escape the
/// boundary — an odd run of backslashes directly before it?
fn tail_is_escaped(s: &str) -> bool {
    s.bytes().rev().take_while(|&b| b == b'\\').count() % 2 == 1
}

/// Standalone image line: `![alt](path)` and nothing else.
fn standalone_image(line: &str) -> Option<(String, String)> {
    let t = line.trim();
    let inner = t.strip_prefix("![")?;
    let close = inner.find("](")?;
    let alt = &inner[..close];
    let url = inner[close + 2..].strip_suffix(')')?;
    if alt.is_empty() || url.is_empty() {
        return None;
    }
    Some((alt.to_string(), url.to_string()))
}

fn flush_para(para: &mut Vec<&str>, blocks: &mut Vec<doc::Block>, line_spacing: f32) {
    if para.is_empty() {
        return;
    }
    let joined = para.join(" ");
    para.clear();
    let runs = parse_inline_runs(&joined);
    if !runs.is_empty() {
        blocks.push(doc::Block::Paragraph {
            runs,
            style: doc::ParagraphStyle {
                line_spacing,
                space_before: 0.0,
                space_after: 8.0,
            },
        });
    }
}

fn flush_quote(quote: &mut Vec<&str>, quote_level: &mut u8, blocks: &mut Vec<doc::Block>) {
    if quote.is_empty() {
        return;
    }
    let joined = quote.join(" ");
    quote.clear();
    let runs = parse_inline_runs(&joined);
    if !runs.is_empty() {
        blocks.push(doc::Block::Quote {
            level: *quote_level,
            runs,
        });
    }
}

fn flush_list(items: &mut Vec<doc::ListItem>, blocks: &mut Vec<doc::Block>) {
    if items.is_empty() {
        return;
    }
    let taken = std::mem::take(items);
    blocks.push(doc::Block::List { items: taken });
}

/// Parse editor content into structured blocks. Line scanner dispatch:
/// fenced code, blockquotes, lists (bulleted/ordered/task/nested), pipe
/// tables, headings, thematic breaks, standalone images, then paragraphs
/// (consecutive lines join with a space — soft wrap).
fn parse_content_blocks(text: &str, line_spacing: f32) -> Vec<doc::Block> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut blocks: Vec<doc::Block> = Vec::new();
    let mut para: Vec<&str> = Vec::new();
    let mut quote: Vec<&str> = Vec::new();
    let mut quote_level: u8 = 1;
    let mut items: Vec<doc::ListItem> = Vec::new();
    // List nesting bookkeeping: base indent of the current list + the first
    // deeper indent observed (2-space vs 4-space style).
    let mut list_base: Option<usize> = None;
    let mut list_step: Option<usize> = None;

    let mut in_fence = false;
    let mut fence_lang = String::new();
    let mut fence_body: Vec<&str> = Vec::new();

    let mut i = 0;
    while i < lines.len() {
        let raw = lines[i];

        // ── Fenced code: consume raw lines until closing fence or EOF ──
        if in_fence {
            if raw.trim_start().starts_with("```") {
                in_fence = false;
                blocks.push(doc::Block::CodeBlock {
                    language: std::mem::take(&mut fence_lang),
                    text: fence_body.join("\n"),
                });
                fence_body.clear();
            } else {
                fence_body.push(raw);
            }
            i += 1;
            continue;
        }

        let line = raw.trim_end();

        // ── Fence open ──
        if line.trim_start().starts_with("```") {
            flush_para(&mut para, &mut blocks, line_spacing);
            flush_quote(&mut quote, &mut quote_level, &mut blocks);
            flush_list(&mut items, &mut blocks);
            in_fence = true;
            fence_lang = line.trim_start()[3..]
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            i += 1;
            continue;
        }

        // ── Blank line: every pending construct ends ──
        if line.trim().is_empty() {
            flush_para(&mut para, &mut blocks, line_spacing);
            flush_quote(&mut quote, &mut quote_level, &mut blocks);
            flush_list(&mut items, &mut blocks);
            i += 1;
            continue;
        }

        // ── Page break: exactly what the Markdown exporter emits for
        //    `Block::PageBreak` (pandoc-style `\newpage`). Recognizing
        //    the directive keeps rich exports a byte-identical fixed
        //    point and lets authors type a real page break. ──
        if line.trim() == "\\newpage" {
            flush_para(&mut para, &mut blocks, line_spacing);
            flush_quote(&mut quote, &mut quote_level, &mut blocks);
            flush_list(&mut items, &mut blocks);
            blocks.push(doc::Block::page_break());
            i += 1;
            continue;
        }

        // ── Blockquote ──
        if let Some((lvl, content)) = strip_quote(line) {
            flush_para(&mut para, &mut blocks, line_spacing);
            flush_list(&mut items, &mut blocks);
            if content.is_empty() {
                // `>` alone: paragraph break inside/after the quote.
                flush_quote(&mut quote, &mut quote_level, &mut blocks);
            } else {
                if !quote.is_empty() {
                    quote_level = quote_level.max(lvl);
                } else {
                    quote_level = lvl;
                }
                quote.push(content);
            }
            i += 1;
            continue;
        }
        if !quote.is_empty() {
            // Non-quote line ends the quote; keep processing this line.
            flush_quote(&mut quote, &mut quote_level, &mut blocks);
        }

        // ── Heading (≤3 leading spaces tolerated via trim_start) ──
        if let Some((level, content)) = heading_level_and_text(line.trim_start()) {
            flush_para(&mut para, &mut blocks, line_spacing);
            flush_quote(&mut quote, &mut quote_level, &mut blocks);
            flush_list(&mut items, &mut blocks);
            blocks.push(doc::Block::Heading {
                level,
                runs: parse_inline_runs(content),
            });
            i += 1;
            continue;
        }

        // ── Thematic break (checked before list: `* * *` is a rule, not
        //    a bullet whose content is `* *`) ──
        if is_horizontal_rule(line) {
            flush_para(&mut para, &mut blocks, line_spacing);
            flush_quote(&mut quote, &mut quote_level, &mut blocks);
            flush_list(&mut items, &mut blocks);
            blocks.push(doc::Block::HorizontalRule);
            i += 1;
            continue;
        }

        // ── List item ──
        if let Some((indent, ordered, checked, runs)) = parse_list_item(line) {
            flush_para(&mut para, &mut blocks, line_spacing);
            flush_quote(&mut quote, &mut quote_level, &mut blocks);
            // Nesting depth is relative to the list's base indent; the step
            // is the first observed deeper indent (2- or 4-space style).
            if items.is_empty() {
                list_base = Some(indent);
                list_step = None;
            }
            let base = list_base.unwrap_or(indent);
            let diff = indent.saturating_sub(base);
            let level = if diff == 0 {
                0
            } else {
                let step = *list_step.get_or_insert(diff);
                diff.div_ceil(step).min(4) as u8
            };
            items.push(doc::ListItem {
                level,
                ordered,
                checked,
                runs,
            });
            i += 1;
            continue;
        }

        // ── Standalone image ──
        if let Some((alt, path)) = standalone_image(line) {
            flush_para(&mut para, &mut blocks, line_spacing);
            flush_quote(&mut quote, &mut quote_level, &mut blocks);
            flush_list(&mut items, &mut blocks);
            let mut data = doc::ImageData::new(path);
            data.alt_text = alt;
            blocks.push(doc::Block::Image { data });
            i += 1;
            continue;
        }

        // ── Pipe table: row + delimiter row + body rows ──
        if line.contains('|') && i + 1 < lines.len() && is_table_delimiter(lines[i + 1]) {
            flush_para(&mut para, &mut blocks, line_spacing);
            flush_quote(&mut quote, &mut quote_level, &mut blocks);
            flush_list(&mut items, &mut blocks);
            let mut raw_rows: Vec<Vec<String>> = vec![split_table_row(line)];
            i += 2; // skip delimiter
            while i < lines.len() {
                let body = lines[i].trim_end();
                if body.trim().is_empty() || !body.contains('|') {
                    break;
                }
                raw_rows.push(split_table_row(body));
                i += 1;
            }
            let num_cols = raw_rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
            let rows = raw_rows
                .into_iter()
                .map(|mut row| {
                    while row.len() < num_cols {
                        row.push(String::new());
                    }
                    row.truncate(num_cols);
                    row.into_iter()
                        .map(|cell| doc::TableCell {
                            runs: parse_inline_runs(&cell),
                        })
                        .collect()
                })
                .collect();
            blocks.push(doc::Block::Table {
                data: doc::TableData {
                    rows,
                    caption: None,
                    column_widths: vec![100.0 / num_cols as f32; num_cols],
                },
            });
            continue; // i already advanced past consumed rows
        }

        // ── List continuation: indented wrapped text joins last item ──
        if !items.is_empty() && (line.starts_with(' ') || line.starts_with('\t')) {
            if let Some(last) = items.last_mut() {
                last.runs.push(doc::TextRun::plain(" "));
                last.runs.extend(parse_inline_runs(line.trim()));
            }
            i += 1;
            continue;
        }

        // ── Plain paragraph line ──
        flush_quote(&mut quote, &mut quote_level, &mut blocks);
        flush_list(&mut items, &mut blocks);
        para.push(line.trim());
        i += 1;
    }

    flush_para(&mut para, &mut blocks, line_spacing);
    flush_quote(&mut quote, &mut quote_level, &mut blocks);
    flush_list(&mut items, &mut blocks);
    // Unclosed fence at EOF still renders its content (never drop text).
    if in_fence {
        blocks.push(doc::Block::CodeBlock {
            language: std::mem::take(&mut fence_lang),
            text: fence_body.join("\n"),
        });
    }
    blocks
}

/// A path under the platform data directory (never relative to the launch
/// cwd). Creates the parent directory so callers can write immediately.
fn data_path(rel: &str) -> PathBuf {
    let path = sylph_storage::data_dir().join(rel);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    path
}

/// Physical pages as the exporters lay the model out: one per page break,
/// plus the cover page, which they end with a page break of their own.
fn export_page_count(model: &doc::Document) -> usize {
    1 + model
        .blocks
        .iter()
        .filter(|b| matches!(b, doc::Block::PageBreak | doc::Block::CoverPage { .. }))
        .count()
}

/// Where unreadable saved models are copied before defaults replace them.
fn recovered_dir() -> PathBuf {
    sylph_storage::data_dir().join("recovered")
}

/// The saved structured model (page setup, cover page, inserted blocks)
/// for `doc_id`, or a fresh one, plus a warning for the status bar when
/// something could not be read. JSON that no longer parses is copied into
/// `recovered` first, so the next save cannot silently destroy it.
fn load_model_or_default(
    storage: &Storage,
    doc_id: i64,
    recovered: &std::path::Path,
) -> (doc::Document, Option<String>) {
    let json = match storage.load_model(doc_id) {
        Ok(Some(json)) => json,
        Ok(None) => return (doc::Document::new(), None),
        Err(e) => {
            let warning = format!("Could not read the page setup: {e}");
            return (doc::Document::new(), Some(warning));
        }
    };
    match serde_json::from_str(&json) {
        Ok(model) => (model, None),
        Err(e) => {
            let millis = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let name = format!("document-{doc_id}-model-{millis}.json");
            let kept = std::fs::create_dir_all(recovered).is_ok()
                && std::fs::write(recovered.join(&name), &json).is_ok();
            let warning = if kept {
                format!("Page setup could not be read ({e}); a copy was kept as recovered/{name}")
            } else {
                format!("Page setup could not be read ({e}) and no copy could be kept")
            };
            (doc::Document::new(), Some(warning))
        }
    }
}

/// OFF = what you see is what you export: one paragraph per nonblank
/// source line, verbatim — no `parse_inline_runs`, so `#`, `**` and
/// `\newpage` stay literal. Styling matches `flush_para` exactly.
fn literal_blocks(content: &str, line_spacing: f32) -> Vec<doc::Block> {
    let mut out = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            // Blank lines are spacing, not content.
            continue;
        }
        out.push(doc::Block::Paragraph {
            runs: vec![doc::TextRun::plain(line)],
            style: doc::ParagraphStyle {
                line_spacing,
                space_before: 0.0,
                space_after: 8.0,
            },
        });
    }
    out
}

/// Merge the structured model with typed editor content for export.
/// Order mirrors the canvas: cover first, then typed text, then inserted
/// objects (tables/images/captions/page breaks) in insertion order.
/// The empty placeholder paragraph from `Document::new()` is dropped.
/// True interleaving of cursor position with blocks needs the editor-kernel
/// phase (SYLPH_PLAN.md §10); this approximation keeps export truthful.
fn export_model(structured: &doc::Document, content: &str, markdown_on: bool) -> doc::Document {
    let mut out = structured.clone();
    let mut blocks: Vec<doc::Block> = Vec::new();
    for b in &structured.blocks {
        if matches!(b, doc::Block::CoverPage { .. }) {
            blocks.push(b.clone());
        }
    }
    if markdown_on {
        blocks.extend(parse_content_blocks(content, structured.line_spacing));
    } else {
        blocks.extend(literal_blocks(content, structured.line_spacing));
    }
    for b in &structured.blocks {
        match b {
            doc::Block::CoverPage { .. } => {}
            doc::Block::Paragraph { runs, .. } if runs.iter().all(|r| r.text.trim().is_empty()) => {
            }
            other => blocks.push(other.clone()),
        }
    }
    if blocks.is_empty() {
        blocks.push(doc::Block::paragraph(""));
    }
    out.blocks = blocks;
    out
}

fn print_export_usage(program: &str) {
    eprintln!("Usage:");
    eprintln!("  {program} --export-pdf <input.(json|md)> <out.pdf>");
    eprintln!("  {program} --export-docx <input.(json|md)> <out.docx>");
    eprintln!("  {program} --export-md <input.(json|md)> <out.md>");
    eprintln!();
    eprintln!("input.json is a serde-serialized sylph-core Document (rich model).");
    eprintln!("input.md is parsed with the SAME markdown parser the editor uses,");
    eprintln!("then exported — the end-to-end proof for typed markdown.");
    eprintln!("No window is opened in headless mode. Exit 0 on success, 1 on failure.");
}

fn try_headless_export(args: &[String]) -> Option<i32> {
    if args.len() < 2 {
        return None;
    }
    let program = args[0].clone();
    let mode = args[1].as_str();
    let is_export = matches!(
        mode,
        "--export-pdf"
            | "--export-docx"
            | "--export-md"
            | "--export-markdown"
            | "--help"
            | "-h"
            | "help"
    );
    if !is_export {
        return None;
    }
    if matches!(mode, "--help" | "-h" | "help") {
        print_export_usage(&program);
        return Some(0);
    }
    if args.len() != 4 {
        eprintln!("error: expected <input.json> and <output> arguments");
        print_export_usage(&program);
        return Some(2);
    }
    let input_path = &args[2];
    let output_path = &args[3];
    let json = if input_path.ends_with(".md") || input_path.ends_with(".markdown") {
        // Markdown input: run the exact parser the GUI editor export uses,
        // so this CLI proves the full "type markdown → document" pipeline.
        let content = match std::fs::read_to_string(input_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: cannot read {input_path}: {e}");
                return Some(1);
            }
        };
        let mut structured = sylph_core::document::Document::new();
        if let Some(stem) = std::path::Path::new(input_path)
            .file_stem()
            .and_then(|s| s.to_str())
        {
            structured.title = stem.to_string();
        }
        // Markdown input is Markdown by definition: headless proofs always
        // parse markdown, whatever the last GUI toggle state may have been.
        let model = export_model(&structured, &content, true);
        match serde_json::to_string(&model) {
            Ok(j) => j,
            Err(e) => {
                eprintln!("error: parsed document failed to serialize: {e}");
                return Some(1);
            }
        }
    } else {
        let content = match std::fs::read_to_string(input_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: cannot read {input_path}: {e}");
                return Some(1);
            }
        };
        // Validate that the input is a real Document before touching Python,
        // so JSON shape errors are reported honestly instead of as export output.
        if let Err(e) = serde_json::from_str::<sylph_core::document::Document>(&content) {
            eprintln!("error: {input_path} is not a valid Document JSON: {e}");
            return Some(1);
        }
        content
    };
    let result = match mode {
        "--export-pdf" => sylph_py_bridge::export_rich_pdf(&json, output_path),
        "--export-docx" => sylph_py_bridge::export_rich_docx(&json, output_path),
        "--export-md" | "--export-markdown" => {
            sylph_py_bridge::export_rich_markdown(&json, output_path)
        }
        _ => unreachable!(),
    };
    if result.starts_with("Exported") {
        println!("{result}");
        Some(0)
    } else {
        eprintln!("{result}");
        Some(1)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = try_headless_export(&args) {
        std::process::exit(code);
    }
    Application::new().run(|cx: &mut App| {
        cx.bind_keys([
            // ── Core Editing (always active) ──
            KeyBinding::new("backspace", Backspace, None),
            KeyBinding::new("delete", Delete, None),
            KeyBinding::new("ctrl-backspace", DeleteWordLeft, None),
            KeyBinding::new("ctrl-delete", DeleteWordRight, None),
            KeyBinding::new("cmd-backspace", DeleteToLineStart, None),
            KeyBinding::new("cmd-delete", DeleteToLineEnd, None),
            KeyBinding::new("enter", Enter, None),
            KeyBinding::new("ctrl-enter", InsertPageBreak, None),
            KeyBinding::new("cmd-enter", InsertPageBreak, None),
            KeyBinding::new("tab", Indent, None),
            KeyBinding::new("shift-tab", Dedent, None),
            // ── Navigation ──
            KeyBinding::new("left", Left, None),
            KeyBinding::new("right", Right, None),
            KeyBinding::new("up", Up, None),
            KeyBinding::new("down", Down, None),
            KeyBinding::new("shift-left", SelectLeft, None),
            KeyBinding::new("shift-right", SelectRight, None),
            KeyBinding::new("shift-up", SelectUp, None),
            KeyBinding::new("shift-down", SelectDown, None),
            KeyBinding::new("cmd-a", SelectAll, None),
            KeyBinding::new("home", Home, None),
            KeyBinding::new("end", End, None),
            KeyBinding::new("pageup", PageUp, None),
            KeyBinding::new("pagedown", PageDown, None),
            // ── Clipboard ──
            KeyBinding::new("cmd-v", Paste, None),
            KeyBinding::new("cmd-c", Copy, None),
            KeyBinding::new("cmd-x", Cut, None),
            // ── File ──
            KeyBinding::new("cmd-s", Save, None),
            // ── Undo/Redo ──
            KeyBinding::new("cmd-z", Undo, None),
            KeyBinding::new("cmd-shift-z", Redo, None),
            KeyBinding::new("ctrl-z", Undo, None),
            KeyBinding::new("ctrl-y", Redo, None),
            // ── Workspace chrome ──
            KeyBinding::new("ctrl-k", OpenCommandPalette, None),
            KeyBinding::new("cmd-k", OpenCommandPalette, None),
            KeyBinding::new("escape", CloseOverlay, None),
        ]);

        let bounds = Bounds::centered(None, size(px(1600.0), px(1280.0)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Untitled — Sylph".into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    window_decorations: Some(WindowDecorations::Client),
                    window_min_size: Some(size(px(1100.0), px(760.0))),
                    ..Default::default()
                },
                |_, cx| {
                    let storage = Storage::open().unwrap_or_else(|e| {
                        eprintln!("Storage init failed: {}", e);
                        Storage::default()
                    });
                    // Reopen what was open last time instead of adding a new
                    // empty "Untitled" on every launch.
                    let doc_id = storage.open_last_or_create().unwrap_or(1);
                    let saved = storage
                        .load_text(doc_id)
                        .unwrap_or(None)
                        .unwrap_or_default();
                    let doc_title = storage
                        .get_title(doc_id)
                        .unwrap_or_else(|_| "Untitled".to_string());
                    let (document, model_warning) =
                        load_model_or_default(&storage, doc_id, &recovered_dir());

                    let editor = cx.new(|cx| TextInput {
                        focus_handle: cx.focus_handle(),
                        content: saved,
                        placeholder: "Start typing to begin…  ·  Ctrl+K for commands".into(),
                        selected_range: 0..0,
                        selection_reversed: false,
                        preferred_column: None,
                        last_layout: None,
                        last_bounds: None,
                        all_lines: Vec::new(),
                        line_char_offsets: Vec::new(),
                        markdown_mode: false,
                        row_metas: Vec::new(),
                        content_height: px(0.0),
                        display_lines: Vec::new(),
                        cursor_row: 0,
                        line_height: px(20.0),
                        scroll_offset_y: px(0.0),
                        is_selecting: false,
                        storage,
                        doc_id,
                        undo_stack: Vec::new(),
                        redo_stack: Vec::new(),
                        show_line_numbers: false,
                        word_wrap: true,
                        save_task: None,
                        save_state: SaveState::Saved,
                    });
                    cx.new(|cx| {
                        let keystroke_subscription = cx.observe_keystrokes(
                            |this: &mut SylphApp,
                             event: &KeystrokeEvent,
                             _window: &mut Window,
                             cx: &mut Context<SylphApp>| {
                                this.on_find_keystroke(event, _window, cx);
                                this.on_title_keystroke(event, _window, cx);
                                this.on_ai_keystroke(event, _window, cx);
                                this.on_field_keystroke(event, _window, cx);
                            },
                        );
                        // Any notify may follow a model change; this one hook
                        // saves it, so no handler has to remember to.
                        let model_observer = cx.observe_self(|this: &mut SylphApp, cx| {
                            this.persist_model_if_changed(cx)
                        });
                        SylphApp {
                            editor,
                            persisted_model: document.clone(),
                            document,
                            model_save_error: None,
                            focus_handle: cx.focus_handle(),
                            sidebar_visible: true,
                            web_layout: false,
                            preview_visible: false,
                            find: FindReplaceState {
                                visible: false,
                                query: String::new(),
                                replacement: String::new(),
                                matches: Vec::new(),
                                current_match: 0,
                            },
                            context_menu: ContextMenuState {
                                visible: false,
                                position: point(px(0.0), px(0.0)),
                            },
                            doc_title,
                            editing_title: false,
                            documents: Vec::new(),
                            dark_mode: false,
                            ai_panel: AiPanelState {
                                visible: false,
                                query: String::new(),
                                response: String::new(),
                                history: Vec::new(),
                            },
                            // A load warning stays up until the next message.
                            status_message: model_warning,
                            editing_field: EditingField::None,
                            field_input: String::new(),
                            paragraph_spacing: 8.0,
                            navigator_tab: NavigatorTab::Outline,
                            inspector_mode: InspectorMode::Paragraph,
                            inspector_visible: true,
                            overlay: WorkspaceOverlay::None,
                            markdown_mode: false,
                            ruler_visible: true,
                            zoom_percent: 100,
                            image_picker_task: None,
                            status_clear_task: None,
                            _keystroke_subscription: keystroke_subscription,
                            _model_observer: model_observer,
                        }
                    })
                },
            )
            .expect("Failed to create window");

        window
            .update(cx, |view, window, cx| {
                view.load_documents(cx);
                window.focus(&view.editor.focus_handle(cx));
                cx.activate(true);
            })
            .expect("Failed to initialize window");
    });
}

#[cfg(test)]
mod export_model_tests {
    use super::*;

    #[test]
    fn heading_parse_requires_space_after_hashes() {
        assert_eq!(heading_level_and_text("# One"), Some((1, "One")));
        assert_eq!(heading_level_and_text("###### Six"), Some((6, "Six")));
        assert_eq!(heading_level_and_text("#nospace"), None);
        assert_eq!(heading_level_and_text("####### Seven"), None);
        assert_eq!(heading_level_and_text("no heading"), None);
        assert_eq!(heading_level_and_text("#"), None);
    }

    #[test]
    fn inline_markers_become_styled_runs() {
        use sylph_core::document::SpanStyle;
        let runs = parse_inline_runs("a **b** *c* `d` ~~e~~ ***f***");
        assert_eq!(runs[0].text, "a ");
        assert!(runs[0].styles.is_empty());
        // Plain separator runs interleave; assert the styled ones in order.
        let styled: Vec<(String, Vec<SpanStyle>)> = runs
            .iter()
            .filter(|r| !r.styles.is_empty())
            .map(|r| (r.text.clone(), r.styles.clone()))
            .collect();
        assert_eq!(styled.len(), 5);
        assert_eq!(styled[0], ("b".to_string(), vec![SpanStyle::Bold]));
        assert_eq!(styled[1], ("c".to_string(), vec![SpanStyle::Italic]));
        assert_eq!(styled[2], ("d".to_string(), vec![SpanStyle::Code]));
        assert_eq!(styled[3], ("e".to_string(), vec![SpanStyle::Strikethrough]));
        assert_eq!(styled[4], ("f".to_string(), vec![SpanStyle::BoldItalic]));
        // No marker text survives in the output.
        let joined: String = runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(joined, "a b c d e f");
    }

    #[test]
    fn snake_case_stays_plain_and_unclosed_markers_are_literal() {
        let runs = parse_inline_runs("snake_case_var and a *unclosed");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "snake_case_var and a *unclosed");
        assert!(runs[0].styles.is_empty());
    }

    #[test]
    fn content_parses_headings_paragraphs_and_blank_separators() {
        let blocks = parse_content_blocks("# Title\n\nline one\nline two\n\n## Sub", 1.15);
        assert_eq!(blocks.len(), 3);
        match &blocks[0] {
            sylph_core::document::Block::Heading { level, runs } => {
                assert_eq!(*level, 1);
                assert_eq!(runs[0].text, "Title");
            }
            b => panic!("expected heading, got {b:?}"),
        }
        match &blocks[1] {
            sylph_core::document::Block::Paragraph { runs, style } => {
                assert_eq!(runs[0].text, "line one line two");
                assert_eq!(style.line_spacing, 1.15);
            }
            b => panic!("expected paragraph, got {b:?}"),
        }
        match &blocks[2] {
            sylph_core::document::Block::Heading { level, .. } => assert_eq!(*level, 2),
            b => panic!("expected heading, got {b:?}"),
        }
    }

    #[test]
    fn inline_pieces_tile_the_source_exactly() {
        // Every source byte is either hidden syntax or kept text, so the
        // pieces must cover the source with no gap or overlap, never be
        // empty, and split only on character boundaries.
        let check = |text: &str| {
            let mut pos = 0;
            for piece in inline_pieces(text) {
                let (InlinePiece::Syntax(len) | InlinePiece::Text { len, .. }) = piece;
                assert!(len > 0, "empty piece in {text:?}");
                pos += len;
                assert!(text.is_char_boundary(pos), "split character in {text:?}");
            }
            assert_eq!(pos, text.len(), "pieces do not tile {text:?}");
        };
        for line in include_str!("../../../fixtures/kitchen-sink.md").lines() {
            check(line);
        }
        let alphabet: Vec<char> = "*`~[]()<>!\\ab h:/.é_".chars().collect();
        let mut seed: u64 = 0x5eed_1234;
        let mut next = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        for _ in 0..20_000 {
            let len = next() % 25;
            let mut s: String = (0..len)
                .map(|_| alphabet[next() % alphabet.len()])
                .collect();
            if next() % 8 == 0 {
                s.push_str("](https://x.io)");
            }
            check(&s);
        }
    }

    #[test]
    fn page_count_matches_the_exported_pages() {
        use sylph_core::document::{Block, CoverPageData, Document};
        assert_eq!(
            export_page_count(&export_model(&Document::new(), "text", true)),
            1
        );
        // The exporters end a cover with a page break, so it is a page of
        // its own; a typed `\newpage` and an inserted break add one each.
        let mut structured = Document::new();
        structured.set_cover_page(CoverPageData::new());
        structured.push_block(Block::page_break());
        let model = export_model(&structured, "a\n\n\\newpage\n\nb", true);
        assert_eq!(export_page_count(&model), 4);
    }

    #[test]
    fn export_model_orders_cover_text_then_structure() {
        use sylph_core::document::{Block, CoverPageData, Document};
        let mut structured = Document::new(); // starts with empty placeholder
        structured.push_block(Block::page_break());
        structured.push_block(Block::table(2, 2));
        structured.set_cover_page(CoverPageData::new().with_title("Cover"));
        let model = export_model(&structured, "# Head\n\nbody", true);
        assert!(matches!(model.blocks[0], Block::CoverPage { .. }));
        assert!(matches!(model.blocks[1], Block::Heading { .. }));
        assert!(matches!(model.blocks[2], Block::Paragraph { .. }));
        // placeholder dropped; page break and table preserved in order
        assert!(matches!(model.blocks[3], Block::PageBreak));
        assert!(matches!(model.blocks[4], Block::Table { .. }));
        assert_eq!(model.blocks.len(), 5);
        // page setup survives the clone
        assert_eq!(model.title, structured.title);
        assert_eq!(model.line_spacing, structured.line_spacing);
    }

    #[test]
    fn export_model_empty_content_still_yields_block() {
        use sylph_core::document::{Block, Document};
        let mut structured = Document::new();
        structured.push_block(Block::page_break());
        let model = export_model(&structured, "", true);
        assert!(!model.blocks.is_empty());
        assert!(matches!(model.blocks[0], Block::PageBreak));
    }

    #[test]
    fn export_off_mode_keeps_markdown_literal() {
        use sylph_core::document::{Block, Document};
        let structured = Document::new();
        let model = export_model(&structured, "# Title\n\n\\newpage\n\n**bold**", false);
        assert!(model
            .blocks
            .iter()
            .all(|b| matches!(b, Block::Paragraph { .. })));
        let texts: Vec<String> = model.blocks.iter().map(Block::plain_text).collect();
        assert_eq!(texts, ["# Title", "\\newpage", "**bold**"]);
    }

    #[test]
    fn bullet_ordered_nested_and_task_lists_parse() {
        use sylph_core::document::Block;
        let md = "- alpha\n- **beta**\n  - nested\n    - deeper\n1. first\n2) second\n- [ ] todo\n- [x] done";
        let blocks = parse_content_blocks(md, 1.15);
        // Consecutive items (no blank lines) form ONE list block; each item
        // carries its own ordered/task/level flags.
        let items = match &blocks[..] {
            [Block::List { items }] => items,
            other => panic!("expected one list, got {other:?}"),
        };
        assert_eq!(items.len(), 8);
        let levels: Vec<u8> = items.iter().map(|i| i.level).collect();
        assert_eq!(levels, [0, 0, 1, 2, 0, 0, 0, 0]);
        assert!(!items[0].ordered && items[0].checked.is_none());
        assert_eq!(items[0].runs[0].text, "alpha");
        // beta run is bold
        assert!(items[1]
            .runs
            .iter()
            .any(|r| r.text == "beta" && !r.styles.is_empty()));

        // ordered items, including the `)` separator form
        let ordered: Vec<_> = items.iter().filter(|i| i.ordered).collect();
        assert_eq!(ordered.len(), 2);
        assert_eq!(ordered[0].runs[0].text, "first");
        assert_eq!(ordered[1].runs[0].text, "second");
        // task items
        let tasks: Vec<_> = items.iter().filter(|i| i.checked.is_some()).collect();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].checked, Some(false));
        assert_eq!(tasks[0].runs[0].text, "todo");
        assert_eq!(tasks[1].checked, Some(true));
        assert_eq!(tasks[1].runs[0].text, "done");
    }

    #[test]
    fn pipe_table_with_styles_and_padding() {
        use sylph_core::document::Block;
        let md = "| Name | Value |\n| :--- | ---: |\n| **A** | 1 |\n| only-one |";
        let blocks = parse_content_blocks(md, 1.15);
        let data = match &blocks[..] {
            [Block::Table { data }] => data,
            other => panic!("expected one table, got {other:?}"),
        };
        assert_eq!(data.rows.len(), 3); // header + 2 body
        assert_eq!(data.col_count(), 2);
        assert_eq!(data.rows[0][0].text(), "Name");
        assert_eq!(data.rows[1][0].text(), "A");
        assert!(!data.rows[1][0].runs[0].styles.is_empty(), "bold in cell");
        // ragged row padded to 2 columns (empty cell → empty runs)
        assert_eq!(data.rows[2][1].text(), "");
        assert!(data.rows[2][1].runs.is_empty());
    }

    #[test]
    fn page_break_directive_parses_back() {
        use sylph_core::document::Block;
        // `\newpage` is exactly what the Markdown exporter emits for a
        // page break; parsing it back keeps rich exports a fixed point.
        let md = "Before\n\n\\newpage\n\n# After";
        let blocks = parse_content_blocks(md, 1.15);
        assert!(matches!(blocks[0], Block::Paragraph { .. }));
        assert!(matches!(blocks[1], Block::PageBreak));
        assert!(matches!(blocks[2], Block::Heading { .. }));
        // The directive only counts as a whole line — the same text
        // mid-sentence stays an ordinary paragraph.
        let text = parse_content_blocks("see the \\newpage later", 1.15);
        assert!(matches!(text.as_slice(), [Block::Paragraph { .. }]));
    }

    #[test]
    fn table_row_split_is_escape_aware() {
        // `a\|b` is one cell whose content is a literal pipe; a naive
        // split-on-pipe would invent an extra column (silent corruption).
        let cells = split_table_row("| a\\|b | c |");
        assert_eq!(cells, vec!["a\\|b", "c"]);
        let runs = parse_inline_runs(&cells[0]);
        assert_eq!(runs[0].text, "a|b");

        // A row ending in an escaped pipe has content, not a closing
        // separator: the escape stays with the cell.
        let cells = split_table_row("| a\\|");
        assert_eq!(cells, vec!["a\\|"]);

        // Even number of backslashes: `\\` is a literal backslash and the
        // pipe after it really is a separator.
        let cells = split_table_row("| a\\\\ | b |");
        assert_eq!(cells, vec!["a\\\\", "b"]);

        // Rows without any pipe still yield one cell.
        assert_eq!(split_table_row("only"), vec!["only"]);
    }

    #[test]
    fn blockquote_multiline_and_depth() {
        use sylph_core::document::Block;
        let md = "> first line\n> second line\n\n>> deep quote";
        let blocks = parse_content_blocks(md, 1.15);
        assert_eq!(blocks.len(), 2);
        match &blocks[0] {
            Block::Quote { level, runs } => {
                assert_eq!(*level, 1);
                assert_eq!(runs[0].text, "first line second line");
            }
            b => panic!("expected quote, got {b:?}"),
        }
        match &blocks[1] {
            Block::Quote { level, runs } => {
                assert_eq!(*level, 2);
                assert_eq!(runs[0].text, "deep quote");
            }
            b => panic!("expected quote, got {b:?}"),
        }
    }

    #[test]
    fn fenced_code_is_verbatim_with_language() {
        use sylph_core::document::Block;
        let md = "before\n\n```rust\nfn main() {\n    **not bold** # not heading\n}\n```\nafter";
        let blocks = parse_content_blocks(md, 1.15);
        match &blocks[..] {
            [Block::Paragraph { .. }, Block::CodeBlock { language, text }, Block::Paragraph { .. }] =>
            {
                assert_eq!(language, "rust");
                assert_eq!(text, "fn main() {\n    **not bold** # not heading\n}");
            }
            other => panic!("expected para/code/para, got {other:?}"),
        }
        // Unclosed fence at EOF still yields its body
        let open = parse_content_blocks("```\nno close", 1.15);
        assert!(matches!(&open[..], [Block::CodeBlock { text, .. }] if text == "no close"));
    }

    #[test]
    fn horizontal_rules_vs_lookalikes() {
        use sylph_core::document::{Block, SpanStyle};
        let md = "---\n\n* * *\n\n___\n\n- item\n\n***bold***\n\n-";
        let blocks = parse_content_blocks(md, 1.15);
        let rules = blocks
            .iter()
            .filter(|b| matches!(b, Block::HorizontalRule))
            .count();
        assert_eq!(rules, 3, "three thematic breaks, got {blocks:?}");
        // "- item" is a list, "***bold***" is a paragraph, "-" is a paragraph
        assert!(blocks.iter().any(|b| matches!(b, Block::List { .. })));
        let para_texts: Vec<String> = blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph { runs, .. } => {
                    Some(runs.iter().map(|r| r.text.as_str()).collect::<String>())
                }
                _ => None,
            })
            .collect();
        // "***bold***" is NOT a thematic break: marker chars are consumed
        // by the bold-italic parse, leaving text "bold" with styles.
        assert!(para_texts.iter().any(|t| t == "bold"));
        assert!(para_texts.iter().any(|t| t == "-"));
        let bold_para = blocks
            .iter()
            .find_map(|b| match b {
                Block::Paragraph { runs, .. }
                    if runs.iter().map(|r| r.text.as_str()).collect::<String>() == "bold" =>
                {
                    Some(runs)
                }
                _ => None,
            })
            .expect("bold paragraph present");
        assert!(bold_para[0]
            .styles
            .iter()
            .any(|s| matches!(s, SpanStyle::BoldItalic)));
    }

    #[test]
    fn links_autolinks_images_and_escapes() {
        use sylph_core::document::{Block, SpanStyle};
        // inline link with nested emphasis + autolink + escape
        let runs =
            parse_inline_runs("see [**docs**](https://x.io/a) or <https://y.io> and \\*literal\\*");
        let joined: String = runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(joined, "see docs or https://y.io and *literal*");
        let doc_runs: Vec<_> = runs
            .iter()
            .filter(|r| r.styles.iter().any(|s| matches!(s, SpanStyle::Link(_))))
            .collect();
        assert_eq!(doc_runs.len(), 2, "two link runs (docs + autolink)");
        match &doc_runs[0].styles[..] {
            [SpanStyle::Bold, SpanStyle::Link(url)] => assert_eq!(url, "https://x.io/a"),
            other => panic!("expected bold+link, got {other:?}"),
        }
        match &doc_runs[1].styles[..] {
            [SpanStyle::Link(url)] => assert_eq!(url, "https://y.io"),
            other => panic!("expected link, got {other:?}"),
        }
        // Non-URL angle text stays literal
        let plain = parse_inline_runs("<not a link>");
        assert_eq!(plain[0].text, "<not a link>");
        assert!(plain[0].styles.is_empty());

        // Standalone image line → Image block
        let blocks = parse_content_blocks("![diagram](fixtures/proof.png)", 1.15);
        match &blocks[..] {
            [Block::Image { data }] => {
                assert_eq!(data.path, "fixtures/proof.png");
                assert_eq!(data.alt_text, "diagram");
            }
            other => panic!("expected image, got {other:?}"),
        }
        // Inline image inside text → alt as plain text
        let inline = parse_inline_runs("top ![icon](i.png) end");
        let joined: String = inline.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(joined, "top icon end");
    }

    #[test]
    fn list_continuation_joins_wrapped_item() {
        use sylph_core::document::Block;
        let md = "- first line\n  wrapped tail\n- second";
        let blocks = parse_content_blocks(md, 1.15);
        match &blocks[..] {
            [Block::List { items }] => {
                assert_eq!(items.len(), 2);
                let t: String = items[0].runs.iter().map(|r| r.text.as_str()).collect();
                assert_eq!(t, "first line wrapped tail");
                assert_eq!(items[1].runs[0].text, "second");
            }
            other => panic!("expected one list, got {other:?}"),
        }
    }

    #[test]
    fn kitchen_sink_document_order() {
        use sylph_core::document::Block;
        let md = concat!(
            "# Title\n\n",
            "Para with [link](https://a.b) and **strong**.\n\n",
            "> quote\n\n",
            "- one\n- two\n\n",
            "| h |\n| --- |\n| 1 |\n\n",
            "```py\nx = 1\n```\n\n",
            "---\n\n",
            "![alt](p.png)\n\n",
            "end"
        );
        let blocks = parse_content_blocks(md, 1.15);
        let kinds: Vec<&str> = blocks
            .iter()
            .map(|b| match b {
                Block::Heading { .. } => "heading",
                Block::Paragraph { .. } => "paragraph",
                Block::Quote { .. } => "quote",
                Block::List { .. } => "list",
                Block::Table { .. } => "table",
                Block::CodeBlock { .. } => "code",
                Block::HorizontalRule => "hr",
                Block::Image { .. } => "image",
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "heading",
                "paragraph",
                "quote",
                "list",
                "table",
                "code",
                "hr",
                "image",
                "paragraph"
            ]
        );
    }

    #[test]
    fn export_model_serializes_to_python_expected_shapes() {
        use sylph_core::document::Document;
        let structured = Document::new();
        let model = export_model(&structured, "# H\n\n**bold** text", true);
        let json = serde_json::to_string(&model).expect("serialize");
        // Rust-shape checks: unit variants as strings, styles as strings.
        assert!(json.contains("\"Heading\""));
        assert!(json.contains("\"PageBreak\"") || json.contains("\"Paragraph\""));
        assert!(json.contains("\"Bold\""));
        // Round-trip: the same JSON the headless CLI validates.
        let back: Document = serde_json::from_str(&json).expect("parse back");
        assert_eq!(back, model);
    }

    #[test]
    #[ignore = "writes a proof artifact for manual headless CLI verification"]
    fn write_merged_proof_json() {
        use sylph_core::document::{Block, CoverPageData, Document};
        let mut structured = Document::new();
        structured.set_cover_page(
            CoverPageData::new()
                .with_title("Merge Proof — cover")
                .with_subtitle("subtitle")
                .with_author("Anmol"),
        );
        structured.push_block(Block::page_break());
        structured.push_block(Block::table(2, 2));
        let model = export_model(
            &structured,
            "# Merged Head\n\nBody **bold** and *italic* text.\n\nSecond paragraph.",
            true,
        );
        let json = serde_json::to_string_pretty(&model).expect("serialize");
        std::fs::create_dir_all("/tmp/opencode").expect("tmp dir");
        std::fs::write("/tmp/opencode/merged.json", &json).expect("write proof");
        assert!(json.contains("Merged Head"));
    }
}

#[cfg(test)]
mod editor_highlight_tests {
    use super::*;

    fn font() -> gpui::Font {
        gpui::font("monospace")
    }

    fn base() -> gpui::Hsla {
        hsla(0.0, 0.0, 0.2, 1.0)
    }

    fn bold_color() -> gpui::Hsla {
        hsla(0.0, 0.0, 0.15, 1.0)
    }

    fn code_color() -> gpui::Hsla {
        hsla(120.0 / 360.0, 0.5, 0.35, 1.0)
    }

    fn link_color() -> gpui::Hsla {
        hsla(210.0 / 360.0, 0.8, 0.45, 1.0)
    }

    fn list_color() -> gpui::Hsla {
        hsla(30.0 / 360.0, 0.7, 0.45, 1.0)
    }

    fn total_len(runs: &[TextRun]) -> usize {
        runs.iter().map(|r| r.len).sum()
    }

    #[test]
    fn struck_runs_carry_a_strike_line() {
        // The canvas draws a real strike line under `~~…~~` content —
        // the same thing the PDF/DOCX export renders for strikethrough.
        let runs = TextInput::markdown_runs("a ~~gone~~ b", font(), base(), false);
        let struck_len: usize = runs
            .iter()
            .filter(|r| r.strikethrough.is_some())
            .map(|r| r.len)
            .sum();
        assert_eq!(struck_len, "gone".len(), "only content is struck");
        assert_eq!(
            runs.iter().map(|r| r.len).sum::<usize>(),
            "a ~~gone~~ b".len()
        );
    }

    #[test]
    fn highlight_runs_cover_every_line_exactly() {
        // If runs don't cover the line, paint boundaries shift left and
        // the editor claims styles the export won't render.
        let lines = [
            "**bold** and *italic* and `code` and ~~gone~~",
            "- [ ] todo",
            "- [x] done",
            "1) ordered and 2. also",
            "> quote **strong**",
            ">> deeper",
            "# Head with `code`",
            "* * *",
            "- - -",
            "---",
            "| a | **b** |",
            "snake_case_name.txt stays plain",
            "[docs](https://example.com) vs <https://x.io>",
            "escaped \\* star stays literal",
            "![alt](img.png)",
            "plain paragraph",
            "",
            "    indented line",
        ];
        for line in lines {
            for in_fence in [false, true] {
                let runs = TextInput::markdown_runs(line, font(), base(), in_fence);
                assert_eq!(
                    total_len(&runs),
                    line.len(),
                    "coverage mismatch for {line:?} (in_fence={in_fence})"
                );
            }
        }
    }

    #[test]
    fn highlight_snake_case_stays_plain() {
        // `_..._` is not an emphasis marker in export; the editor must
        // not pretend it is.
        let line = "some_file_name.txt";
        let runs = TextInput::markdown_runs(line, font(), base(), false);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].len, line.len());
        assert_eq!(runs[0].color, base());
    }

    #[test]
    fn highlight_spaced_rule_is_a_rule_not_a_bullet() {
        // Export parses `* * *` as a thematic break before list detection.
        for line in ["* * *", "- - -"] {
            let runs = TextInput::markdown_runs(line, font(), base(), false);
            assert_eq!(runs.len(), 1, "{line}");
            assert_eq!(runs[0].color, list_color(), "{line}");
        }
    }

    #[test]
    fn highlight_ordered_accepts_paren_separator() {
        let line = "3) third";
        let runs = TextInput::markdown_runs(line, font(), base(), false);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].len, 3, "marker includes the space");
        assert_eq!(runs[0].color, list_color());
        assert_eq!(runs[1].len, 5);
        assert_eq!(runs[1].color, base());
        assert_eq!(total_len(&runs), line.len());
    }

    #[test]
    fn highlight_fence_body_is_verbatim() {
        // Inside a fence export stores text verbatim: no headings, no
        // lists and no emphasis may be claimed by the editor either.
        let line = "# not a heading - still code";
        let runs = TextInput::markdown_runs(line, font(), base(), true);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].len, line.len());
        assert_eq!(runs[0].color, code_color());
    }

    #[test]
    fn highlight_emphasis_does_not_shift_boundaries() {
        // Marker bytes used to fall outside every run, shifting colors
        // left: `**ab**` painted `**ab` bold. Runs must cover exactly.
        let runs = TextInput::markdown_runs("**ab**", font(), base(), false);
        let lens: Vec<usize> = runs.iter().map(|r| r.len).collect();
        assert_eq!(lens, vec![2, 2, 2]);
        assert_eq!(runs[0].color, base(), "opening marker stays plain");
        assert_eq!(runs[1].color, bold_color());
        assert_eq!(runs[2].color, base(), "closing marker stays plain");
    }

    #[test]
    fn highlight_task_box_shares_the_marker_color() {
        // Export treats `[x]` as metadata, not as link-ish text.
        let line = "- [x] done";
        let runs = TextInput::markdown_runs(line, font(), base(), false);
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].len, 2);
        assert_eq!(runs[0].color, list_color());
        assert_eq!(runs[1].len, 4);
        assert_eq!(runs[1].color, list_color());
        assert_eq!(runs[2].len, 4);
        assert_eq!(runs[2].color, base());
        assert_eq!(total_len(&runs), line.len());
    }

    #[test]
    fn highlight_link_span_colors_whole_link_like_export() {
        let line = "[docs](https://example.com)";
        let runs = TextInput::markdown_runs(line, font(), base(), false);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].len, line.len());
        assert_eq!(runs[0].color, link_color());
    }

    #[test]
    fn highlight_inline_image_alt_stays_plain() {
        // Export keeps image alt text plain; the editor must not paint
        // the whole span as a link.
        let line = "see ![alt](img.png) here";
        let runs = TextInput::markdown_runs(line, font(), base(), false);
        assert_eq!(total_len(&runs), line.len());
        assert!(runs.iter().all(|r| r.color == base()));
    }

    #[test]
    fn export_format_extension_matches_renderer_targets() {
        // The GUI menu writes .md through the same model + renderer as
        // the headless CLI; the extension is the observable contract.
        assert_eq!(ExportFormat::Pdf.ext(), "pdf");
        assert_eq!(ExportFormat::Docx.ext(), "docx");
        assert_eq!(ExportFormat::Markdown.ext(), "md");
    }
}

#[cfg(test)]
mod markdown_wysiwyg_tests {
    use super::*;

    #[test]
    fn heading_metrics_have_a_pt_source_and_px_render() {
        // The style inspector shows the spec's points; the canvas paints
        // pixels (pt × 4/3 at 96dpi). One source, two renderings.
        assert_eq!(heading_metrics_pt(1), (28.0, 12.0, 6.0));
        assert_eq!(heading_metrics_pt(6), (12.0, 4.0, 4.0));
        let (s, b, a) = heading_metrics(1);
        assert!((s - 28.0 * 4.0 / 3.0).abs() < 1e-4, "size px");
        assert!((b - 12.0 * 4.0 / 3.0).abs() < 1e-4, "space before px");
        assert!((a - 6.0 * 4.0 / 3.0).abs() < 1e-4, "space after px");
    }

    fn on(line: &str) -> DisplayLine {
        display_line(line, false, true)
    }

    fn off(line: &str) -> DisplayLine {
        display_line(line, false, false)
    }

    #[test]
    fn heading_on_hides_syntax_and_applies_spec_metrics() {
        let dl = on("# working as a header 1");
        assert_eq!(dl.kind, DisplayKind::Heading);
        // The spec: `line.slice(2).trim()` — marker gone, text kept.
        assert_eq!(dl.text, "working as a header 1");
        assert!(dl.bold);
        let (size, before, after) = heading_metrics(1);
        assert_eq!(dl.font_size, Some(size));
        assert_eq!((dl.space_before, dl.space_after), (before, after));
        // 28pt bold, 12pt space before, 6pt space after → px at 96dpi.
        assert!((size - 112.0 / 3.0).abs() < 0.01);
        assert!((before - 16.0).abs() < 0.01);
        assert!((after - 8.0).abs() < 0.01);
    }

    #[test]
    fn heading_off_stays_literal_paragraph() {
        let dl = off("# working as a header 1");
        assert_eq!(dl.kind, DisplayKind::Paragraph);
        assert_eq!(dl.text, "# working as a header 1");
        assert!(dl.font_size.is_none());
        // Identity mapping: every source offset maps to itself.
        for i in 0..=dl.text.len() {
            assert_eq!(dl.src_to_disp(i), i);
            assert_eq!(dl.disp_to_src(i), i);
        }
    }

    #[test]
    fn off_mode_never_transforms_any_construct() {
        for line in ["# h", "> q", "- x", "---", "```", "1. a", "* * *"] {
            let dls = build_display_lines(&[line.to_string()], false);
            let dl = &dls[0];
            assert_eq!(dl.text, line, "line: {line}");
            assert_eq!(dl.kind, DisplayKind::Paragraph, "line: {line}");
            assert!(dl.font_size.is_none(), "line: {line}");
        }
    }

    #[test]
    fn heading_mapping_addresses_source_positions() {
        let dl = on("# Title");
        // Marker bytes clamp to the start of the visible text.
        assert_eq!(dl.src_to_disp(0), 0); // before `#`
        assert_eq!(dl.src_to_disp(2), 0); // after `# `
        assert_eq!(dl.src_to_disp(3), 1); // inside "Title"
                                          // Clicks: display start lands on the line start, content maps back
                                          // into the source content.
        assert_eq!(dl.disp_to_src(0), 0);
        assert_eq!(dl.disp_to_src(1), 3); // after 'T' → after 'T' in source
        assert_eq!(dl.disp_to_src(5), 7); // end of "Title" = line end
        assert_eq!(dl.disp_to_src(dl.text.len()), "# Title".len());
    }

    #[test]
    fn bullet_and_task_box_replacements_map_to_markers() {
        let dl = on("- [x] ship it");
        assert_eq!(dl.kind, DisplayKind::List);
        assert_eq!(dl.text, "• ☑ ship it");
        // `- ` renders as `• `, `[x] ` as `☑ ` — source inside either
        // marker clamps to the marker's start.
        assert_eq!(dl.src_to_disp(1), 0); // inside `- `
        assert_eq!(dl.src_to_disp(5), 4); // inside `[x] `
        assert_eq!(dl.disp_to_src(5), 2); // on `☑` → marker source start
        assert_eq!(dl.disp_to_src(10), 8); // in "ship it" content
        assert_eq!(dl.disp_to_src(dl.text.len()), "- [x] ship it".len());
    }

    #[test]
    fn ordered_list_marker_stays_verbatim() {
        let dl = on("3) third");
        assert_eq!(dl.kind, DisplayKind::List);
        assert_eq!(dl.text, "3) third");
    }

    #[test]
    fn quote_rule_and_fence_are_transformed() {
        let q = on("> hello");
        assert_eq!(q.kind, DisplayKind::Quote);
        assert_eq!(q.text, "hello");
        assert!(q.italic);

        let r = on("---");
        assert_eq!(r.kind, DisplayKind::Rule);
        assert_eq!(r.text, "");

        let f = on("```rust");
        assert_eq!(f.kind, DisplayKind::Fence);
        assert_eq!(f.text, "");
    }

    #[test]
    fn page_break_directive_matches_export_parser() {
        let page_break = on("\\newpage");
        assert_eq!(page_break.kind, DisplayKind::PageBreak);
        assert!(page_break.text.is_empty());

        let literal = off("\\newpage");
        assert_eq!(literal.kind, DisplayKind::Paragraph);
        assert_eq!(literal.text, "\\newpage");

        let fenced = display_line("\\newpage", true, true);
        assert_eq!(fenced.kind, DisplayKind::Code);
        assert_eq!(fenced.text, "\\newpage");
    }

    #[test]
    fn fence_state_tracks_across_lines() {
        let lines: Vec<String> = ["```rust", "let x = # 1;", "```"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let dls = build_display_lines(&lines, true);
        assert_eq!(dls[0].kind, DisplayKind::Fence);
        assert_eq!(dls[1].kind, DisplayKind::Code);
        assert!(dls[1].mono);
        assert_eq!(dls[1].text, "let x = # 1;");
        assert_eq!(dls[2].kind, DisplayKind::Fence);
        // Source offsets stay absolute across the block.
        assert_eq!(dls[2].src_offset, lines[0].len() + 1 + lines[1].len() + 1);
        // OFF keeps fence state as a highlighter flag, display literal.
        let dls_off = build_display_lines(&lines, false);
        assert!(dls_off[1].fenced);
        assert_eq!(dls_off[1].text, "let x = # 1;");
    }

    #[test]
    fn all_six_heading_levels_get_metrics() {
        for level in 1..=6u8 {
            let line = format!("{} text", "#".repeat(level as usize));
            let dl = on(&line);
            assert_eq!(dl.kind, DisplayKind::Heading, "level {level}");
            assert_eq!(dl.text, "text");
            let (size, before, after) = heading_metrics(level);
            assert_eq!(dl.font_size, Some(size), "level {level}");
            assert!(before >= after);
        }
        // Seven hashes is not a heading — export keeps it literal, and
        // so must the canvas.
        let dl = on("####### not");
        assert_eq!(dl.kind, DisplayKind::Paragraph);
        assert_eq!(dl.text, "####### not");
    }

    #[test]
    fn mapping_round_trips_across_constructs() {
        for line in [
            "# h",
            "###### deep heading",
            "> quoted",
            "- item",
            "1. ordered",
            "- [ ] todo",
            "plain text",
            "```",
            "",
            "  ## indented",
            "***",
        ] {
            let dl = display_line(line, false, true);
            // Display end maps to source end (trailing markers included);
            // fully hidden lines (fences, rules) have no display end to map.
            if !dl.text.is_empty() {
                assert_eq!(dl.disp_to_src(dl.text.len()), line.len(), "line: {line:?}");
            }
            for d in 0..=dl.text.len() {
                if !dl.text.is_char_boundary(d) {
                    continue;
                }
                let s = dl.disp_to_src(d);
                assert!(s <= line.len(), "line: {line:?}, d: {d}");
                // Re-mapping lands at or before the same spot (markers
                // clamp to their display start).
                assert!(dl.src_to_disp(s) <= d, "line: {line:?}, d: {d}");
            }
            for s in 0..=line.len() {
                let d = dl.src_to_disp(s);
                assert!(d <= dl.text.len(), "line: {line:?}, s: {s}");
                assert!(dl.disp_to_src(d) <= s, "line: {line:?}, s: {s}");
            }
        }
    }

    #[test]
    fn inline_markers_are_hidden_and_styled() {
        use sylph_core::document::SpanStyle;
        let dl = on("a **b** c");
        assert_eq!(dl.text, "a b c");
        assert_eq!(dl.styles, vec![(2..3, vec![SpanStyle::Bold])]);
        // The caret maps across the hidden markers like Word's formatting:
        // before the bold text it sits before `**` (typing stays plain),
        // after it, before the closing `**` (typing continues the bold).
        assert_eq!(dl.src_to_disp(4), 2);
        assert_eq!(dl.disp_to_src(2), 2);
        assert_eq!(dl.disp_to_src(3), 5);

        let dl = on("see [docs](https://x.io) now");
        assert_eq!(dl.text, "see docs now");
        assert_eq!(
            dl.styles,
            vec![(4..8, vec![SpanStyle::Link("https://x.io".into())])]
        );

        // An escape drops only its backslash; code keeps inner markers.
        let dl = on("\\*lit\\*");
        assert_eq!(dl.text, "*lit*");
        assert!(dl.styles.is_empty());
        let dl = on("`a*b`");
        assert_eq!(dl.text, "a*b");
        assert_eq!(dl.styles, vec![(0..3, vec![SpanStyle::Code])]);

        // Headings and list items get the same treatment.
        let dl = on("# **Bold** title");
        assert_eq!(
            (dl.text.as_str(), dl.kind),
            ("Bold title", DisplayKind::Heading)
        );
        assert_eq!(dl.styles, vec![(0..4, vec![SpanStyle::Bold])]);
        let dl = on("- *it*");
        assert_eq!(dl.text, "• it");
        assert_eq!(
            dl.styles,
            vec![("• ".len().."• it".len(), vec![SpanStyle::Italic])]
        );

        // Markdown OFF stays literal.
        let off = display_line("a **b**", false, false);
        assert_eq!(off.text, "a **b**");
        assert!(off.styles.is_empty());
    }

    #[test]
    fn pipe_table_rows_stay_raw_text() {
        let lines: Vec<String> = [
            "| **a** | b |",
            "|---|---|",
            "| c | *d* |",
            "",
            "after **x**",
        ]
        .map(String::from)
        .to_vec();
        let dls = build_display_lines(&lines, true);
        assert_eq!(dls[0].text, "| **a** | b |");
        assert_eq!(dls[2].text, "| c | *d* |");
        assert!(dls[0].styles.is_empty());
        // The table ends at the blank line; later lines render inline.
        assert_eq!(dls[4].text, "after x");
    }

    #[test]
    fn only_a_paragraph_line_starts_a_table() {
        // The parser checks headings, list items, quotes and images before
        // tables, so none of them starts one: the rows under them are an
        // ordinary paragraph, and the canvas must style them like one.
        for first in ["# a | b", "- a | b", "> a | b", "![a | b](p.png)"] {
            let lines: Vec<String> = [first, "|---|---|", "| **x** | y |"]
                .map(String::from)
                .to_vec();
            let blocks = parse_content_blocks(&lines.join("\n"), 1.0);
            assert!(
                !blocks.iter().any(|b| matches!(b, doc::Block::Table { .. })),
                "export made a table under {first:?}"
            );
            assert_eq!(
                build_display_lines(&lines, true)[2].text,
                "| x | y |",
                "canvas rows under {first:?}"
            );
        }
        // A paragraph line does start one, and its rows stay raw text.
        let lines: Vec<String> = ["a | b", "|---|---|", "| **x** | y |"]
            .map(String::from)
            .to_vec();
        assert_eq!(build_display_lines(&lines, true)[2].text, "| **x** | y |");
    }

    #[test]
    fn canvas_shows_exactly_the_text_and_styles_the_export_keeps() {
        use sylph_core::document::SpanStyle;
        fn per_byte(len: usize, spans: &[(Range<usize>, Vec<SpanStyle>)]) -> Vec<Vec<SpanStyle>> {
            let mut out = vec![Vec::new(); len];
            for (range, styles) in spans {
                for byte in range.clone() {
                    out[byte].extend(styles.iter().cloned());
                }
            }
            out
        }
        let lines: Vec<String> = include_str!("../../../fixtures/kitchen-sink.md")
            .lines()
            .map(String::from)
            .collect();
        let dls = build_display_lines(&lines, true);
        let mut checked = 0;
        for ((line, dl), table) in lines.iter().zip(&dls).zip(table_rows(&lines, &dls)) {
            // What the export reads for this line's inline Markdown.
            let content = match dl.kind {
                DisplayKind::Paragraph if !table && standalone_image(line.trim_end()).is_none() => {
                    line.as_str()
                }
                DisplayKind::Heading => heading_level_and_text(line.trim()).unwrap().1,
                _ => continue,
            };
            let runs = parse_inline_runs(content);
            let text: String = runs.iter().map(|r| r.text.as_str()).collect();
            assert_eq!(dl.text, text, "text of {line:?}");
            let mut spans = Vec::new();
            let mut at = 0;
            for run in &runs {
                spans.push((at..at + run.text.len(), run.styles.clone()));
                at += run.text.len();
            }
            assert_eq!(
                per_byte(dl.text.len(), &dl.styles),
                per_byte(text.len(), &spans),
                "styles of {line:?}"
            );
            checked += 1;
        }
        assert!(checked >= 5, "only {checked} fixture lines checked");
    }

    #[test]
    fn heading_runs_are_bold_and_cover_text() {
        let dl = on("# Title");
        let runs = display_runs(
            &dl,
            0..dl.text.len(),
            gpui::font("monospace"),
            hsla(0., 0., 0.2, 1.0),
        );
        let total: usize = runs.iter().map(|r| r.len).sum();
        assert_eq!(total, dl.text.len());
        let bold = gpui::font("monospace").bold();
        assert!(runs.iter().all(|r| r.font.weight == bold.weight));
    }

    #[test]
    fn word_count_never_counts_markdown_tokens() {
        use crate::ui::count_words;
        // The screenshot case: 10 whitespace tokens, but `#` is a markdown
        // token — 9 words.
        assert_eq!(
            count_words("# working as a header 1\nworks like a charm"),
            9
        );
        assert_eq!(count_words("## Three hashes ##"), 2);
        assert_eq!(count_words("**bold** and ~~strike~~"), 3);
        assert_eq!(count_words("# ## ###"), 0);
        assert_eq!(count_words(""), 0);
    }

    #[test]
    fn status_counts_logical_lines_and_char_columns() {
        use crate::ui::cursor_status;
        let content = "# working as a header 1\nworks like a charm";
        // Cursor at the end of line 2 → Ln 2, Col 19 (characters, not
        // bytes), 9 words.
        assert_eq!(cursor_status(content, content.len()), (2, 19, 9));
        // Blank lines are logical lines; soft wraps never add lines.
        let blank = "one\n\n\ntwo";
        assert_eq!(cursor_status(blank, blank.len()), (4, 4, 2));
        // Multi-byte characters count as one column.
        let emoji = "ab\u{1F642}cd";
        assert_eq!(cursor_status(emoji, emoji.len()).0, 1);
        assert_eq!(cursor_status(emoji, emoji.len()).1, 6);
    }

    #[test]
    fn block_status_counts_logical_blocks() {
        use crate::ui::block_status;
        // A soft-wrapped paragraph is one block; Ln would have said 3.
        let p = "one\ntwo\nthree";
        assert_eq!(block_status(p, p.len()), (1, 1));

        let doc = "# H\n\npara a\npara b\n\n- x\n- y";
        // Caret inside a soft-wrapped paragraph → that paragraph's block.
        assert_eq!(block_status(doc, doc.find("para b").unwrap()), (2, 3));
        // The list is one block, whatever line the caret sits on.
        assert_eq!(block_status(doc, doc.len()), (3, 3));
        // A blank line reports the block before it.
        assert_eq!(block_status(doc, 3), (1, 3));

        // A table header only counts as a table once its delimiter row is
        // read, so the counter looks one line ahead.
        let t = "intro\na | b\n--- | ---\n1 | 2";
        assert_eq!(block_status(t, t.find("a | b").unwrap()), (2, 2));

        // An unclosed fence still flushes its code block at end of text.
        let fenced = "```\ncode\n```\nafter";
        assert_eq!(block_status(fenced, fenced.find("code").unwrap()), (1, 2));

        assert_eq!(block_status("", 0), (1, 1));
        // A byte offset inside a multi-byte character must not panic.
        assert_eq!(block_status("a\u{1F642}b", 2), (1, 1));
    }

    #[test]
    fn caret_text_page_counts_breaks_above_the_caret_line() {
        use crate::ui::caret_text_page;
        let text = "a\n\\newpage\n\nb";
        assert_eq!(caret_text_page(text, 0), 1);
        // The break line itself still ends page 1.
        assert_eq!(caret_text_page(text, text.find("\\newpage").unwrap()), 1);
        // The blank line after the break is already on page 2.
        assert_eq!(caret_text_page(text, text.find("\n\nb").unwrap() + 1), 2);
        assert_eq!(caret_text_page(text, text.len()), 2);
        // A `\newpage` inside a code fence is text, not a break.
        let fenced = "```\n\\newpage\n```\nb";
        assert_eq!(caret_text_page(fenced, fenced.len()), 1);
        // A byte offset inside a multi-byte character must not panic.
        assert_eq!(caret_text_page("\u{1F642}", 1), 1);
    }

    #[test]
    fn block_status_lookahead_needs_a_real_table() {
        use crate::ui::block_status;
        // A bare `---` passes `is_table_delimiter`, but under a pipe-less
        // line it is a thematic break: the caret line keeps its own block.
        let rule = "Intro\n---\nMore";
        assert_eq!(block_status(rule, 0), (1, 3));
        // A heading containing a pipe is still a heading (that branch runs
        // before the table check), so the row below it forms no table.
        let heading = "# a | b\n--- | ---";
        assert_eq!(block_status(heading, 0), (1, 2));
        // The real header case still looks ahead.
        let table = "intro\na | b\n--- | ---";
        assert_eq!(block_status(table, table.find("a | b").unwrap()), (2, 2));
    }

    #[test]
    fn formatting_commands_refuse_while_markdown_is_off() {
        // The refusal policy: with Markdown OFF the guard blocks every
        // formatting command, so content is left byte-identical instead of
        // gaining a literal `#` or `**` that would stay literal on canvas
        // and in export.
        //
        // This covers the guard's decision, not the full command path:
        // building a `SylphApp` needs a gpui test context plus storage,
        // which this workspace has no harness for yet. Every command
        // calls this guard as its first statement.
        assert_eq!(
            SylphApp::markdown_required_message(false),
            Some("Turn on Markdown to use formatting")
        );
        assert_eq!(SylphApp::markdown_required_message(true), None);
    }
}

#[cfg(test)]
mod save_state_tests {
    use super::SaveState;

    #[test]
    fn save_state_reports_the_real_result() {
        assert_eq!(SaveState::from_result(Ok(())), SaveState::Saved);
        let failed = SaveState::from_result(Err("disk full".into()));
        assert_eq!(failed, SaveState::Failed("disk full".to_string()));
        // A failure says why instead of claiming the changes were saved.
        assert_eq!(failed.label(), "Save failed: disk full");
    }

    #[test]
    fn save_state_labels_use_word_docs_wording() {
        assert_eq!(SaveState::Saved.label(), "All changes saved");
        assert_eq!(SaveState::Saving.label(), "Saving…");
    }
}

#[cfg(test)]
mod model_persistence_tests {
    use super::{doc, load_model_or_default};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use sylph_storage::Storage;

    /// A private database and recovered dir per test: parallel tests never
    /// share state and the real user data dir stays untouched.
    fn temp_dir() -> std::path::PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "sylph-model-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ))
    }

    #[test]
    fn saved_model_loads_back() {
        let dir = temp_dir();
        let storage = Storage::open_in(&dir).unwrap();
        let id = storage.create_document("Report").unwrap();
        let mut model = doc::Document::new();
        model.set_page_size(doc::PageSize::Letter);
        model.push_block(doc::Block::table(2, 2));
        let json = serde_json::to_string(&model).unwrap();
        storage.save_model(id, &json).unwrap();

        let (loaded, warning) = load_model_or_default(&storage, id, &dir.join("recovered"));
        assert_eq!(loaded, model);
        assert_eq!(warning, None);
    }

    #[test]
    fn document_without_a_model_gets_defaults_silently() {
        let dir = temp_dir();
        let storage = Storage::open_in(&dir).unwrap();
        let id = storage.create_document("Older document").unwrap();
        let (loaded, warning) = load_model_or_default(&storage, id, &dir.join("recovered"));
        assert_eq!(loaded, doc::Document::new());
        assert_eq!(warning, None);
    }

    #[test]
    fn unreadable_model_is_kept_before_defaults_take_over() {
        let dir = temp_dir();
        let storage = Storage::open_in(&dir).unwrap();
        let id = storage.create_document("From a newer Sylph").unwrap();
        let json = r#"{"blocks":[{"Hologram":{}}]}"#;
        storage.save_model(id, json).unwrap();

        let recovered = dir.join("recovered");
        let (loaded, warning) = load_model_or_default(&storage, id, &recovered);
        assert_eq!(loaded, doc::Document::new());
        let warning = warning.expect("an unreadable model must be reported");
        assert!(
            warning.contains("a copy was kept as recovered/"),
            "{warning}"
        );
        // The original JSON survives byte-for-byte.
        let kept: Vec<_> = std::fs::read_dir(&recovered).unwrap().collect();
        assert_eq!(kept.len(), 1);
        let path = kept[0].as_ref().unwrap().path();
        assert_eq!(std::fs::read_to_string(path).unwrap(), json);
    }
}
