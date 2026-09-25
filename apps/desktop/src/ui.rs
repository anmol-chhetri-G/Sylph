use crate::{
    heading_level_and_text, heading_metrics_pt, BoldText, CloseOverlay, ContextMenuCopy,
    ContextMenuCut, ContextMenuPaste, CycleBodyFont, CycleHeading, ExportPdf, InsertPageBreak,
    InsertTable, InspectorMode, ItalicText, NavigatorTab, NewDocument, OpenCommandPalette,
    OpenFindBar, OpenModalShowcase, PasteImage, Redo, SaveDoc, SetPageMargins, SetPageSize,
    ShowImageInspector, ShowParagraphInspector, ShowVersionHistory, StrikethroughText, SylphApp,
    ToggleDarkMode, ToggleInspector, ToggleMarkdownMode, ToggleRuler, ToggleSidebar, Undo,
    WorkspaceOverlay,
};
use gpui::prelude::*;
use gpui::{
    div, img, px, rgb, rgba, Context, Div, Focusable, MouseButton, Pixels, Rgba, Stateful, Window,
};

const UI_FONT: &str = "Hanken Grotesk";
const PROSE_FONT: &str = "EB Garamond";
pub(crate) const MONO_FONT: &str = "JetBrains Mono";

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

/// Spacing (points) shown for the block at the cursor: headings carry the
/// spec's space-before/after; everything else is the paragraph default the
/// export renderer applies (0 before / 8 after).
pub(crate) fn spacing_pt_for(heading_level: u8) -> (f32, f32) {
    if heading_level == 0 {
        let d = sylph_core::document::ParagraphStyle::default();
        (d.space_before, d.space_after)
    } else {
        let (_, before, after) = heading_metrics_pt(heading_level);
        (before, after)
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

fn icon(glyph: &str, color: Rgba, size: f32) -> Div {
    div()
        .font_family("Noto Sans")
        .text_size(px(size))
        .text_color(color)
        .child(glyph.to_string())
}

fn divider(color: Rgba) -> Div {
    div().w(px(1.0)).h(px(16.0)).bg(color)
}

fn label(text: impl Into<String>, color: Rgba, size: f32) -> Div {
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
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.overlay = WorkspaceOverlay::CommandPalette;
        cx.notify();
    }

    fn close_overlay(&mut self, _: &CloseOverlay, _window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = WorkspaceOverlay::None;
        self.find.visible = false;
        cx.notify();
    }

    fn open_modal_showcase(
        &mut self,
        _: &OpenModalShowcase,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.overlay = WorkspaceOverlay::ModalShowcase;
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

    fn show_image_inspector(
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
        self.markdown_mode = !self.markdown_mode;
        // The toggle is the single source of truth: the canvas follows it
        // (ON = WYSIWYG with hidden syntax, OFF = literal source).
        let on = self.markdown_mode;
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

    fn ui_primary(&self) -> Rgba {
        if self.dark_mode {
            rgb(0x3b82f6)
        } else {
            rgb(0x0037b0)
        }
    }

    fn ui_text(&self) -> Rgba {
        if self.dark_mode {
            rgb(0xf8fafc)
        } else {
            rgb(0x0b1c30)
        }
    }

    fn ui_muted(&self) -> Rgba {
        if self.dark_mode {
            rgb(0x94a3b8)
        } else {
            rgb(0x515f74)
        }
    }

    fn ui_faint(&self) -> Rgba {
        if self.dark_mode {
            rgb(0x64748b)
        } else {
            rgb(0x747686)
        }
    }

    fn ui_workspace(&self) -> Rgba {
        if self.dark_mode {
            rgb(0x0b1220)
        } else {
            rgb(0xe5eeff)
        }
    }

    fn ui_page(&self) -> Rgba {
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

    fn ui_panel_low(&self) -> Rgba {
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

    fn ui_border(&self) -> Rgba {
        if self.dark_mode {
            rgb(0x1e293b)
        } else {
            rgb(0xc4c5d7)
        }
    }

    /// Returns width in pixels for the current page size + orientation.
    fn page_width(&self) -> Pixels {
        px(self.document.page_width() * 96.0 / 72.0)
    }

    /// Returns height in pixels for the current page size + orientation.
    fn page_height(&self) -> Pixels {
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

    fn title_bar(&self, _window: &mut Window) -> Div {
        let title = if self.doc_title.is_empty() {
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
                    .child(
                        img("assets/icons/sylph-logo.png")
                            .w(px(16.0))
                            .h(px(16.0))
                            .rounded(px(2.0)),
                    )
                    .child(label(title, title_color, 12.0)),
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

    fn menu_bar(&self, cx: &mut Context<Self>) -> Div {
        let menu_fg = self.ui_text();
        let mut nav = div().flex().items_center().gap(px(2.0));
        for (index, item) in ["File", "Edit", "View", "Insert", "Format", "Tools", "Help"]
            .iter()
            .enumerate()
        {
            nav = nav.child(
                div()
                    .px(px(8.0))
                    .py(px(2.0))
                    .rounded(px(2.0))
                    .font_family(UI_FONT)
                    .text_size(px(13.0))
                    .font_weight(if index == 0 {
                        gpui::FontWeight(700.0)
                    } else {
                        gpui::FontWeight(400.0)
                    })
                    .text_color(if index == 0 {
                        self.ui_primary()
                    } else {
                        menu_fg
                    })
                    .when(index == 0, |s| s.bg(self.ui_panel_high()))
                    .hover(|s| s.bg(self.hover_color()))
                    .child((*item).to_string()),
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
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .px(px(8.0))
                            .py(px(2.0))
                            .hover(|s| s.bg(self.hover_color()))
                            .cursor_pointer()
                            .child(icon("⌯", menu_fg, 16.0))
                            .child(label("Share", menu_fg, 12.0)),
                    )
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
                                    this.open_modal_showcase(&OpenModalShowcase, window, cx);
                                }),
                            )
                            .child("Export"),
                    )
                    .child(
                        div()
                            .w(px(24.0))
                            .h(px(24.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(self.ui_primary())
                            .text_color(rgb(0xffffff))
                            .child(icon("●", rgb(0xffffff), 9.0)),
                    ),
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
                    cx.listener(move |this, _, _window, cx| {
                        match name {
                            "Print" => {
                                this.web_layout = false;
                                this.sidebar_visible = true;
                                this.inspector_visible = true;
                            }
                            "Web" => {
                                this.web_layout = true;
                            }
                            _ => {
                                this.web_layout = false;
                                this.sidebar_visible = false;
                                this.inspector_visible = false;
                            }
                        }
                        cx.notify();
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
        let history = item("◷", "Version Galley", false).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                this.show_version_history(&ShowVersionHistory, window, cx);
            }),
        );
        let print = item("▤", "Print Simulation", false).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.preview_visible = !this.preview_visible;
                cx.notify();
            }),
        );
        let setup = item("⚙", "Page Setup & Margins", false).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                this.open_modal_showcase(&OpenModalShowcase, window, cx);
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
                    .child(print)
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

        let heading_label = match self.current_heading_level(cx) {
            0 => "Normal",
            1 => "Heading 1",
            2 => "Heading 2",
            3 => "Heading 3",
            4 => "Heading 4",
            5 => "Heading 5",
            6 => "Heading 6",
            _ => "Normal",
        };
        let font_size_display = self.document.body_font_size.round() as i32;
        let controls = div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(
                div()
                    .h(px(28.0))
                    .w(px(140.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(self.surface_color())
                    .border_1()
                    .border_color(border)
                    .rounded(px(2.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(self.hover_color()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.cycle_heading(&CycleHeading, window, cx);
                        }),
                    )
                    .child(label(heading_label, text, 12.0))
                    .child(icon("⌄", muted, 12.0))
                    .id("heading-style")
                    .tooltip(|_, cx| {
                        cx.new(|_| ToolbarTip("Block style — click to cycle Normal → H1 … H6"))
                            .into()
                    }),
            )
            .child(
                div()
                    .h(px(28.0))
                    .w(px(150.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(self.surface_color())
                    .border_1()
                    .border_color(border)
                    .rounded(px(2.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(self.hover_color()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.cycle_body_font(&CycleBodyFont, window, cx);
                        }),
                    )
                    .child(label(self.document.body_font.clone(), text, 12.0))
                    .child(icon("⌄", muted, 12.0))
                    .id("body-font")
                    .tooltip(|_, cx| cx.new(|_| ToolbarTip("Body font — click to cycle")).into()),
            )
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
            let button = with_tip(self.compact_button(glyph, active), id, tip);
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
            div()
                .h(px(28.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .gap(px(4.0))
                .border_1()
                .border_color(border)
                .rounded(px(2.0))
                .cursor_pointer()
                .hover(|s| s.bg(self.hover_color()))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.cycle_line_spacing(window, cx);
                    }),
                )
                .child(label(
                    format!("{:.2}", self.document.line_spacing),
                    text,
                    12.0,
                ))
                .child(icon("↕", muted, 12.0))
                .id("line-spacing")
                .tooltip(|_, cx| {
                    cx.new(|_| ToolbarTip("Line spacing — click to cycle"))
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
                self.tool_button("▦", false),
                "insert-showcase",
                "Examples gallery",
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.open_modal_showcase(&OpenModalShowcase, window, cx);
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
                let content = self.editor.read(cx).content.clone();
                let mut headings: Vec<(u8, String)> = Vec::new();
                if self.markdown_mode {
                    // Real headings parsed from editor content with the
                    // same rules as the export parser: nothing inside ```
                    // fences is a heading, and `# ` with no text is not
                    // one either.
                    let mut in_fence = false;
                    for line in content.lines() {
                        if line.trim_start().starts_with("```") {
                            in_fence = !in_fence;
                            continue;
                        }
                        if in_fence {
                            continue;
                        }
                        if let Some((level, text)) = heading_level_and_text(line.trim_start()) {
                            headings.push((level, text.to_string()));
                        }
                    }
                    // Also include structured heading blocks from the rich document.
                    for b in &self.document.blocks {
                        if let sylph_core::document::Block::Heading { level, runs } = b {
                            let s: String = runs.iter().map(|r| r.text.as_str()).collect();
                            if !s.trim().is_empty() {
                                headings.push((*level, s));
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
                    for (level, title) in headings.iter().take(30) {
                        let indent = px((*level as f32 - 1.0).clamp(0.0, 4.0) * 12.0);
                        body = body.child(
                            div()
                                .pl(indent)
                                .py(px(5.0))
                                .text_color(muted)
                                .hover(|s| s.bg(hover).text_color(text))
                                .child(label(title.clone(), muted, 12.0)),
                        );
                    }
                }
            }
            NavigatorTab::Pages => {
                // Real page count = explicit PageBreak blocks + 1. Thumbnails not built.
                let breaks = self
                    .document
                    .blocks
                    .iter()
                    .filter(|b| matches!(b, sylph_core::document::Block::PageBreak))
                    .count();
                let count = breaks + 1;
                body = body
                    .child(
                        label(format!("PAGES ({})", count), muted, 11.0)
                            .font_weight(gpui::FontWeight(600.0)),
                    )
                    .child(div().py(px(6.0)).child(label(
                        "Page thumbnails aren’t available yet.",
                        muted,
                        11.0,
                    )))
                    .child(
                        div()
                            .mt(px(10.0))
                            .w(px(150.0))
                            .h(px(194.0))
                            .mx_auto()
                            .border_2()
                            .border_color(primary)
                            .bg(self.ui_page())
                            .child(
                                div()
                                    .m(px(14.0))
                                    .h(px(6.0))
                                    .w(px(70.0))
                                    .bg(self.ui_panel_high()),
                            )
                            .child(
                                label("P.1", primary, 10.0)
                                    .absolute()
                                    .right(px(8.0))
                                    .top(px(8.0)),
                            ),
                    )
                    .child(label("1", primary, 11.0).text_center());
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
            .child(
                div()
                    .py(px(6.0))
                    .child(label("No recent files yet.", muted, 11.0)),
            );

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

    #[allow(dead_code)]
    fn render_sample_table(&self) -> Div {
        let border = self.ui_border();
        let text = self.ui_text();
        let muted = self.ui_muted();
        let positive = rgb(0x059669);
        let negative = rgb(0xe11d48);
        let mut table = div()
            .w_full()
            .border_1()
            .border_color(border)
            .rounded(px(2.0))
            .overflow_hidden();
        let header = div()
            .flex()
            .bg(self.ui_panel_high())
            .child(
                label("Region", muted, 12.0)
                    .w(px(180.0))
                    .px(px(10.0))
                    .py(px(8.0)),
            )
            .child(
                label("Target (M$)", muted, 12.0)
                    .w(px(120.0))
                    .px(px(10.0))
                    .py(px(8.0))
                    .text_right(),
            )
            .child(
                label("Actual (M$)", muted, 12.0)
                    .w(px(120.0))
                    .px(px(10.0))
                    .py(px(8.0))
                    .text_right(),
            )
            .child(
                label("Delta (%)", muted, 12.0)
                    .flex_1()
                    .px(px(10.0))
                    .py(px(8.0))
                    .text_right(),
            );
        table = table.child(header);
        for (region, target, actual, delta, color) in [
            ("North America", "$42.0", "$48.2", "+14.7%", positive),
            ("EMEA Central", "$31.5", "$34.1", "+8.2%", positive),
            ("Asia-Pacific", "$22.0", "$21.8", "-0.9%", negative),
        ] {
            table = table.child(
                div()
                    .flex()
                    .border_t_1()
                    .border_color(border)
                    .child(
                        label(region, text, 12.0)
                            .w(px(180.0))
                            .px(px(10.0))
                            .py(px(8.0)),
                    )
                    .child(
                        label(target, muted, 12.0)
                            .w(px(120.0))
                            .px(px(10.0))
                            .py(px(8.0))
                            .text_right()
                            .font_family(MONO_FONT),
                    )
                    .child(
                        label(actual, text, 12.0)
                            .w(px(120.0))
                            .px(px(10.0))
                            .py(px(8.0))
                            .text_right()
                            .font_family(MONO_FONT),
                    )
                    .child(
                        label(delta, color, 12.0)
                            .flex_1()
                            .px(px(10.0))
                            .py(px(8.0))
                            .text_right()
                            .font_family(MONO_FONT),
                    ),
            );
        }
        table
    }

    #[allow(dead_code)]
    fn render_sample_figure(&self, cx: &mut Context<Self>) -> Div {
        let border = self.ui_border();
        let muted = self.ui_muted();
        let figure = div()
            .h(px(144.0))
            .w_full()
            .relative()
            .flex()
            .items_end()
            .justify_center()
            .gap(px(22.0))
            .px(px(32.0))
            .pb(px(16.0))
            .bg(self.ui_panel_low())
            .border_1()
            .border_color(border)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.show_image_inspector(&ShowImageInspector, window, cx);
                }),
            );
        let bars = [
            ("NA", 82.0),
            ("EMEA", 62.0),
            ("APAC", 46.0),
            ("LATAM", 32.0),
        ];
        let mut graphic = figure;
        for (name, height) in bars {
            graphic = graphic.child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.0))
                    .child(
                        div()
                            .w(px(36.0))
                            .h(px(height))
                            .bg(self.ui_primary())
                            .rounded(px(2.0)),
                    )
                    .child(label(name, muted, 9.0)),
            );
        }
        div().my(px(16.0)).child(graphic).child(
            label(
                "Figure 1: Revenue breakdown by geographic segment and latency percentile.",
                muted,
                11.0,
            )
            .font_family(PROSE_FONT)
            .italic()
            .text_center(),
        )
    }

    #[allow(dead_code)]
    /// DEAD: the live canvas is `render_blank_editor_page` (see
    /// `center_canvas`). Kept only as the legacy showcase layout — do not
    /// rewire it without porting the page-setup sync first.
    fn render_page(&mut self, cx: &mut Context<Self>) -> Div {
        let border = self.ui_border();
        let text = self.ui_text();
        let muted = self.ui_muted();
        let page = self.ui_page();
        let page_header = div()
            .flex_shrink_0()
            .mb(px(22.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .font_family(UI_FONT)
                    .text_size(px(10.0))
                    .text_color(muted)
                    .child("SECTION I · CORPORATE EDITORIAL")
                    .child(label("DOC REF: SYL-2024-Q3", muted, 10.0).font_family(MONO_FONT)),
            )
            .child(
                label(self.doc_title.clone(), text, 34.0)
                    .font_family(PROSE_FONT)
                    .font_weight(gpui::FontWeight(600.0))
                    .line_height(px(40.0)),
            )
            .child(
                label(
                    "Fiscal Year 2024 — Q3 Executive Summary & Regional Performance",
                    muted,
                    13.0,
                )
                .font_family(UI_FONT)
                .mt(px(4.0)),
            )
            .child(
                div()
                    .w_full()
                    .h(px(1.0))
                    .mt(px(16.0))
                    .bg(self.ui_panel_high()),
            );

        // The editor grows with its content (no fixed clip: the caret can
        // never float in an empty page), and the canvas body size follows
        // the toolbar's font-size control — 11pt default = 14.67px at
        // 96dpi, so what the toolbar says is what the page shows.
        let body_px = self.document.body_font_size * (4.0 / 3.0);
        let content_height = self.editor.read(cx).content_height;
        let editor_height = px(430.0).max(content_height + px(24.0));
        let editor = div()
            .w_full()
            .h(editor_height)
            .flex_shrink_0()
            .mb(px(10.0))
            .overflow_hidden()
            .font_family(PROSE_FONT)
            .text_size(px(body_px))
            .line_height(px((body_px * 1.6).max(24.0)))
            .text_color(text)
            .child(self.editor.clone())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.commit_field_edit(cx);
                }),
            );

        let mut page_content = div()
            .w(px(816.0))
            .min_h(px(1056.0))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .relative()
            .p(px(96.0))
            .bg(page)
            .text_color(text)
            .font_family(PROSE_FONT)
            .shadow_md()
            .child(
                div()
                    .absolute()
                    .top(px(96.0))
                    .left(px(96.0))
                    .right(px(96.0))
                    .bottom(px(96.0))
                    .border_1()
                    .border_color(border),
            )
            .child(page_header)
            .child(editor)
            .child(self.render_sample_table())
            .child(self.render_sample_figure(cx))
            .child(
                div()
                    .absolute()
                    .left(px(96.0))
                    .right(px(96.0))
                    .bottom(px(48.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .font_family(UI_FONT)
                    .text_size(px(10.0))
                    .text_color(muted)
                    .child("Sylph Editorial Proof — Confidential")
                    .child(label("1", muted, 10.0).font_family(MONO_FONT)),
            );

        if !self.document.blocks.is_empty() {
            for block in &self.document.blocks {
                match block {
                    sylph_core::document::Block::PageBreak => {}
                    sylph_core::document::Block::Image { data } => {
                        page_content = page_content.child(
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .child(img(data.path.clone()).max_w(px(480.0)).max_h(px(300.0)))
                                .child(
                                    label(
                                        data.caption.clone().unwrap_or_else(|| "Figure 1".into()),
                                        muted,
                                        11.0,
                                    )
                                    .italic(),
                                ),
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
                        page_content = page_content.child(table);
                    }
                    _ => {}
                }
            }
        }
        page_content
    }

    #[allow(dead_code)]
    fn render_second_page(&self) -> Div {
        let text = self.ui_text();
        let muted = self.ui_muted();
        div()
            .w(px(816.0))
            .h(px(600.0))
            .flex_shrink_0()
            .relative()
            .p(px(96.0))
            .bg(self.ui_page())
            .text_color(text)
            .font_family(PROSE_FONT)
            .shadow_md()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .font_family(UI_FONT)
                    .text_size(px(9.0))
                    .text_color(muted)
                    .child("Sylph Document Engine · Performance galley")
                    .child("Quarterly Report — 2"),
            )
            .child(
                label("2.2 Operational Costs & Infrastructure Fleet", text, 22.0)
                    .font_family(PROSE_FONT)
                    .font_weight(gpui::FontWeight(600.0))
                    .mt(px(28.0)),
            )
            .child(
                div()
                    .mt(px(18.0))
                    .font_family(PROSE_FONT)
                    .text_size(px(14.0))
                    .line_height(px(24.0))
                    .text_color(text)
                    .child("Transitioning from on-demand compute allocations to reserved container nodes reduced volatile spike pricing during peak trading intervals. The primary driver of efficiency was the localized deployment of cache nodes directly within Frankfurt Internet Exchange, which bypassed commercial cloud transit overhead for roughly 62% of transaction payloads."),
            )
            .child(
                div()
                    .mt(px(16.0))
                    .font_family(PROSE_FONT)
                    .text_size(px(14.0))
                    .line_height(px(24.0))
                    .text_color(text)
                    .child("Furthermore, memory footprint analysis revealed that deterministic deallocation strategies prevented the typical micro-stalls associated with large-scale document editing sessions in concurrent team workflows..."),
            )
    }

    /// Structured rich blocks (images, tables) rendered under the editor —
    /// shared by the Print page and the Web flow so both show exactly the
    /// same document content.
    fn rich_block_divs(&self) -> Vec<Div> {
        let border = self.ui_border();
        let text = self.ui_text();
        let muted = self.ui_muted();
        let mut out = Vec::new();
        for block in &self.document.blocks {
            match block {
                sylph_core::document::Block::PageBreak => {}
                sylph_core::document::Block::Image { data } => {
                    out.push(
                        div()
                            .mt(px(16.0))
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(img(data.path.clone()).max_w(px(480.0)).max_h(px(300.0)))
                            .child(
                                label(
                                    data.caption.clone().unwrap_or_else(|| "Figure 1".into()),
                                    muted,
                                    11.0,
                                )
                                .italic(),
                            ),
                    );
                }
                sylph_core::document::Block::Table { data } => {
                    let mut table = div().w_full().mt(px(16.0)).border_1().border_color(border);
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
                    out.push(table);
                }
                _ => {}
            }
        }
        out
    }

    fn render_blank_editor_page(&mut self, cx: &mut Context<Self>) -> Div {
        let border = self.ui_border();
        let text = self.ui_text();
        let muted = self.ui_muted();
        let page = self.ui_page();
        let page_w = self.page_width();
        let page_h = self.page_height();
        let (margin_top, margin_right, margin_bottom, margin_left) = self.page_margins_px();
        // The body size is set in points; the canvas paints pixels
        // (pt × 4/3 at 96dpi) — what the toolbar says is what the page
        // shows. The editor fills the page's layout box but never clips
        // the caret: content taller than one page grows the galley
        // (explicit page breaks split off further pages).
        let content_height = self.editor.read(cx).content_height;
        let editor_h = (page_h - margin_top - margin_bottom).max(content_height + px(24.0));
        let text_size = px(self.document.body_font_size * (4.0 / 3.0));
        let line_height = text_size * self.document.line_spacing;
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
            );

        let mut page_content = div()
            .w(page_w)
            .min_h(page_h)
            .flex_shrink_0()
            .flex()
            .flex_col()
            .relative()
            .pt(margin_top)
            .pb(margin_bottom)
            .pl(margin_left)
            .pr(margin_right)
            .bg(page)
            .text_color(text)
            .font_family(PROSE_FONT)
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
            .child(editor);

        page_content = page_content.children(self.rich_block_divs());

        // Page number footer — page 1
        page_content = page_content.child(
            div()
                .absolute()
                .left(px(0.0))
                .right(px(0.0))
                .bottom(margin_bottom - px(24.0))
                .flex()
                .justify_center()
                .font_family(MONO_FONT)
                .text_size(px(10.0))
                .text_color(muted)
                .child("1"),
        );

        page_content
    }

    /// Web layout: the document flows at the canvas width — no fixed page
    /// box, no margins, no page breaks (Word's Web Layout view). Shares the
    /// editor and the rich blocks with the Print page, so content and font
    /// size stay identical across views.
    fn render_web_editor(&mut self, cx: &mut Context<Self>) -> Div {
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
            );
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
            .child(editor)
            .children(self.rich_block_divs())
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
        let page_count = 1 + self
            .document
            .blocks
            .iter()
            .filter(|block| matches!(block, sylph_core::document::Block::PageBreak))
            .count();
        let page_w = self.page_width();
        let page_gap_border = self.ui_border();
        let page_gap_muted = self.ui_muted();
        let page_gap = || {
            div()
                .w(page_w)
                .h(px(96.0))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(10.0))
                .text_color(page_gap_muted)
                .child(div().flex_1().h(px(1.0)).bg(page_gap_border))
                .child(
                    label("↵  PAGE BREAK · NEW PAGE", page_gap_muted, 10.0).font_family(MONO_FONT),
                )
                .child(div().flex_1().h(px(1.0)).bg(page_gap_border))
        };
        // Web layout drops the page metaphor: one continuous flow, no
        // page gaps, no extra pages, no ruler (page geometry is off).
        let web = self.web_layout;
        let mut pages = div()
            .w_full()
            .flex_shrink_0()
            .py(px(32.0))
            .px(px(16.0))
            .flex()
            .flex_col()
            .items_center()
            .child(if web {
                self.render_web_editor(cx)
            } else {
                self.render_blank_editor_page(cx)
            });
        if !web {
            for page_number in 2..=page_count {
                pages = pages
                    .child(page_gap())
                    .child(self.render_blank_page(page_number));
            }
        }
        let canvas = div()
            .id("document-canvas")
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
            .child(pages);
        canvas
    }

    /*
    fn legacy_center_canvas(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let has_page_break = self
            .document
            .blocks
            .iter()
            .any(|block| matches!(block, sylph_core::document::Block::PageBreak));
        let page_gap = if has_page_break {
            div()
                .w(px(816.0))
                .h(px(96.0))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(10.0))
                .text_color(self.ui_muted())
                .child(div().flex_1().h(px(1.0)).bg(self.ui_border()))
                .child(
                    label("↵  PAGE BREAK · NEW PAGE", self.ui_muted(), 10.0).font_family(MONO_FONT),
                )
                .child(div().flex_1().h(px(1.0)).bg(self.ui_border()))
        } else {
            div()
                .h(px(48.0))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .children((0..3).map(|_| {
                    div()
                        .w(px(6.0))
                        .h(px(6.0))
                        .rounded_full()
                        .bg(self.ui_border())
                }))
        };
        let pages = div()
            .w_full()
            .flex_shrink_0()
            .py(px(32.0))
            .px(px(16.0))
            .flex()
            .flex_col()
            .items_center()
            .child(self.render_page(cx))
            .child(page_gap)
            .child(self.render_second_page());
        let canvas = div()
            .id("document-canvas")
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .overflow_y_scroll()
            .scrollbar_width(px(8.0))
            .bg(self.ui_workspace())
            .child(if self.ruler_visible {
                self.ruler()
            } else {
                div().h(px(0.0))
            })
            .child(pages);
        canvas
    }
    */

    fn paragraph_inspector(&self, cx: &mut Context<Self>) -> Div {
        let border = self.ui_border();
        let panel = self.ui_panel_low();
        let text = self.ui_text();
        let muted = self.ui_muted();
        let primary = self.ui_primary();
        let stepper = |name: &str, value: &str| {
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(label(name, muted, 11.0))
                .child(
                    div()
                        .h(px(56.0))
                        .px(px(10.0))
                        .flex()
                        .items_center()
                        .justify_between()
                        .bg(panel)
                        .child(label(value, text, 12.0).font_family(MONO_FONT))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(icon("⌃", muted, 10.0))
                                .child(icon("⌄", muted, 10.0)),
                        ),
                )
        };
        let current_ls = self.document.line_spacing;
        // Real spacing for the block at the cursor (heading metrics or the
        // paragraph default the export applies) — not placeholder numbers.
        let (before_pt, after_pt) = spacing_pt_for(self.current_heading_level(cx));
        let line_spacing = div()
            .flex()
            .items_center()
            .gap(px(2.0))
            .p(px(2.0))
            .bg(panel)
            .children(
                ["1.0", "1.15", "1.5", "2.0"]
                    .iter()
                    .enumerate()
                    .map(|(_i, v)| {
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
                                cx.listener(move |this, _, window, cx| {
                                    this.set_line_spacing_value(val, window, cx);
                                }),
                            )
                            .child(*v)
                    }),
            );
        let align = div()
            .flex()
            .items_center()
            .gap(px(2.0))
            .p(px(2.0))
            .bg(panel)
            .children(["≡", "≣", "≡", "▤"].iter().enumerate().map(|(i, glyph)| {
                div()
                    .flex_1()
                    .py(px(8.0))
                    .text_center()
                    .text_color(if i == 0 { primary } else { muted })
                    .when(i == 0, |s| s.bg(self.surface_color()))
                    .child(icon(glyph, if i == 0 { primary } else { muted }, 16.0))
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
                            .child(stepper("Before", &format!("{before_pt:.0} pt")))
                            .child(stepper("After", &format!("{after_pt:.0} pt"))),
                    )
                    .child(label("Line Spacing", muted, 11.0).mt(px(14.0)))
                    .child(line_spacing)
                    .child(div().h(px(1.0)).my(px(16.0)).bg(border))
                    .child(
                        label("ALIGNMENT & INDENTS", muted, 11.0)
                            .font_weight(gpui::FontWeight(600.0)),
                    )
                    .child(align)
                    .child(
                        div()
                            .mt(px(8.0))
                            .flex()
                            .gap(px(4.0))
                            .child(stepper("First Line", "0.00 cm"))
                            .child(stepper("Hanging", "0.00 cm")),
                    )
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
                            .child(label("Defaults", primary, 10.0)),
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
                                label(
                                    format!("{} · 210 × 297 mm", self.document.page_size.name()),
                                    text,
                                    12.0,
                                )
                                .font_weight(gpui::FontWeight(600.0)),
                            )
                            .child(icon("⌄", muted, 14.0))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    this.set_page_size(&SetPageSize, window, cx)
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
                            .child(self.compact_button("▣  Portrait", true))
                            .child(self.compact_button("▱  Landscape", false)),
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
                        label(
                            "Presets (click to cycle: Narrow → Normal → Wide)",
                            muted,
                            10.0,
                        )
                        .mt(px(4.0)),
                    )
                    .child(
                        div()
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
                                    this.set_page_margins(&SetPageMargins, window, cx);
                                }),
                            )
                            .child(label("Margins", text, 11.0))
                            .child(icon("⌄", muted, 12.0)),
                    )
                    .child(label("Pagination Style", muted, 11.0).mt(px(12.0)))
                    .child(
                        div()
                            .h(px(44.0))
                            .px(px(10.0))
                            .flex()
                            .items_center()
                            .justify_between()
                            .bg(panel)
                            .child(label("#  Bottom center, from page 1", text, 11.0))
                            .child(icon("⌄", muted, 14.0)),
                    )
                    .child(
                        div()
                            .mt(px(16.0))
                            .p(px(10.0))
                            .bg(panel)
                            .font_family(MONO_FONT)
                            .text_size(px(10.0))
                            .text_color(muted)
                            .child("Color Profile             FOGRA39 (CMYK)")
                            .child("Grid Baseline            12pt Regular")
                            .child("Hyphenation              Active (En-US)"),
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

    fn history_inspector(&self, cx: &mut Context<Self>) -> Div {
        let border = self.ui_border();
        let panel = self.ui_panel_low();
        let text = self.ui_text();
        let muted = self.ui_muted();
        let primary = self.ui_primary();
        let mut timeline = div().p(px(16.0)).relative();
        timeline = timeline.child(
            div()
                .absolute()
                .left(px(31.0))
                .top(px(22.0))
                .bottom(px(20.0))
                .w(px(1.0))
                .bg(border),
        );
        for (index, (title, meta)) in [
            ("Auto-save", "Just now · 1,284 words"),
            ("Final draft for review", "2h ago · 1,279 words"),
            ("Auto-save", "Yesterday 18:04 · 1,102 words"),
            ("First full draft", "Mon 09:12 · 860 words"),
        ]
        .iter()
        .enumerate()
        {
            let selected = index == 1;
            timeline = timeline.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.0))
                    .relative()
                    .mb(px(18.0))
                    .child(
                        div()
                            .w(px(30.0))
                            .flex_shrink_0()
                            .flex()
                            .justify_center()
                            .pt(px(6.0))
                            .child(
                                div()
                                    .w(px(if selected { 10.0 } else { 8.0 }))
                                    .h(px(if selected { 10.0 } else { 8.0 }))
                                    .rounded_full()
                                    .bg(if index == 0 { primary } else { border })
                                    .border_2()
                                    .border_color(if selected { primary } else { border }),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .p(px(10.0))
                            .when(selected, |s| {
                                s.bg(self.ui_panel_high())
                                    .border_l_3()
                                    .border_color(primary)
                            })
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(label(*title, text, 13.0).font_weight(gpui::FontWeight(
                                        if selected { 600.0 } else { 500.0 },
                                    )))
                                    .when(index == 0, |s| {
                                        s.child(label("Current", rgb(0x059669), 10.0))
                                    }),
                            )
                            .child(
                                label(*meta, if selected { primary } else { muted }, 11.0)
                                    .font_family(MONO_FONT)
                                    .mt(px(4.0)),
                            ),
                    ),
            );
        }
        div()
            .w(px(360.0))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
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
                    .bg(self.surface_color())
                    .border_b_1()
                    .border_color(border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(icon("◷", primary, 18.0))
                            .child(
                                label("Version History", text, 14.0)
                                    .font_weight(gpui::FontWeight(600.0)),
                            ),
                    )
                    .child(self.inspector_close_button(cx)),
            )
            .child(
                div()
                    .h(px(44.0))
                    .px(px(16.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(panel)
                    .border_b_1()
                    .border_color(border)
                    .child(label("◯  Compare versions", text, 12.0))
                    .child(label("Auto-saves every 2s", muted, 11.0)),
            )
            .child(div().flex_1().overflow_hidden().child(timeline))
            .child(
                div()
                    .p(px(16.0))
                    .flex()
                    .gap(px(8.0))
                    .bg(panel)
                    .border_t_1()
                    .border_color(border)
                    .child(self.compact_button("Compare with current", false).flex_1())
                    .child(self.compact_button("Restore this version", true).flex_1()),
            )
    }

    fn inspector(&self, cx: &mut Context<Self>) -> Div {
        match self.inspector_mode {
            InspectorMode::Paragraph => self.paragraph_inspector(cx),
            InspectorMode::Image => self.image_inspector(cx),
            InspectorMode::History => self.history_inspector(cx),
        }
    }

    fn status_bar(&self, cx: &mut Context<Self>) -> Div {
        let muted = self.ui_muted();
        let text = self.ui_text();
        let (line, column, word_count) = {
            let editor = self.editor.read(cx);
            cursor_status(&editor.content, editor.cursor_offset())
        };
        let page_count = 1 + self
            .document
            .blocks
            .iter()
            .filter(|block| matches!(block, sylph_core::document::Block::PageBreak))
            .count();
        // Never leak build paths here — the save indicator reads like
        // Word/Docs ("All changes saved") with transient action feedback.
        let save_state = self
            .status_message
            .as_deref()
            .unwrap_or("All changes saved");
        div()
            .h(px(28.0))
            .w_full()
            .flex()
            .items_center()
            .justify_between()
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
                        "Page 1 of {}  ·  Section 1  ·  Ln {}, Col {}",
                        page_count, line, column
                    ),
                    muted,
                    11.0,
                )
                .font_family(MONO_FONT),
            )
            .child(
                label(format!("{} words", word_count), text, 11.0)
                    .font_weight(gpui::FontWeight(500.0)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child("English (US)")
                    .child("·")
                    .child("UTF-8")
                    .child("·")
                    .child(label(format!("●  {}", save_state), rgb(0x059669), 11.0))
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.status_message = Some("All changes are saved locally".to_string());
                    cx.notify();
                }),
            )
    }

    fn command_palette(&self, cx: &mut Context<Self>) -> Div {
        let panel = self.surface_color();
        let text = self.ui_text();
        let muted = self.ui_muted();
        let primary = self.ui_primary();
        let border = self.ui_border();
        let mut rows = div().border_t_1().border_color(border);
        for (index, (glyph, title, shortcut)) in [
            ("H1", "Heading 1", "# + space"),
            ("H2", "Heading 2", "## + space"),
            ("☷", "Bullet list", "- + space"),
            ("1.", "Numbered list", "1. + space"),
            ("▦", "Table", "grid"),
            ("▧", "Image", "upload"),
            ("↵", "Page break", "Ctrl+Enter"),
            ("<>\u{00a0}", "Code block", ""),
        ]
        .iter()
        .enumerate()
        {
            let row = div()
                .h(px(50.0))
                .px(px(32.0))
                .flex()
                .items_center()
                .gap(px(16.0))
                .when(index == 0, |s| s.bg(self.ui_panel_low()))
                .hover(|s| s.bg(self.ui_panel_low()))
                .cursor_pointer()
                .child(label(*glyph, if index == 0 { primary } else { muted }, 14.0).w(px(20.0)))
                .child(label(*title, text, 16.0))
                .child(
                    label(*shortcut, muted, 12.0)
                        .font_family(MONO_FONT)
                        .ml_auto(),
                );
            let row = match *title {
                "Heading 1" => row.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.set_heading(1, window, cx);
                        this.close_overlay(&CloseOverlay, window, cx);
                    }),
                ),
                "Heading 2" => row.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.set_heading(2, window, cx);
                        this.close_overlay(&CloseOverlay, window, cx);
                    }),
                ),
                "Table" => row.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.open_modal_showcase(&OpenModalShowcase, window, cx);
                    }),
                ),
                "Image" => row.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.show_image_inspector(&ShowImageInspector, window, cx);
                        this.paste_image(&PasteImage, window, cx);
                        this.close_overlay(&CloseOverlay, window, cx);
                    }),
                ),
                "Page break" => row.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.insert_page_break(&InsertPageBreak, window, cx);
                        this.close_overlay(&CloseOverlay, window, cx);
                    }),
                ),
                _ => row,
            };
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
                            .child(label("/ Type a command...", muted, 16.0))
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
                            .child(label("● Sylph Commands v1.4", primary, 11.0)),
                    ),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.close_overlay(&CloseOverlay, window, cx);
                }),
            )
    }

    fn modal_showcase(&self, cx: &mut Context<Self>) -> Div {
        let panel = self.surface_color();
        let text = self.ui_text();
        let muted = self.ui_muted();
        let primary = self.ui_primary();
        let border = self.ui_border();
        let setup = div()
            .w(px(360.0))
            .h(px(680.0))
            .bg(panel)
            .rounded(px(8.0))
            .border_1()
            .border_color(border)
            .p(px(24.0))
            .child(label("Page Setup & Margins", text, 16.0).font_weight(gpui::FontWeight(600.0)))
            .child(label("Physical format", muted, 11.0).mt(px(28.0)))
            .child(
                div()
                    .h(px(44.0))
                    .mt(px(8.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(self.ui_panel_low())
                    .child(label(
                        format!(
                            "{} · {} × {} mm",
                            self.document.page_size.name(),
                            if self.document.page_size == sylph_core::document::PageSize::A4 {
                                210
                            } else {
                                216
                            },
                            if self.document.page_size == sylph_core::document::PageSize::A4 {
                                297
                            } else {
                                279
                            }
                        ),
                        text,
                        13.0,
                    ))
                    .child(icon("⌄", muted, 14.0)),
            )
            .child(label("Orientation", muted, 11.0).mt(px(16.0)))
            .child(
                div()
                    .flex()
                    .mt(px(8.0))
                    .child(self.compact_button("▣  Portrait", true))
                    .child(self.compact_button("▱  Landscape", false)),
            )
            .child(label("Margins", muted, 11.0).mt(px(16.0)))
            .child({
                let m = &self.document.page_margins;
                let cm = 2.54 / 72.0;
                let mt = format!("Top     {:.2} cm", m.top * cm);
                let mb = format!("Bottom  {:.2} cm", m.bottom * cm);
                let ml = format!("Left    {:.2} cm", m.left * cm);
                let mr = format!("Right   {:.2} cm", m.right * cm);
                div()
                    .grid()
                    .grid_cols(2)
                    .gap(px(4.0))
                    .children([mt, mb, ml, mr].iter().map(|v| {
                        div()
                            .p(px(12.0))
                            .bg(self.ui_panel_low())
                            .font_family(MONO_FONT)
                            .text_size(px(11.0))
                            .child(v.clone())
                    }))
            })
            .child(
                div()
                    .absolute()
                    .bottom(px(20.0))
                    .left(px(24.0))
                    .right(px(24.0))
                    .flex()
                    .justify_between()
                    .child(self.compact_button("Cancel", false).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.close_overlay(&CloseOverlay, window, cx)
                        }),
                    ))
                    .child(self.compact_button("Apply", true).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.close_overlay(&CloseOverlay, window, cx)
                        }),
                    )),
            );

        let table = div()
            .w(px(526.0))
            .h(px(680.0))
            .bg(panel)
            .rounded(px(8.0))
            .border_1()
            .border_color(border)
            .p(px(24.0))
            .child(label("▦  Insert Table", text, 16.0).font_weight(gpui::FontWeight(600.0)))
            .child(
                label(
                    "3 × 4 Table                 Highlight grid to set dimensions",
                    muted,
                    12.0,
                )
                .mt(px(34.0)),
            )
            .child(
                div()
                    .h(px(268.0))
                    .mt(px(24.0))
                    .p(px(16.0))
                    .grid()
                    .grid_cols(10)
                    .gap(px(2.0))
                    .bg(self.ui_panel_low())
                    .children((0..60).map(|i| {
                        div()
                            .h(px(22.0))
                            .bg(if i / 10 < 4 && i % 10 < 3 {
                                self.ui_panel_high()
                            } else {
                                self.surface_color()
                            })
                            .border_1()
                            .border_color(border)
                    })),
            )
            .child(label("TABLE STYLE PRESET", muted, 11.0).mt(px(22.0)))
            .child(
                div()
                    .flex()
                    .gap(px(8.0))
                    .mt(px(8.0))
                    .child(self.compact_button("Grid", false).flex_1())
                    .child(self.compact_button("Striped", true).flex_1())
                    .child(self.compact_button("Header-only", false).flex_1()),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(20.0))
                    .left(px(24.0))
                    .right(px(24.0))
                    .flex()
                    .justify_end()
                    .gap(px(12.0))
                    .child(self.compact_button("Cancel", false).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.close_overlay(&CloseOverlay, window, cx)
                        }),
                    ))
                    .child(self.compact_button("Insert", true).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.insert_table(&InsertTable, window, cx);
                            this.overlay = WorkspaceOverlay::None;
                        }),
                    )),
            );

        let export = div()
            .w(px(360.0))
            .h(px(680.0))
            .bg(panel)
            .rounded(px(8.0))
            .border_1()
            .border_color(border)
            .p(px(24.0))
            .child(label("⇩  Export Document", text, 16.0).font_weight(gpui::FontWeight(600.0)))
            .child(label("SELECT FORMAT", muted, 11.0).mt(px(34.0)))
            .child(
                div()
                    .mt(px(8.0))
                    .p(px(14.0))
                    .bg(self.ui_panel_low())
                    .border_1()
                    .border_color(primary)
                    .child(label("PDF Document", text, 14.0))
                    .child(
                        label("Print-ready, embedded fonts & vector graphics", muted, 11.0)
                            .mt(px(22.0)),
                    ),
            )
            .child(
                div()
                    .mt(px(8.0))
                    .p(px(14.0))
                    .border_1()
                    .border_color(border)
                    .child(label("Markdown (.md)", text, 14.0))
                    .child(label("Plain text with CommonMark syntax", muted, 11.0).mt(px(22.0))),
            )
            .child(
                div()
                    .mt(px(16.0))
                    .p(px(12.0))
                    .bg(self.ui_panel_low())
                    .child(label("☑  Include table of contents", text, 12.0))
                    .child(label("☑  Embed fonts and SVG assets", text, 12.0).mt(px(8.0))),
            )
            .child(label("Destination", muted, 11.0).mt(px(16.0)))
            .child(
                div()
                    .p(px(12.0))
                    .bg(self.ui_panel_low())
                    .font_family(MONO_FONT)
                    .text_size(px(11.0))
                    .child("~/Documents/Quarterly_Report_2024.pdf"),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(20.0))
                    .left(px(24.0))
                    .right(px(24.0))
                    .flex()
                    .justify_end()
                    .child(self.compact_button("Export PDF", true).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.export_pdf(&ExportPdf, window, cx);
                            this.overlay = WorkspaceOverlay::None;
                        }),
                    )),
            );

        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(20.0))
            .bg(rgba(0x0b1c3088))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(20.0))
                    .child(setup)
                    .child(table)
                    .child(export),
            )
            .child(label("Sylph Professional Document Workspace · Architectural Composition Layer · Esc to dismiss all", rgb(0xeaf1ff), 12.0))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, cx| {
                this.close_overlay(&CloseOverlay, window, cx);
            }))
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
            .on_action(cx.listener(Self::open_command_palette))
            .on_action(cx.listener(Self::close_overlay))
            .on_action(cx.listener(Self::open_modal_showcase))
            .on_action(cx.listener(Self::show_paragraph_inspector))
            .on_action(cx.listener(Self::show_image_inspector))
            .on_action(cx.listener(Self::show_version_history))
            .on_action(cx.listener(Self::toggle_markdown_mode))
            .on_action(cx.listener(Self::toggle_ruler))
            .on_action(cx.listener(Self::set_page_margins))
            .on_action(cx.listener(Self::cycle_heading))
            .on_action(cx.listener(Self::cycle_body_font))
            .child(self.title_bar(window))
            .child(self.menu_bar(cx))
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
                    .child(self.center_canvas(cx))
                    .child(if self.inspector_visible {
                        self.inspector(cx)
                    } else {
                        div().w(px(0.0))
                    }),
            )
            .child(self.status_bar(cx));

        root = root.when(self.overlay == WorkspaceOverlay::CommandPalette, |this| {
            this.child(self.command_palette(cx))
        });
        root = root.when(self.overlay == WorkspaceOverlay::ModalShowcase, |this| {
            this.child(self.modal_showcase(cx))
        });
        root
    }
}

#[cfg(test)]
mod chrome_tests {
    use super::*;

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
        assert_eq!(spacing_pt_for(0), (0.0, 8.0));
        assert_eq!(spacing_pt_for(1), (12.0, 6.0));
        assert_eq!(spacing_pt_for(3), (8.0, 4.0));
        assert_eq!(spacing_pt_for(6), (4.0, 4.0));
    }

    #[test]
    fn word_count_never_counts_markdown_tokens() {
        assert_eq!(count_words("# Title\n\nSome **bold** text."), 4);
        assert_eq!(count_words("## Intro"), 1);
        assert_eq!(count_words("- item"), 1);
    }
}
