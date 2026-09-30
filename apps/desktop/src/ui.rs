use crate::style_controls::Picker;
use crate::{
    AddCoverPage, BoldText, CloseOverlay, ContextMenuCopy, ContextMenuCut, ContextMenuPaste,
    ContextMenuSelectAll, CycleBodyFont, EditingField, ExportFormat, FindAndReplace, FocusMode,
    Heading1, Heading2, Heading3, Heading4, Heading5, Heading6, InsertPageBreak, InsertTable,
    InspectorMode, ItalicText, NavigatorTab, NewDocument, NormalText, OpenCommandPalette,
    OpenExportDialog, OpenFindBar, OpenPageSetup, PasteImage, PrintLayout, Redo, ReplaceAll,
    ReplaceCurrent, SaveDoc, SaveState, ShowImageInspector, ShowParagraphInspector, ShowShortcuts,
    ShowVersionHistory, StrikethroughText, SylphApp, ToggleDarkMode, ToggleInspector,
    ToggleMarkdownMode, ToggleRuler, ToggleSidebar, Undo, WebLayout, WorkspaceOverlay,
};
use gpui::prelude::*;
use gpui::{
    anchored, deferred, div, img, px, rgb, rgba, Context, Div, Focusable, MouseButton, Pixels,
    Rgba, Stateful, Window,
};

pub(crate) const UI_FONT: &str = "Hanken Grotesk";
pub(crate) const PROSE_FONT: &str = "EB Garamond";
pub(crate) const MONO_FONT: &str = "JetBrains Mono";
/// Space between page sheets in print layout.
const PAGE_GAP: Pixels = px(24.0);

/// One entry of a menu-bar menu.
pub(crate) enum MenuEntry {
    /// A command: its label, the action it runs (the one its shortcut runs)
    /// and, for a toggle, whether it is on.
    Item(&'static str, Box<dyn gpui::Action>, Option<bool>),
    Separator,
}

/// Hover tooltip for toolbar buttons: a small dark chip with the action's
/// name (and honest notes on controls that are present but not wired yet).
struct ToolbarTip(&'static str);

impl Render for ToolbarTip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(4.0))
            .bg(rgb(0x1c2333))
            .text_color(rgb(0xe6e9f0))
            .font_family(UI_FONT)
            .text_size(px(11.0))
            .child(self.0)
    }
}

/// Attach a hover tooltip to a toolbar button. GPUI tooltips need a
/// stateful (id'd) element, so the caller supplies a stable id.
fn with_tip(button: Div, id: &'static str, tip: &'static str) -> Stateful<Div> {
    button
        .id(id)
        .tooltip(move |_, cx| cx.new(move |_| ToolbarTip(tip)).into())
}

/// Which view-mode chip is lit. Exactly one: Web wins over Focus, and
/// Focus (both side panels hidden) beats Print, the default layout.
pub(crate) fn view_mode_active(web: bool, sidebar: bool, inspector: bool) -> (bool, bool, bool) {
    if web {
        (false, true, false)
    } else if !sidebar && !inspector {
        (false, false, true)
    } else {
        (true, false, false)
    }
}

/// Ruler marks for a page, in centimeters: even-centimeter ticks across
/// the real width, a tick at the page edge itself, and a marker at each
/// margin edge (they replace any tick they would collide with). Returns
/// sorted `(position_cm, is_margin_edge)` pairs.
fn ruler_marks(width_cm: f32, left_cm: f32, right_cm: f32) -> Vec<(f32, bool)> {
    let mut marks: Vec<(f32, bool)> = Vec::new();
    if width_cm <= 0.0 {
        return marks;
    }
    let near_edge = |cm: f32| (cm - left_cm).abs() < 0.45 || (cm - right_cm).abs() < 0.45;
    let mut cm = 0.0;
    while cm < width_cm - 0.01 {
        if !near_edge(cm) {
            marks.push((cm, false));
        }
        cm += 2.0;
    }
    if !near_edge(width_cm) {
        marks.push((width_cm, false));
    }
    for edge in [left_cm, right_cm] {
        if (0.0..=width_cm).contains(&edge) {
            marks.push((edge, true));
        }
    }
    marks.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    marks
}

/// Words = whitespace-separated tokens containing at least one
/// alphanumeric character. Markdown tokens (`#`, `##`, `**`, `-`) are
/// never counted as words, in either mode.
pub(crate) fn count_words(content: &str) -> usize {
    content
        .split_whitespace()
        .filter(|token| token.chars().any(|c| c.is_alphanumeric()))
        .count()
}

/// Status-bar position at `cursor`: 1-based logical line and character
/// column — logical blocks, never visual wraps, and never raw byte
/// offsets (multi-byte characters count as one column).
pub(crate) fn cursor_status(content: &str, cursor: usize) -> (usize, usize, usize) {
    let mut cursor = cursor.min(content.len());
    while cursor < content.len() && !content.is_char_boundary(cursor) {
        cursor += 1;
    }
    let before = &content[..cursor];
    let line = before.matches('\n').count() + 1;
    let col_start = before.rfind('\n').map(|p| p + 1).unwrap_or(0);
    let column = before[col_start..].chars().count() + 1;
    (line, column, count_words(content))
}

/// Status-bar block position in Markdown mode: 1-based block at the caret
/// and the document's block total, both from the export parser itself —
/// parse up to the end of the caret's line and count, so the counter can
/// never disagree with export. Soft-wrapped lines of one paragraph share a
/// block; a blank line reports the block before it.
pub(crate) fn block_status(content: &str, cursor: usize) -> (usize, usize) {
    // Snap the cursor exactly like `cursor_status` (a raw byte offset may
    // point into the middle of a multi-byte character).
    let mut cursor = cursor.min(content.len());
    while cursor < content.len() && !content.is_char_boundary(cursor) {
        cursor += 1;
    }
    // `line_spacing` never changes how text groups into blocks, so any
    // value counts the same.
    let total = crate::parse_content_blocks(content, 1.0).len().max(1);

    let end = content[cursor..]
        .find('\n')
        .map_or(content.len(), |p| cursor + p);
    // The parser flushes whatever is pending at end of text, so a partly
    // parsed caret block still counts as exactly one block.
    let mut index = crate::parse_content_blocks(&content[..end], 1.0).len();

    // One exception to "the prefix ends at the caret's line": a pipe-table
    // header only becomes a table when its delimiter row follows. Parse one
    // row further, but trust it only if a table really formed — a bare
    // `---` also passes `is_table_delimiter`, and under a pipe-less line or
    // a heading it is a thematic break, not a delimiter row.
    if let Some(rest) = content[end..].strip_prefix('\n') {
        let next = rest.split('\n').next().unwrap_or_default();
        if crate::is_table_delimiter(next) {
            let ahead = crate::parse_content_blocks(&content[..end + 1 + next.len()], 1.0);
            if matches!(
                ahead.last(),
                Some(sylph_core::document::Block::Table { .. })
            ) {
                index = ahead.len();
            }
        }
    }
    (index.clamp(1, total), total)
}

/// "A4 · 210 × 297 mm": the paper format with its real dimensions, derived
/// from the model's points (1 pt = 25.4 / 72 mm) instead of hard-coded.
pub(crate) fn page_format_label(size: &sylph_core::document::PageSize) -> String {
    let (w_pt, h_pt) = size.dimensions();
    let mm = |pt: f32| (pt * 25.4 / 72.0).round();
    format!("{} · {} × {} mm", size.name(), mm(w_pt), mm(h_pt))
}

/// The two lines a document shows in Recent Files. A real title leads, with
/// the time and the start of the text below it; an "Untitled" document
/// leads with the start of its text instead, since that is what identifies
/// it. With Markdown ON the text goes through the export parser, so
/// "# Report" reads "Report", as on the canvas; OFF shows it literally.
pub(crate) fn document_label(
    title: &str,
    body: &str,
    updated: &str,
    markdown_on: bool,
) -> (String, String) {
    let first = first_text(body, markdown_on);
    let title = title.trim();
    let untitled = title.is_empty() || title == "Untitled";
    let name = if !untitled {
        title.to_string()
    } else if !first.is_empty() {
        first.clone()
    } else {
        "Untitled".to_string()
    };
    let detail = if untitled || first.is_empty() {
        updated.to_string()
    } else {
        format!("{updated} · {first}")
    };
    (name, detail)
}

/// The first text a document shows, whitespace collapsed. Only the start
/// is read, so a long document costs nothing extra per frame.
fn first_text(body: &str, markdown_on: bool) -> String {
    let mut end = body.len().min(1024);
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    let start = &body[..end];
    let text = if markdown_on {
        crate::parse_content_blocks(start, 1.0)
            .iter()
            .map(block_plain_text)
            .find(|t| !t.trim().is_empty())
            .unwrap_or_default()
    } else {
        start
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or_default()
            .to_string()
    };
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A block's visible text without Markdown markers (first item or row
/// only for lists and tables; first line for code).
fn block_plain_text(block: &sylph_core::document::Block) -> String {
    use sylph_core::document::Block;
    let join = |runs: &[sylph_core::document::TextRun]| -> String {
        runs.iter().map(|r| r.text.as_str()).collect()
    };
    match block {
        Block::Heading { runs, .. } | Block::Paragraph { runs, .. } | Block::Quote { runs, .. } => {
            join(runs)
        }
        Block::List { items } => items.first().map(|i| join(&i.runs)).unwrap_or_default(),
        Block::CodeBlock { text, .. } => text.lines().next().unwrap_or_default().to_string(),
        Block::Table { data } => data
            .rows
            .first()
            .map(|row| row.iter().map(|c| c.text()).collect::<Vec<_>>().join(" · "))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// 1-based page of the caret within the typed text (Markdown mode): one
/// more than the page breaks the export parser finds above the caret's
/// line. A `\newpage` on the caret's own line still belongs to the page it
/// ends, and one inside a code fence is text — exactly as in export.
pub(crate) fn caret_text_page(content: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(content.len());
    while cursor < content.len() && !content.is_char_boundary(cursor) {
        cursor += 1;
    }
    let line_start = content[..cursor].rfind('\n').map_or(0, |p| p + 1);
    1 + crate::parse_content_blocks(&content[..line_start], 1.0)
        .iter()
        .filter(|b| matches!(b, sylph_core::document::Block::PageBreak))
        .count()
}

pub(crate) fn icon(glyph: &str, color: Rgba, size: f32) -> Div {
    div()
        .font_family("Noto Sans")
        .text_size(px(size))
        .text_color(color)
        .child(glyph.to_string())
}

fn divider(color: Rgba) -> Div {
    div().w(px(1.0)).h(px(16.0)).bg(color)
}

pub(crate) fn label(text: impl Into<String>, color: Rgba, size: f32) -> Div {
    div()
        .font_family(UI_FONT)
        .text_size(px(size))
        .text_color(color)
        .child(text.into())
}

impl SylphApp {
    fn open_command_palette(
        &mut self,
        _: &OpenCommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.overlay = WorkspaceOverlay::CommandPalette;
        self.palette = crate::PaletteState::default();
        // The palette takes the keyboard, so typing filters commands
        // instead of reaching the document.
        window.focus(&self.focus_handle);
        cx.notify();
    }

    fn close_overlay(&mut self, _: &CloseOverlay, window: &mut Window, cx: &mut Context<Self>) {
        if self.overlay == WorkspaceOverlay::CommandPalette {
            self.close_palette(window, cx);
        }
        self.overlay = WorkspaceOverlay::None;
        self.context_menu.visible = false;
        self.open_menu = None;
        if self.find.visible {
            // Hand the keyboard back to the document.
            self.hide_find_bar(window, cx);
        }
        cx.notify();
    }

    fn open_export_dialog(
        &mut self,
        _: &OpenExportDialog,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.overlay = WorkspaceOverlay::Export;
        cx.notify();
    }

    fn open_page_setup(&mut self, _: &OpenPageSetup, _window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = WorkspaceOverlay::PageSetup;
        cx.notify();
    }

    fn show_paragraph_inspector(
        &mut self,
        _: &ShowParagraphInspector,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.inspector_mode = InspectorMode::Paragraph;
        self.inspector_visible = true;
        cx.notify();
    }

    pub(crate) fn show_image_inspector(
        &mut self,
        _: &ShowImageInspector,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.inspector_mode = InspectorMode::Image;
        self.inspector_visible = true;
        cx.notify();
    }

    fn show_version_history(
        &mut self,
        _: &ShowVersionHistory,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.inspector_mode = InspectorMode::History;
        self.inspector_visible = true;
        self.load_revisions(cx);
        cx.notify();
    }

    fn toggle_inspector(
        &mut self,
        _: &ToggleInspector,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.inspector_visible = !self.inspector_visible;
        cx.notify();
    }

    fn toggle_markdown_mode(
        &mut self,
        _: &ToggleMarkdownMode,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Stored in the document model, so it is saved with the document.
        self.document.markdown = !self.document.markdown;
        self.sync_markdown_mode(cx);
    }

    /// Make the canvas and the export follow the document's Markdown flag
    /// (ON = WYSIWYG with hidden syntax, OFF = literal source).
    pub(crate) fn sync_markdown_mode(&mut self, cx: &mut Context<Self>) {
        let on = self.document.markdown;
        self.markdown_mode = on;
        self.editor.update(cx, |editor, cx| {
            editor.markdown_mode = on;
            cx.notify();
        });
        cx.notify();
    }

    fn toggle_ruler(&mut self, _: &ToggleRuler, _window: &mut Window, cx: &mut Context<Self>) {
        self.ruler_visible = !self.ruler_visible;
        cx.notify();
    }

    pub(crate) fn ui_primary(&self) -> Rgba {
        if self.dark_mode {
            rgb(0x3b82f6)
        } else {
            rgb(0x0037b0)
        }
    }

    pub(crate) fn ui_text(&self) -> Rgba {
        if self.dark_mode {
            rgb(0xf8fafc)
        } else {
            rgb(0x0b1c30)
        }
    }

    pub(crate) fn ui_muted(&self) -> Rgba {
        if self.dark_mode {
            rgb(0x94a3b8)
        } else {
            rgb(0x515f74)
        }
    }

    fn ui_success(&self) -> Rgba {
        if self.dark_mode {
            rgb(0x34d399)
        } else {
            rgb(0x059669)
        }
    }

    fn ui_danger(&self) -> Rgba {
        if self.dark_mode {
            rgb(0xf87171)
        } else {
            rgb(0xb91c1c)
        }
    }

    fn ui_workspace(&self) -> Rgba {
        if self.dark_mode {
            rgb(0x0b1220)
        } else {
            rgb(0xe5eeff)
        }
    }

    pub(crate) fn ui_page(&self) -> Rgba {
        if self.dark_mode {
            rgb(0x111c2e)
        } else {
            rgb(0xffffff)
        }
    }

    fn ui_title_bar(&self) -> Rgba {
        if self.dark_mode {
            rgb(0x020b1b)
        } else {
            rgb(0x213145)
        }
    }

    pub(crate) fn ui_panel_low(&self) -> Rgba {
        if self.dark_mode {
            rgb(0x111c2e)
        } else {
            rgb(0xeff4ff)
        }
    }

    fn ui_panel_high(&self) -> Rgba {
        if self.dark_mode {
            rgb(0x1e293b)
        } else {
            rgb(0xdce9ff)
        }
    }

    pub(crate) fn ui_border(&self) -> Rgba {
        if self.dark_mode {
            rgb(0x1e293b)
        } else {
            rgb(0xc4c5d7)
        }
    }

    /// Returns width in pixels for the current page size + orientation.
    pub(crate) fn page_width(&self) -> Pixels {
        px(self.document.page_width() * 96.0 / 72.0)
    }

    /// Returns height in pixels for the current page size + orientation.
    pub(crate) fn page_height(&self) -> Pixels {
        px(self.document.page_height() * 96.0 / 72.0)
    }

    /// Returns (top, right, bottom, left) margins in pixels, converting from points.
    fn page_margins_px(&self) -> (Pixels, Pixels, Pixels, Pixels) {
        let m = &self.document.page_margins;
        let s = 96.0 / 72.0;
        (
            px(m.top * s),
            px(m.right * s),
            px(m.bottom * s),
            px(m.left * s),
        )
    }

    fn tool_button(&self, glyph: &str, active: bool) -> Div {
        let text = self.ui_text();
        let primary = self.ui_primary();
        let hover = self.hover_color();
        let base = if active {
            self.ui_panel_high()
        } else {
            self.surface_color()
        };
        div()
            .w(px(28.0))
            .h(px(28.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(2.0))
            .bg(base)
            .hover(|s| s.bg(hover))
            .cursor_pointer()
            .child(icon(glyph, if active { primary } else { text }, 16.0))
    }

    /// Portrait / Landscape: the lit one follows the document, and a click
    /// sets it (the canvas, the ruler and both exporters follow).
    fn orientation_buttons(&self, cx: &mut Context<Self>) -> [Div; 2] {
        let landscape = self.document.landscape;
        [
            self.compact_button("▣  Portrait", !landscape)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.set_orientation(false, cx)),
                ),
            self.compact_button("▱  Landscape", landscape)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.set_orientation(true, cx)),
                ),
        ]
    }

    fn compact_button(&self, text: impl Into<String>, active: bool) -> Div {
        let fg = if active {
            self.ui_primary()
        } else {
            self.ui_muted()
        };
        let bg = if active {
            self.ui_panel_high()
        } else {
            self.surface_color()
        };
        div()
            .h(px(28.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(2.0))
            .bg(bg)
            .text_color(fg)
            .font_family(UI_FONT)
            .text_size(px(12.0))
            .hover(|s| s.bg(self.hover_color()).text_color(self.ui_text()))
            .cursor_pointer()
            .child(text.into())
    }

    fn title_bar(&self, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        let title = if self.editing_title {
            format!("{}▏", self.doc_title)
        } else if self.doc_title.is_empty() {
            "Untitled — Sylph".to_string()
        } else {
            format!("{} — Sylph", self.doc_title)
        };
        let title_color = rgb(0xeaf1ff);
        div()
            .h(px(32.0))
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .px(px(8.0))
            .bg(self.ui_title_bar())
            .text_color(title_color)
            .font_family(UI_FONT)
            .text_size(px(12.0))
            .on_mouse_down(MouseButton::Left, |_event, window, _cx| {
                window.start_window_move();
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    // The logo returns with an embedded asset (plan task 7.1):
                    // a relative string path is fetched as a URL and never drew.
                    .child(
                        label(title, title_color, 12.0)
                            .px(px(4.0))
                            .rounded(px(3.0))
                            .when(self.editing_title, |t| t.bg(rgb(0x334155)))
                            // Double-click renames; a single press still
                            // drags the window like the rest of the bar.
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                                    if event.click_count == 2 {
                                        cx.stop_propagation();
                                        this.start_title_edit(window, cx);
                                    }
                                }),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        div()
                            .w(px(20.0))
                            .h(px(20.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|s| s.bg(rgb(0x334155)))
                            .on_mouse_down(MouseButton::Left, |_event, window, _cx| {
                                window.minimize_window();
                            })
                            .child(icon("−", title_color, 14.0)),
                    )
                    .child(
                        div()
                            .w(px(20.0))
                            .h(px(20.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|s| s.bg(rgb(0x334155)))
                            .on_mouse_down(MouseButton::Left, |_event, window, _cx| {
                                window.toggle_fullscreen();
                            })
                            .child(icon("□", title_color, 13.0)),
                    )
                    .child(
                        div()
                            .w(px(20.0))
                            .h(px(20.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(2.0))
                            .hover(|s| s.bg(rgb(0xba1a1a)).text_color(rgb(0xffffff)))
                            .on_mouse_down(MouseButton::Left, |_event, window, _cx| {
                                window.remove_window();
                            })
                            .child(icon("×", title_color, 14.0)),
                    ),
            )
    }

    /// The File/Edit/View/Insert/Format/Help menus. Every entry runs the same
    /// action its keyboard shortcut runs, and toggles show whether they are on.
    fn menus(&self, cx: &mut Context<Self>) -> Vec<(&'static str, Vec<MenuEntry>)> {
        use MenuEntry::{Item, Separator};
        let (print, web, focus) = view_mode_active(
            self.web_layout,
            self.sidebar_visible,
            self.inspector_visible,
        );
        let level = self.current_heading_level(cx);
        vec![
            (
                "File",
                vec![
                    Item("New document", Box::new(NewDocument), None),
                    Item("Rename…", Box::new(crate::RenameDocument), None),
                    Separator,
                    Item("Export…", Box::new(OpenExportDialog), None),
                    Item("Page setup…", Box::new(OpenPageSetup), None),
                    Separator,
                    Item("Save now", Box::new(SaveDoc), None),
                ],
            ),
            (
                "Edit",
                vec![
                    Item("Undo", Box::new(Undo), None),
                    Item("Redo", Box::new(Redo), None),
                    Separator,
                    Item("Cut", Box::new(ContextMenuCut), None),
                    Item("Copy", Box::new(ContextMenuCopy), None),
                    Item("Paste", Box::new(ContextMenuPaste), None),
                    Item("Select all", Box::new(ContextMenuSelectAll), None),
                    Separator,
                    Item("Find", Box::new(OpenFindBar), None),
                    Item("Find and replace", Box::new(FindAndReplace), None),
                ],
            ),
            (
                "View",
                vec![
                    Item("Print layout", Box::new(PrintLayout), Some(print)),
                    Item("Web layout", Box::new(WebLayout), Some(web)),
                    Item("Focus mode", Box::new(FocusMode), Some(focus)),
                    Separator,
                    Item(
                        "Markdown mode",
                        Box::new(ToggleMarkdownMode),
                        Some(self.markdown_mode),
                    ),
                    Item("Ruler", Box::new(ToggleRuler), Some(self.ruler_visible)),
                    Item("Dark mode", Box::new(ToggleDarkMode), Some(self.dark_mode)),
                    Separator,
                    Item(
                        "Sidebar",
                        Box::new(ToggleSidebar),
                        Some(self.sidebar_visible),
                    ),
                    Item(
                        "Inspector",
                        Box::new(ToggleInspector),
                        Some(self.inspector_visible),
                    ),
                    Item("Version history", Box::new(ShowVersionHistory), None),
                ],
            ),
            (
                "Insert",
                vec![
                    Item("Table…", Box::new(InsertTable), None),
                    Item("Image…", Box::new(PasteImage), None),
                    Item("Page break", Box::new(InsertPageBreak), None),
                    Separator,
                    Item(
                        "Cover page",
                        Box::new(AddCoverPage),
                        Some(self.document.has_cover_page()),
                    ),
                ],
            ),
            (
                "Format",
                vec![
                    Item("Bold", Box::new(BoldText), None),
                    Item("Italic", Box::new(ItalicText), None),
                    Item("Strikethrough", Box::new(StrikethroughText), None),
                    Separator,
                    Item("Normal text", Box::new(NormalText), Some(level == 0)),
                    Item("Heading 1", Box::new(Heading1), Some(level == 1)),
                    Item("Heading 2", Box::new(Heading2), Some(level == 2)),
                    Item("Heading 3", Box::new(Heading3), Some(level == 3)),
                    Item("Heading 4", Box::new(Heading4), Some(level == 4)),
                    Item("Heading 5", Box::new(Heading5), Some(level == 5)),
                    Item("Heading 6", Box::new(Heading6), Some(level == 6)),
                    Separator,
                    Item("Next body font", Box::new(CycleBodyFont), None),
                ],
            ),
            (
                "Help",
                vec![Item("Keyboard shortcuts", Box::new(ShowShortcuts), None)],
            ),
        ]
    }

    /// One open menu under its title. Clicks inside never reach the root
    /// (which closes menus on any other press); an item closes the menu and
    /// dispatches its action, so it goes wherever its shortcut would.
    fn menu_dropdown(
        &self,
        entries: Vec<MenuEntry>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (text, muted, primary, border, hover) = (
            self.ui_text(),
            self.ui_muted(),
            self.ui_primary(),
            self.ui_border(),
            self.ui_panel_low(),
        );
        let mut list = div()
            .absolute()
            .top(px(28.0))
            .left(px(0.0))
            .min_w(px(280.0))
            .p(px(4.0))
            .flex()
            .flex_col()
            .bg(self.surface_color())
            .border_1()
            .border_color(border)
            .rounded(px(6.0))
            .shadow_lg()
            .font_family(UI_FONT)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        for entry in entries {
            list = list.child(match entry {
                MenuEntry::Separator => div().h(px(1.0)).my(px(4.0)).bg(border),
                MenuEntry::Item(name, action, checked) => {
                    let keys = window
                        .highest_precedence_binding_for_action(action.as_ref())
                        .map(|binding| crate::shortcut_text(&binding))
                        .unwrap_or_default();
                    div()
                        .h(px(28.0))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .rounded(px(4.0))
                        .cursor_pointer()
                        .hover(move |row| row.bg(hover))
                        .child(
                            label(if checked == Some(true) { "✓" } else { "" }, primary, 12.0)
                                .w(px(14.0)),
                        )
                        .child(label(name, text, 12.0).flex_1())
                        .child(label(keys, muted, 11.0))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                this.open_menu = None;
                                window.dispatch_action(action.boxed_clone(), cx);
                                cx.notify();
                            }),
                        )
                }
            });
        }
        deferred(list).with_priority(2)
    }

    /// Help → Keyboard shortcuts: every binding that uses a modifier, one row
    /// per command, generated from the real key bindings so it cannot be
    /// wrong. (macOS-only conventions are hidden on other systems.)
    fn shortcuts_dialog(&self, cx: &mut Context<Self>) -> Div {
        let (text, muted, border) = (self.ui_text(), self.ui_muted(), self.ui_border());
        let mut rows: Vec<(String, Vec<String>)> = Vec::new();
        for binding in crate::key_bindings() {
            let m = *binding.keystrokes()[0].modifiers();
            if !(m.control || m.alt || m.shift || m.platform) {
                continue;
            }
            if m.platform && !cfg!(target_os = "macos") {
                continue;
            }
            let title = crate::action_title(binding.action().name());
            let keys = crate::shortcut_text(&binding);
            match rows.iter_mut().find(|(t, _)| *t == title) {
                Some((_, all)) => all.push(keys),
                None => rows.push((title, vec![keys])),
            }
        }
        let mut list = div()
            .id("shortcut-list")
            .max_h(px(460.0))
            .overflow_y_scroll()
            .flex()
            .flex_col();
        for (title, keys) in rows {
            list = list.child(
                div()
                    .py(px(5.0))
                    .flex()
                    .justify_between()
                    .gap(px(16.0))
                    .border_b_1()
                    .border_color(border)
                    .child(label(title, text, 12.0))
                    .child(label(keys.join("  or  "), muted, 11.0).font_family(MONO_FONT)),
            );
        }
        self.modal("Keyboard shortcuts", 560.0, div().child(list), cx)
    }

    fn menu_bar(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let menu_fg = self.ui_text();
        let mut nav = div().flex().items_center().gap(px(2.0));
        for (index, (name, entries)) in self.menus(cx).into_iter().enumerate() {
            let open = self.open_menu == Some(index);
            nav = nav.child(
                div()
                    .id(("menu", index))
                    .relative()
                    .px(px(8.0))
                    .py(px(2.0))
                    .rounded(px(2.0))
                    .font_family(UI_FONT)
                    .text_size(px(13.0))
                    .text_color(if open { self.ui_primary() } else { menu_fg })
                    .cursor_pointer()
                    .when(open, |s| s.bg(self.ui_panel_high()))
                    .hover(|s| s.bg(self.hover_color()))
                    .child(name)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.open_menu = if this.open_menu == Some(index) {
                                None
                            } else {
                                Some(index)
                            };
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    // Once a menu is open, pointing at another title opens it.
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered && this.open_menu.is_some() && this.open_menu != Some(index) {
                            this.open_menu = Some(index);
                            cx.notify();
                        }
                    }))
                    .when(open, |title| {
                        title.child(self.menu_dropdown(entries, window, cx))
                    }),
            );
        }

        div()
            .h(px(36.0))
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .px(px(8.0))
            .bg(self.surface_color())
            .border_b_1()
            .border_color(self.ui_border())
            .child(nav)
            .child(
                div()
                    .px(px(14.0))
                    .py(px(4.0))
                    .rounded_full()
                    .bg(self.ui_primary())
                    .text_color(rgb(0xffffff))
                    .font_family(UI_FONT)
                    .font_weight(gpui::FontWeight(600.0))
                    .text_size(px(12.0))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.open_export_dialog(&OpenExportDialog, window, cx);
                        }),
                    )
                    .child("Export"),
            )
    }

    fn utility_bar(&self, cx: &mut Context<Self>) -> Div {
        let border = self.ui_border();
        let muted = self.ui_muted();
        let mut bar = div()
            .h(px(48.0))
            .w_full()
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(8.0))
            .bg(self.surface_color())
            .border_b_1()
            .border_color(border);

        bar = bar.child(
            with_tip(
                self.tool_button("☰", self.sidebar_visible),
                "toggle-sidebar",
                "Toggle sidebar",
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.toggle_sidebar(&ToggleSidebar, window, cx)),
            ),
        );
        bar = bar.child(
            with_tip(
                self.tool_button("◧", self.inspector_visible),
                "toggle-inspector",
                "Toggle inspector",
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.toggle_inspector(&ToggleInspector, window, cx)
                }),
            ),
        );
        bar = bar.child(divider(border));
        bar = bar.child(
            with_tip(
                self.tool_button("＋", false),
                "new-document",
                "New document",
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.new_document(&NewDocument, window, cx)),
            ),
        );
        bar = bar.child(with_tip(
            self.tool_button("□", false),
            "utility-open",
            "Not wired in v1",
        ));
        bar = bar.child(
            with_tip(
                self.tool_button("▣", false),
                "save-document",
                "Save document",
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.save_doc(&SaveDoc, window, cx)),
            ),
        );
        bar = bar.child(divider(border));
        for (glyph, id, tip, action) in [("↶", "undo", "Undo", 0), ("↷", "redo", "Redo", 1)] {
            let button = with_tip(self.tool_button(glyph, false), id, tip);
            bar = bar.child(match action {
                0 => button.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.undo(&Undo, window, cx)),
                ),
                _ => button.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.redo(&Redo, window, cx)),
                ),
            });
        }
        bar = bar.child(divider(border));
        bar = bar.child(
            with_tip(self.tool_button("✂", false), "edit-cut", "Cut").on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.context_menu_cut(&ContextMenuCut, window, cx)
                }),
            ),
        );
        bar = bar.child(
            with_tip(self.tool_button("□", false), "edit-copy", "Copy").on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.context_menu_copy(&ContextMenuCopy, window, cx)
                }),
            ),
        );
        bar = bar.child(
            with_tip(self.tool_button("▣", false), "edit-paste", "Paste").on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.context_menu_paste(&ContextMenuPaste, window, cx)
                }),
            ),
        );
        bar = bar.child(divider(border));
        bar = bar.child(
            with_tip(self.tool_button("⌕", false), "open-find", "Find").on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.open_find_bar(&OpenFindBar, window, cx)),
            ),
        );
        bar = bar.child(divider(border));
        bar = bar.child(
            div()
                .h(px(28.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .gap(px(4.0))
                .border_1()
                .border_color(border)
                .rounded(px(2.0))
                .font_family(UI_FONT)
                .text_size(px(12.0))
                .text_color(self.ui_text())
                .child(format!("{}%", self.zoom_percent))
                .child(icon("⌄", muted, 12.0)),
        );
        bar = bar.child(divider(border));

        let mut modes = div()
            .h(px(28.0))
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(3.0))
            .bg(self.ui_panel_low())
            .border_1()
            .border_color(border)
            .rounded(px(2.0));
        let (print_active, web_active, focus_active) = view_mode_active(
            self.web_layout,
            self.sidebar_visible,
            self.inspector_visible,
        );
        for (name, active) in [
            ("Print", print_active),
            ("Web", web_active),
            ("Focus", focus_active),
        ] {
            let (id, tip) = match name {
                "Print" => ("mode-print", "Print layout — fixed page canvas"),
                "Web" => ("mode-web", "Web layout — continuous width, no pages"),
                _ => ("mode-focus", "Focus — hide the side panels"),
            };
            modes = modes.child(
                with_tip(self.compact_button(name, active), id, tip).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _window, cx| match name {
                        "Print" => this.set_view_mode(false, true, cx),
                        "Web" => this.set_view_mode(true, true, cx),
                        _ => this.set_view_mode(false, false, cx),
                    }),
                ),
            );
        }
        bar = bar.child(modes).child(divider(border));
        bar = bar.child(
            with_tip(
                self.tool_button("▥", self.ruler_visible),
                "toggle-ruler",
                "Toggle ruler",
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.toggle_ruler(&ToggleRuler, window, cx)),
            ),
        );
        bar = bar.child(with_tip(
            self.tool_button("＋", false),
            "utility-add",
            "Not wired in v1",
        ));
        bar
    }

    fn global_navigation(&self, cx: &mut Context<Self>) -> Div {
        let border = self.ui_border();
        let muted = self.ui_muted();
        let text = self.ui_text();
        let primary = self.ui_primary();
        let hover = self.hover_color();
        let item = |glyph: &'static str, name: &'static str, active: bool| {
            div()
                .h(px(40.0))
                .w_full()
                .px(px(12.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .rounded(px(2.0))
                .font_family(UI_FONT)
                .text_size(px(13.0))
                .font_weight(if active {
                    gpui::FontWeight(700.0)
                } else {
                    gpui::FontWeight(400.0)
                })
                .text_color(if active { primary } else { muted })
                .when(active, |s| s.bg(self.ui_panel_high()))
                .hover(|s| s.bg(hover).text_color(text))
                .cursor_pointer()
                .child(icon(glyph, if active { primary } else { muted }, 18.0))
                .child(name)
        };

        let editor = item("▱", "Editor Canvas", true).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                this.show_paragraph_inspector(&ShowParagraphInspector, window, cx);
            }),
        );
        let outline = item("☷", "Document Outline", false).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.navigator_tab = NavigatorTab::Outline;
                cx.notify();
            }),
        );
        let inspector = item("☷", "Typography Inspector", false).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                this.show_paragraph_inspector(&ShowParagraphInspector, window, cx);
            }),
        );
        let history = item("◷", "Version history", false).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                this.show_version_history(&ShowVersionHistory, window, cx);
            }),
        );
        let setup = item("⚙", "Page Setup & Margins", false).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                this.open_page_setup(&OpenPageSetup, window, cx);
            }),
        );

        div()
            .w(px(288.0))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(self.ui_panel_low())
            .border_r_1()
            .border_color(border)
            .child(
                div()
                    .h(px(44.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(border)
                    .child(label("NAVIGATION", text, 14.0).font_weight(gpui::FontWeight(600.0)))
                    .child(
                        div()
                            .w(px(24.0))
                            .h(px(24.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|s| s.bg(hover))
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    this.toggle_sidebar(&ToggleSidebar, window, cx);
                                }),
                            )
                            .child(icon("◧", muted, 16.0)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .p(px(8.0))
                    .child(editor)
                    .child(outline)
                    .child(inspector)
                    .child(history)
                    .child(setup),
            )
            .child(
                div()
                    .h(px(48.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_t_1()
                    .border_color(border)
                    .child(label("Dark workspace", muted, 11.0))
                    .child(
                        div()
                            .w(px(32.0))
                            .h(px(16.0))
                            .rounded_full()
                            .bg(if self.dark_mode { primary } else { border })
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    this.toggle_dark_mode(&ToggleDarkMode, window, cx);
                                }),
                            )
                            .child(
                                div()
                                    .w(px(12.0))
                                    .h(px(12.0))
                                    .mt(px(2.0))
                                    .ml(if self.dark_mode { px(18.0) } else { px(2.0) })
                                    .rounded_full()
                                    .bg(self.surface_color()),
                            ),
                    ),
            )
    }

    fn format_bar(&self, cx: &mut Context<Self>) -> Div {
        let border = self.ui_border();
        let muted = self.ui_muted();
        let text = self.ui_text();
        let mut bar = div()
            .h(px(40.0))
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .px(px(8.0))
            .bg(self.ui_panel_low())
            .border_b_1()
            .border_color(border);

        let caret_style = self.caret_style(cx);
        let markdown_on = self.markdown_mode;
        let font_size_display = self.document.resolved_style(caret_style).size.round() as i32;
        let controls = div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(
                self.picker_field(
                    Picker::Style,
                    140.0,
                    crate::style_controls::style_name(caret_style),
                    None,
                    markdown_on,
                    cx,
                )
                .id("heading-style")
                .tooltip(move |_, cx| {
                    cx.new(|_| {
                        ToolbarTip(if markdown_on {
                            "Paragraph style"
                        } else {
                            "Styles need Markdown mode (toggle in the toolbar)"
                        })
                    })
                    .into()
                }),
            )
            .child({
                let font = self.document.style_font(caret_style).to_string();
                self.picker_field(Picker::Font, 150.0, font.clone(), Some(font), true, cx)
                    .id("body-font")
                    .tooltip(|_, cx| {
                        cx.new(|_| ToolbarTip("Font of every paragraph in this style"))
                            .into()
                    })
            })
            .child(
                div()
                    .h(px(28.0))
                    .w(px(72.0))
                    .px(px(4.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(self.surface_color())
                    .border_1()
                    .border_color(border)
                    .rounded(px(2.0))
                    .child(
                        div()
                            .px(px(4.0))
                            .cursor_pointer()
                            .hover(|s| s.bg(self.hover_color()))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    this.adjust_body_font_size(-1, window, cx);
                                }),
                            )
                            .id("font-size-down")
                            .tooltip(|_, cx| cx.new(|_| ToolbarTip("Decrease body size")).into())
                            .child(icon("−", muted, 12.0)),
                    )
                    .child(label(font_size_display.to_string(), text, 12.0))
                    .child(
                        div()
                            .px(px(4.0))
                            .cursor_pointer()
                            .hover(|s| s.bg(self.hover_color()))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    this.adjust_body_font_size(1, window, cx);
                                }),
                            )
                            .id("font-size-up")
                            .tooltip(|_, cx| cx.new(|_| ToolbarTip("Increase body size")).into())
                            .child(icon("＋", muted, 12.0)),
                    ),
            )
            .child(divider(border));

        let mut emphasis = div().flex().items_center().gap(px(2.0));
        for (glyph, id, action, active, tip) in [
            ("B", "emph-bold", 0, true, "Bold"),
            ("I", "emph-italic", 1, false, "Italic"),
            (
                "U",
                "emph-underline",
                2,
                false,
                "Underline — not supported in Markdown v1",
            ),
            ("S", "emph-strike", 3, false, "Strikethrough"),
        ] {
            // Markdown OFF = literal text, so B/I/S would only insert raw
            // markers: dim them and say why. Underline is unwired either
            // way, so it keeps its own honest tip.
            let enabled = action == 2 || markdown_on;
            let button = if enabled {
                self.compact_button(glyph, active)
            } else {
                self.compact_button(glyph, false).opacity(0.45)
            };
            let tip = if enabled {
                tip
            } else {
                "Formatting needs Markdown mode"
            };
            let button = with_tip(button, id, tip);
            emphasis = emphasis.child(match action {
                0 => button.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.bold_text(&BoldText, window, cx)),
                ),
                1 => button.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.italic_text(&ItalicText, window, cx)),
                ),
                3 => button.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.strikethrough_text(&StrikethroughText, window, cx)
                    }),
                ),
                _ => button,
            });
        }

        let mut align = div().flex().items_center().gap(px(2.0));
        for (glyph, id) in [
            ("A", "tool-color"),
            ("☷", "tool-columns"),
            ("≣", "tool-list"),
            ("⇥", "tool-indent"),
            ("↔", "tool-distribute"),
        ] {
            align = align.child(with_tip(
                self.tool_button(glyph, glyph == "A"),
                id,
                "Not wired in v1",
            ));
        }
        align = align.child(
            self.picker_field(
                Picker::LineSpacing,
                72.0,
                format!(
                    "↕ {}",
                    crate::style_controls::spacing_label(
                        self.document.style_line_spacing(caret_style)
                    )
                ),
                None,
                true,
                cx,
            )
            .id("line-spacing")
            .tooltip(|_, cx| {
                cx.new(|_| ToolbarTip("Line spacing of every paragraph in this style"))
                    .into()
            }),
        );

        let mut inserts = div().flex().items_center().gap(px(2.0));
        inserts = inserts.child(with_tip(
            self.tool_button("↗", false),
            "insert-link",
            "Not wired in v1",
        ));
        inserts = inserts.child(
            with_tip(self.tool_button("▧", false), "insert-image", "Paste image").on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.show_image_inspector(&ShowImageInspector, window, cx);
                    this.paste_image(&PasteImage, window, cx);
                }),
            ),
        );
        inserts = inserts.child(
            with_tip(
                self.tool_button("↵", false),
                "insert-page-break",
                "Page break",
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.insert_page_break(&InsertPageBreak, window, cx)
                }),
            ),
        );

        let mut left = controls
            .child(emphasis)
            .child(divider(border))
            .child(align)
            .child(divider(border))
            .child(inserts);
        left = left.child(divider(border)).child(
            with_tip(
                self.tool_button("＋", false),
                "command-palette",
                "Command palette",
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.overlay = WorkspaceOverlay::CommandPalette;
                    this.open_command_palette(&OpenCommandPalette, window, cx);
                }),
            ),
        );

        bar = bar.child(left).child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(label("Markdown", muted, 12.0))
                .child(
                    div()
                        .w(px(32.0))
                        .h(px(16.0))
                        .rounded_full()
                        .bg(if self.markdown_mode {
                            self.ui_primary()
                        } else {
                            border
                        })
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                this.toggle_markdown_mode(&ToggleMarkdownMode, window, cx);
                            }),
                        )
                        .child(
                            div()
                                .w(px(12.0))
                                .h(px(12.0))
                                .mt(px(2.0))
                                .ml(if self.markdown_mode {
                                    px(18.0)
                                } else {
                                    px(2.0)
                                })
                                .rounded_full()
                                .bg(self.surface_color()),
                        ),
                ),
        );
        bar
    }

    /// Every document, newest first; click one to open it (the current one
    /// is saved first). Titles are often still "Untitled", so each row also
    /// shows when it changed and how its text starts. The open document's
    /// row reads the live editor text, which is ahead of its last save.
    fn recent_files(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let text = self.ui_text();
        let muted = self.ui_muted();
        let highlight = self.ui_panel_high();
        let editor = self.editor.read(cx);
        let mut list = div()
            .id("recent-files")
            .max_h(px(220.0))
            .overflow_y_scroll()
            .py(px(4.0))
            .flex()
            .flex_col()
            .gap(px(2.0));
        if self.documents.is_empty() {
            return list.child(label("No documents yet.", muted, 11.0));
        }
        for doc in &self.documents {
            let open = doc.id == editor.doc_id;
            let body = if open {
                editor.content.as_str()
            } else {
                doc.text_start.as_str()
            };
            let (name, detail) = document_label(&doc.title, body, &doc.updated, self.markdown_mode);
            let row = div()
                .px(px(8.0))
                .py(px(4.0))
                .rounded(px(4.0))
                .flex()
                .flex_col()
                .child(label(name, text, 12.0).truncate())
                .child(label(detail, muted, 10.5).truncate());
            let id = doc.id;
            list = list.child(if open {
                row.bg(highlight)
            } else {
                row.cursor_pointer()
                    .hover(move |s| s.bg(highlight))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.switch_document(id, window, cx)
                        }),
                    )
            });
        }
        list
    }

    fn navigator(&self, cx: &mut Context<Self>) -> Div {
        let border = self.ui_border();
        let muted = self.ui_muted();
        let text = self.ui_text();
        let primary = self.ui_primary();
        let hover = self.hover_color();
        let mut tabs = div()
            .h(px(36.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .bg(self.surface_color())
            .border_b_1()
            .border_color(border);
        for (name, tab) in [
            ("Outline", NavigatorTab::Outline),
            ("Pages", NavigatorTab::Pages),
            ("Assets", NavigatorTab::Assets),
        ] {
            let active = self.navigator_tab == tab;
            let mut button = div()
                .h(px(36.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .relative()
                .font_family(UI_FONT)
                .text_size(px(12.0))
                .font_weight(if active {
                    gpui::FontWeight(600.0)
                } else {
                    gpui::FontWeight(400.0)
                })
                .text_color(if active { primary } else { muted })
                .hover(|s| s.text_color(text))
                .cursor_pointer()
                .child(name);
            if active {
                button = button.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(2.0))
                        .bg(primary),
                );
            }
            let selected = tab;
            tabs = tabs.child(button.on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.navigator_tab = selected;
                    cx.notify();
                }),
            ));
        }

        let mut body = div().flex_1().overflow_hidden().px(px(8.0)).py(px(4.0));
        match self.navigator_tab {
            NavigatorTab::Outline => {
                body = body
                    .child(label("DOCUMENT MAP", muted, 11.0).font_weight(gpui::FontWeight(600.0)));
                // The Markdown toggle is the single source of truth: the
                // outline only parses headings when it is ON, so OFF never
                // claims structure the canvas does not show.
                // Typed headings carry their offset, so clicking one jumps
                // there; heading blocks from the model have none.
                let mut headings: Vec<(u8, String, Option<usize>)> = Vec::new();
                if self.markdown_mode {
                    headings.extend(
                        self.outline(cx)
                            .into_iter()
                            .map(|(level, text, offset)| (level, text, Some(offset))),
                    );
                    // Also include structured heading blocks from the rich document.
                    for b in &self.document.blocks {
                        if let sylph_core::document::Block::Heading { level, runs } = b {
                            let s: String = runs.iter().map(|r| r.text.as_str()).collect();
                            if !s.trim().is_empty() {
                                headings.push((*level, s, None));
                            }
                        }
                    }
                }
                if !self.markdown_mode {
                    body = body.child(div().py(px(8.0)).child(label(
                        "Markdown mode is off — turn it on to see headings.",
                        muted,
                        12.0,
                    )));
                } else if headings.is_empty() {
                    body = body.child(div().py(px(8.0)).child(label(
                        "No headings yet — start a line with # to add one.",
                        muted,
                        12.0,
                    )));
                } else {
                    for (level, title, offset) in headings.into_iter().take(200) {
                        let indent = px((level as f32 - 1.0).clamp(0.0, 4.0) * 12.0);
                        let row = div()
                            .pl(indent)
                            .py(px(5.0))
                            .text_color(muted)
                            .hover(|s| s.bg(hover).text_color(text))
                            .child(label(title, muted, 12.0).truncate());
                        body = body.child(match offset {
                            Some(offset) => row.cursor_pointer().on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, window, cx| {
                                    this.go_to_offset(offset, window, cx);
                                }),
                            ),
                            None => row,
                        });
                    }
                }
            }
            NavigatorTab::Pages => {
                // Page chrome and export share one truth: the export view.
                let count = self.page_count(cx);
                body = body
                    .child(
                        label(format!("PAGES ({})", count), muted, 11.0)
                            .font_weight(gpui::FontWeight(600.0)),
                    )
                    .child(self.page_tiles(count, cx));
            }
            NavigatorTab::Assets => {
                body =
                    body.child(label("ASSETS", muted, 11.0).font_weight(gpui::FontWeight(600.0)));
                let mut images: Vec<String> = Vec::new();
                let mut tables = 0usize;
                for b in &self.document.blocks {
                    match b {
                        sylph_core::document::Block::Image { data } => {
                            images.push(data.path.clone())
                        }
                        sylph_core::document::Block::Table { .. } => tables += 1,
                        _ => {}
                    }
                }
                if images.is_empty() && tables == 0 {
                    body = body.child(div().py(px(8.0)).child(label(
                        "No images or tables yet — insert one to see it here.",
                        muted,
                        12.0,
                    )));
                } else {
                    for path in images.iter().take(20) {
                        let name = path.rsplit('/').next().unwrap_or(path.as_str());
                        body = body.child(
                            div()
                                .mt(px(8.0))
                                .p(px(10.0))
                                .bg(self.surface_color())
                                .border_1()
                                .border_color(border)
                                .child(label(format!("▧  {}", name), text, 12.0))
                                .child(label(path.clone(), muted, 10.0)),
                        );
                    }
                    if tables > 0 {
                        body = body.child(div().py(px(6.0)).child(label(
                            format!("{} table(s) in document", tables),
                            muted,
                            11.0,
                        )));
                    }
                }
            }
        }

        let recent = div()
            .p(px(8.0))
            .bg(self.ui_panel_low())
            .border_t_1()
            .border_color(border)
            .child(label("RECENT FILES", muted, 11.0).font_weight(gpui::FontWeight(600.0)))
            .child(self.recent_files(cx));

        div()
            .w(px(260.0))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(self.ui_panel_low())
            .border_r_1()
            .border_color(border)
            .child(tabs)
            .child(body)
            .child(recent)
    }

    fn ruler(&self) -> Div {
        let muted = self.ui_muted();
        let primary = self.ui_primary();
        let page_w = self.page_width();
        // Real page geometry (pt → cm at 2.54 cm/in): ticks and margin
        // markers track the current page size and margins, so the ruler
        // matches the page below it — A4, Letter, landscape, or edited.
        let pt_to_cm = |pt: f32| pt * 2.54 / 72.0;
        let width_cm = pt_to_cm(self.document.page_width());
        let left_cm = pt_to_cm(self.document.page_margins.left);
        let right_cm = width_cm - pt_to_cm(self.document.page_margins.right);
        let fmt = |cm: f32| {
            let r = (cm * 10.0).round() / 10.0;
            if (r - r.round()).abs() < 1e-3 {
                format!("{:.0}", r)
            } else {
                format!("{:.1}", r)
            }
        };
        let width_px: f32 = page_w.into();
        let mut ticks = div()
            .relative()
            .w(page_w)
            .h(px(20.0))
            .font_family(MONO_FONT)
            .text_size(px(8.0))
            .text_color(muted);
        for (cm, is_margin) in ruler_marks(width_cm, left_cm, right_cm) {
            // Each mark is absolutely placed at its proportional x, with a
            // fixed-width centered label so the tick lands on the number.
            ticks = ticks.child(
                div()
                    .absolute()
                    .left(px(width_px * (cm / width_cm) - 12.0))
                    .w(px(24.0))
                    .h(px(20.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_end()
                    .when(is_margin, |s| s.text_color(primary))
                    .child(fmt(cm))
                    .child(
                        div()
                            .w(px(1.0))
                            .h(if is_margin { px(10.0) } else { px(6.0) })
                            .bg(if is_margin { primary } else { self.ui_border() }),
                    ),
            );
        }
        div()
            .h(px(20.0))
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(self.surface_color())
            .border_b_1()
            .border_color(self.ui_border())
            .child(ticks)
    }

    /// Structured rich blocks (images, tables) rendered under the editor —
    /// shared by the Print page and the Web flow so both show exactly the
    /// same document content.
    fn rich_block_divs(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let border = self.ui_border();
        let text = self.ui_text();
        let mut out = Vec::new();
        for (idx, block) in self.document.blocks.iter().enumerate() {
            // Captions are click-to-edit; an empty one shows a muted prompt
            // (never exported) instead of an invented "Figure 1".
            let caption = |field: EditingField, value: &Option<String>, cx: &mut Context<Self>| {
                self.edit_field(
                    field,
                    value.as_deref().unwrap_or_default(),
                    "Add a caption",
                    9.0,
                    cx,
                )
                .italic()
            };
            match block {
                sylph_core::document::Block::Image { data } => {
                    out.push(
                        div()
                            .mt(px(16.0))
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(
                                // A PathBuf loads from disk; a String is parsed
                                // as a URL, which GPUI can't fetch, so inserted
                                // images never painted.
                                img(std::path::PathBuf::from(&data.path))
                                    .max_w(px(480.0))
                                    .max_h(px(300.0)),
                            )
                            .child(caption(EditingField::ImageCaption(idx), &data.caption, cx)),
                    );
                }
                sylph_core::document::Block::Table { data } => {
                    let mut table = div().w_full().border_1().border_color(border);
                    for row in &data.rows {
                        let mut row_div = div().flex().border_b_1().border_color(border);
                        for cell in row {
                            row_div = row_div.child(
                                label(cell.text(), text, 12.0)
                                    .flex_1()
                                    .px(px(8.0))
                                    .py(px(6.0)),
                            );
                        }
                        table = table.child(row_div);
                    }
                    out.push(
                        div()
                            .w_full()
                            .mt(px(16.0))
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(table)
                            .child(caption(EditingField::TableCaption(idx), &data.caption, cx)),
                    );
                }
                _ => {}
            }
        }
        out
    }

    /// Title, subtitle, author and date of the cover page, centred. The text
    /// fields are click-to-edit (Enter commits, Escape cancels) and show a
    /// muted prompt while empty; the date is automatic.
    fn cover_fields(
        &self,
        cover: &sylph_core::document::CoverPageData,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(12.0))
            .child(
                self.edit_field(
                    EditingField::CoverTitle,
                    &cover.title,
                    "Add a title",
                    28.0,
                    cx,
                )
                .font_weight(gpui::FontWeight(700.0)),
            )
            .child(self.edit_field(
                EditingField::CoverSubtitle,
                &cover.subtitle,
                "Add a subtitle",
                16.0,
                cx,
            ))
            .child(self.edit_field(
                EditingField::CoverAuthor,
                &cover.author,
                "Add an author",
                12.0,
                cx,
            ))
            .child(label(cover.date.clone(), self.ui_muted(), 11.0 * 4.0 / 3.0))
    }

    /// One click-to-edit field (cover text, a caption), sized in points
    /// like the body text. While empty it shows a muted prompt, which is
    /// never exported.
    fn edit_field(
        &self,
        field: EditingField,
        value: &str,
        prompt: &str,
        size_pt: f32,
        cx: &mut Context<Self>,
    ) -> Div {
        let text = self.ui_text();
        let base = div()
            .px(px(8.0))
            .py(px(2.0))
            .rounded(px(4.0))
            .text_size(px(size_pt * 4.0 / 3.0));
        if self.editing_field == field {
            return base
                .border_1()
                .border_color(self.ui_primary())
                .text_color(text)
                .child(format!("{}█", self.field_input));
        }
        let (shown, color) = if value.trim().is_empty() {
            (prompt.to_string(), self.ui_muted())
        } else {
            (value.to_string(), text)
        };
        let value = value.to_string();
        let hover = self.ui_panel_low();
        base.text_color(color)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(shown)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    this.begin_field_edit(field.clone(), value.clone(), window, cx)
                }),
            )
    }

    /// The cover as its own page ahead of the body. The exporters end the
    /// cover with a page break, so it is page 1 of the document; like
    /// Word's default, it shows no page number.
    fn render_cover_page(
        &self,
        cover: &sylph_core::document::CoverPageData,
        cx: &mut Context<Self>,
    ) -> Div {
        let (margin_top, margin_right, margin_bottom, margin_left) = self.page_margins_px();
        div()
            .w(self.page_width())
            .h(self.page_height())
            .flex_shrink_0()
            .flex()
            .flex_col()
            .justify_center()
            .pt(margin_top)
            .pr(margin_right)
            .pb(margin_bottom)
            .pl(margin_left)
            .bg(self.ui_page())
            .text_color(self.ui_text())
            // The document's own typeface, like the body text below it.
            .font_family(self.document.body_font.clone())
            .shadow_md()
            .relative()
            .child(self.cover_fields(cover, cx))
            // Word's "Remove Current Cover Page", on the page itself.
            .child(
                div()
                    .absolute()
                    .top(px(12.0))
                    .right(px(12.0))
                    .px(px(8.0))
                    .py(px(4.0))
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(self.ui_border())
                    .bg(self.surface_color())
                    .cursor_pointer()
                    .hover(|s| s.bg(self.hover_color()))
                    .child(
                        label("✕  Remove cover page", self.ui_muted(), 11.0).font_family(UI_FONT),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.document.remove_cover_page();
                            this.set_status(
                                "Cover page removed · Insert → Cover page adds one back",
                                cx,
                            );
                            cx.notify();
                        }),
                    ),
            )
    }

    /// The body pages of print layout: page sheets stacked with a gap,
    /// and the editor laid over them, its rows flowing from one page's
    /// text area to the next (see `pagination`). Clicking anywhere on a
    /// page, including its margins or an empty page, puts the caret there.
    fn render_body_pages(
        &mut self,
        first_page_number: usize,
        page_count: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let text = self.ui_text();
        let page_h = self.page_height();
        let (margin_top, margin_right, margin_bottom, margin_left) = self.page_margins_px();
        let flow = crate::PageFlow {
            content_height: (page_h - margin_top - margin_bottom).into(),
            gap: (margin_bottom + PAGE_GAP + margin_top).into(),
        };
        self.editor
            .update(cx, |editor, _| editor.page_flow = Some(flow));
        // The body size is set in points; the canvas paints pixels
        // (pt × 4/3 at 96dpi) — what the toolbar says is what the page
        // shows.
        let content_height = self.editor.read(cx).content_height;
        let editor_h = content_height.max(px(flow.content_height)) + px(24.0);
        let text_size = px(self.document.body_font_size * (4.0 / 3.0));
        let line_height = text_size * self.document.line_spacing;
        let editor = div()
            .absolute()
            .top(margin_top)
            .left(margin_left)
            .right(margin_right)
            .h(editor_h)
            .overflow_hidden()
            .font_family(self.document.body_font.clone())
            .text_size(text_size)
            .line_height(line_height)
            .text_color(text)
            .child(self.editor.clone())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.commit_field_edit(cx);
                }),
            )
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_editor_right_click));

        let mut sheets = div().flex().flex_col().gap(PAGE_GAP);
        for page in 0..page_count.max(1) {
            sheets = sheets.child(self.render_blank_page(first_page_number + page));
        }
        // Inserted objects (images, tables) follow the typed text.
        let blocks = div()
            .absolute()
            .top(margin_top + content_height)
            .left(margin_left)
            .right(margin_right)
            .flex()
            .flex_col()
            .children(self.rich_block_divs(cx));
        div()
            .relative()
            .flex_shrink_0()
            .text_color(text)
            .font_family(PROSE_FONT)
            .child(sheets)
            .child(editor)
            .child(blocks)
            // Clicks on a page outside the text (margins, below the last
            // line, an empty page) move the caret to the nearest text.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                    let inside_text = this
                        .editor
                        .read(cx)
                        .last_bounds
                        .is_some_and(|bounds| bounds.contains(&event.position));
                    if inside_text {
                        return;
                    }
                    this.commit_field_edit(cx);
                    let position = event.position;
                    this.editor.update(cx, |editor, cx| {
                        let offset = editor.index_for_mouse_position(position);
                        editor.move_to(offset, cx);
                    });
                    window.focus(&this.editor.focus_handle(cx));
                }),
            )
    }

    /// Web layout: the document flows at the canvas width — no fixed page
    /// box, no margins, no page breaks (Word's Web Layout view). Shares the
    /// editor and the rich blocks with the Print page, so content and font
    /// size stay identical across views.
    fn render_web_editor(&mut self, cx: &mut Context<Self>) -> Div {
        // One continuous flow: no pages.
        self.editor.update(cx, |editor, _| editor.page_flow = None);
        let text = self.ui_text();
        let page = self.ui_page();
        let text_size = px(self.document.body_font_size * (4.0 / 3.0));
        let line_height = text_size * self.document.line_spacing;
        let content_height = self.editor.read(cx).content_height;
        let editor_h = px(430.0).max(content_height + px(24.0));
        let editor = div()
            .w_full()
            .h(editor_h)
            .flex_shrink_0()
            .overflow_hidden()
            .font_family(self.document.body_font.clone())
            .text_size(text_size)
            .line_height(line_height)
            .text_color(text)
            .child(self.editor.clone())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.commit_field_edit(cx);
                }),
            )
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_editor_right_click));
        // No pages here, so the cover becomes a title block atop the flow.
        let border = self.ui_border();
        let cover = self.document.cover_page().cloned().map(|cover| {
            div()
                .w_full()
                .pb(px(32.0))
                .mb(px(24.0))
                .border_b_1()
                .border_color(border)
                .child(self.cover_fields(&cover, cx))
        });
        div()
            .w_full()
            .min_h(px(560.0))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .relative()
            .p(px(40.0))
            .bg(page)
            .text_color(text)
            .font_family(PROSE_FONT)
            .shadow_md()
            .children(cover)
            .child(editor)
            .children(self.rich_block_divs(cx))
    }

    fn render_blank_page(&self, page_number: usize) -> Div {
        let border = self.ui_border();
        let muted = self.ui_muted();
        let page_w = self.page_width();
        let page_h = self.page_height();
        let (margin_top, margin_right, margin_bottom, margin_left) = self.page_margins_px();
        div()
            .w(page_w)
            .h(page_h)
            .flex_shrink_0()
            .relative()
            .bg(self.ui_page())
            .shadow_md()
            .child(
                div()
                    .absolute()
                    .top(margin_top)
                    .left(margin_left)
                    .right(margin_right)
                    .bottom(margin_bottom)
                    .border_1()
                    .border_color(border),
            )
            .child(
                div()
                    .absolute()
                    .left(margin_left)
                    .right(margin_right)
                    .bottom(margin_bottom - px(24.0))
                    .flex()
                    .justify_center()
                    .font_family(MONO_FONT)
                    .text_size(px(10.0))
                    .text_color(muted)
                    .child(page_number.to_string()),
            )
    }

    fn center_canvas(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let page_count = self.page_count(cx);
        // Web layout drops the page metaphor: one continuous flow, no
        // page gaps, no extra pages, no ruler (page geometry is off).
        let web = self.web_layout;
        // A cover is page 1, as in export, so the body starts on page 2.
        let cover = self.document.cover_page().cloned();
        let first_body_page = if cover.is_some() { 2 } else { 1 };
        let mut pages = div()
            .w_full()
            .flex_shrink_0()
            .py(px(32.0))
            .px(px(16.0))
            .flex()
            .flex_col()
            .items_center();
        if let (Some(cover), false) = (&cover, web) {
            pages = pages
                .child(self.render_cover_page(cover, cx))
                .child(div().h(px(32.0)).flex_shrink_0());
        }
        pages = pages.child(if web {
            self.render_web_editor(cx)
        } else {
            let body_pages = page_count + 1 - first_body_page;
            self.render_body_pages(first_body_page, body_pages, cx)
        });
        div()
            .id("document-canvas")
            .track_scroll(&self.editor.read(cx).canvas_scroll)
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .overflow_y_scroll()
            .scrollbar_width(px(8.0))
            .bg(self.ui_workspace())
            .child(if self.ruler_visible && !web {
                self.ruler()
            } else {
                div().h(px(0.0))
            })
            .child(pages)
    }

    fn paragraph_inspector(&self, cx: &mut Context<Self>) -> Div {
        let border = self.ui_border();
        let panel = self.ui_panel_low();
        let text = self.ui_text();
        let muted = self.ui_muted();
        let primary = self.ui_primary();
        let current_ls = self
            .document
            .style_line_spacing(self.current_heading_level(cx));
        // Real spacing for the block at the cursor (heading metrics or the
        // paragraph default the export applies) — not placeholder numbers.
        let resolved = self.document.resolved_style(self.current_heading_level(cx));
        let (before_pt, after_pt) = (resolved.space_before, resolved.space_after);
        let line_spacing = div()
            .flex()
            .items_center()
            .gap(px(2.0))
            .p(px(2.0))
            .bg(panel)
            .children(["1.0", "1.15", "1.5", "2.0"].iter().map(|v| {
                let val: f32 = v.parse().unwrap_or(1.0);
                let is_active = (current_ls - val).abs() < 0.01;
                div()
                    .flex_1()
                    .py(px(10.0))
                    .text_center()
                    .font_family(MONO_FONT)
                    .text_size(px(11.0))
                    .text_color(if is_active { primary } else { muted })
                    .cursor_pointer()
                    .hover(|s| s.bg(self.surface_color()))
                    .when(is_active, |s| {
                        s.bg(self.surface_color())
                            .font_weight(gpui::FontWeight(700.0))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.set_style_spacing(Some(val), cx);
                        }),
                    )
                    .child(*v)
            }));
        div()
            .w(px(300.0))
            .flex_shrink_0()
            .h_full()
            .overflow_hidden()
            .bg(self.surface_color())
            .border_l_1()
            .border_color(border)
            .child(
                div()
                    .h(px(52.0))
                    .px(px(16.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(panel)
                    .border_b_1()
                    .border_color(border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(icon("☷", primary, 18.0))
                            .child(
                                label("Paragraph & Format", text, 13.0)
                                    .font_weight(gpui::FontWeight(700.0)),
                            ),
                    )
                    .child(self.inspector_close_button(cx)),
            )
            .child(
                div()
                    .p(px(16.0))
                    .font_family(UI_FONT)
                    .child(
                        label("SPACING & FLOW", muted, 11.0).font_weight(gpui::FontWeight(600.0)),
                    )
                    .child(
                        div()
                            .mt(px(8.0))
                            .flex()
                            .gap(px(4.0))
                            .child(self.spacing_stepper(true, before_pt, cx))
                            .child(self.spacing_stepper(false, after_pt, cx)),
                    )
                    .child(label("Line Spacing", muted, 11.0).mt(px(14.0)))
                    .child(line_spacing)
                    .child(div().h(px(1.0)).my(px(16.0)).bg(border))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                label("Page Setup", muted, 11.0)
                                    .font_weight(gpui::FontWeight(600.0)),
                            )
                            .child(
                                label("Defaults", primary, 10.0)
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.restore_page_defaults(cx)
                                        }),
                                    ),
                            ),
                    )
                    .child(label("Standard Format", muted, 11.0).mt(px(10.0)))
                    .child(
                        div()
                            .h(px(44.0))
                            .px(px(10.0))
                            .flex()
                            .items_center()
                            .justify_between()
                            .bg(panel)
                            .child(
                                label(page_format_label(&self.document.page_size), text, 12.0)
                                    .font_weight(gpui::FontWeight(600.0)),
                            )
                            .child(icon("⌄", muted, 14.0))
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    this.open_page_setup(&OpenPageSetup, window, cx)
                                }),
                            ),
                    )
                    .child(label("Orientation", muted, 11.0).mt(px(12.0)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.0))
                            .p(px(2.0))
                            .bg(panel)
                            .children(self.orientation_buttons(cx)),
                    )
                    .child(label("Margins", muted, 11.0).mt(px(12.0)))
                    .child({
                        let m = &self.document.page_margins;
                        let cm = 2.54 / 72.0; // 1pt = 2.54/72 cm
                        let mt = format!("Top     {:.2} cm", m.top * cm);
                        let mb = format!("Bottom  {:.2} cm", m.bottom * cm);
                        let ml = format!("Left    {:.2} cm", m.left * cm);
                        let mr = format!("Right   {:.2} cm", m.right * cm);
                        div().grid().grid_cols(2).gap(px(4.0)).children(
                            [mt, mb, ml, mr].iter().map(|v| {
                                div()
                                    .p(px(10.0))
                                    .bg(panel)
                                    .font_family(MONO_FONT)
                                    .text_size(px(10.0))
                                    .text_color(text)
                                    .child(v.clone())
                            }),
                        )
                    })
                    .child(
                        div()
                            .mt(px(4.0))
                            .h(px(44.0))
                            .px(px(10.0))
                            .flex()
                            .items_center()
                            .justify_between()
                            .bg(panel)
                            .border_1()
                            .border_color(border)
                            .rounded(px(2.0))
                            .cursor_pointer()
                            .hover(|s| s.bg(self.hover_color()))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    this.open_page_setup(&OpenPageSetup, window, cx)
                                }),
                            )
                            .child(label(
                                match crate::margin_preset_of(&self.document.page_margins) {
                                    Some(i) => format!("{} margins", crate::MARGIN_PRESETS[i].0),
                                    None => "Custom margins".to_string(),
                                },
                                text,
                                11.0,
                            ))
                            .child(icon("⌄", muted, 12.0)),
                    ),
            )
    }

    fn inspector_close_button(&self, cx: &mut Context<Self>) -> Div {
        div()
            .w(px(24.0))
            .h(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(2.0))
            .hover(|s| s.bg(self.hover_color()))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.toggle_inspector(&ToggleInspector, window, cx)
                }),
            )
            .child(icon("›", self.ui_muted(), 20.0))
    }

    fn image_inspector(&self, cx: &mut Context<Self>) -> Div {
        let border = self.ui_border();
        let panel = self.ui_panel_low();
        let text = self.ui_text();
        let muted = self.ui_muted();
        let primary = self.ui_primary();
        div()
            .w(px(300.0))
            .flex_shrink_0()
            .h_full()
            .overflow_hidden()
            .bg(self.surface_color())
            .border_l_1()
            .border_color(border)
            .child(
                div()
                    .h(px(52.0))
                    .px(px(16.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .bg(panel)
                    .border_b_1()
                    .border_color(border)
                    .child(icon("▧", primary, 18.0))
                    .child(label("Image", text, 14.0).font_weight(gpui::FontWeight(700.0)))
                    .child(label("Figure 1", muted, 12.0))
                    .child(self.inspector_close_button(cx)),
            )
            .child(
                div()
                    .p(px(16.0))
                    .child(label("SIZE", muted, 11.0).font_weight(gpui::FontWeight(600.0)))
                    .child(div().mt(px(8.0)).flex().gap(px(4.0)).child(label("Width\n480 px", text, 12.0).flex_1().p(px(12.0)).bg(panel)).child(label("Height\n300 px", text, 12.0).flex_1().p(px(12.0)).bg(panel)))
                    .child(label("WRAPPING", muted, 11.0).mt(px(20.0)))
                    .child(div().flex().p(px(2.0)).bg(panel).child(self.compact_button("Inline", true)).child(self.compact_button("Wrap", false)).child(self.compact_button("Break", false)))
                    .child(label("POSITION", muted, 11.0).mt(px(20.0)))
                    .child(div().h(px(44.0)).p(px(12.0)).bg(panel).child(label("☷  Centered", text, 12.0)))
                    .child(label("ALT TEXT", muted, 11.0).mt(px(20.0)))
                    .child(div().p(px(12.0)).bg(panel).font_family(PROSE_FONT).text_size(px(14.0)).child("Revenue breakdown by geographic region with clean modernist chart layout."))
                    .child(label("CAPTION", muted, 11.0).mt(px(20.0)))
                    .child(div().p(px(12.0)).bg(panel).font_family(PROSE_FONT).text_size(px(14.0)).child("Figure 1: Revenue by region"))
                    .child(
                        div()
                            .h(px(44.0))
                            .mt(px(24.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .border_1()
                            .border_color(border)
                            .child(label("▧  Replace image...", text, 12.0)),
                    )
                    .child(label("▥  Delete", rgb(0xba1a1a), 12.0).text_center().mt(px(20.0))),
            )
    }

    /// Version history: one entry per editing session, newest first, from
    /// the document's own saves. Select a version to restore it; restoring
    /// is one undoable edit and becomes the newest version, so no version is
    /// ever lost.
    fn history_inspector(&self, cx: &mut Context<Self>) -> Div {
        let border = self.ui_border();
        let panel = self.ui_panel_low();
        let text = self.ui_text();
        let muted = self.ui_muted();
        let primary = self.ui_primary();
        let live = &self.editor.read(cx).content;
        let mut list = div()
            .id("history-list")
            .flex_1()
            .overflow_y_scroll()
            .p(px(12.0))
            .flex()
            .flex_col()
            .gap(px(6.0));
        if self.revisions.is_empty() {
            list = list.child(label(
                "No saved versions yet. Sylph saves as you type, and each editing session becomes a version here.",
                muted,
                12.0,
            ));
        }
        for revision in &self.revisions {
            let current = revision.text == *live;
            let selected = self.selected_revision == Some(revision.id);
            let id = revision.id;
            let preview = first_text(&revision.text, self.markdown_mode);
            let mut row = div()
                .p(px(10.0))
                .flex()
                .flex_col()
                .gap(px(3.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(if selected { primary } else { border })
                .when(selected, |r| r.bg(panel))
                .cursor_pointer()
                .hover(move |r| r.bg(panel))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.selected_revision = (this.selected_revision != Some(id)).then_some(id);
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .child(
                            label(revision.saved.clone(), text, 12.0)
                                .font_weight(gpui::FontWeight(600.0)),
                        )
                        .when(current, |r| r.child(label("Current", primary, 11.0))),
                )
                .child(label(
                    format!("{} words", count_words(&revision.text)),
                    muted,
                    11.0,
                ))
                .child(
                    label(
                        if preview.is_empty() {
                            "(empty)".to_string()
                        } else {
                            preview
                        },
                        muted,
                        11.0,
                    )
                    .truncate(),
                );
            if selected && !current {
                row = row.child(
                    div().mt(px(6.0)).child(
                        self.compact_button("Restore this version", true)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| this.restore_revision(id, cx)),
                            ),
                    ),
                );
            }
            list = list.child(row);
        }
        div()
            .w(px(300.0))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(self.surface_color())
            .border_l_1()
            .border_color(border)
            .font_family(UI_FONT)
            .child(
                div()
                    .h(px(52.0))
                    .px(px(16.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(panel)
                    .border_b_1()
                    .border_color(border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(icon("◷", primary, 18.0))
                            .child(
                                label("Version history", text, 13.0)
                                    .font_weight(gpui::FontWeight(700.0)),
                            ),
                    )
                    .child(self.inspector_close_button(cx)),
            )
            .child(list)
            .child(
                div()
                    .p(px(12.0))
                    .border_t_1()
                    .border_color(border)
                    .child(label(
                        "Restoring brings back the text. Page setup and inserted objects keep their current state.",
                        muted,
                        10.5,
                    )),
            )
    }

    fn inspector(&self, cx: &mut Context<Self>) -> Div {
        match self.inspector_mode {
            InspectorMode::Paragraph => self.paragraph_inspector(cx),
            InspectorMode::Image => self.image_inspector(cx),
            InspectorMode::History => self.history_inspector(cx),
        }
    }

    /// The right-click menu at the pointer: clipboard, select all, Bold and
    /// Italic, and Find. Items act on mouse-down (the root closes the menu on
    /// the same press), and the menu stays inside the window near its edges.
    fn context_menu_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let modifier = if cfg!(target_os = "macos") {
            "⌘"
        } else {
            "Ctrl+"
        };
        let text = self.ui_text();
        let muted = self.ui_muted();
        let hover = self.ui_panel_low();
        let border = self.ui_border();
        let row = |name: &'static str, key: &str| {
            div()
                .h(px(28.0))
                .px(px(12.0))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(24.0))
                .rounded(px(4.0))
                .cursor_pointer()
                .hover(move |r| r.bg(hover))
                .child(label(name, text, 12.0))
                .child(label(format!("{modifier}{key}"), muted, 11.0))
        };
        let separator = || div().h(px(1.0)).my(px(4.0)).bg(border);
        let menu = div()
            .w(px(220.0))
            .p(px(4.0))
            .flex()
            .flex_col()
            .bg(self.surface_color())
            .border_1()
            .border_color(border)
            .rounded(px(6.0))
            .shadow_lg()
            .font_family(UI_FONT)
            .child(row("Cut", "X").on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.context_menu_cut(&ContextMenuCut, window, cx)
                }),
            ))
            .child(row("Copy", "C").on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.context_menu_copy(&ContextMenuCopy, window, cx)
                }),
            ))
            .child(row("Paste", "V").on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.context_menu_paste(&ContextMenuPaste, window, cx)
                }),
            ))
            .child(row("Select all", "A").on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.context_menu_select_all(&ContextMenuSelectAll, window, cx)
                }),
            ))
            .child(separator())
            .child(row("Bold", "B").on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.context_menu.visible = false;
                    this.bold_text(&BoldText, window, cx);
                }),
            ))
            .child(row("Italic", "I").on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.context_menu.visible = false;
                    this.italic_text(&ItalicText, window, cx);
                }),
            ))
            .child(separator())
            .child(row("Find…", "F").on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.context_menu.visible = false;
                    this.open_find_bar(&OpenFindBar, window, cx);
                }),
            ));
        deferred(
            anchored()
                .position(self.context_menu.position)
                .snap_to_window_with_margin(px(8.0))
                .child(menu),
        )
        .with_priority(1)
    }

    /// Find & replace, floating over the canvas like Docs: the query with a
    /// match count and previous/next, a match-case toggle, and the
    /// replacement with Replace / Replace all. Click a field (or press Tab)
    /// to type in it; Enter finds the next match (replaces, in the Replace
    /// field), Shift+Enter the previous; Escape closes.
    fn find_bar(&self, cx: &mut Context<Self>) -> Div {
        let text = self.ui_text();
        let muted = self.ui_muted();
        let primary = self.ui_primary();
        let border = self.ui_border();
        let hover = self.ui_panel_low();
        let count = self.find.matches.len();
        let status = if self.find.query.is_empty() {
            String::new()
        } else if count == 0 {
            "No results".to_string()
        } else {
            format!("{} of {count}", self.find.current_match + 1)
        };
        let field = |value: &str, prompt: &str, active: bool, replace: bool| {
            let (shown, color) = if value.is_empty() {
                (prompt.to_string(), muted)
            } else {
                (value.to_string(), text)
            };
            div()
                .id(if replace {
                    "find-replacement"
                } else {
                    "find-query"
                })
                .flex_1()
                .min_w(px(0.0))
                .h(px(28.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .rounded(px(4.0))
                .border_1()
                .border_color(if active { primary } else { border })
                .cursor_text()
                .child(label(shown, color, 12.0).truncate())
                .when(active, |f| f.child(label("▏", primary, 12.0)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        this.find.replace_focused = replace;
                        window.focus(&this.focus_handle);
                        cx.notify();
                    }),
                )
        };
        let button = |glyph: &str, tip: &'static str, id: &'static str, on: bool| {
            with_tip(self.compact_button(glyph, on), id, tip).hover(move |b| b.bg(hover))
        };
        let replace_button = |name: &'static str, id: &'static str| {
            div()
                .id(id)
                .h(px(28.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .rounded(px(4.0))
                .border_1()
                .border_color(border)
                .cursor_pointer()
                .hover(move |b| b.bg(hover))
                .child(label(name, text, 12.0))
        };
        div()
            .absolute()
            .top(px(12.0))
            .right(px(20.0))
            .w(px(440.0))
            .p(px(8.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .bg(self.surface_color())
            .border_1()
            .border_color(border)
            .rounded(px(6.0))
            .shadow_lg()
            .font_family(UI_FONT)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(field(
                        &self.find.query,
                        "Find",
                        !self.find.replace_focused,
                        false,
                    ))
                    .child(label(status, muted, 11.0).w(px(64.0)).flex_shrink_0())
                    .child(
                        button("↑", "Previous match (Shift+Enter)", "find-prev", false)
                            .on_click(cx.listener(|this, _, _, cx| this.find_navigate(false, cx))),
                    )
                    .child(
                        button("↓", "Next match (Enter)", "find-next", false)
                            .on_click(cx.listener(|this, _, _, cx| this.find_navigate(true, cx))),
                    )
                    .child(
                        button("Aa", "Match case", "find-case", self.find.match_case)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_find_case(cx))),
                    )
                    .child(button("✕", "Close (Esc)", "find-close", false).on_click(
                        cx.listener(|this, _, window, cx| this.hide_find_bar(window, cx)),
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(field(
                        &self.find.replacement,
                        "Replace with",
                        self.find.replace_focused,
                        true,
                    ))
                    .child(
                        replace_button("Replace", "find-replace-one").on_click(cx.listener(
                            |this, _, window, cx| this.replace_current(&ReplaceCurrent, window, cx),
                        )),
                    )
                    .child(replace_button("Replace all", "find-replace-all").on_click(
                        cx.listener(|this, _, window, cx| {
                            this.replace_all(&ReplaceAll, window, cx)
                        }),
                    )),
            )
    }

    fn status_bar(&self, cx: &mut Context<Self>) -> Div {
        let muted = self.ui_muted();
        let text = self.ui_text();
        // Markdown ON counts the blocks export will produce; OFF exports
        // one paragraph per source line, so the line *is* the block.
        let markdown_on = self.markdown_mode;
        // Cached per edit and caret move: these parse the whole text.
        let (line, column, words, (block, of_blocks), body_page) = self.caret_status(cx);
        let (position, body_page, word_count, save_state) = {
            let editor = self.editor.read(cx);
            let position = if markdown_on {
                format!("Block {block} of {of_blocks}")
            } else {
                format!("Ln {line}, Col {column}")
            };
            // OFF exports no typed page breaks, so the caret stays on the
            // first body page; inserted breaks all follow the typed text.
            let save_state = match (&editor.save_state, &self.model_save_error) {
                // No database / an unreadable document outrank everything.
                (state @ (SaveState::Unpersisted(_) | SaveState::ReadOnly(_)), _) => state.clone(),
                // Page setup / inserted objects failed to save: a good
                // text save must not hide that.
                (_, Some(reason)) => SaveState::Failed {
                    error: reason.clone(),
                    last_ok_at: editor.last_saved_at,
                },
                (state, None) => state.clone(),
            };
            (position, body_page, words, save_state)
        };
        let page_count = self.page_count(cx);
        // A cover is page 1, so the body's pages come after it.
        // Print layout knows the caret's real page (text flows across
        // pages); web layout has one page.
        let body_page = if self.web_layout {
            body_page
        } else {
            self.editor.read(cx).cursor_page + 1
        };
        let caret_page = usize::from(self.document.has_cover_page()) + body_page;
        // The save indicator always shows the real save state in Word/Docs
        // wording (never a storage path). Transient action feedback gets
        // its own slot beside it and clears itself.
        let save_color = match save_state {
            SaveState::Saved => self.ui_success(),
            SaveState::Saving => muted,
            _ => self.ui_danger(),
        };
        // The position and word count never shrink; long transient
        // messages and save errors truncate with an ellipsis instead of
        // squeezing the groups into each other ("Col 169 words…").
        div()
            .h(px(28.0))
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.0))
            .px(px(8.0))
            .bg(self.ui_panel_low())
            .border_t_1()
            .border_color(self.ui_border())
            .font_family(UI_FONT)
            .text_size(px(11.0))
            .text_color(muted)
            .child(
                label(
                    format!(
                        "Page {} of {}  ·  Section 1  ·  {}",
                        caret_page, page_count, position
                    ),
                    muted,
                    11.0,
                )
                .font_family(MONO_FONT)
                .flex_shrink_0(),
            )
            .child(
                label(format!("{} words", word_count), text, 11.0)
                    .font_weight(gpui::FontWeight(500.0))
                    .flex_shrink_0(),
            )
            .child(
                div()
                    .min_w(px(0.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    // Stays for the whole export; transient messages expire.
                    .when(self.export_running, |row| {
                        row.child(label("Exporting…", text, 11.0).flex_shrink_0())
                            .child("·")
                    })
                    .when_some(self.status_message.clone(), |row, message| {
                        row.child(label(message, text, 11.0).min_w(px(0.0)).truncate())
                            .child("·")
                    })
                    .child(div().flex_shrink_0().child("English (US)"))
                    .child("·")
                    .child(div().flex_shrink_0().child("UTF-8"))
                    .child("·")
                    .child(
                        label(format!("●  {}", save_state.label()), save_color, 11.0)
                            .max_w(px(320.0))
                            .truncate(),
                    )
                    .child("·")
                    .child(icon("⌕", muted, 12.0))
                    .child(
                        div()
                            .w(px(64.0))
                            .h(px(4.0))
                            .rounded_full()
                            .bg(self.ui_border())
                            .child(
                                div()
                                    .w(px(32.0))
                                    .h(px(4.0))
                                    .rounded_full()
                                    .bg(self.ui_primary()),
                            ),
                    )
                    .child(format!("{}%", self.zoom_percent)),
            )
    }

    fn command_palette(&self, cx: &mut Context<Self>) -> Div {
        let panel = self.surface_color();
        let text = self.ui_text();
        let muted = self.ui_muted();
        let primary = self.ui_primary();
        let border = self.ui_border();
        let mut rows = div().border_t_1().border_color(border);
        let matches = crate::palette_matches(&self.palette.query);
        if matches.is_empty() {
            rows = rows.child(
                div()
                    .h(px(50.0))
                    .px(px(32.0))
                    .flex()
                    .items_center()
                    .child(label("No matching command", muted, 14.0)),
            );
        }
        for (index, &command) in matches.iter().enumerate() {
            let (glyph, title, shortcut) = crate::PALETTE_COMMANDS[command];
            // Markdown commands dim with the rest of the Markdown-driven
            // controls.
            let enabled = !crate::palette_command_needs_markdown(title) || self.markdown_mode;
            let selected = index == self.palette.selected;
            let row = div()
                .h(px(50.0))
                .px(px(32.0))
                .flex()
                .items_center()
                .gap(px(16.0))
                .when(selected, |s| s.bg(self.ui_panel_low()))
                .hover(|s| s.bg(self.ui_panel_low()))
                .cursor_pointer()
                .when(!enabled, |s| s.opacity(0.45))
                .child(
                    label(
                        glyph,
                        if selected && enabled { primary } else { muted },
                        14.0,
                    )
                    .w(px(20.0)),
                )
                .child(label(title, if enabled { text } else { muted }, 16.0))
                .child(
                    label(shortcut, muted, 12.0)
                        .font_family(MONO_FONT)
                        .ml_auto(),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        this.run_palette_command(title, window, cx);
                    }),
                );
            rows = rows.child(row);
        }
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x0b1c3066))
            .child(
                div()
                    .w(px(660.0))
                    .bg(panel)
                    .rounded(px(12.0))
                    .border_1()
                    .border_color(border)
                    .shadow_lg()
                    .child(
                        div()
                            .h(px(56.0))
                            .px(px(20.0))
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .border_b_1()
                            .border_color(border)
                            .child(icon("⌕", muted, 18.0))
                            .child(if self.palette.query.is_empty() {
                                label("Type a command…", muted, 16.0)
                            } else {
                                label(format!("{}▏", self.palette.query), text, 16.0)
                            })
                            .child(
                                label("ESC", muted, 11.0)
                                    .ml_auto()
                                    .px(px(8.0))
                                    .py(px(6.0))
                                    .bg(self.ui_panel_low()),
                            ),
                    )
                    .child(rows)
                    .child(
                        div()
                            .h(px(36.0))
                            .px(px(20.0))
                            .flex()
                            .items_center()
                            .justify_between()
                            .font_family(UI_FONT)
                            .text_size(px(11.0))
                            .text_color(muted)
                            .child("Navigate with ↑ ↓  ·  Select with ↵")
                            .child(label(
                                match matches.len() {
                                    1 => "1 command".to_string(),
                                    n => format!("{n} commands"),
                                },
                                primary,
                                11.0,
                            )),
                    ),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.close_overlay(&CloseOverlay, window, cx);
                }),
            )
    }

    /// A centred dialog over the dimmed workspace. Clicking outside it (or
    /// Escape) closes it; clicks inside stay inside.
    fn modal(&self, title: &str, width: f32, body: Div, cx: &mut Context<Self>) -> Div {
        let text = self.ui_text();
        let muted = self.ui_muted();
        let hover = self.ui_panel_low();
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x0b1c3066))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close_overlay(&CloseOverlay, window, cx)),
            )
            .child(
                div()
                    .w(px(width))
                    .p(px(20.0))
                    .flex()
                    .flex_col()
                    .gap(px(14.0))
                    .bg(self.surface_color())
                    .border_1()
                    .border_color(self.ui_border())
                    .rounded(px(8.0))
                    .shadow_lg()
                    .font_family(UI_FONT)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                label(title.to_string(), text, 16.0)
                                    .font_weight(gpui::FontWeight(600.0)),
                            )
                            .child(
                                div()
                                    .px(px(6.0))
                                    .rounded(px(4.0))
                                    .cursor_pointer()
                                    .hover(move |b| b.bg(hover))
                                    .child(label("✕", muted, 14.0))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, window, cx| {
                                            this.close_overlay(&CloseOverlay, window, cx)
                                        }),
                                    ),
                            ),
                    )
                    .child(body),
            )
    }

    /// A choice card: a title and one line of detail, highlighted when chosen.
    fn choice(&self, name: &str, detail: &str, chosen: bool) -> Div {
        let (text, muted, primary, hover) = (
            self.ui_text(),
            self.ui_muted(),
            self.ui_primary(),
            self.ui_panel_low(),
        );
        div()
            .flex_1()
            .p(px(12.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .rounded(px(6.0))
            .border_1()
            .border_color(if chosen { primary } else { self.ui_border() })
            .when(chosen, |c| c.bg(hover))
            .cursor_pointer()
            .hover(move |c| c.bg(hover))
            .child(
                label(name.to_string(), if chosen { primary } else { text }, 13.0)
                    .font_weight(gpui::FontWeight(600.0)),
            )
            .child(label(detail.to_string(), muted, 11.0))
    }

    /// Export: pick a format, then choose where to save it (Save As).
    fn export_dialog(&self, cx: &mut Context<Self>) -> Div {
        let muted = self.ui_muted();
        let formats = [
            (
                ExportFormat::Pdf,
                "PDF",
                "Print-ready pages with the document's page setup and embedded fonts.",
            ),
            (
                ExportFormat::Docx,
                "Word document (.docx)",
                "Editable in Word, LibreOffice and Google Docs.",
            ),
            (
                ExportFormat::Markdown,
                "Markdown (.md)",
                "Plain text with Markdown formatting.",
            ),
        ];
        let mut body = div().flex().flex_col().gap(px(8.0)).child(label(
            "Choose a format; you pick where to save it next.",
            muted,
            12.0,
        ));
        for (format, name, detail) in formats {
            body = body.child(self.choice(name, detail, false).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.export_document(format, cx)),
            ));
        }
        self.modal("Export", 440.0, body, cx)
    }

    /// Page setup: paper, orientation and Word's margin presets. Every choice
    /// applies at once; the canvas, the ruler and the exports follow it.
    fn page_setup_dialog(&self, cx: &mut Context<Self>) -> Div {
        use sylph_core::document::PageSize;
        let muted = self.ui_muted();
        let heading =
            |name: &str| label(name.to_string(), muted, 11.0).font_weight(gpui::FontWeight(600.0));
        let mut paper = div().flex().gap(px(8.0));
        for size in [PageSize::A4, PageSize::Letter] {
            let chosen = self.document.page_size == size;
            let label_text = page_format_label(&size);
            let (name, dims) = label_text.split_once(" · ").unwrap_or((size.name(), ""));
            paper = paper.child(self.choice(name, dims, chosen).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.set_paper(size.clone(), cx)),
            ));
        }
        let current = crate::margin_preset_of(&self.document.page_margins);
        let cm = |pt: f32| pt / 72.0 * 2.54;
        let mut margins = div().flex().gap(px(8.0));
        for (index, (name, m)) in crate::MARGIN_PRESETS.iter().enumerate() {
            let detail = if m.left == m.top {
                format!("{:.2} cm all round", cm(m.top))
            } else {
                format!("{:.2} cm, sides {:.2} cm", cm(m.top), cm(m.left))
            };
            margins = margins.child(
                self.choice(name, &detail, current == Some(index))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.apply_margin_preset(index, cx)),
                    ),
            );
        }
        let body = div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(heading("PAPER"))
            .child(paper)
            .child(heading("ORIENTATION"))
            .child(
                div()
                    .flex()
                    .gap(px(4.0))
                    .children(self.orientation_buttons(cx)),
            )
            .child(heading("MARGINS"))
            .child(margins)
            .when(current.is_none(), |b| {
                b.child(label(
                    "The current margins are custom (not one of the presets).",
                    muted,
                    11.0,
                ))
            })
            .child(div().flex().justify_end().child(
                self.compact_button("Done", true).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.close_overlay(&CloseOverlay, window, cx)
                    }),
                ),
            ));
        self.modal("Page setup", 600.0, body, cx)
    }

    /// Insert Table: point at a size in the grid, click to insert an editable
    /// Markdown table after the caret's line.
    fn table_dialog(&self, cx: &mut Context<Self>) -> Div {
        const SIZE: usize = 8;
        let (rows, cols) = self.table_picker;
        let (muted, primary, border, text) = (
            self.ui_muted(),
            self.ui_primary(),
            self.ui_border(),
            self.ui_text(),
        );
        let lit_bg = self.ui_panel_high();
        let mut grid = div().flex().flex_col().gap(px(3.0));
        for r in 0..SIZE {
            let mut line = div().flex().gap(px(3.0));
            for c in 0..SIZE {
                let lit = r < rows && c < cols;
                line = line.child(
                    div()
                        .id(("table-cell", r * SIZE + c))
                        .size(px(20.0))
                        .rounded(px(2.0))
                        .border_1()
                        .border_color(if lit { primary } else { border })
                        .when(lit, |cell| cell.bg(lit_bg))
                        .cursor_pointer()
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered {
                                this.table_picker = (r + 1, c + 1);
                                cx.notify();
                            }
                        }))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.insert_markdown_table(r + 1, c + 1, cx)
                            }),
                        ),
                );
            }
            grid = grid.child(line);
        }
        let size_text = if rows == 0 {
            "Point at a size".to_string()
        } else {
            format!("{cols} × {rows} table")
        };
        let body = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.0))
            .child(grid)
            .child(label(size_text, text, 13.0))
            .child(label(
                "Inserted as a Markdown table after the caret's line.",
                muted,
                11.0,
            ));
        self.modal("Insert table", 300.0, body, cx)
    }
}

impl Render for SylphApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .font_family(UI_FONT)
            .text_size(px(13.0))
            .bg(self.bg_color())
            .track_focus(&self.focus_handle(cx))
            .key_context("SylphApp")
            .on_action(cx.listener(Self::save_doc))
            .on_action(cx.listener(Self::summarize_doc))
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::toggle_inspector))
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
            .on_action(cx.listener(Self::rename_document))
            .on_action(cx.listener(Self::toggle_dark_mode))
            .on_action(cx.listener(Self::export_docx))
            .on_action(cx.listener(Self::export_pdf))
            .on_action(cx.listener(Self::export_markdown))
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
            .on_action(cx.listener(Self::toggle_orientation))
            .on_action(cx.listener(Self::set_image_caption))
            .on_action(cx.listener(Self::set_table_caption))
            .on_action(cx.listener(Self::editing_cover_title))
            .on_action(cx.listener(Self::editing_cover_subtitle))
            .on_action(cx.listener(Self::editing_cover_author))
            .on_action(cx.listener(Self::open_command_palette))
            .on_action(cx.listener(Self::close_overlay))
            .on_action(cx.listener(Self::open_export_dialog))
            .on_action(cx.listener(Self::open_page_setup))
            .on_action(cx.listener(Self::print_layout))
            .on_action(cx.listener(Self::web_layout))
            .on_action(cx.listener(Self::focus_mode))
            .on_action(cx.listener(Self::show_shortcuts))
            .on_action(cx.listener(Self::show_paragraph_inspector))
            .on_action(cx.listener(Self::show_image_inspector))
            .on_action(cx.listener(Self::show_version_history))
            .on_action(cx.listener(Self::toggle_markdown_mode))
            .on_action(cx.listener(Self::toggle_ruler))
            .on_action(cx.listener(Self::set_page_margins))
            .on_action(cx.listener(Self::cycle_heading))
            .on_action(cx.listener(Self::find_and_replace))
            .on_action(cx.listener(Self::paste_into_find))
            .on_action(cx.listener(Self::heading_1))
            .on_action(cx.listener(Self::heading_2))
            .on_action(cx.listener(Self::heading_3))
            .on_action(cx.listener(Self::heading_4))
            .on_action(cx.listener(Self::heading_5))
            .on_action(cx.listener(Self::heading_6))
            .on_action(cx.listener(Self::normal_text))
            .on_action(cx.listener(Self::strikethrough_text))
            .on_action(cx.listener(Self::cycle_body_font))
            .child(self.title_bar(window, cx))
            .child(self.menu_bar(window, cx))
            .child(self.utility_bar(cx))
            .child(self.format_bar(cx))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .overflow_hidden()
                    .bg(self.ui_workspace())
                    .child(if self.sidebar_visible {
                        self.global_navigation(cx)
                    } else {
                        div().w(px(0.0))
                    })
                    .child(if self.sidebar_visible {
                        self.navigator(cx)
                    } else {
                        div().w(px(0.0))
                    })
                    .child(
                        // The find bar floats over the canvas's top-right
                        // corner without scrolling with the pages.
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .h_full()
                            .relative()
                            .flex()
                            .child(self.center_canvas(cx))
                            .when(self.find.visible, |area| area.child(self.find_bar(cx))),
                    )
                    .child(if self.inspector_visible {
                        self.inspector(cx)
                    } else {
                        div().w(px(0.0))
                    }),
            )
            .child(self.status_bar(cx));

        // Menu items act on mouse-down, before this closes the menu.
        root = root.on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                if this.context_menu.visible
                    || this.open_menu.is_some()
                    || this.open_picker.is_some()
                {
                    this.context_menu.visible = false;
                    this.open_menu = None;
                    this.open_picker = None;
                    cx.notify();
                }
            }),
        );
        root = root.when(self.context_menu.visible, |this| {
            this.child(self.context_menu_view(cx))
        });
        root = root.when(self.overlay == WorkspaceOverlay::CommandPalette, |this| {
            this.child(self.command_palette(cx))
        });
        root = root.when(self.overlay == WorkspaceOverlay::Export, |this| {
            this.child(self.export_dialog(cx))
        });
        root = root.when(self.overlay == WorkspaceOverlay::PageSetup, |this| {
            this.child(self.page_setup_dialog(cx))
        });
        root = root.when(self.overlay == WorkspaceOverlay::InsertTable, |this| {
            this.child(self.table_dialog(cx))
        });
        root = root.when(self.overlay == WorkspaceOverlay::Shortcuts, |this| {
            this.child(self.shortcuts_dialog(cx))
        });
        root
    }
}

#[cfg(test)]
mod chrome_tests {
    use super::*;

    #[test]
    fn document_label_identifies_untitled_documents_by_their_text() {
        let when = "2026-09-25 17:17";
        // "Untitled" leads with the text itself; Markdown ON reads it the
        // way the canvas shows it (no `#`), OFF literally.
        assert_eq!(
            document_label("Untitled", "\n\n# Quarterly  report\nbody", when, true),
            ("Quarterly report".to_string(), when.to_string())
        );
        assert_eq!(
            document_label("Untitled", "\n\n# Quarterly  report\nbody", when, false),
            ("# Quarterly report".to_string(), when.to_string())
        );
        // A real title leads; the text start moves to the detail line.
        assert_eq!(
            document_label("Report", "**Q3** results", when, true),
            ("Report".to_string(), format!("{when} · Q3 results"))
        );
        // Nothing typed yet.
        assert_eq!(
            document_label("Untitled", "", when, true),
            ("Untitled".to_string(), when.to_string())
        );
        // Only the start is read, and a cut inside a character is safe.
        let long = "é".repeat(2000);
        assert!(document_label("Untitled", &long, when, true)
            .0
            .starts_with("éé"));
    }

    #[test]
    fn page_format_label_uses_real_dimensions() {
        use sylph_core::document::PageSize;
        assert_eq!(page_format_label(&PageSize::A4), "A4 · 210 × 297 mm");
        // Letter was shown as "210 × 297 mm" in the inspector.
        assert_eq!(
            page_format_label(&PageSize::Letter),
            "Letter · 216 × 279 mm"
        );
    }

    #[test]
    fn ruler_marks_follow_page_geometry() {
        // A4: 21 cm wide with 2.54 cm margins → edges at 2.54 / 18.46.
        let marks = ruler_marks(21.0, 2.54, 18.46);
        assert!(marks.iter().any(|(cm, m)| *m && (cm - 2.54).abs() < 1e-3));
        assert!(marks.iter().any(|(cm, m)| *m && (cm - 18.46).abs() < 1e-3));
        assert!(
            marks.iter().any(|(cm, m)| !*m && (cm - 21.0).abs() < 1e-3),
            "page width tick"
        );
        // No plain tick collides with a margin marker, and positions ascend.
        assert!(!marks
            .iter()
            .any(|(cm, m)| !*m && ((cm - 2.54).abs() < 0.45 || (cm - 18.46).abs() < 0.45)));
        assert!(marks.windows(2).all(|w| w[0].0 <= w[1].0));

        // Letter (21.59 cm): different geometry, same rules — the ruler
        // is derived from the page setup, not hardcoded to A4.
        let letter = ruler_marks(21.59, 2.54, 19.05);
        assert!(letter.iter().any(|(cm, m)| *m && (cm - 19.05).abs() < 1e-3));
        assert!(letter
            .iter()
            .any(|(cm, m)| !*m && (cm - 21.59).abs() < 1e-3));
    }

    #[test]
    fn view_mode_chips_are_exclusive_and_total() {
        assert_eq!(view_mode_active(false, true, true), (true, false, false));
        assert_eq!(view_mode_active(true, true, true), (false, true, false));
        assert_eq!(view_mode_active(false, false, false), (false, false, true));
        // Half-hidden chrome is still Print; Web wins over Focus.
        assert_eq!(view_mode_active(false, true, false), (true, false, false));
        assert_eq!(view_mode_active(true, false, false), (false, true, false));
    }

    #[test]
    fn inspector_spacing_reflects_the_block_at_the_cursor() {
        // Headings show the spec's points; Normal shows the paragraph
        // default the export renderer applies (0 before / 8 after) —
        // never hardcoded placeholder numbers.
        let doc = sylph_core::document::Document::new();
        let spacing = |level| {
            let style = doc.resolved_style(level);
            (style.space_before, style.space_after)
        };
        assert_eq!(spacing(0), (0.0, 8.0));
        assert_eq!(spacing(1), (12.0, 6.0));
        assert_eq!(spacing(3), (8.0, 4.0));
        assert_eq!(spacing(6), (4.0, 4.0));
    }

    #[test]
    fn word_count_never_counts_markdown_tokens() {
        assert_eq!(count_words("# Title\n\nSome **bold** text."), 4);
        assert_eq!(count_words("## Intro"), 1);
        assert_eq!(count_words("- item"), 1);
    }
}
