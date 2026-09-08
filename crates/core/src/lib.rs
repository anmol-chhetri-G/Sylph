pub mod document;
pub mod text;

use yrs::{Doc, GetString, Text, TextRef, Transact};

pub use text::{
    byte_offset_from_utf16, next_grapheme_boundary, next_word_boundary, previous_grapheme_boundary,
    previous_word_boundary, snap_to_char_boundary, utf16_offset_from_byte, utf8_range_from_utf16,
};

pub struct CrdtDocument {
    doc: Doc,
    text: TextRef,
}

impl CrdtDocument {
    pub fn new() -> Self {
        let doc = Doc::new();
        let text = doc.get_or_insert_text("content");
        Self { doc, text }
    }

    pub fn doc(&self) -> &Doc {
        &self.doc
    }

    pub fn doc_mut(&mut self) -> &mut Doc {
        &mut self.doc
    }

    pub fn get_text(&self) -> String {
        let txn = self.doc.transact();
        self.text.get_string(&txn)
    }

    pub fn insert(&self, index: u32, text: &str) {
        let mut txn = self.doc.transact_mut();
        self.text.insert(&mut txn, index, text);
    }

    pub fn delete(&self, index: u32, length: u32) {
        let mut txn = self.doc.transact_mut();
        self.text.remove_range(&mut txn, index, length);
    }

    pub fn replace(&self, index: u32, length: u32, new_text: &str) {
        let mut txn = self.doc.transact_mut();
        self.text.remove_range(&mut txn, index, length);
        self.text.insert(&mut txn, index, new_text);
    }

    pub fn len(&self) -> u32 {
        let txn = self.doc.transact();
        self.text.len(&txn)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for CrdtDocument {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Construction ──────────────────────────────────────────────

    #[test]
    fn test_new_document_is_empty() {
        let doc = CrdtDocument::new();
        assert_eq!(doc.get_text(), "");
        assert_eq!(doc.len(), 0);
        assert!(doc.is_empty());
    }

    #[test]
    fn test_default_trait() {
        let doc = CrdtDocument::default();
        assert!(doc.is_empty());
        assert_eq!(doc.len(), 0);
    }

    #[test]
    fn test_doc_accessors() {
        let mut doc = CrdtDocument::new();
        let _ = doc.doc();
        let _ = doc.doc_mut();
    }

    // ── Insert ────────────────────────────────────────────────────

    #[test]
    fn test_insert_at_beginning() {
        let doc = CrdtDocument::new();
        doc.insert(0, "abc");
        assert_eq!(doc.get_text(), "abc");
        assert_eq!(doc.len(), 3);
    }

    #[test]
    fn test_insert_at_end() {
        let doc = CrdtDocument::new();
        doc.insert(0, "abc");
        doc.insert(3, "def");
        assert_eq!(doc.get_text(), "abcdef");
    }

    #[test]
    fn test_insert_in_middle() {
        let doc = CrdtDocument::new();
        doc.insert(0, "ac");
        doc.insert(1, "b");
        assert_eq!(doc.get_text(), "abc");
    }

    #[test]
    fn test_insert_empty_string() {
        let doc = CrdtDocument::new();
        doc.insert(0, "hello");
        doc.insert(2, "");
        assert_eq!(doc.get_text(), "hello");
        assert_eq!(doc.len(), 5);
    }

    #[test]
    fn test_insert_unicode() {
        let doc = CrdtDocument::new();
        doc.insert(0, "Hello");
        doc.insert(5, " 🌍");
        assert_eq!(doc.get_text(), "Hello 🌍");
    }

    #[test]
    fn test_insert_multichar_emoji() {
        let doc = CrdtDocument::new();
        doc.insert(0, "Hi");
        doc.insert(2, " 👨‍👩‍👧‍👦");
        assert_eq!(doc.get_text(), "Hi 👨‍👩‍👧‍👦");
    }

    #[test]
    fn test_insert_newlines() {
        let doc = CrdtDocument::new();
        doc.insert(0, "line1");
        doc.insert(5, "\nline2\nline3");
        assert_eq!(doc.get_text(), "line1\nline2\nline3");
        assert_eq!(doc.len(), 17);
    }

    // ── Delete ────────────────────────────────────────────────────

    #[test]
    fn test_delete_from_start() {
        let doc = CrdtDocument::new();
        doc.insert(0, "abcdef");
        doc.delete(0, 2);
        assert_eq!(doc.get_text(), "cdef");
    }

    #[test]
    fn test_delete_from_end() {
        let doc = CrdtDocument::new();
        doc.insert(0, "abcdef");
        doc.delete(4, 2);
        assert_eq!(doc.get_text(), "abcd");
    }

    #[test]
    fn test_delete_from_middle() {
        let doc = CrdtDocument::new();
        doc.insert(0, "abcdef");
        doc.delete(2, 2);
        assert_eq!(doc.get_text(), "abef");
    }

    #[test]
    fn test_delete_entire_text() {
        let doc = CrdtDocument::new();
        doc.insert(0, "hello");
        doc.delete(0, 5);
        assert!(doc.is_empty());
        assert_eq!(doc.get_text(), "");
    }

    #[test]
    fn test_delete_zero_length() {
        let doc = CrdtDocument::new();
        doc.insert(0, "hello");
        doc.delete(2, 0);
        assert_eq!(doc.get_text(), "hello");
    }

    // ── Replace ───────────────────────────────────────────────────

    #[test]
    fn test_replace_entire_text() {
        let doc = CrdtDocument::new();
        doc.insert(0, "hello");
        doc.replace(0, 5, "world");
        assert_eq!(doc.get_text(), "world");
    }

    #[test]
    fn test_replace_subset_with_longer() {
        let doc = CrdtDocument::new();
        doc.insert(0, "abcde");
        doc.replace(1, 2, "XYZ");
        assert_eq!(doc.get_text(), "aXYZde");
    }

    #[test]
    fn test_replace_subset_with_shorter() {
        let doc = CrdtDocument::new();
        doc.insert(0, "abcde");
        doc.replace(1, 3, "x");
        assert_eq!(doc.get_text(), "axe");
    }

    #[test]
    fn test_replace_with_empty() {
        let doc = CrdtDocument::new();
        doc.insert(0, "hello");
        doc.replace(1, 3, "");
        assert_eq!(doc.get_text(), "ho");
    }

    #[test]
    fn test_replace_at_zero() {
        let doc = CrdtDocument::new();
        doc.insert(0, "abc");
        doc.replace(0, 1, "X");
        assert_eq!(doc.get_text(), "Xbc");
    }

    // ── Sequential Operations ─────────────────────────────────────

    #[test]
    fn test_sequential_inserts_and_deletes() {
        let doc = CrdtDocument::new();
        doc.insert(0, "aaa");
        doc.insert(3, "bbb");
        assert_eq!(doc.get_text(), "aaabbb");
        doc.delete(0, 3);
        assert_eq!(doc.get_text(), "bbb");
        doc.insert(0, "ccc");
        assert_eq!(doc.get_text(), "cccbbb");
    }

    #[test]
    fn test_sequential_replaces() {
        let doc = CrdtDocument::new();
        doc.insert(0, "aaaaa");
        doc.replace(0, 1, "b");
        assert_eq!(doc.get_text(), "baaaa");
        doc.replace(4, 1, "b");
        assert_eq!(doc.get_text(), "baaab");
        doc.replace(2, 1, "b");
        assert_eq!(doc.get_text(), "babab");
    }

    // ── Len / Is Empty ────────────────────────────────────────────

    #[test]
    fn test_len_after_inserts() {
        let doc = CrdtDocument::new();
        assert_eq!(doc.len(), 0);
        doc.insert(0, "a");
        assert_eq!(doc.len(), 1);
        doc.insert(1, "bb");
        assert_eq!(doc.len(), 3);
        doc.insert(0, "c");
        assert_eq!(doc.len(), 4);
    }

    #[test]
    fn test_len_after_deletes() {
        let doc = CrdtDocument::new();
        doc.insert(0, "hello");
        assert_eq!(doc.len(), 5);
        doc.delete(0, 2);
        assert_eq!(doc.len(), 3);
        doc.delete(0, 3);
        assert!(doc.is_empty());
    }

    #[test]
    fn test_is_empty_only_when_empty() {
        let doc = CrdtDocument::new();
        assert!(doc.is_empty());
        doc.insert(0, "x");
        assert!(!doc.is_empty());
        doc.delete(0, 1);
        assert!(doc.is_empty());
    }

    // ── Compile-time Type Checks ──────────────────────────────────

    #[test]
    fn test_api_signatures_compile() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<CrdtDocument>();
        assert_sync::<CrdtDocument>();

        // Verify method signatures exist at compile time
        let mut doc = CrdtDocument::new();
        let _: &Doc = doc.doc();
        let _: &mut Doc = doc.doc_mut();
        let _: String = doc.get_text();
        let _: u32 = doc.len();
        let _: bool = doc.is_empty();
    }
}
