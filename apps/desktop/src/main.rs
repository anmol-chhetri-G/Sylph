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
        let (Some(bounds), Some(line_height)) = (self.last_bounds, Some(self.line_height)) else {
            return;
        };
        if line_height <= px(0.0) || self.all_lines.is_empty() {
            return;
        }

        let cursor = self.cursor_offset().min(self.content.len());
        let mut line_index = 0;
        for (index, offset) in self.line_char_offsets.iter().enumerate() {
            if *offset > cursor {
                break;
            }
            line_index = index;
        }

        let line_top = line_height * line_index as f32;
        let line_bottom = line_top + line_height;
        let visible_height = bounds.size.height;
        let content_height = line_height * self.all_lines.len() as f32;
        let max_scroll = (content_height - visible_height).max(px(0.0));
        let mut scroll = self.scroll_offset_y;

        if line_top < scroll {
            scroll = line_top;
        } else if line_bottom > scroll + visible_height {
            scroll = line_bottom - visible_height;
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
        let line_height = if self.line_height > px(0.0) {
            self.line_height
        } else {
            px(20.0)
        };
        let local_y = position.y - bounds.top() + self.scroll_offset_y;
        if local_y < px(0.0) {
            return 0;
        }
        let line_idx = (local_y / line_height).floor().max(0.0) as usize;
        if line_idx >= self.all_lines.len() {
            return self.content.len();
        }
        let line = &self.all_lines[line_idx];
        let char_offset = self.line_char_offsets.get(line_idx).copied().unwrap_or(0);
        let local_x = (position.x - bounds.left()).max(px(0.0));
        let local_idx = line.closest_index_for_x(local_x);
        snap_to_char_boundary(
            &self.content,
            (char_offset + local_idx).min(self.content.len()),
        )
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

        let total_content_height = px(line_height * self.all_lines.len() as f32);
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

    fn markdown_runs(line: &str, font: gpui::Font, base_color: gpui::Hsla) -> Vec<TextRun> {
        let mut runs = Vec::new();
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
        let color_bold = hsla(0.0, 0.0, 0.15, 1.0);
        let color_italic = hsla(0.0, 0.0, 0.35, 1.0);
        let color_code = hsla(120.0 / 360.0, 0.5, 0.35, 1.0);
        let color_link = hsla(210.0 / 360.0, 0.8, 0.45, 1.0);
        let color_list = hsla(30.0 / 360.0, 0.7, 0.45, 1.0);
        let color_quote = hsla(0.0, 0.0, 0.5, 1.0);

        if trimmed.starts_with("# ") {
            runs.push(TextRun {
                len: 2,
                font: font.clone(),
                color: color_header,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            runs.push(TextRun {
                len: trimmed.len() - 2,
                font: font.clone(),
                color: color_header,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            return runs;
        }
        if trimmed.starts_with("## ") {
            runs.push(TextRun {
                len: 3,
                font: font.clone(),
                color: color_header,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            runs.push(TextRun {
                len: trimmed.len() - 3,
                font: font.clone(),
                color: color_header,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            return runs;
        }
        if trimmed.starts_with("### ") {
            runs.push(TextRun {
                len: 4,
                font: font.clone(),
                color: color_header,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            runs.push(TextRun {
                len: trimmed.len() - 4,
                font: font.clone(),
                color: color_header,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            return runs;
        }
        if trimmed.starts_with("#### ")
            || trimmed.starts_with("##### ")
            || trimmed.starts_with("###### ")
        {
            let prefix_len = trimmed.find(' ').unwrap_or(0) + 1;
            runs.push(TextRun {
                len: prefix_len,
                font: font.clone(),
                color: color_header,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            runs.push(TextRun {
                len: trimmed.len() - prefix_len,
                font: font.clone(),
                color: color_header,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            return runs;
        }

        if trimmed.starts_with("> ") {
            runs.push(TextRun {
                len: 2,
                font: font.clone(),
                color: color_quote,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            runs.push(TextRun {
                len: trimmed.len() - 2,
                font: font.clone(),
                color: color_quote,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            return runs;
        }

        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            runs.push(TextRun {
                len: 2,
                font: font.clone(),
                color: color_list,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            runs.push(TextRun {
                len: trimmed.len() - 2,
                font: font.clone(),
                color: base_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
            return runs;
        }

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

        let mut chars = trimmed.char_indices().peekable();
        let mut seg_start = 0;
        let mut in_bold = false;
        let mut in_italic = false;
        let mut in_code = false;

        while let Some((i, ch)) = chars.next() {
            if ch == '`' {
                let seg_end = i;
                if seg_end > seg_start {
                    let color = if in_code {
                        color_code
                    } else if in_bold {
                        color_bold
                    } else if in_italic {
                        color_italic
                    } else {
                        base_color
                    };
                    runs.push(TextRun {
                        len: seg_end - seg_start,
                        font: font.clone(),
                        color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    });
                }
                in_code = !in_code;
                seg_start = i;
                if !in_code {
                    runs.push(TextRun {
                        len: 1,
                        font: font.clone(),
                        color: color_code,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    });
                    seg_start = i + 1;
                }
            } else if !in_code && ch == '*' && chars.peek() == Some(&(i + 1, '*')) {
                let seg_end = i;
                if seg_end > seg_start {
                    let color = if in_bold {
                        color_bold
                    } else if in_italic {
                        color_italic
                    } else {
                        base_color
                    };
                    runs.push(TextRun {
                        len: seg_end - seg_start,
                        font: font.clone(),
                        color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    });
                }
                in_bold = !in_bold;
                chars.next();
                seg_start = i + 2;
            } else if !in_code && !in_bold && ch == '*' {
                let seg_end = i;
                if seg_end > seg_start {
                    let color = if in_italic { color_italic } else { base_color };
                    runs.push(TextRun {
                        len: seg_end - seg_start,
                        font: font.clone(),
                        color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    });
                }
                in_italic = !in_italic;
                seg_start = i + 1;
            } else if !in_code && (ch == '[' || ch == ']') {
                let seg_end = i;
                if seg_end > seg_start {
                    runs.push(TextRun {
                        len: seg_end - seg_start,
                        font: font.clone(),
                        color: base_color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    });
                }
                runs.push(TextRun {
                    len: 1,
                    font: font.clone(),
                    color: color_link,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                });
                seg_start = i + 1;
            }
        }

        if seg_start < trimmed.len() {
            let color = if in_code {
                color_code
            } else if in_bold {
                color_bold
            } else if in_italic {
                color_italic
            } else {
                base_color
            };
            runs.push(TextRun {
                len: trimmed.len() - seg_start,
                font: font.clone(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }

        if runs.is_empty() {
            runs.push(TextRun {
                len: line.len(),
                font: font.clone(),
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
        let line_count = self.all_lines.len() as f32;
        let line_height_f: f32 = self.line_height.into();
        let total_content_height = line_height_f * line_count;
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
        let text = self.content.clone();
        let _ = std::fs::create_dir_all("output");
        let _ = std::fs::write("output/document.txt", &text);
        let _ = self.storage.save_text(self.doc_id, &text);
        cx.notify();
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        self.save_task.take();
        let doc_id = self.doc_id;
        let text = self.content.clone();
        self.save_task = Some(cx.spawn(
            move |_this: WeakEntity<TextInput>, _cx: &mut gpui::AsyncApp| async move {
                // Keep writes out of the typing path while saving shortly after
                // the user pauses. Dropping the previous task debounces bursts.
                gpui::Timer::after(std::time::Duration::from_millis(750)).await;
                let _ = std::fs::create_dir_all("output");
                let _ = std::fs::write("output/document.txt", &text);
                if let Ok(storage) = sylph_storage::Storage::open() {
                    let _ = storage.save_text(doc_id, &text);
                }
            },
        ));
    }

    fn summarize(&mut self, _: &Summarize, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.content.clone();
        let summary = sylph_py_bridge::summarize_text(&text);
        let _ = std::fs::create_dir_all("output");
        let _ = std::fs::write("output/summary.txt", &summary);
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

struct PrepaintState {
    lines: Vec<ShapedLine>,
    line_numbers: Vec<ShapedLine>,
    gutter_width: gpui::Pixels,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
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
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = &input.content;
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let scroll_offset_y = input.scroll_offset_y;
        let show_line_numbers = input.show_line_numbers;
        let word_wrap = input.word_wrap;
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();

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

        let mut shaped_lines = Vec::new();
        let mut visual_char_offsets = Vec::new();
        let mut char_offset = 0;
        let mut cursor_line = 0;
        let mut cursor_x_in_line = px(0.0);
        let mut sel_start_line = 0;
        let mut sel_start_x = px(0.0);
        let mut sel_end_line = 0;
        let mut sel_end_x = px(0.0);

        let available_width = bounds.size.width - gutter_width;

        for (i, line_text) in lines.iter().enumerate() {
            let line_start = char_offset;
            let line_end = char_offset + line_text.len();

            if word_wrap && available_width > px(0.0) && !line_text.is_empty() {
                let mut remaining = line_text.as_str();
                let mut local_offset = 0usize;

                while !remaining.is_empty() {
                    let check_run = TextRun {
                        len: remaining.len(),
                        font: style.font(),
                        color: text_color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let shaped = window.text_system().shape_line(
                        remaining.to_string().into(),
                        font_size,
                        &[check_run],
                        None,
                    );

                    visual_char_offsets.push(line_start + local_offset);

                    if shaped.width > available_width && remaining.len() > 1 {
                        let break_idx = shaped.closest_index_for_x(available_width);
                        let actual_break = if break_idx > 0 {
                            let slice = &remaining[..break_idx];
                            slice.rfind(' ').map(|p| p + 1).unwrap_or(break_idx)
                        } else {
                            1
                        };

                        let segment = &remaining[..actual_break];
                        let seg_runs = TextInput::markdown_runs(segment, style.font(), text_color);
                        let seg_shaped = window.text_system().shape_line(
                            segment.to_string().into(),
                            font_size,
                            &seg_runs,
                            None,
                        );

                        let vis_idx = shaped_lines.len();
                        let seg_start = line_start + local_offset;
                        let seg_end = seg_start + segment.len();

                        if cursor >= seg_start && cursor <= seg_end {
                            cursor_line = vis_idx;
                            cursor_x_in_line = seg_shaped.x_for_index(cursor - seg_start);
                        }
                        if !selected_range.is_empty() {
                            let sel_start = selected_range.start;
                            let sel_end = selected_range.end;
                            if sel_start >= seg_start && sel_start < seg_end {
                                sel_start_line = vis_idx;
                                sel_start_x = seg_shaped.x_for_index(sel_start - seg_start);
                            } else if sel_start >= seg_end
                                && i == lines.len() - 1
                                && actual_break >= remaining.len()
                            {
                                sel_start_line = vis_idx;
                                sel_start_x = seg_shaped.x_for_index(segment.len());
                            }
                            if sel_end > seg_start && sel_end <= seg_end {
                                sel_end_line = vis_idx;
                                sel_end_x = seg_shaped.x_for_index(sel_end - seg_start);
                            } else if sel_end > seg_end
                                && i == lines.len() - 1
                                && actual_break >= remaining.len()
                            {
                                sel_end_line = vis_idx;
                                sel_end_x = seg_shaped.x_for_index(segment.len());
                            }
                        }

                        shaped_lines.push(seg_shaped);
                        remaining = &remaining[actual_break..];
                        local_offset += actual_break;
                    } else {
                        let vis_idx = shaped_lines.len();
                        if cursor >= line_start && cursor <= line_end {
                            cursor_line = vis_idx;
                            cursor_x_in_line = shaped.x_for_index(cursor - line_start);
                        }
                        if !selected_range.is_empty() {
                            let sel_start = selected_range.start;
                            let sel_end = selected_range.end;
                            if sel_start >= line_start && sel_start < line_end {
                                sel_start_line = vis_idx;
                                sel_start_x = shaped.x_for_index(sel_start - line_start);
                            } else if sel_start >= line_end && i == lines.len() - 1 {
                                sel_start_line = vis_idx;
                                sel_start_x = shaped.x_for_index(line_text.len());
                            }
                            if sel_end > line_start && sel_end <= line_end {
                                sel_end_line = vis_idx;
                                sel_end_x = shaped.x_for_index(sel_end - line_start);
                            } else if sel_end > line_end && i == lines.len() - 1 {
                                sel_end_line = vis_idx;
                                sel_end_x = shaped.x_for_index(line_text.len());
                            }
                        }
                        shaped_lines.push(shaped);
                        remaining = "";
                    }
                }
            } else {
                let runs = TextInput::markdown_runs(line_text, style.font(), text_color);
                let shaped = window.text_system().shape_line(
                    line_text.clone().into(),
                    font_size,
                    &runs,
                    None,
                );

                visual_char_offsets.push(line_start);

                if cursor >= line_start && cursor <= line_end {
                    cursor_line = i;
                    cursor_x_in_line = shaped.x_for_index(cursor - line_start);
                }
                if !selected_range.is_empty() {
                    let sel_start = selected_range.start;
                    let sel_end = selected_range.end;
                    if sel_start >= line_start && sel_start < line_end {
                        sel_start_line = i;
                        sel_start_x = shaped.x_for_index(sel_start - line_start);
                    } else if sel_start >= line_end && i == lines.len() - 1 {
                        sel_start_line = i;
                        sel_start_x = shaped.x_for_index(line_text.len());
                    }
                    if sel_end > line_start && sel_end <= line_end {
                        sel_end_line = i;
                        sel_end_x = shaped.x_for_index(sel_end - line_start);
                    } else if sel_end > line_end && i == lines.len() - 1 {
                        sel_end_line = i;
                        sel_end_x = shaped.x_for_index(line_text.len());
                    }
                }
                shaped_lines.push(shaped);
            }

            char_offset += line_text.len() + 1;
        }

        let char_offsets: Vec<usize> = visual_char_offsets;

        let (selection, cursor_quad) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(
                            bounds.left() + gutter_width + cursor_x_in_line,
                            bounds.top() + line_height * cursor_line as f32 - scroll_offset_y,
                        ),
                        size(px(1.), line_height),
                    ),
                    gpui::blue(),
                )),
            )
        } else {
            let top_line = sel_start_line.min(sel_end_line);
            let bot_line = sel_start_line.max(sel_end_line);
            let left_x = if sel_start_line <= sel_end_line {
                sel_start_x
            } else {
                sel_end_x
            };
            let right_x = if sel_start_line <= sel_end_line {
                sel_end_x
            } else {
                sel_start_x
            };

            if top_line == bot_line {
                (
                    Some(fill(
                        Bounds::from_corners(
                            point(
                                bounds.left() + gutter_width + left_x,
                                bounds.top() + line_height * top_line as f32 - scroll_offset_y,
                            ),
                            point(
                                bounds.left() + gutter_width + right_x,
                                bounds.top() + line_height * (top_line as f32 + 1.0)
                                    - scroll_offset_y,
                            ),
                        ),
                        rgba(0x3311ff30),
                    )),
                    None,
                )
            } else {
                (
                    Some(fill(
                        Bounds::from_corners(
                            point(
                                bounds.left() + gutter_width + left_x,
                                bounds.top() + line_height * top_line as f32 - scroll_offset_y,
                            ),
                            point(
                                bounds.right(),
                                bounds.top() + line_height * (bot_line as f32 + 1.0)
                                    - scroll_offset_y,
                            ),
                        ),
                        rgba(0x3311ff30),
                    )),
                    None,
                )
            }
        };

        let last_line = shaped_lines.last().cloned();
        self.input.update(cx, |input, _cx| {
            input.last_layout = last_line;
            input.all_lines = shaped_lines.clone();
            input.line_char_offsets = char_offsets;
            input.line_height = line_height;
        });

        let line_numbers = if show_line_numbers {
            let number_color = hsla(0., 0., 0.4, 0.5);
            (0..line_count)
                .map(|i| {
                    let num_str = format!("{}", i + 1);
                    let run = TextRun {
                        len: num_str.len(),
                        font: style.font(),
                        color: number_color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    window
                        .text_system()
                        .shape_line(num_str.into(), font_size, &[run], None)
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        PrepaintState {
            lines: shaped_lines,
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
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let line_height = window.line_height();
        let gutter_width = prepaint.gutter_width;

        for (i, num) in prepaint.line_numbers.iter().enumerate() {
            let y = bounds.top() + line_height * i as f32 - scroll_offset_y;
            let num_w = num.width;
            let x = bounds.left() + gutter_width - num_w - px(8.0);
            num.paint(point(x, y), line_height, window, cx).ok();
        }

        for (i, line) in prepaint.lines.iter().enumerate() {
            let y = bounds.top() + line_height * i as f32 - scroll_offset_y;
            line.paint(
                point(bounds.left() + gutter_width, y),
                line_height,
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
        let last_line = prepaint.lines.last().cloned();
        let last_bounds = Bounds::new(
            point(bounds.left() + gutter_width, bounds.top()),
            size(bounds.size.width - gutter_width, bounds.size.height),
        );
        self.input.update(cx, |input, _cx| {
            input.last_layout = last_line;
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

struct SylphApp {
    editor: Entity<TextInput>,
    document: sylph_core::document::Document,
    focus_handle: FocusHandle,
    sidebar_visible: bool,
    preview_visible: bool,
    find: FindReplaceState,
    context_menu: ContextMenuState,
    doc_title: String,
    editing_title: bool,
    documents: Vec<(i64, String)>,
    dark_mode: bool,
    ai_panel: AiPanelState,
    status_message: Option<String>,
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
        TogglePreview,
        AddCoverPage,
        PasteImage,
        BoldText,
        ItalicText,
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
    ]
);

impl SylphApp {
    fn save_doc(&mut self, _: &SaveDoc, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let text = editor.content.clone();
            let _ = std::fs::create_dir_all("output");
            let _ = std::fs::write("output/document.txt", &text);
            let _ = editor.storage.save_text(editor.doc_id, &text);
            cx.notify();
        });
    }

    fn summarize_doc(&mut self, _: &SummarizeDoc, _window: &mut Window, cx: &mut Context<Self>) {
        let text = self.editor.read(cx).content.clone();
        let summary = sylph_py_bridge::summarize_text(&text);
        let _ = std::fs::create_dir_all("output");
        let _ = std::fs::write("output/summary.txt", &summary);
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
            .list_documents()
            .unwrap_or_default();
    }

    fn save_current_document(&mut self, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, _cx| {
            let text = editor.content.clone();
            let _ = editor.storage.save_text(editor.doc_id, &text);
        });
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

        self.editor.update(cx, |editor, cx| {
            editor.doc_id = doc_id;
            editor.content = saved.clone();
            editor.selected_range = 0..0;
            editor.selection_reversed = false;
            editor.preferred_column = None;
            editor.scroll_offset_y = px(0.0);
            editor.undo_stack.clear();
            editor.redo_stack.clear();
            cx.notify();
        });
        // Structured blocks are not persisted with the legacy text-only storage yet.
        // Reset the transient block list when changing documents so a page break,
        // table, or image cannot leak into the next document.
        self.document = sylph_core::document::Document::new();
        self.doc_title = doc_title;
    }

    fn new_document(&mut self, _: &NewDocument, _window: &mut Window, cx: &mut Context<Self>) {
        self.save_current_document(cx);
        let doc_id = self
            .editor
            .read(cx)
            .storage
            .create_document("Untitled")
            .unwrap_or(1);
        self.load_document_by_id(doc_id, cx);
        self.load_documents(cx);
        cx.notify();
    }

    fn switch_document(&mut self, doc_id: i64, _window: &mut Window, cx: &mut Context<Self>) {
        let current_id = self.editor.read(cx).doc_id;
        if doc_id == current_id {
            return;
        }
        self.save_current_document(cx);
        self.load_document_by_id(doc_id, cx);
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

    fn export_document(&mut self, to_pdf: bool, cx: &mut Context<Self>) {
        let text = self.editor.read(cx).content.clone();
        let ext = if to_pdf { "pdf" } else { "docx" };
        let path = format!("output/{}.{}", self.doc_title.replace(' ', "_"), ext);
        let _ = std::fs::create_dir_all("output");
        let result = if to_pdf {
            sylph_py_bridge::export_to_pdf(&text, &path)
        } else {
            sylph_py_bridge::export_to_docx(&text, &path)
        };
        self.status_message = Some(result);
        cx.notify();
    }

    fn export_docx(&mut self, _: &ExportDocx, _window: &mut Window, cx: &mut Context<Self>) {
        self.export_document(false, cx);
    }

    fn export_pdf(&mut self, _: &ExportPdf, _window: &mut Window, cx: &mut Context<Self>) {
        self.export_document(true, cx);
    }

    fn toggle_preview(&mut self, _: &TogglePreview, _window: &mut Window, cx: &mut Context<Self>) {
        self.preview_visible = !self.preview_visible;
        cx.notify();
    }

    fn add_cover_page(&mut self, _: &AddCoverPage, _window: &mut Window, cx: &mut Context<Self>) {
        if self.document.has_cover_page() {
            self.document.remove_cover_page();
            self.status_message = Some("Cover page removed".to_string());
        } else {
            let mut cp = sylph_core::document::CoverPageData::with_template(
                sylph_core::document::CoverTemplate::Classic,
            );
            cp.title = self.doc_title.clone();
            self.document.set_cover_page(cp);
            self.status_message = Some("Cover page added (Classic)".to_string());
        }
        cx.notify();
    }

    fn add_image_asset(&mut self, bytes: &[u8], extension: &str, cx: &mut Context<Self>) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let filename = format!("image_{}.{}", timestamp, extension);
        let dir = "output/images";
        let _ = std::fs::create_dir_all(dir);
        let path = format!("{}/{}", dir, filename);
        if std::fs::write(&path, bytes).is_ok() {
            self.document
                .push_block(sylph_core::document::Block::image(&path));
            self.status_message = Some(format!("Image added: {}", filename));
        } else {
            self.status_message = Some("Could not store the selected image".to_string());
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
            self.status_message = Some(format!("Unsupported image type: .{}", extension));
            cx.notify();
            return;
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let filename = format!("image_{}.{}", timestamp, extension);
        let dir = PathBuf::from("output/images");
        let _ = std::fs::create_dir_all(&dir);
        let destination = dir.join(&filename);
        if std::fs::copy(&source, &destination).is_ok() {
            let path = destination.to_string_lossy().into_owned();
            self.document
                .push_block(sylph_core::document::Block::image(&path));
            self.status_message = Some(format!("Image added: {}", filename));
        } else {
            self.status_message = Some("Could not copy the selected image".to_string());
        }
        cx.notify();
    }

    fn open_image_picker(&mut self, cx: &mut Context<Self>) {
        self.status_message = Some("Choose an image file…".to_string());
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
                    let _ = this.update(cx, |this, cx| {
                        this.status_message = Some(message);
                        cx.notify();
                    });
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

    fn bold_text(&mut self, _: &BoldText, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let text = editor.content.clone();
            let sel = editor.selected_range.clone();
            if sel.start < sel.end && sel.end <= text.len() {
                let selected = &text[sel.clone()];
                let new_text = format!("**{}**", selected);
                editor.replace_text_in_range(Some(sel), &new_text, cx);
            }
        });
        cx.notify();
    }

    fn italic_text(&mut self, _: &ItalicText, _window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            let text = editor.content.clone();
            let sel = editor.selected_range.clone();
            if sel.start < sel.end && sel.end <= text.len() {
                let selected = &text[sel.clone()];
                let new_text = format!("*{}*", selected);
                editor.replace_text_in_range(Some(sel), &new_text, cx);
            }
        });
        cx.notify();
    }

    fn set_heading(&mut self, level: u8, _window: &mut Window, cx: &mut Context<Self>) {
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
        self.status_message = Some("Table inserted (3×3)".to_string());
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
        self.status_message = Some("Page break inserted".to_string());
        cx.notify();
    }

    fn set_page_size(&mut self, _: &SetPageSize, _window: &mut Window, cx: &mut Context<Self>) {
        // Toggle between A4 and Letter
        self.document.set_page_size(match self.document.page_size {
            sylph_core::document::PageSize::A4 => sylph_core::document::PageSize::Letter,
            sylph_core::document::PageSize::Letter => sylph_core::document::PageSize::A4,
        });
        self.status_message = Some(format!("Page size: {}", self.document.page_size.name()));
        cx.notify();
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
                                        .map(|(id, title)| {
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

fn main() {
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
                        title: Some("Quarterly Report — Sylph".into()),
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
                    let doc_id = storage.create_document("Untitled").unwrap_or(1);
                    let saved = storage
                        .load_text(doc_id)
                        .unwrap_or(None)
                        .unwrap_or_default();
                    let doc_title = storage
                        .get_title(doc_id)
                        .unwrap_or_else(|_| "Untitled".to_string());

                    let editor = cx.new(|cx| TextInput {
                        focus_handle: cx.focus_handle(),
                        content: saved,
                        placeholder: String::new(),
                        selected_range: 0..0,
                        selection_reversed: false,
                        preferred_column: None,
                        last_layout: None,
                        last_bounds: None,
                        all_lines: Vec::new(),
                        line_char_offsets: Vec::new(),
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
                        SylphApp {
                            editor,
                            document: sylph_core::document::Document::new(),
                            focus_handle: cx.focus_handle(),
                            sidebar_visible: true,
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
                            status_message: None,
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
                            _keystroke_subscription: keystroke_subscription,
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
