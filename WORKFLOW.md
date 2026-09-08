# WORKFLOW.md — Build → Test → Verify → Commit → Next

## The Rules

```
┌─────────────┐     ┌──────────┐     ┌────────────┐     ┌────────┐
│ Build code  │ ──▶ │ cargo    │ ──▶ │ Pass?      │ ──▶ │ Log +  │
│ (write)     │     │ test     │     │ (automated)│     │ commit │
└─────────────┘     └──────────┘     └─────┬──────┘     └────────┘
                                           │ fail
                                           ▼
                                    ┌──────────────┐
                                    │ Diagnose:     │
                                    │ which code    │
                                    │ broke, why    │
                                    └──────┬───────┘
                                           ▼
                                    ┌──────────────┐
                                    │ Fix the code  │
                                    │ not the test  │
                                    └──────┬───────┘
                                           │
                                    back to Build
```

## Cargo Commands

```bash
# Build entire workspace
cargo build

# Run all tests
cargo test

# Check for warnings
cargo clippy

# Format code
cargo fmt

# Build specific crate
cargo build -p sylph-core

# Run specific test
cargo test -p sylph-core
```

## Checkpointing

```
Task 1: PASS ✓
Task 2: FAIL ✗  ← failed here
        ↓
  resume_from: 2
  completed: [1]
        ↓
  re-run starts at task 2
```

## Version 0.1.0 Checklist

- [ ] `cargo build` succeeds for all crates
- [ ] `cargo test` passes for all crates
- [ ] No compiler warnings with `cargo clippy`
- [ ] Code formatted with `cargo fmt`
- [ ] GPUI window opens (when gpui dep added)
- [ ] yrs::Doc initializes
- [ ] SQLite database creates
- [ ] PyO3 bridge calls Python
