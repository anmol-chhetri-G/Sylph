//! The bundled OFL fonts (assets/fonts).

/// Body fonts the font list offers: bundled families (or Word's names for
/// their metric-compatible stand-ins, see `FONT_ALIASES`), so each one
/// renders the same on every machine.
pub(crate) const BODY_FONTS: [&str; 9] = [
    "Times New Roman",
    "Arial",
    "Courier New",
    "EB Garamond",
    "Source Serif 4",
    "Lora",
    "Hanken Grotesk",
    "Inter",
    "JetBrains Mono",
];

/// Word's standard fonts, which universities ask for by name, drawn with
/// their open (OFL) metric-compatible equivalents: every character has the
/// same width, so lines and pages break as in Word, as LibreOffice does.
/// The document (and its DOCX) keeps the Word name, so Word uses the real
/// font; the canvas and the PDF use the stand-in.
pub(crate) const FONT_ALIASES: [(&str, &str); 3] = [
    ("Times New Roman", "Liberation Serif"),
    ("Arial", "Liberation Sans"),
    ("Courier New", "Liberation Mono"),
];

/// The bundled family that draws font `name`.
pub(crate) fn render_family(name: &str) -> &str {
    FONT_ALIASES
        .iter()
        .find(|(word, _)| word.eq_ignore_ascii_case(name))
        .map_or(name, |(_, bundled)| bundled)
}

/// The bundled OFL fonts (assets/fonts, each family with its OFL.txt).
///
/// The Devanagari families are registered for cosmic-text's script
/// fallback, which asks for exactly "Noto Sans Devanagari". They are never
/// named in a `font_family` or offered in a picker: GPUI 0.2.2 drops
/// faces without an `m` glyph when a family is resolved by name.
pub(crate) fn bundled_fonts() -> Vec<std::borrow::Cow<'static, [u8]>> {
    macro_rules! font {
        ($path:literal) => {
            std::borrow::Cow::Borrowed(
                &include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../assets/fonts/",
                    $path
                ))[..],
            )
        };
    }
    vec![
        font!("EBGaramond/Regular.ttf"),
        font!("EBGaramond/Italic.ttf"),
        font!("EBGaramond/Bold.ttf"),
        font!("EBGaramond/BoldItalic.ttf"),
        font!("SourceSerif4/Regular.ttf"),
        font!("SourceSerif4/Italic.ttf"),
        font!("SourceSerif4/Bold.ttf"),
        font!("SourceSerif4/BoldItalic.ttf"),
        font!("Lora/Regular.ttf"),
        font!("Lora/Italic.ttf"),
        font!("Lora/Bold.ttf"),
        font!("Lora/BoldItalic.ttf"),
        font!("Inter/Regular.ttf"),
        font!("Inter/Italic.ttf"),
        font!("Inter/Bold.ttf"),
        font!("Inter/BoldItalic.ttf"),
        font!("LiberationSerif/Regular.ttf"),
        font!("LiberationSerif/Bold.ttf"),
        font!("LiberationSerif/Italic.ttf"),
        font!("LiberationSerif/BoldItalic.ttf"),
        font!("LiberationSans/Regular.ttf"),
        font!("LiberationSans/Bold.ttf"),
        font!("LiberationSans/Italic.ttf"),
        font!("LiberationSans/BoldItalic.ttf"),
        font!("LiberationMono/Regular.ttf"),
        font!("LiberationMono/Bold.ttf"),
        font!("LiberationMono/Italic.ttf"),
        font!("LiberationMono/BoldItalic.ttf"),
        font!("HankenGrotesk/Regular.ttf"),
        font!("HankenGrotesk/SemiBold.ttf"),
        font!("HankenGrotesk/Bold.ttf"),
        font!("HankenGrotesk/Italic.ttf"),
        font!("HankenGrotesk/BoldItalic.ttf"),
        font!("JetBrainsMono/Regular.ttf"),
        font!("JetBrainsMono/Bold.ttf"),
        font!("JetBrainsMono/Italic.ttf"),
        font!("JetBrainsMono/BoldItalic.ttf"),
        font!("NotoSansDevanagari/Regular.ttf"),
        font!("NotoSansDevanagari/Bold.ttf"),
        font!("NotoSerifDevanagari/Regular.ttf"),
        font!("NotoSerifDevanagari/Bold.ttf"),
    ]
}

#[cfg(test)]
mod bundled_font_tests {
    use super::{bundled_fonts, BODY_FONTS};
    use crate::ui;

    /// The family names inside the bundled files (name ID 16, else 1).
    fn bundled_families() -> Vec<String> {
        bundled_fonts()
            .iter()
            .map(|bytes| {
                let face = ttf_parser::Face::parse(bytes, 0).expect("a valid font");
                let name = |id| {
                    face.names()
                        .into_iter()
                        .find(|n| n.name_id == id && n.is_unicode())
                        .and_then(|n| n.to_string())
                };
                name(16).or_else(|| name(1)).expect("a family name")
            })
            .collect()
    }

    #[test]
    fn every_body_font_has_bold_and_italic_faces() {
        // GPUI does not fake missing styles: without these faces, Ctrl+B
        // and Ctrl+I silently did nothing in that font.
        let mut faces: Vec<(String, u16, bool)> = Vec::new();
        for bytes in bundled_fonts() {
            let face = ttf_parser::Face::parse(&bytes, 0).unwrap();
            let family = face
                .names()
                .into_iter()
                .find(|n| n.name_id == 1 && n.is_unicode())
                .and_then(|n| n.to_string())
                .unwrap();
            faces.push((family, face.weight().to_number(), face.is_italic()));
        }
        for family in BODY_FONTS.map(super::render_family) {
            for (weight, italic) in [(400, false), (400, true), (700, false), (700, true)] {
                assert!(
                    faces
                        .iter()
                        .any(|(f, w, i)| f == family && *w == weight && *i == italic),
                    "{family} lacks weight {weight} italic {italic}"
                );
            }
        }
    }

    #[test]
    fn every_family_the_ui_names_is_bundled() {
        let families = bundled_families();
        for family in BODY_FONTS.iter().map(|f| super::render_family(f)).chain([
            ui::UI_FONT,
            ui::PROSE_FONT,
            ui::MONO_FONT,
        ]) {
            assert!(
                families.iter().any(|f| f == family),
                "{family} is named but not bundled: {families:?}"
            );
        }
        assert!(BODY_FONTS.contains(&sylph_core::document::Document::new().body_font.as_str()));
    }

    #[test]
    fn every_bundled_family_ships_its_licence() {
        let fonts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts");
        for dir in std::fs::read_dir(&fonts).unwrap().flatten() {
            if dir.path().is_dir() {
                assert!(
                    dir.path().join("OFL.txt").is_file(),
                    "{}",
                    dir.path().display()
                );
            }
        }
    }

    #[test]
    fn devanagari_fonts_keep_the_m_glyph_gpui_requires() {
        for bytes in bundled_fonts() {
            let face = ttf_parser::Face::parse(&bytes, 0).unwrap();
            assert!(face.glyph_index('m').is_some());
        }
    }
}
