//! Values derived from the text, computed once per edit, not per frame.

/// A value computed from inputs `K` and reused until they change. Render
/// code takes `&self`, so the slot is a `RefCell`.
pub(crate) struct Memo<K, V> {
    slot: std::cell::RefCell<Option<(K, V)>>,
    /// How many times the value was computed (for tests).
    computed: std::cell::Cell<usize>,
}

impl<K: PartialEq, V: Clone> Memo<K, V> {
    pub(crate) fn new() -> Self {
        Self {
            slot: std::cell::RefCell::new(None),
            computed: std::cell::Cell::new(0),
        }
    }

    /// The value for `key`: the cached one if `key` is unchanged, else
    /// `compute()` (then cached).
    pub(crate) fn get(&self, key: K, compute: impl FnOnce() -> V) -> V {
        if let Some((cached, value)) = &*self.slot.borrow() {
            if *cached == key {
                return value.clone();
            }
        }
        let value = compute();
        self.computed.set(self.computed.get() + 1);
        *self.slot.borrow_mut() = Some((key, value.clone()));
        value
    }
}

/// What the status bar shows about the text at the caret: (line, column,
/// words, (block, of blocks), caret's page among the text's pages).
pub(crate) type CaretStatus = (usize, usize, usize, (usize, usize), usize);

#[cfg(test)]
mod memo_tests {
    use super::Memo;

    #[test]
    fn a_value_is_computed_once_per_key() {
        let memo: Memo<u64, usize> = Memo::new();
        let mut calls = 0;
        for _ in 0..100 {
            // An idle frame: same key, no work.
            memo.get(1, || {
                calls += 1;
                7
            });
        }
        assert_eq!(calls, 1);
        assert_eq!(memo.get(2, || 9), 9);
        assert_eq!(memo.get(2, || unreachable!()), 9);
        assert_eq!(memo.computed.get(), 2);
    }
}
