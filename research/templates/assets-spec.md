# Assets spec (navigator → ASSETS)

Source: `apps/desktop/src/ui.rs` `navigator()` Assets tab (was hardcoded "Telemetry Chart").

## Current (after fix)
- Lists real `Block::Image` paths + table count from `Document.blocks`.
- Empty state: "Under dev — no assets yet."

## To build
- [ ] Thumbnails from `output/images/`, file size, dimensions.
- [ ] Linked vs embedded state, reveal-in-folder, replace/remove.
- [ ] Caption + alt-text editing inline.
- [ ] Orphan-asset GC (images on disk but not in doc).

## Non-goals (v0.1)
- No image editing, no stock library.
