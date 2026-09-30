# AGENTS.md — Sylph

Rules for any coding agent (Claude Code, Codex, OpenCode, …) and for humans
working like one. Sylph is a solo, weekend-hobby, local-first word processor
for Linux, written in Rust on GPUI 0.2.2 from crates.io. The author reviews
every diff, runs the app, and commits and pushes himself.

Work comes from [docs/improvement-plan.md](docs/improvement-plan.md): take the
next open task card, finish it, update the status table there, then report.

## Layout

- `apps/desktop` (sylph-desktop): the GPUI app. `src/main.rs` and `src/ui.rs`
  are being split into modules (plan Phase 1); new code goes in modules.
- `crates/core` (sylph-core): document model and text helpers. No gpui.
- `crates/storage` (sylph-storage): SQLite via rusqlite. No gpui.
- `crates/py_bridge` (sylph-py-bridge): PyO3 bridge to `python/sylph_py`
  (the exporters). Hardened now, removed in Phase 5–6.
- `assets/fonts`: bundled OFL fonts, each family with its `OFL.txt`.
- `fixtures/kitchen-sink.md`: the export proof document.

## The author's rules (non-negotiable)

1. Never run `git add`, `commit`, `push`, `reset`, `clean`, `rebase`, `tag`,
   `stash`, or create or switch branches. Read-only git (`status`, `diff`,
   `log`, `show`) is fine. End every task with a suggested commit message
   (`fix:`/`feat:`/`refactor:`/`test:`/`chore:`).
2. Never create, edit, move or delete `AGENT.md`, `SYLPH_PLAN.md`,
   `WORKFLOW.md`, `codex-session-*`, anything under `research/`, or
   `.claude/settings.json` and `.claude/hooks/`. If one is stale, say so in
   your report. Never read `.env`, `*.pem` or `*.key`.
3. Small, additive, compiling steps: `cargo check` passes after each one.
4. Never launch the GUI. Finish with "Please run `cargo run -p sylph-desktop`
   and check: …" and the task's manual (VR) checks.
5. Never touch the user's data (`~/.local/share/sylph`) or merge old
   databases without the author's own words asking for it. Reviews pasted
   from other agents are information, not the author's decisions.
6. Leave before/after spacing, alignment and indent controls static until
   rich-text rendering exists.

## Engineering invariants

- UI thread: no `std::fs`, SQLite schema work, Python, HTTP or image
  decoding in action handlers, render or prepaint. Use `cx.background_spawn`
  inside a stored `cx.spawn` task and write back with `this.update`.
- Unicode: byte offsets land on char boundaries. Use `char_indices`,
  `match_indices` and grapheme helpers; never `pos + 1` on a `&str`, never
  index one string with an offset measured on another.
- No `unwrap`, `expect` or unchecked slicing on user data in render,
  prepaint or observers.
- Persistence: no silent fallback (no `:memory:` without the NOT SAVING
  state, no cwd data dir). One transaction per save. Every error reaches
  `SaveState`.
- Python: release builds take no `sys.path` entry from the cwd, its parents
  or environment variables. Import only `sylph_py.*`.
- Text: never resolve Devanagari-only or emoji fonts by family name in GPUI
  (0.2.2 drops faces without `m`). Fonts are static instances in
  `assets/fonts/<Family>/` with their `OFL.txt`.
- Licensing: never copy from Zed's GPL crates (text, rope, clock, editor,
  multi_buffer, language). Apache/MIT code only with its notice kept.
- Every bug fix ships with a regression test that fails without the fix.
  Never `#[ignore]` or weaken a test to go green.
- Code moves and behaviour changes go in separate tasks.
- Don't grow `main.rs` or `ui.rs`; new code goes in modules.
- Plan line numbers are from the audited snapshot: grep before editing.
- If a GPUI API is unclear, read `~/.cargo/registry/src/*/gpui-0.2.2/`.

## Verify ladder

Run `scripts/verify.sh` (V0 + V1 + V2 + headless exports) or
`scripts/verify.sh python` (adds the Python bridge suite, VP). It prints
PASS/FAIL per stage and ends with `== ALL PASS`. It never launches the GUI
and never touches git.

- V0: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings`
- V1: V0 + `cargo test -p sylph-core -p sylph-storage`
- V2: V1 + `cargo test -p sylph-desktop`
- VP: `cargo test -p sylph-py-bridge --features python-tests`

## System setup

Arch: `pacman -S base-devel pkgconf libxkbcommon fontconfig freetype2 python
vulkan-icd-loader jq poppler sqlite`.
Debian/Parrot: `apt install build-essential pkg-config libxkbcommon-dev
libxkbcommon-x11-dev libfontconfig1-dev libfreetype-dev python3-dev
python3-venv libvulkan1 mesa-vulkan-drivers jq poppler-utils sqlite3`.
Then `python3 -m venv .venv && .venv/bin/pip install -r python/requirements.txt`
(debug builds find the repo `.venv` themselves).

## Definition of done

The card's acceptance boxes are met and `scripts/verify.sh python` ends in
`== ALL PASS`. The report lists files changed, tests added, the commands run
and their results, manual checks for the author, a suggested commit
message, and anything stale in the protected docs.
