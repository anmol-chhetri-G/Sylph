//! Paragraph styles in the format bar: the Style, Font and Line spacing
//! dropdowns. Font and spacing act on the style at the caret (Normal or
//! Heading 1–6), so changing them changes every paragraph of that style,
//! as Word's styles do.

use gpui::prelude::*;
use gpui::{deferred, div, px, AnyElement, Context, Div, MouseButton, Window};
use sylph_core::document::{LINE_SPACING_CHOICES, PARAGRAPH_SPACING_CHOICES};

use crate::fonts::BODY_FONTS;
use crate::ui::{icon, label};
use crate::SylphApp;

/// The format-bar dropdown that is open, if any.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Picker {
    Style,
    Font,
    LineSpacing,
}

/// "Normal", "Heading 1" … "Heading 6".
pub(crate) fn style_name(level: u8) -> String {
    match level {
        1..=6 => format!("Heading {level}"),
        _ => "Normal".to_string(),
    }
}

/// How a line spacing reads in the controls: "1.0", "1.15", "1.5".
pub(crate) fn spacing_label(spacing: f32) -> String {
    let text = format!("{spacing:.2}");
    match text.strip_suffix('0') {
        Some(short) => short.to_string(),
        None => text,
    }
}

impl SylphApp {
    /// The paragraph style the caret is in: 0 Normal, 1–6 headings.
    /// Without Markdown mode every line is Normal text.
    pub(crate) fn caret_style(&self, cx: &mut Context<Self>) -> u8 {
        self.current_heading_level(cx)
    }

    /// Give the editor the document's heading styles when they differ
    /// (the canvas lays heading rows out with them).
    pub(crate) fn sync_editor_styles(&mut self, cx: &mut Context<Self>) {
        let styles = self.document.resolved_styles();
        if self.editor.read(cx).styles != styles {
            self.editor.update(cx, |editor, cx| {
                editor.styles = styles;
                cx.notify();
            });
        }
    }

    pub(crate) fn toggle_picker(&mut self, picker: Picker, cx: &mut Context<Self>) {
        self.open_picker = if self.open_picker == Some(picker) {
            None
        } else {
            Some(picker)
        };
        self.open_menu = None;
        cx.notify();
    }

    /// Line spacing for every paragraph of the caret's style.
    pub(crate) fn set_style_spacing(&mut self, spacing: Option<f32>, cx: &mut Context<Self>) {
        let level = self.caret_style(cx);
        match (level, spacing) {
            (1..=6, None) => self.document.heading_styles[level as usize - 1].line_spacing = None,
            (_, Some(spacing)) => self.document.set_style_line_spacing(level, spacing),
            (_, None) => {}
        }
        let value = spacing.map_or("automatic".to_string(), spacing_label);
        self.set_status(
            format!(
                "Line spacing {value} for every {} paragraph",
                style_name(level)
            ),
            cx,
        );
        self.open_picker = None;
        cx.notify();
    }

    /// Space above (`before`) or below every paragraph of the caret's
    /// style, in points.
    pub(crate) fn set_style_paragraph_space(
        &mut self,
        before: bool,
        points: f32,
        cx: &mut Context<Self>,
    ) {
        let level = self.caret_style(cx);
        if before {
            self.document.set_style_space_before(level, points);
        } else {
            self.document.set_style_space_after(level, points);
        }
        self.set_status(
            format!(
                "{} {} pt for every {} paragraph",
                if before {
                    "Space before"
                } else {
                    "Space after"
                },
                points,
                style_name(level)
            ),
            cx,
        );
        self.open_picker = None;
        cx.notify();
    }

    /// Font for every paragraph of the caret's style.
    pub(crate) fn set_style_font(&mut self, font: &str, cx: &mut Context<Self>) {
        let level = self.caret_style(cx);
        self.document.set_style_font(level, font);
        self.set_status(
            format!("{font} for every {} paragraph", style_name(level)),
            cx,
        );
        self.open_picker = None;
        cx.notify();
    }

    /// One font for the body and every heading.
    pub(crate) fn set_font_everywhere(&mut self, font: &str, cx: &mut Context<Self>) {
        self.document.set_font_everywhere(font);
        self.set_status(format!("{font} for the whole document"), cx);
        self.open_picker = None;
        cx.notify();
    }

    /// A dropdown field of the format bar: `value` and a chevron, opening
    /// `picker` below it.
    pub(crate) fn picker_field(
        &self,
        picker: Picker,
        width: f32,
        value: impl Into<String>,
        value_font: Option<String>,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let (text, muted) = (self.ui_text(), self.ui_muted());
        let open = self.open_picker == Some(picker);
        let mut shown = label(value, if enabled { text } else { muted }, 12.0).truncate();
        if let Some(font) = value_font {
            shown = shown.font_family(font);
        }
        let field = div()
            .relative()
            .h(px(28.0))
            .w(px(width))
            .px(px(8.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(4.0))
            .bg(if open {
                self.hover_color()
            } else {
                self.surface_color()
            })
            .border_1()
            .border_color(self.ui_border())
            .rounded(px(2.0))
            .cursor_pointer()
            .hover(|s| s.bg(self.hover_color()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    // Keep the root's click-away handler from closing it again.
                    cx.stop_propagation();
                    this.toggle_picker(picker, cx);
                }),
            )
            .child(shown)
            .child(icon("⌄", muted, 12.0));
        if open {
            field.child(self.picker_panel(picker, cx))
        } else {
            field
        }
    }

    fn picker_panel(&self, picker: Picker, cx: &mut Context<Self>) -> impl IntoElement {
        let level = self.caret_style(cx);
        let style = style_name(level);
        let (header, rows): (String, Vec<AnyElement>) = match picker {
            Picker::Style => (
                if self.markdown_mode {
                    "Paragraph style".to_string()
                } else {
                    "Styles need Markdown mode".to_string()
                },
                (0..=6u8)
                    .map(|l| {
                        self.picker_row(style_name(l), None, l == level, cx, move |this, cx| {
                            this.open_picker = None;
                            this.apply_heading_level(l, cx);
                        })
                    })
                    .collect(),
            ),
            Picker::Font => {
                let current = self.document.style_font(level).to_string();
                let mut rows: Vec<AnyElement> = BODY_FONTS
                    .iter()
                    .map(|&font| {
                        self.picker_row(
                            font.to_string(),
                            Some(font.to_string()),
                            font == current,
                            cx,
                            move |this, cx| this.set_style_font(font, cx),
                        )
                    })
                    .collect();
                rows.push(self.picker_separator());
                let everywhere = current.clone();
                rows.push(self.picker_row(
                    format!("Use {current} for the whole document"),
                    None,
                    false,
                    cx,
                    move |this, cx| this.set_font_everywhere(&everywhere, cx),
                ));
                (format!("Font · every {style} paragraph"), rows)
            }
            Picker::LineSpacing => {
                let current = self.document.style_line_spacing(level);
                let explicit = match level {
                    1..=6 => self.document.heading_styles[level as usize - 1].line_spacing,
                    _ => Some(self.document.line_spacing),
                };
                let mut rows: Vec<AnyElement> = Vec::new();
                if level > 0 {
                    rows.push(self.picker_row(
                        format!(
                            "Automatic ({})",
                            spacing_label(sylph_core::document::HEADING_LINE_SPACING)
                        ),
                        None,
                        explicit.is_none(),
                        cx,
                        |this, cx| this.set_style_spacing(None, cx),
                    ));
                }
                rows.extend(LINE_SPACING_CHOICES.iter().map(|&choice| {
                    self.picker_row(
                        spacing_label(choice),
                        None,
                        explicit.is_some() && (current - choice).abs() < 0.01,
                        cx,
                        move |this, cx| this.set_style_spacing(Some(choice), cx),
                    )
                }));
                let resolved = self.document.resolved_style(level);
                for (before, title, current) in [
                    (true, "Space before", resolved.space_before),
                    (false, "Space after", resolved.space_after),
                ] {
                    rows.push(self.picker_separator());
                    rows.push(self.picker_heading(&format!("{title} (pt)")));
                    rows.push(self.spacing_chips(before, current, cx));
                }
                (format!("Spacing · every {style} paragraph"), rows)
            }
        };
        let panel = div()
            .absolute()
            .top(px(30.0))
            .left(px(0.0))
            .min_w(px(240.0))
            .p(px(4.0))
            .flex()
            .flex_col()
            .bg(self.surface_color())
            .border_1()
            .border_color(self.ui_border())
            .rounded(px(6.0))
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(8.0))
                    .py(px(6.0))
                    .child(label(header, self.ui_muted(), 11.0)),
            )
            .children(rows);
        deferred(panel).with_priority(2)
    }

    /// One row of point values for space before/after; the current one is
    /// highlighted.
    fn spacing_chips(&self, before: bool, current: f32, cx: &mut Context<Self>) -> AnyElement {
        let (text, primary, hover) = (self.ui_text(), self.ui_primary(), self.ui_panel_low());
        let mut row = div().px(px(6.0)).pb(px(4.0)).flex().gap(px(4.0));
        for points in PARAGRAPH_SPACING_CHOICES {
            let active = (current - points).abs() < 0.01;
            row = row.child(
                div()
                    .flex_1()
                    .h(px(26.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(if active { primary } else { self.ui_border() })
                    .cursor_pointer()
                    .hover(move |chip| chip.bg(hover))
                    .child(label(
                        format!("{points}"),
                        if active { primary } else { text },
                        12.0,
                    ))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _: &mut Window, cx| {
                            cx.stop_propagation();
                            this.set_style_paragraph_space(before, points, cx);
                        }),
                    ),
            );
        }
        row.into_any_element()
    }

    fn picker_heading(&self, title: &str) -> AnyElement {
        div()
            .px(px(8.0))
            .py(px(4.0))
            .child(label(title.to_string(), self.ui_muted(), 11.0))
            .into_any_element()
    }

    fn picker_separator(&self) -> AnyElement {
        div()
            .h(px(1.0))
            .my(px(4.0))
            .bg(self.ui_border())
            .into_any_element()
    }

    fn picker_row(
        &self,
        text: String,
        font: Option<String>,
        checked: bool,
        cx: &mut Context<Self>,
        on_pick: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let mut name = label(text, self.ui_text(), 13.0).flex_1();
        if let Some(font) = font {
            name = name.font_family(font);
        }
        let hover = self.ui_panel_low();
        div()
            .h(px(30.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .rounded(px(4.0))
            .cursor_pointer()
            .hover(move |row| row.bg(hover))
            .child(label(if checked { "✓" } else { "" }, self.ui_primary(), 12.0).w(px(14.0)))
            .child(name)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _: &mut Window, cx| {
                    cx.stop_propagation();
                    on_pick(this, cx);
                }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{spacing_label, style_name};

    #[test]
    fn heading_lines_carry_their_style_level() {
        for level in 1..=6u8 {
            let line = format!("{} Title", "#".repeat(level as usize));
            assert_eq!(crate::display_line(&line, false, true).heading, level);
        }
        // Normal text, and headings while Markdown is off, are Normal.
        assert_eq!(crate::display_line("Body", false, true).heading, 0);
        assert_eq!(crate::display_line("# Title", false, false).heading, 0);
        assert_eq!(crate::display_line("# Title", true, true).heading, 0);
    }

    #[test]
    fn labels_read_like_word() {
        assert_eq!(style_name(0), "Normal");
        assert_eq!(style_name(3), "Heading 3");
        assert_eq!(spacing_label(1.0), "1.0");
        assert_eq!(spacing_label(1.15), "1.15");
        assert_eq!(spacing_label(1.5), "1.5");
        assert_eq!(spacing_label(2.0), "2.0");
    }
}
