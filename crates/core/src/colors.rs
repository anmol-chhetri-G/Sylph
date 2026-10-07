//! The one colour table (plan card F1): the colours the canvas and the
//! exporters share, the palette the colour menus offer, and the hex form
//! colours are stored in ("#rrggbb", as in `CharFormat::color`).
//!
//! Exports are the reference. Headings and fenced code have no colour of
//! their own there (black text), so the canvas draws them in the page's
//! text colour, which is black on the light page and follows the theme in
//! the dark one. The Python exporters repeat the constants below where they
//! draw them (`export.py`: inline code `set_text_color(200, 50, 50)`, links
//! `(0, 90, 180)`).

/// An sRGB colour.
pub type Rgb = [u8; 3];

/// Inline code's text colour: the PDF's red, now on the canvas too.
pub const CODE_TEXT: Rgb = [200, 50, 50];

/// Link text, as the PDF draws it.
pub const LINK_TEXT: Rgb = [0, 90, 180];

/// "#rrggbb" for `rgb`.
pub fn to_hex(rgb: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

/// The colour of "#rrggbb" (or "rrggbb"; any case), `None` when it is not
/// exactly six hex digits.
pub fn parse_hex(text: &str) -> Option<Rgb> {
    let digits = text.strip_prefix('#').unwrap_or(text);
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(digits, 16).ok()?;
    Some([(value >> 16) as u8, (value >> 8) as u8, value as u8])
}

/// `text` as the canonical stored form ("#rrggbb", lower case), for a hex
/// typed in the custom-colour box.
pub fn normalize_hex(text: &str) -> Option<String> {
    parse_hex(text.trim()).map(to_hex)
}

/// The palette's columns, left to right: the greys, then the hues.
pub const PALETTE_COLUMNS: usize = 10;

/// Docs' standard swatches, row by row (`PALETTE_COLUMNS` per row): black to
/// white, the saturated colours, then lighter and darker shades of each.
pub const PALETTE: &[(&str, &str)] = &[
    ("Black", "#000000"),
    ("Dark gray 4", "#434343"),
    ("Dark gray 3", "#666666"),
    ("Dark gray 2", "#999999"),
    ("Dark gray 1", "#b7b7b7"),
    ("Gray", "#cccccc"),
    ("Light gray 1", "#d9d9d9"),
    ("Light gray 2", "#efefef"),
    ("Light gray 3", "#f3f3f3"),
    ("White", "#ffffff"),
    ("Red berry", "#980000"),
    ("Red", "#ff0000"),
    ("Orange", "#ff9900"),
    ("Yellow", "#ffff00"),
    ("Green", "#00ff00"),
    ("Cyan", "#00ffff"),
    ("Cornflower blue", "#4a86e8"),
    ("Blue", "#0000ff"),
    ("Purple", "#9900ff"),
    ("Magenta", "#ff00ff"),
    ("Light red berry 3", "#e6b8af"),
    ("Light red 3", "#f4cccc"),
    ("Light orange 3", "#fce5cd"),
    ("Light yellow 3", "#fff2cc"),
    ("Light green 3", "#d9ead3"),
    ("Light cyan 3", "#d0e0e3"),
    ("Light cornflower blue 3", "#c9daf8"),
    ("Light blue 3", "#cfe2f3"),
    ("Light purple 3", "#d9d2e9"),
    ("Light magenta 3", "#ead1dc"),
    ("Dark red berry 2", "#85200c"),
    ("Dark red 2", "#990000"),
    ("Dark orange 2", "#b45f06"),
    ("Dark yellow 2", "#bf9000"),
    ("Dark green 2", "#38761d"),
    ("Dark cyan 2", "#134f5c"),
    ("Dark cornflower blue 2", "#1155cc"),
    ("Dark blue 2", "#0b5394"),
    ("Dark purple 2", "#351c75"),
    ("Dark magenta 2", "#741b47"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exporters_draw_code_and_links_in_the_tables_colours() {
        // export.py repeats these two (see the module docs): a change in
        // one place that forgets the other breaks the canvas/PDF match.
        let python = include_str!("../../../python/sylph_py/export.py");
        let call = |[r, g, b]: Rgb| format!("pdf.set_text_color({r}, {g}, {b})");
        assert!(python.contains(&call(CODE_TEXT)), "inline code colour");
        assert!(python.contains(&call(LINK_TEXT)), "link colour");
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(parse_hex("#c0392b"), Some([0xc0, 0x39, 0x2b]));
        assert_eq!(parse_hex("C0392B"), Some([0xc0, 0x39, 0x2b]));
        assert_eq!(to_hex([0xc0, 0x39, 0x2b]), "#c0392b");
        assert_eq!(normalize_hex("  #FFAA00 ").as_deref(), Some("#ffaa00"));
    }

    #[test]
    fn bad_hex_is_refused_not_guessed() {
        for bad in [
            "", "#", "#fff", "#12345", "#1234567", "#12345g", "red", "#+12345",
        ] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
        // Multibyte text of the right byte length must not panic.
        assert_eq!(parse_hex("नम"), None);
        assert_eq!(parse_hex("#ééé"), None);
    }

    #[test]
    fn the_palette_is_a_full_grid_of_valid_unique_colours() {
        assert_eq!(PALETTE.len() % PALETTE_COLUMNS, 0);
        let mut seen = std::collections::HashSet::new();
        for (name, hex) in PALETTE {
            assert!(parse_hex(hex).is_some(), "{name}: {hex}");
            assert_eq!(&to_hex(parse_hex(hex).unwrap()), hex, "{name} is canonical");
            assert!(seen.insert(*hex), "{name} repeats {hex}");
        }
    }
}
