# Sylph v0.1.0 — TODO

## P0 — Must fix (editor is broken without these)
- [x] Up/Down arrow navigation between lines
- [x] Ctrl+A / Cmd+A select all
- [x] Scroll when content overflows viewport (mouse wheel)
- [x] Mouse click places cursor on correct line (multiline)

## P1 — Should fix (basic editor expectations)
- [x] Home/End keys (start/end of line)
- [x] Shift+Up/Down line selection
- [x] Sidebar toggle (Cmd+B or ▶/◀ button)
- [x] Undo/Redo (Cmd+Z / Cmd+Shift+Z)
- [x] Word-by-word navigation (Alt+Left/Right)
- [x] Double-click to select word
- [x] Triple-click to select line
- [x] Page Up/Page Down

## P2 — Nice to have (editor features)
- [x] Tab indentation (Tab/Shift+Tab)
- [x] Find & Replace (Cmd+F)
- [x] Line numbers in gutter
- [x] Syntax highlighting (markdown)
- [x] Word wrap at viewport edge
- [x] Drag to select text
- [x] Right-click context menu (copy/cut/paste)

## P3 — App features (after editor works)
- [x] Document title editing in toolbar
- [x] Sidebar document list with switching
- [x] Auto-save on timer (debounced)
- [x] Logo (PNG/img() integration)
- [x] Dark theme toggle
- [x] Export to PDF/DOCX (via Python)
- [x] AI commands: rewrite, chat with doc (via Python bridge)

## P4 — Polish & Quality (code cleanup, bug fixes, UX)
### Critical bugs
- [x] Fix latent panic in page_up/page_down when cursor past last line
- [x] Fix duplicate dead code block (triple-backtick check)
- [x] Fix scroll_offset_y no upper bound (infinite scroll past top)
- [x] Fix multi-line selection highlight incomplete (deferred - complex)
- [x] Fix export injecting status text into editor content
- [x] Fix Storage::Default panicking on DB failure

### Code deduplication
- [x] Extract new_document/switch_document shared logic
- [x] Extract move_vertically/select_vertically shared logic
- [x] Extract export_docx/export_pdf shared logic
- [x] Extract find_next/find_prev shared logic
- [x] Extract Python path setup in py_bridge into helper

### Dead code cleanup
- [x] Remove unused actions (RewriteText, SwitchDocument)
- [x] Remove unused variables (_text, _gutter_bg)
- [x] Remove unused sylph-storage dep from core
- [x] Remove dead free functions in storage (init_db, save/load_crdt_state)
- [x] Fix clippy warnings (len_without_is_empty, empty_line_after_doc_comments)
- [x] Remove unused color helper methods (text_color, gutter_color)

### Error handling
- [x] Replace unwrap() on paint calls with graceful fallback
- [x] Add bounds checking before as u32 casts in apply_edit
- [x] Replace .unwrap() on window create with .expect()
- [x] Fix Storage::Default to fallback to in-memory DB

### UX improvements
- [x] Add Shift+Tab dedent
- [x] Add Cmd+Backspace delete to line start
- [x] Add Cmd+Delete delete to line end
- [x] Add Ctrl+Backspace delete word left
- [x] Add Ctrl+Delete delete word right
- [x] Cap scroll_offset_y at content bounds
