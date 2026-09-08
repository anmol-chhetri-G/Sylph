use crate::{
    BoldText, CloseOverlay, ContextMenuCopy, ContextMenuCut, ContextMenuPaste, ExportPdf,
    InsertPageBreak, InsertTable, InspectorMode, ItalicText, NavigatorTab, NewDocument,
    OpenCommandPalette, OpenFindBar, OpenModalShowcase, PasteImage, Redo, SaveDoc, SetPageSize,
    ShowImageInspector, ShowParagraphInspector, ShowVersionHistory, SylphApp, ToggleDarkMode,
    ToggleInspector, ToggleMarkdownMode, ToggleRuler, ToggleSidebar, Undo, WorkspaceOverlay,
};
use gpui::prelude::*;
use gpui::{div, img, px, rgb, rgba, Context, Div, Focusable, MouseButton, Rgba, Stateful, Window};

const UI_FONT: &str = "Hanken Grotesk";
const PROSE_FONT: &str = "EB Garamond";
const MONO_FONT: &str = "JetBrains Mono";

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
            "Quarterly Report — Sylph".to_string()
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

        bar = bar.child(self.tool_button("☰", self.sidebar_visible).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| this.toggle_sidebar(&ToggleSidebar, window, cx)),
        ));
        bar = bar.child(self.tool_button("◧", self.inspector_visible).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| this.toggle_inspector(&ToggleInspector, window, cx)),
        ));
        bar = bar.child(divider(border));
        bar = bar.child(self.tool_button("＋", false).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| this.new_document(&NewDocument, window, cx)),
        ));
        bar = bar.child(self.tool_button("□", false));
        bar = bar.child(self.tool_button("▣", false).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| this.save_doc(&SaveDoc, window, cx)),
        ));
        bar = bar.child(divider(border));
        for (glyph, action) in [("↶", 0), ("↷", 1)] {
            let button = self.tool_button(glyph, false);
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
        bar = bar.child(self.tool_button("✂", false).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| this.context_menu_cut(&ContextMenuCut, window, cx)),
        ));
        bar = bar.child(self.tool_button("□", false).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| this.context_menu_copy(&ContextMenuCopy, window, cx)),
        ));
        bar = bar.child(self.tool_button("▣", false).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                this.context_menu_paste(&ContextMenuPaste, window, cx)
            }),
        ));
        bar = bar.child(divider(border));
        bar = bar.child(self.tool_button("⌕", false).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| this.open_find_bar(&OpenFindBar, window, cx)),
        ));
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
        for (name, active) in [("Print", true), ("Web", false), ("Focus", false)] {
            modes = modes.child(self.compact_button(name, active));
        }
        bar = bar.child(modes).child(divider(border));
        bar = bar.child(self.tool_button("▥", self.ruler_visible).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| this.toggle_ruler(&ToggleRuler, window, cx)),
        ));
        bar = bar.child(self.tool_button("＋", false));
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
                    .child(label("Heading 1", text, 12.0))
                    .child(icon("⌄", muted, 12.0)),
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
                    .child(label("Source Serif 4", text, 12.0))
                    .child(icon("⌄", muted, 12.0)),
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
                    .child(icon("−", muted, 12.0))
                    .child(label("11", text, 12.0))
                    .child(icon("＋", muted, 12.0)),
            )
            .child(divider(border));

        let mut emphasis = div().flex().items_center().gap(px(2.0));
        for (glyph, action, active) in [
            ("B", 0, true),
            ("I", 1, false),
            ("U", 2, false),
            ("S", 3, false),
        ] {
            let button = self.compact_button(glyph, active);
            emphasis = emphasis.child(match action {
                0 => button.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.bold_text(&BoldText, window, cx)),
                ),
                1 => button.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.italic_text(&ItalicText, window, cx)),
                ),
                _ => button,
            });
        }

        let mut align = div().flex().items_center().gap(px(2.0));
        for glyph in ["A", "☷", "≣", "⇥", "↔"] {
            align = align.child(self.tool_button(glyph, glyph == "A"));
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
                .child(label("1.15", text, 12.0))
                .child(icon("↕", muted, 12.0)),
        );

        let mut inserts = div().flex().items_center().gap(px(2.0));
        inserts = inserts.child(self.tool_button("↗", false));
        inserts = inserts.child(self.tool_button("▧", false).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                this.show_image_inspector(&ShowImageInspector, window, cx);
                this.paste_image(&PasteImage, window, cx);
            }),
        ));
        inserts = inserts.child(self.tool_button("▦", false).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                this.open_modal_showcase(&OpenModalShowcase, window, cx);
            }),
        ));
        inserts = inserts.child(self.tool_button("↵", false).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| this.insert_page_break(&InsertPageBreak, window, cx)),
        ));

        let mut left = controls
            .child(emphasis)
            .child(divider(border))
            .child(align)
            .child(divider(border))
            .child(inserts);
        left = left
            .child(divider(border))
            .child(self.tool_button("＋", false).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.overlay = WorkspaceOverlay::CommandPalette;
                    this.open_command_palette(&OpenCommandPalette, window, cx);
                }),
            ));

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
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .py(px(8.0))
                        .child(
                            label("1  Introduction", text, 12.0)
                                .font_weight(gpui::FontWeight(600.0)),
                        )
                        .child(label("2", muted, 10.0)),
                );
                for item in [("1.1 Background", "p.1"), ("1.2 Scope & Target", "p.1")] {
                    body = body.child(
                        div()
                            .pl(px(24.0))
                            .py(px(5.0))
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_color(muted)
                            .hover(|s| s.bg(hover).text_color(text))
                            .child(label(item.0, muted, 12.0))
                            .child(label(item.1, self.ui_faint(), 10.0).font_family(MONO_FONT)),
                    );
                }
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .py(px(8.0))
                        .child(
                            label("2  Performance Analysis", text, 12.0)
                                .font_weight(gpui::FontWeight(600.0)),
                        )
                        .child(label("2", muted, 10.0)),
                );
                body = body.child(
                    div()
                        .relative()
                        .pl(px(24.0))
                        .py(px(7.0))
                        .flex()
                        .items_center()
                        .justify_between()
                        .bg(self.ui_panel_high())
                        .text_color(primary)
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(3.0))
                                .bg(primary),
                        )
                        .child(
                            label("2.1 Revenue Trajectory", primary, 12.0)
                                .font_weight(gpui::FontWeight(600.0)),
                        )
                        .child(label("p.1", primary, 10.0).font_family(MONO_FONT)),
                );
                body = body.child(
                    div()
                        .pl(px(24.0))
                        .py(px(7.0))
                        .text_color(muted)
                        .hover(|s| s.bg(hover).text_color(text))
                        .child(label("2.2 Operational Costs", muted, 12.0)),
                );
                for title in ["3  Conclusion & Roadmap", "4  Regulatory Addenda"] {
                    body = body.child(
                        div()
                            .py(px(8.0))
                            .text_color(text)
                            .hover(|s| s.bg(hover))
                            .child(label(title, text, 12.0).font_weight(gpui::FontWeight(600.0))),
                    );
                }
            }
            NavigatorTab::Pages => {
                body = body
                    .child(label("THUMBNAILS", muted, 11.0).font_weight(gpui::FontWeight(600.0)))
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
                body = body
                    .child(label("ASSETS", muted, 11.0).font_weight(gpui::FontWeight(600.0)))
                    .child(
                        div()
                            .mt(px(8.0))
                            .p(px(10.0))
                            .bg(self.surface_color())
                            .border_1()
                            .border_color(border)
                            .child(label("▧  Telemetry Chart", text, 12.0))
                            .child(label("Figure 1 · linked", muted, 10.0)),
                    );
            }
        }

        let recent = div()
            .p(px(8.0))
            .bg(self.ui_panel_low())
            .border_t_1()
            .border_color(border)
            .child(label("RECENT GALLEY FILES", muted, 11.0).font_weight(gpui::FontWeight(600.0)))
            .child(
                div()
                    .mt(px(6.0))
                    .p(px(8.0))
                    .bg(self.surface_color())
                    .child(label("▣  Q3 Financial Review.sylph", text, 12.0))
                    .child(label("Modified 2h ago · 4.2 MB", muted, 10.0)),
            )
            .child(
                div()
                    .p(px(8.0))
                    .child(label("▤  Architecture RFC-04.md", text, 12.0))
                    .child(label("Modified yesterday · 312 KB", muted, 10.0)),
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
        let mut ticks = div()
            .w(px(816.0))
            .h(px(20.0))
            .flex()
            .items_end()
            .justify_between()
            .px(px(8.0))
            .font_family(MONO_FONT)
            .text_size(px(8.0))
            .text_color(muted);
        for (index, value) in [
            "0", "1", "2.5", "4", "6", "8", "10", "12", "14", "16", "18.5", "20", "21",
        ]
        .iter()
        .enumerate()
        {
            ticks = ticks.child(
                div()
                    .relative()
                    .pb(px(3.0))
                    .when(index == 2 || index == 10, |s| s.text_color(primary))
                    .child((*value).to_string())
                    .child(
                        div()
                            .absolute()
                            .left(px(4.0))
                            .bottom_0()
                            .w(px(1.0))
                            .h(if index == 2 || index == 10 {
                                px(10.0)
                            } else {
                                px(6.0)
                            })
                            .bg(if index == 2 || index == 10 {
                                primary
                            } else {
                                self.ui_border()
                            }),
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

        let editor = div()
            .w_full()
            .h(px(430.0))
            .flex_shrink_0()
            .mb(px(10.0))
            .overflow_hidden()
            .font_family(PROSE_FONT)
            .text_size(px(14.0))
            .line_height(px(24.0))
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

    fn render_blank_editor_page(&mut self, cx: &mut Context<Self>) -> Div {
        let border = self.ui_border();
        let text = self.ui_text();
        let muted = self.ui_muted();
        let page = self.ui_page();
        let editor = div()
            .w_full()
            .h(px(864.0))
            .flex_shrink_0()
            .overflow_hidden()
            .font_family(PROSE_FONT)
            .text_size(px(14.0))
            .line_height(px(24.0))
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
            .child(editor);

        for block in &self.document.blocks {
            match block {
                sylph_core::document::Block::PageBreak => {}
                sylph_core::document::Block::Image { data } => {
                    page_content = page_content.child(
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
                    page_content = page_content.child(table);
                }
                _ => {}
            }
        }

        page_content
    }

    fn render_blank_page(&self, _page_number: usize) -> Div {
        let border = self.ui_border();
        div()
            .w(px(816.0))
            .h(px(1056.0))
            .flex_shrink_0()
            .relative()
            .bg(self.ui_page())
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
    }

    fn center_canvas(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let page_count = 1 + self
            .document
            .blocks
            .iter()
            .filter(|block| matches!(block, sylph_core::document::Block::PageBreak))
            .count();
        let page_gap_border = self.ui_border();
        let page_gap_muted = self.ui_muted();
        let page_gap = || {
            div()
                .w(px(816.0))
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
        let mut pages = div()
            .w_full()
            .flex_shrink_0()
            .py(px(32.0))
            .px(px(16.0))
            .flex()
            .flex_col()
            .items_center()
            .child(self.render_blank_editor_page(cx));
        for page_number in 2..=page_count {
            pages = pages
                .child(page_gap())
                .child(self.render_blank_page(page_number));
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
            .child(if self.ruler_visible {
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
                    .map(|(i, v)| {
                        div()
                            .flex_1()
                            .py(px(10.0))
                            .text_center()
                            .font_family(MONO_FONT)
                            .text_size(px(11.0))
                            .text_color(if i == 1 { primary } else { muted })
                            .when(i == 1, |s| {
                                s.bg(self.surface_color())
                                    .font_weight(gpui::FontWeight(700.0))
                            })
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
                            .child(stepper("Before", "6 pt"))
                            .child(stepper("After", "6 pt")),
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
                                label("PHYSICAL GALLEY & PAPER", muted, 11.0)
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
                    .child(label("Margins (ISO standard)", muted, 11.0).mt(px(12.0)))
                    .child(
                        div().grid().grid_cols(2).gap(px(4.0)).children(
                            [
                                "Top     2.54 cm",
                                "Bottom  2.54 cm",
                                "Left    2.54 cm",
                                "Right   2.54 cm",
                            ]
                            .iter()
                            .map(|v| {
                                div()
                                    .p(px(10.0))
                                    .bg(panel)
                                    .font_family(MONO_FONT)
                                    .text_size(px(10.0))
                                    .text_color(text)
                                    .child(*v)
                            }),
                        ),
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
            let (line, column) = editor.cursor_line_and_column(editor.cursor_offset());
            let words = editor.content.split_whitespace().count();
            (line + 1, column + 1, words)
        };
        let page_count = 1 + self
            .document
            .blocks
            .iter()
            .filter(|block| matches!(block, sylph_core::document::Block::PageBreak))
            .count();
        let save_state = self.status_message.as_deref().unwrap_or("Saved 2s ago");
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
                    .child(label("A4 · 210 × 297 mm", text, 13.0))
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
            .child(
                div().grid().grid_cols(2).gap(px(4.0)).children(
                    [
                        "Top     2.54 cm",
                        "Bottom  2.54 cm",
                        "Left    2.54 cm",
                        "Right   2.54 cm",
                    ]
                    .iter()
                    .map(|v| {
                        div()
                            .p(px(12.0))
                            .bg(self.ui_panel_low())
                            .font_family(MONO_FONT)
                            .text_size(px(11.0))
                            .child(*v)
                    }),
                ),
            )
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
