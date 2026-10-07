//! Font colour and highlight in the format bar (plan card F1): two buttons
//! that open a Docs-style palette (Automatic / None, the standard swatches,
//! a custom hex). Like B / I / U they act on the selected text, or with
//! only a caret on the text typed next, until the caret moves.

use gpui::prelude::*;
use gpui::{div, px, AnyElement, Context, Focusable, KeystrokeEvent, MouseButton, Rgba, Window};
use std::ops::Range;
use sylph_core::colors::{self, PALETTE, PALETTE_COLUMNS};
use sylph_core::format::CharFormat;

use crate::rich_row::hsla_of_hex;
use crate::ui::label;
use crate::{SylphApp, TextInput};

/// Which of the two colours a menu sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Paint {
    /// The text itself.
    Text,
    /// The background behind it.
    Highlight,
}

impl Paint {
    fn get(self, format: &CharFormat) -> Option<&str> {
        match self {
            Paint::Text => format.color.as_deref(),
            Paint::Highlight => format.highlight.as_deref(),
        }
    }

    fn set(self, format: &mut CharFormat, value: Option<String>) {
        match self {
            Paint::Text => format.color = value,
            Paint::Highlight => format.highlight = value,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Paint::Text => "Text colour",
            Paint::Highlight => "Highlight",
        }
    }

    /// The row that clears it.
    fn reset_label(self) -> &'static str {
        match self {
            Paint::Text => "Automatic (the style's colour)",
            Paint::Highlight => "None",
        }
    }
}

/// Colours chosen with only a caret, for the text typed next (cleared when
/// the caret moves). `Some(None)` is a choice to clear the colour.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PendingPaint {
    color: Option<Option<String>>,
    highlight: Option<Option<String>>,
}

impl PendingPaint {
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }

    fn is_empty(&self) -> bool {
        self.color.is_none() && self.highlight.is_none()
    }

    fn choose(&mut self, paint: Paint, value: Option<String>) {
        match paint {
            Paint::Text => self.color = Some(value),
            Paint::Highlight => self.highlight = Some(value),
        }
    }

    fn chosen(&self, paint: Paint) -> Option<&Option<String>> {
        match paint {
            Paint::Text => self.color.as_ref(),
            Paint::Highlight => self.highlight.as_ref(),
        }
    }

    fn apply(&self, format: &mut CharFormat) {
        if let Some(color) = &self.color {
            Paint::Text.set(format, color.clone());
        }
        if let Some(color) = &self.highlight {
            Paint::Highlight.set(format, color.clone());
        }
    }
}

impl TextInput {
    /// Give just-typed text `inserted` the pending colour choices.
    pub(crate) fn apply_pending_paint(&mut self, inserted: Range<usize>) {
        if inserted.is_empty() || self.pending_paint.is_empty() {
            return;
        }
        let pending = self.pending_paint.clone();
        self.formats.apply(inserted, |f| pending.apply(f));
    }

    /// The colour the controls show: the selection's start, else the
    /// pending choice, else the character before the caret.
    fn paint_shown(&self, paint: Paint) -> Option<String> {
        let range = &self.selected_range;
        if range.is_empty() {
            if let Some(chosen) = self.pending_paint.chosen(paint) {
                return chosen.clone();
            }
        }
        let at = if range.is_empty() {
            let cursor = self.cursor_offset().min(self.content.len());
            self.content[..cursor]
                .chars()
                .next_back()
                .map_or(0, |c| cursor - c.len_utf8())
        } else {
            range.start
        };
        paint.get(&self.formats.format_at(at)).map(str::to_string)
    }
}

/// What a key does in the custom-hex box.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HexKey {
    /// Typing continues.
    Editing,
    /// Enter: apply what was typed.
    Apply,
    /// Escape: leave the box.
    Cancel,
}

/// A key in the hex box: hex digits (a leading `#` is ignored) up to six,
/// Backspace, Enter, Escape. Everything else is ignored, so a typo never
/// reaches the document.
pub(crate) fn edit_hex_input(
    input: &mut String,
    key: &str,
    key_char: Option<&str>,
    shortcut: bool,
) -> HexKey {
    match key {
        "enter" => return HexKey::Apply,
        "escape" => return HexKey::Cancel,
        "backspace" => {
            input.pop();
            return HexKey::Editing;
        }
        _ => {}
    }
    if shortcut {
        return HexKey::Editing;
    }
    for c in key_char.unwrap_or("").chars() {
        if c.is_ascii_hexdigit() && input.len() < 6 {
            input.push(c.to_ascii_lowercase());
        }
    }
    HexKey::Editing
}

/// The state of the colour menus beyond which one is open.
#[derive(Default)]
pub(crate) struct ColourUi {
    /// The custom-hex box, while it holds the keyboard.
    pub(crate) hex_input: Option<(Paint, String)>,
}

impl SylphApp {
    /// Set `paint` on the selected text, or for the next typed text.
    pub(crate) fn set_paint(
        &mut self,
        paint: Paint,
        value: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.open_picker = None;
        self.colour_ui.hex_input = None;
        let name = paint.name();
        let what = match &value {
            Some(hex) => hex.clone(),
            None => paint.reset_label().to_string(),
        };
        if self.has_selection(cx) {
            let applied = value.clone();
            self.editor.update(cx, |editor, cx| {
                editor.format_selection(|f| paint.set(f, applied.clone()), cx)
            });
            self.set_status(format!("{name}: {what} for the selected text"), cx);
        } else {
            self.editor.update(cx, |editor, _| {
                editor.pending_paint.choose(paint, value);
            });
            self.set_status(format!("{name}: {what} for the next typed text"), cx);
        }
        cx.notify();
    }

    /// The palette button of the format bar: `A` (or the marker) over a bar
    /// in the colour in use, opening `paint`'s palette.
    pub(crate) fn paint_button(
        &self,
        paint: Paint,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let shown = self.editor.read(cx).paint_shown(paint);
        let swatch = shown.as_deref().and_then(hsla_of_hex).map(Rgba::from);
        let open = self.open_picker == Some(crate::style_controls::Picker::for_paint(paint));
        let (glyph, id, bar_default) = match paint {
            Paint::Text => ("A", "paint-text", self.ui_text()),
            Paint::Highlight => ("▰", "paint-highlight", self.ui_muted()),
        };
        let bar = match (paint, swatch) {
            (_, Some(color)) => color,
            (Paint::Text, None) => bar_default,
            // No highlight: an empty, outlined bar.
            (Paint::Highlight, None) => Rgba {
                a: 0.0,
                ..bar_default
            },
        };
        let picker = crate::style_controls::Picker::for_paint(paint);
        let field = div()
            .id(id)
            .relative()
            .h(px(28.0))
            .px(px(6.0))
            .flex()
            .items_center()
            .gap(px(3.0))
            .rounded(px(2.0))
            .bg(if open {
                self.hover_color()
            } else {
                self.surface_color()
            })
            .cursor_pointer()
            .hover(|s| s.bg(self.hover_color()))
            .tooltip(move |_, cx| {
                cx.new(|_| {
                    crate::ui::ToolbarTip(match paint {
                        Paint::Text => "Text colour",
                        Paint::Highlight => "Highlight colour",
                    })
                })
                .into()
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    // Keep the root's click-away handler from closing it again.
                    cx.stop_propagation();
                    this.colour_ui.hex_input = None;
                    this.toggle_picker(picker, cx);
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(label(glyph, self.ui_text(), 13.0))
                    .child(
                        div()
                            .w(px(14.0))
                            .h(px(3.0))
                            .bg(bar)
                            .border_1()
                            .border_color(self.ui_border()),
                    ),
            )
            .child(crate::ui::icon("⌄", self.ui_muted(), 12.0));
        if open {
            field.child(self.colour_panel(paint, cx))
        } else {
            field
        }
    }

    /// The open palette: reset row, the swatch grid, custom hex.
    pub(crate) fn colour_panel(&self, paint: Paint, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.editor.read(cx).paint_shown(paint);
        let selected = self.has_selection(cx);
        let header = if selected {
            format!("{} · selected text", paint.name())
        } else {
            format!("{} · next typed text", paint.name())
        };
        let mut rows: Vec<AnyElement> = vec![self.picker_row(
            paint.reset_label().to_string(),
            None,
            current.is_none(),
            cx,
            move |this, cx| this.set_paint(paint, None, cx),
        )];
        rows.push(self.picker_separator());
        for chunk in PALETTE.chunks(PALETTE_COLUMNS) {
            let mut row = div().px(px(6.0)).py(px(1.0)).flex().gap(px(2.0));
            for &(name, hex) in chunk {
                let color = hsla_of_hex(hex).map(Rgba::from).unwrap_or(self.ui_text());
                let active = current.as_deref() == Some(hex);
                row = row.child(
                    div()
                        .id(hex)
                        .w(px(20.0))
                        .h(px(20.0))
                        .rounded(px(3.0))
                        .bg(color)
                        .border_1()
                        .border_color(if active {
                            self.ui_primary()
                        } else {
                            self.ui_border()
                        })
                        .cursor_pointer()
                        .tooltip(move |_, cx| cx.new(|_| crate::ui::ToolbarTip(name)).into())
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _: &mut Window, cx| {
                                cx.stop_propagation();
                                this.set_paint(paint, Some(hex.to_string()), cx);
                            }),
                        ),
                );
            }
            rows.push(row.into_any_element());
        }
        rows.push(self.picker_separator());
        rows.push(self.custom_colour_row(paint, cx));
        self.picker_shell(header, rows)
    }

    /// "Custom: #______": click to type six hex digits, Enter to apply.
    fn custom_colour_row(&self, paint: Paint, cx: &mut Context<Self>) -> AnyElement {
        let typing = match &self.colour_ui.hex_input {
            Some((p, text)) if *p == paint => Some(text.clone()),
            _ => None,
        };
        let shown = match &typing {
            Some(text) => format!("Custom: #{text}▏ (Enter to apply)"),
            None => "Custom colour…".to_string(),
        };
        let hover = self.ui_panel_low();
        div()
            .h(px(30.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .rounded(px(4.0))
            .cursor_pointer()
            .hover(move |row| row.bg(hover))
            .child(label(shown, self.ui_text(), 13.0))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window: &mut Window, cx| {
                    cx.stop_propagation();
                    this.colour_ui.hex_input = Some((paint, String::new()));
                    // The box takes the keyboard from the page, as the
                    // title does while it is renamed.
                    window.focus(&this.focus_handle);
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    /// A key while the custom-hex box is open (registered with the other
    /// keystroke observers).
    pub(crate) fn on_colour_keystroke(
        &mut self,
        event: &KeystrokeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((paint, mut text)) = self.colour_ui.hex_input.take() else {
            return;
        };
        // The palette closed (a click away): leave the box.
        if self.open_picker.is_none() {
            window.focus(&self.editor.focus_handle(cx));
            return;
        }
        let modifiers = &event.keystroke.modifiers;
        let action = edit_hex_input(
            &mut text,
            event.keystroke.key.as_str(),
            event.keystroke.key_char.as_deref(),
            modifiers.control || modifiers.platform || modifiers.alt,
        );
        match action {
            HexKey::Editing => self.colour_ui.hex_input = Some((paint, text)),
            HexKey::Cancel => {
                window.focus(&self.editor.focus_handle(cx));
                self.open_picker = None;
            }
            HexKey::Apply => match colors::normalize_hex(&text) {
                Some(hex) => {
                    window.focus(&self.editor.focus_handle(cx));
                    self.set_paint(paint, Some(hex), cx);
                    return;
                }
                None => {
                    self.colour_ui.hex_input = Some((paint, text));
                    self.set_status("A colour is six hex digits, like c0392b", cx);
                }
            },
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{edit_hex_input, HexKey, Paint, PendingPaint};
    use sylph_core::format::CharFormat;

    fn type_keys(keys: &[(&str, Option<&str>)]) -> (String, Vec<HexKey>) {
        let mut text = String::new();
        let results = keys
            .iter()
            .map(|&(key, ch)| edit_hex_input(&mut text, key, ch, false))
            .collect();
        (text, results)
    }

    #[test]
    fn the_hex_box_takes_only_hex_digits_up_to_six() {
        let keys: Vec<(&str, Option<&str>)> = "C0392BFF"
            .chars()
            .map(|c| ("x", Some(Box::leak(c.to_string().into_boxed_str()) as &str)))
            .collect();
        assert_eq!(type_keys(&keys).0, "c0392b");
        // Letters that are not hex, punctuation and a pasted `#` add nothing.
        assert_eq!(
            type_keys(&[("g", Some("g")), ("#", Some("#")), (" ", Some(" "))]).0,
            ""
        );
    }

    #[test]
    fn backspace_enter_and_escape() {
        let mut text = "c03".to_string();
        assert_eq!(
            edit_hex_input(&mut text, "backspace", None, false),
            HexKey::Editing
        );
        assert_eq!(text, "c0");
        assert_eq!(
            edit_hex_input(&mut text, "enter", None, false),
            HexKey::Apply
        );
        assert_eq!(
            edit_hex_input(&mut text, "escape", None, false),
            HexKey::Cancel
        );
        // Backspace on an empty box is not an error.
        let mut empty = String::new();
        assert_eq!(
            edit_hex_input(&mut empty, "backspace", None, false),
            HexKey::Editing
        );
        // Ctrl+A is a shortcut, not the digit a.
        assert_eq!(
            edit_hex_input(&mut empty, "a", Some("a"), true),
            HexKey::Editing
        );
        assert_eq!(empty, "");
    }

    #[test]
    fn pending_colours_apply_only_what_was_chosen() {
        let mut pending = PendingPaint::default();
        assert!(pending.is_empty());
        pending.choose(Paint::Text, Some("#ff0000".into()));
        let mut format = CharFormat {
            highlight: Some("#ffff00".into()),
            ..Default::default()
        };
        pending.apply(&mut format);
        assert_eq!(format.color.as_deref(), Some("#ff0000"));
        // The highlight was not chosen, so it is left alone.
        assert_eq!(format.highlight.as_deref(), Some("#ffff00"));
        // Choosing "none" clears the colour on what is typed.
        pending.choose(Paint::Highlight, None);
        pending.apply(&mut format);
        assert_eq!(format.highlight, None);
        pending.clear();
        assert!(pending.is_empty());
    }
}
