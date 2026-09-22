# Page setup spec (toolbar + future dialog)

Source: `apps/desktop/src/main.rs` Page Setup group (A4/Letter toggle, Normal/Narrow
toggle, Classic-only Cover button) + `crates/core/src/document.rs`
`PageSize`, `PageMargins`, `CoverTemplate`, `CoverPageData`.

## Current (after fix)
- Working: A4/Letter toggle, margin presets, landscape flag, Classic cover add/remove.
- Everything else is "Under dev": full page-setup dialog, template picker
  (Classic/Modern/Bold/Minimal/Academic), custom margins, header/footer,
  page-number ranges.

## To build
- [ ] Real Page Setup dialog: size, orientation, custom margins (pt), live preview.
- [ ] Cover template picker wired to `CoverTemplate` + `export.py` renderers
  (`_render_cover_page_docx/pdf`).
- [ ] Headers/footers, page-number start/skip-cover.
- [ ] Persist per-document in structured storage (not text-only save).

## Cover templates (kept here, not removed from code yet)
- Classic / Modern / Academic / Bold / Minimal — title size + alignment variants.
- Decision: keep enum in code (needed by export); picker UI is the "under dev" part.
