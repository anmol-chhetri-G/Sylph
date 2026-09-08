# Sylph — Product, Architecture, and Delivery Plan

**Planning baseline:** 2026-09-08  
**Primary platform:** Linux desktop  
**Product mode:** local-first, single-user, native Rust/GPUI  
**Status:** active implementation plan; the current shell is usable as a visual prototype while the editor kernel and document model are being consolidated

## Implementation ledger

- [x] Repository, existing Rust/GPUI editor, storage layer, and core document model inspected.
- [x] Stitch reference HTML/PNG states inspected and Editorial Precision tokens adopted.
- [x] `feature/editor-kernel` branch established with recoverable checkpoints.
- [x] Unicode-safe grapheme/word navigation foundation added to the text input.
- [x] Debounced local text autosave foundation added (750 ms after edits).
- [x] A4-first startup defaults and the native Linux window shell added.
- [x] Initial Stitch-inspired workspace chrome, page canvas, navigator, inspector, overlays, and dark-theme tokens added.
- [x] Independent left navigator and right inspector visibility controls added.
- [x] Native center-canvas scroll container and visible page-break flow/status feedback added.
- [x] First toolbar/command-palette actions wired to editor commands (clipboard, headings, table, image, and page break).
- [ ] Replace the temporary visual document scaffold with one structured document source of truth.
- [ ] Make center-canvas scrolling, pagination, page breaks, and block insertion pass interaction tests.
- [ ] Persist structured documents, assets, settings, and revisions atomically.

## 1. Product decision

Sylph will be a focused, native document editor for reports and long-form writing. It will feel like a calm editorial workstation: an A4 page in the center, compact professional tooling around it, and contextual controls that appear when they are useful.

The central architectural decision is:

> A structured rich-document model is the source of truth. Markdown is a first-class import/export and editing mode, but it is not the only internal representation.

Markdown is excellent for headings, emphasis, links, lists, code, quotes, and simple tables. It does not losslessly express page size and sections, exact margins, headers/footers, page-number ranges, text colour, image wrapping/position, captions, references, comments, or arbitrary page elements. Using Markdown as the canonical model would recreate the current split-brain problems as soon as those features are added.

Sylph therefore has two deliberate boundaries:

1. **Rich document → editor/layout/export:** lossless and authoritative.
2. **Markdown ↔ rich document:** convenient, testable, and explicit about features that cannot be represented in ordinary Markdown.

Collaboration is not a v0.1 requirement. The existing Yrs dependency will remain behind a backend boundary until the structured model is stable; a plain `Yrs::Text` must not remain the editor’s canonical content store.

## 2. What was inspected

### Repository baseline

- `apps/desktop/src/main.rs` is a 3,961-line GPUI application containing the window, input handling, custom text element, commands, persistence calls, and most rendering.
- `crates/core/src/document.rs` contains an early serializable block model with paragraphs, headings, images, tables, captions, cover pages, horizontal rules, page breaks, A4/Letter sizes, and margins.
- `crates/core/src/lib.rs` contains `CrdtDocument`, but it currently stores one plain Yrs text field named `content`.
- `crates/storage/src/lib.rs` stores text bytes in an append-only `crdt_updates` table and has basic document titles/listing. It does not yet store the structured document, assets, settings, or revision metadata.
- `crates/py_bridge` calls Python for AI and export work. The bridge already exposes richer export entry points, but the desktop app currently exports the plain CRDT text.
- `MY_VIEW_AND_ESTIMATION_FOR_PROJECT.md` contains the earlier feature/design specification. It should be treated as input, not as executable architecture; its “DELETE AFTER...” instruction is not being followed because it is a user-owned record and deleting it was not requested.

### Stitch design package

The extracted package was rendered locally with cached headless Chromium and its HTML structure was inspected. The visual direction is strong and should be adopted as the product design language, translated into native GPUI components rather than shipped as HTML/Tailwind.

The strongest states are:

- main editing workspace: three-tier chrome, left navigation/outline, centered page, contextual right inspector, status rail;
- empty state: command palette with keyboard hints and a page thumbnail/outline view;
- selected image: image inspector for size, wrapping, position, alt text, caption, replacement, and deletion;
- modal trio: page setup, table insertion, and export dialogs;
- version history: timeline drawer with compare and restore actions;
- dark workspace: the same geometry with dark editorial surfaces and cobalt focus states.

The package’s `editorial_precision/DESIGN.md` is the visual source of truth. Its key tokens are:

- cobalt primary `#1D4ED8`;
- slate chrome and text (`#475569`, `#0F172A`, `#64748B`);
- workspace `#F1F5F9`, page `#FFFFFF`, panel `#F8FAFC`;
- hairline borders `#E2E8F0` / `#CBD5E1`;
- Hanken Grotesk for UI chrome and EB Garamond for prose, with local/system fallbacks;
- 4 px spacing cadence, 28 px compact controls, 24 px status rail;
- restrained shadows and crisp 2–4 px radii.

## 3. Current risks to resolve before feature work

### The two-document problem

Keyboard edits are made in `TextInput.content` and mirrored into a plain Yrs text field. Tables, images, cover pages, page breaks, captions, and page settings are stored separately in `SylphApp.document`. Those structures are not the content being edited, saved, or exported. For example, inserting a table or page break appends a block to the separate document rather than inserting at the current text selection.

This is the first architectural fix. Adding more toolbar buttons before it would make the data loss and cursor bugs harder to remove.

### Position and Unicode correctness

The input handler names its ranges UTF-16, while much of the editor slices Rust strings and updates Yrs with byte offsets. A Rust byte offset, a Unicode scalar offset, a grapheme boundary, and a UTF-16 offset are not interchangeable. Emoji, combining marks, IME input, and non-Latin text can therefore place the cursor incorrectly or panic on a slice.

The editor kernel needs one logical position type and conversion adapters at GPUI/Yrs boundaries. All edits must be validated at grapheme-safe boundaries.

### Layout is not rendering

The current text element lays out lines in a viewport. A Word/Docs-style editor needs a deterministic layout result: blocks measured into fragments, fragments flowed into A4 content boxes, explicit page breaks honored, and hit-testing mapped back to document positions. The same layout contract must feed the screen and exports.

### Persistence is currently text-only

The current two-second save task writes `output/document.txt` and a text blob to SQLite. It creates a new `Untitled` document at application startup, so the durable document identity and structured state are not yet modeled. The runtime database is also under a tracked output path, which is unsafe for source control.

### Baseline quality is not clean yet

The inspection run found:

- `cargo test --workspace`: passed, 105 tests total (73 core, 23 storage, 9 Python bridge);
- `cargo fmt --all -- --check`: failed on existing formatting differences;
- `cargo clippy --workspace --all-targets -- -D warnings`: failed on the manual `Default` implementation for `PageSize`;
- Git has only `master`, no configured remote, and no tags;
- `MY_VIEW_AND_ESTIMATION_FOR_PROJECT.md` is untracked and must be preserved as user work;
- `crates/storage/output/sylph.db` was modified by the test run and must not be silently committed as source.

The first implementation checkpoint must establish a clean, intentional baseline without deleting or overwriting those user-owned changes.

## 4. Target architecture

The application should be split into small vertical responsibilities. The exact crate names can change, but the dependency direction should not:

```text
GPUI desktop shell
        ↓
editor state + commands + selection
        ↓
document model ← layout engine → exporters
        ↓                 ↓
storage/revisions      asset store
```

### `sylph-core`: model and invariants

The model should own no GPUI types and no filesystem behavior. Its conceptual shape is:

```text
Document
├── identity: DocumentId, title, schema version
├── settings: page defaults, language, theme-independent preferences
├── sections[]
│   ├── page geometry: size, orientation, margins
│   ├── header/footer definitions
│   ├── page-number policy
│   └── blocks[]
├── named styles
├── assets[]
└── references / generated-element definitions
```

The first model vocabulary should include:

- stable `DocumentId`, `SectionId`, `BlockId`, `AssetId`, and `ReferenceId`;
- paragraphs and headings with inline spans;
- inline marks: bold, italic, underline, strike, code, text colour, highlight, font family, size, links, and reference anchors;
- paragraph attributes: alignment, indentation, before/after spacing, line spacing, list membership, keep-with-next, and outline level;
- bullet and numbered lists with nesting and stable numbering definitions;
- tables with rows, cells, cell spans, cell styles, widths, and captions;
- images/assets with intrinsic dimensions, alt text, caption, anchor, wrapping, and position;
- explicit page/section breaks;
- headers, footers, and field nodes such as current page and total pages;
- generated definitions for table of contents, list of figures, and list of tables;
- footnotes/endnotes and bibliography references after the core editing kernel is reliable.

Page numbers must be fields, not literal text. A page-number policy should support “start at page”, optional “stop at page”, continue from a section, and formats such as Arabic/Roman numerals. The renderer decides whether a field is visible on a given physical page.

All physical dimensions use points or millimetres in the model. Pixels are only a viewport scale. A4 portrait is 595 × 842 points; the default margin is configurable and should start at the chosen minimal report-safe default rather than being hard-coded into the renderer.

### `sylph-editor`: commands, selection, and history

The UI should dispatch commands such as `InsertText`, `DeleteRange`, `ToggleMark`, `SetParagraphStyle`, `InsertBlock`, `SetPageSetup`, `InsertAsset`, and `RestoreRevision`. Commands operate on a transaction and return an inverse/undo description plus affected ranges.

The editor state should contain:

- document snapshot or backend handle;
- `Selection { anchor, head }` in stable logical positions;
- preferred horizontal column for vertical movement;
- composition/IME state;
- undo/redo stacks of document transactions;
- layout invalidation information;
- dirty/save state.

The GPUI layer should translate keyboard, mouse, clipboard, and IME events into this state. It should not directly mutate SQLite, build Markdown delimiters, or append blocks to an unrelated model.

### `sylph-layout`: pagination and hit testing

The layout engine receives a document plus viewport settings and returns a layout snapshot:

- pages and their content rectangles;
- positioned block/inline fragments;
- line boxes and text metrics;
- object bounds and selection regions;
- header/footer and page-number fields;
- source-position ↔ screen-position hit-test maps;
- invalidation keys for incremental relayout.

This enables the screen renderer, PDF export, print preview, and eventually DOCX pagination to share the same semantics. A first version may use full-document relayout; incremental layout is a measured optimization after correctness.

### `sylph-storage`: durable documents, assets, and revisions

Storage should be independent of the editor view. A practical local-first design is:

- an OS data directory for the Sylph library index;
- structured document snapshots/revisions stored as versioned serialized payloads;
- content-addressed or UUID-named assets outside the text payload;
- metadata for title, last opened, updated time, schema version, and save status;
- atomic writes: write a temporary payload, flush/close it, then replace the target;
- migration functions for every schema version.

The user-facing `.sylph` project/package format can be introduced after the model stabilizes. Git should be an optional project-history layer, never the only autosave or recovery mechanism.

### Markdown and export adapters

Markdown import/export should live outside the model. Normal CommonMark/GFM features map naturally. Unsupported rich features need one of these explicit behaviors:

1. preserve them in Sylph’s native payload;
2. export a readable fallback plus a warning;
3. use documented fenced/directive extensions for round-trip Markdown.

PDF, HTML, DOCX, and ODT should consume the structured model or a shared intermediate representation. The Python bridge can remain as an adapter while export correctness is established; it must not be the only representation of the document.

## 5. UI and interaction plan

The Stitch design is adopted with these native responsibilities:

- **Top chrome:** title/menu row, quick actions, formatting ribbon, zoom/view controls;
- **Left navigator:** outline first, pages/thumbnails, assets, references, and version entry points;
- **Center canvas:** scrollable workspace containing measurable physical pages, ruler, selection, caret, block controls, and page gaps;
- **Right inspector:** context-sensitive paragraph, image, table, and page setup controls;
- **Bottom status rail:** page count, word count, language, save state, zoom, and current position;
- **Overlays:** command palette, selection mini-toolbar, context menus, table grid, page setup, export, and version history;
- **Themes:** light and dark tokens share geometry and semantic roles; no duplicated layout logic.

The permanent chrome must not consume the writing surface unnecessarily. The right inspector collapses, and the left navigator can switch from outline to pages/assets. The center page remains the visual priority.

## 6. Delivery order and gates

Each phase ends with a usable, testable checkpoint. Feature breadth is deliberately delayed until the editing kernel and persistence are trustworthy.

### Phase 0 — baseline and design lock

1. Preserve the user’s untracked notes and inspect the tracked runtime database.
2. Move runtime data out of the repository or add a deliberate fixture policy.
3. Make formatting and strict Clippy pass without changing product behavior.
4. Record design tokens, core terminology, keyboard conventions, and test corpus.
5. Add a small architecture decision record for the structured model/Markdown boundary.

**Gate:** clean source baseline; tests, formatting, and lint pass; no runtime DB is treated as source.

### Phase 1 — editor kernel and single source of truth

1. Introduce stable IDs, inline spans, paragraphs, blocks, and `Selection`.
2. Make typing, deletion, replacement, newline, paste, and formatting operate on one document state.
3. Replace byte/UTF-16 assumptions with explicit conversion helpers and grapheme-safe boundaries.
4. Port arrow, Home/End, word, page, shift-selection, mouse hit testing, and IME behavior to the new positions.
5. Keep the first renderer simple; correctness matters more than page polish here.

**Gate:** Unicode/IME-safe editing, selection, undo/redo, and keyboard navigation pass unit and interaction tests with no second document buffer.

### Phase 2 — local save, autosave, and recovery

1. Persist the structured document and metadata under a stable document identity.
2. Debounce autosave around 500–1,000 ms after the last edit, execute it off the UI path, and expose `Unsaved → Saving → Saved/Error`.
3. Write atomically and create a recoverable checkpoint after a crash or forced close.
4. Add revision records with parent, timestamp, word count, checksum, and optional user label.
5. Add load-on-startup for the most recent document instead of creating a new `Untitled` every launch.

**Gate:** kill/reopen tests recover the last acknowledged edit; a failed save leaves the previous valid revision intact.

### Phase 3 — A4 page layout

1. Implement physical page geometry, zoom, margins, orientation, section breaks, and page gaps.
2. Flow paragraphs/headings/lists into multiple pages with stable source mappings.
3. Implement explicit page breaks and page-aware caret/selection behavior.
4. Add headers, footers, and page-number fields with start/end policies.
5. Build the page setup inspector and print-layout status indicators.

**Gate:** a fixture report paginates deterministically at A4, page breaks land correctly, and page-number settings render from the selected start through the configured end.

### Phase 4 — rich formatting and Markdown shortcuts

1. Inline bold/italic/underline/strike/code, font size, font family, text colour, highlight, and links.
2. Paragraph alignment, indentation, line/paragraph spacing, and named styles.
3. Bulleted, numbered, and nested lists.
4. Toolbar state reflects the selection; shortcuts mutate attributes rather than inserting raw delimiters.
5. Markdown shortcuts such as `# `, `- `, and `**...**` are recognized at safe boundaries and can be shown/hidden in source mode.

**Gate:** formatting survives save/load and is stable through undo/redo; Markdown round-trip tests document any intentional loss.

### Phase 5 — document objects and generated structure

1. Asset import/paste, asset storage, intrinsic sizing, alt text, captions, wrapping, and replacement.
2. Editable tables with insertion/deletion, navigation, cell selection, widths, captions, and basic styles.
3. Heading IDs and generated table of contents.
4. Figure/table captions and generated lists of figures/tables.
5. References, footnotes/endnotes, and bibliography model.

**Gate:** a report fixture containing images, tables, captions, headings, and references can be edited, saved, reopened, and regenerated without losing structure.

### Phase 6 — Stitch-quality shell and workflows

1. Replace the prototype chrome with the Editorial Precision tokens and component system.
2. Implement outline/pages/assets navigator states, contextual inspector states, command palette, table picker, page setup, export, and version history overlays.
3. Add selection mini-toolbar, clear focus/hover/disabled states, dark mode, and keyboard-first access.
4. Add empty, loading, error, recovery, and unsaved states.

**Gate:** the main, empty, image-selected, modal, history, and dark states represented in the Stitch package are all reachable in the native app and remain usable at the target window sizes.

### Phase 7 — import/export and interoperability

1. Markdown import/export with native-extension warnings where needed.
2. PDF and print output from the shared layout semantics.
3. HTML export for inspection and sharing.
4. DOCX/ODT export and basic import using fixtures from LibreOffice/Word-compatible documents.
5. Compare generated output against golden fixtures and surface unsupported-feature warnings instead of silently dropping content.

**Gate:** export is deterministic enough for a report workflow and round-trip tests identify every known limitation.

### Phase 8 — hardening

- performance measurements for large documents, many images, and long tables;
- layout/selection property tests and fuzzing for edit operations;
- accessibility and keyboard-only pass;
- crash-recovery drills and migration tests;
- packaging for Linux, desktop icon/install metadata, and user documentation;
- only then consider optional Yrs collaboration, cloud sync, comments/suggestions, and plugins.

## 7. Feature map

| User need | Canonical owner | Planned phase |
|---|---|---:|
| A4 and minimal/default margins | section/page settings + layout | 3 |
| page start/end numbering | page-number field policy | 3 |
| bold/italic/underline/colour | inline span attributes | 4 |
| references/citations | reference and footnote model | 5 |
| images/logo/associated people | asset blocks + captions | 5 |
| TOC/figures/tables | generated elements | 5 |
| page break | explicit block/section break | 3 |
| alignment and line gaps | paragraph attributes | 4 |
| bullet/number lists | list blocks/definitions | 4 |
| tables | table block/cell model | 5 |
| autosave | storage + revision service | 2 |
| version history/restore | revision graph | 2 and 6 |
| Markdown editing | adapter + source mode | 4 and 7 |
| PDF/HTML/DOCX/ODT | export adapters | 7 |

## 8. Version-control and recovery protocol

### Source-code Git

Until a remote is configured, the local repository is the recovery authority:

1. `master` remains stable and should only contain passing checkpoints.
2. Use `feature/<short-name>` for each vertical slice and `fix/<short-name>` for repairs.
3. Before every substantial implementation, record status and make a checkpoint commit or a rescue branch. Never include the user’s untracked notes or runtime database unless explicitly requested.
4. Make one conceptual change per commit. Suggested prefixes: `docs:`, `feat(core):`, `feat(editor):`, `feat(layout):`, `fix:`, `test:`, `refactor:`.
5. Every implementation commit runs formatting, targeted tests, workspace tests, and (where applicable) a screenshot/golden fixture check.
6. Tag meaningful local milestones such as `v0.1.0-foundation`, `v0.1.0-editor-kernel`, and `v0.1.0-page-layout`.

### User-requested stage notification

For every substantial implementation step:

1. send a `notify-send` stage message;
2. wait 10 seconds;
3. execute the step;
4. report the command/result and next checkpoint.

If desktop notification permission is unavailable, report the failure and still use the 10-second stage pause before proceeding.

### Recovery rules

- Prefer `git revert` of a completed commit over destructive history rewrites.
- Before repair, create a rescue branch and capture `git diff`/status.
- Never use `git reset --hard` or broad checkout operations for routine recovery.
- Restore a document through Sylph’s revision graph by creating a new head revision; do not delete the old revision.
- Keep source-code history and document-content history separate so a bad UI commit cannot destroy a report.

## 9. Test strategy

### Core invariants

- every block has a stable ID;
- all selections point to valid grapheme boundaries;
- applying a transaction then its inverse returns the exact prior document;
- serialization/deserialization is lossless for the native model;
- page dimensions and margins remain positive and deterministic;
- generated elements update when their source headings/captions change.

### Integration fixtures

Maintain small fixtures for: empty document, Unicode/IME text, styled paragraphs, nested lists, a multi-page A4 report, page numbering with an end page, image/caption, table/caption, TOC, references, and a deliberately unsupported Markdown feature.

### Regression tests

Every fixed cursor bug gets a focused test. In particular: arrow navigation over wrapped lines, trailing newlines, emoji/combining marks, selections spanning blocks, tables, page breaks, and save/reopen after an interrupted write.

## 10. First implementation slice after this plan

The first code slice should be **“single source of truth and safe positions”**, not a new toolbar feature:

1. establish a clean baseline and isolate runtime DB writes;
2. define the first structured text/selection types in core;
3. add a command/transaction path for insert/delete/replace;
4. make the current GPUI input use that path;
5. add Unicode, selection, undo/redo, and save/load tests;
6. only after that render the first structured paragraph in the page canvas.

This order directly addresses the current arrow-key and line-editing failures while creating the foundation needed for every requested Word/Docs feature.

## 11. Definition of v0.1

Sylph v0.1 is complete when a user can create and reopen a local A4 report, type and navigate reliably, apply core rich formatting, insert page breaks/images/tables, generate a TOC and page numbers, configure margins, recover from a crash, inspect/restore revisions, and export a visually faithful PDF/HTML plus a documented Markdown representation. DOCX/ODT compatibility may remain limited if unsupported features are clearly reported.

The non-goals for v0.1 are collaboration, cloud sync, real-time multi-user CRDT behavior, full Word compatibility, arbitrary desktop publishing/freeform canvas layout, and a plugin marketplace.
