//! Text-coordinate helpers shared by the editor, IME adapter, and layout code.
//!
//! GPUI's platform input callbacks use UTF-16 offsets, while Rust strings and
//! GPUI's shaped-line indices use UTF-8 byte offsets.  Keeping the conversion
//! at this boundary prevents a supplementary Unicode character from turning
//! into an invalid string slice or a misplaced caret.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

/// Convert a UTF-16 offset to a valid UTF-8 byte boundary.
///
/// If the requested offset falls inside a surrogate pair, the beginning of
/// that code point is returned.  Offsets beyond the string are clamped to the
/// end of the string.
pub fn byte_offset_from_utf16(text: &str, utf16_offset: usize) -> usize {
    let mut utf16_index = 0;

    for (byte_index, ch) in text.char_indices() {
        if utf16_index >= utf16_offset {
            return byte_index;
        }

        let next_utf16_index = utf16_index + ch.len_utf16();
        if utf16_offset < next_utf16_index {
            return byte_index;
        }

        utf16_index = next_utf16_index;
    }

    text.len()
}

/// Convert a UTF-8 byte offset to a UTF-16 offset.
///
/// Invalid byte offsets are snapped backward to the preceding character
/// boundary instead of being used to slice the string.
pub fn utf16_offset_from_byte(text: &str, byte_offset: usize) -> usize {
    let boundary = snap_to_char_boundary(text, byte_offset);
    text[..boundary].chars().map(char::len_utf16).sum()
}

/// Convert a platform UTF-16 range into a safe Rust byte range.
pub fn utf8_range_from_utf16(text: &str, range: Range<usize>) -> Range<usize> {
    byte_offset_from_utf16(text, range.start)..byte_offset_from_utf16(text, range.end)
}

/// Snap an offset to the closest valid UTF-8 character boundary at or before
/// the requested byte offset.
pub fn snap_to_char_boundary(text: &str, byte_offset: usize) -> usize {
    let mut boundary = byte_offset.min(text.len());
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

/// Return the previous extended grapheme boundary before `byte_offset`.
pub fn previous_grapheme_boundary(text: &str, byte_offset: usize) -> usize {
    let offset = snap_to_char_boundary(text, byte_offset);
    if offset == 0 {
        return 0;
    }

    text.grapheme_indices(true)
        .map(|(index, _)| index)
        .rfind(|&index| index < offset)
        .unwrap_or(0)
}

/// Return the next extended grapheme boundary after `byte_offset`.
pub fn next_grapheme_boundary(text: &str, byte_offset: usize) -> usize {
    let offset = snap_to_char_boundary(text, byte_offset);
    if offset >= text.len() {
        return text.len();
    }

    text.grapheme_indices(true)
        .map(|(index, _)| index)
        .find(|&index| index > offset)
        .unwrap_or(text.len())
}

/// Move to the beginning of the word to the left of `byte_offset`.
pub fn previous_word_boundary(text: &str, byte_offset: usize) -> usize {
    let offset = snap_to_char_boundary(text, byte_offset);
    if offset == 0 {
        return 0;
    }

    let mut candidate = 0;
    for (start, segment) in text.split_word_bound_indices() {
        if start >= offset {
            break;
        }

        let end = start + segment.len();
        if segment.chars().all(char::is_whitespace) {
            continue;
        }

        candidate = start;
        if end >= offset {
            return start;
        }
    }

    candidate
}

/// Move to the beginning of the next word to the right of `byte_offset`.
pub fn next_word_boundary(text: &str, byte_offset: usize) -> usize {
    let offset = snap_to_char_boundary(text, byte_offset);
    if offset >= text.len() {
        return text.len();
    }

    let mut passed_word = false;
    for (start, segment) in text.split_word_bound_indices() {
        let end = start + segment.len();
        if end <= offset {
            continue;
        }

        if segment.chars().all(char::is_whitespace) {
            continue;
        }

        if start <= offset && offset < end {
            passed_word = true;
            continue;
        }

        if passed_word || start >= offset {
            return start;
        }
    }

    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_ascii_offsets_in_both_directions() {
        let text = "Sylph";
        assert_eq!(byte_offset_from_utf16(text, 3), 3);
        assert_eq!(utf16_offset_from_byte(text, 3), 3);
        assert_eq!(utf8_range_from_utf16(text, 1..4), 1..4);
    }

    #[test]
    fn converts_supplementary_characters() {
        let text = "A🌍B";
        assert_eq!(text.len(), 6);
        assert_eq!(byte_offset_from_utf16(text, 1), 1);
        assert_eq!(byte_offset_from_utf16(text, 2), 1);
        assert_eq!(byte_offset_from_utf16(text, 3), 5);
        assert_eq!(utf16_offset_from_byte(text, 5), 3);
        assert_eq!(utf8_range_from_utf16(text, 1..3), 1..5);
    }

    #[test]
    fn snaps_invalid_byte_offsets_backward() {
        let text = "A🌍B";
        assert_eq!(snap_to_char_boundary(text, 2), 1);
        assert_eq!(utf16_offset_from_byte(text, 2), 1);
    }

    #[test]
    fn moves_by_extended_graphemes() {
        let text = "é👨‍👩‍👧‍👦x";
        let family_start = "é".len();
        let x_start = family_start + "👨‍👩‍👧‍👦".len();

        assert_eq!(next_grapheme_boundary(text, 0), family_start);
        assert_eq!(next_grapheme_boundary(text, family_start), x_start);
        assert_eq!(previous_grapheme_boundary(text, x_start), family_start);
        assert_eq!(previous_grapheme_boundary(text, text.len()), x_start);
    }

    #[test]
    fn clamps_offsets_past_the_end() {
        let text = "hello";
        assert_eq!(byte_offset_from_utf16(text, 999), text.len());
        assert_eq!(utf16_offset_from_byte(text, 999), text.len());
        assert_eq!(next_grapheme_boundary(text, 999), text.len());
        assert_eq!(previous_grapheme_boundary(text, 0), 0);
    }

    #[test]
    fn navigates_words_without_splitting_unicode() {
        let text = "alpha 🌍 café";
        let emoji_start = text.find('🌍').unwrap();
        let cafe_start = text.find("café").unwrap();

        assert_eq!(previous_word_boundary(text, text.len()), cafe_start);
        assert_eq!(previous_word_boundary(text, emoji_start), 0);
        assert_eq!(next_word_boundary(text, 0), emoji_start);
        assert_eq!(next_word_boundary(text, emoji_start), cafe_start);
        assert_eq!(next_word_boundary(text, cafe_start), text.len());
    }
}
