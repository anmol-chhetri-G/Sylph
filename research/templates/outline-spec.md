# Outline spec (navigator → DOCUMENT MAP)

Source: `apps/desktop/src/ui.rs` `navigator()` Outline tab (was hardcoded demo items).

## Current (after fix)
- Parses real `# .. ######` headings from editor text + rich `Block::Heading` blocks.
- Empty state: "Under dev — no headings yet."

## To build
- [ ] Click-to-scroll to heading (hit-test via layout).
- [ ] Live page numbers (needs layout contract: blocks → fragments → A4 boxes).
- [ ] Collapse/expand sections, drag-reorder.
- [ ] Sync selection highlight with caret.

## Non-goals (v0.1)
- No mini-map rendering, no thumbnail outline.
