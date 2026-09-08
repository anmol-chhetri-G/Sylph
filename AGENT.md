# AGENT.md: Sylph Builder Operating System

## Core Directive
You are building Sylph — a native document editor in Rust using GPUI, yrs CRDT, SQLite, and PyO3.

Execute all tasks using this workflow.

---

## Phase 1: Rapid Iteration (Try, Fall, Get Up)

1. **TRY:** Implement the first logical approach. Do not over-engineer; get to a testable state quickly.
2. **CHECK:** Run `cargo build`, `cargo test`, or execute the code. Gather output.
3. **FALL:** If attempt fails, log it as "successful discovery of an invalid approach." Read the error.
4. **GET UP & RETRY:** Patch the obvious error. Re-run.
5. **PIVOT:** If same failure occurs twice, change one variable (swap library, invert logic, change data structure).

## Phase 2: Escalation Protocol

6. **EXHAUSTION THRESHOLD:** If 3 distinct methods fail, halt coding. Admit current knowledge is insufficient.
7. **RESEARCH:** Read documentation, search forums, analyze similar codebases.
8. **BUILD:** Architect an entirely new solution based on research.
9. **IMPROVE:** Fix minor bugs. Remove hacky workarounds.
10. **MAKE IT WORK:** Cycle Build → Check → Improve relentlessly.

## Phase 3: Post-Mortem & Polish

11. **DEEP ANALYSIS:** Review codebase. Remove dead code, debug artifacts. Optimize.
12. **APPEAL ENHANCEMENT:** A working tool must be appealing. Improve UX/UI, readability, value add.

## Phase 4: The Infinite Loop

13. **DEPLOY:** Finalize current state.
14. **SUGGEST:** Output summary of what was built, failures overcome, enhancements made.
15. **FEED:** Await next directive. Loop to Phase 1.

---

### Execution Rules
- **Rule of 3:** Never attempt same failing approach more than 2 times.
- **No Silent Failures:** At exhaustion threshold, state: "Initiating Escalation Protocol."
- **Never Stop at Functional:** Task not done until Phase 3 complete.

---

### Version 0.1.0 Focus

Prove the architecture: GPUI → CRDT → SQLite → PyO3

- [ ] Task 1: GPUI Window & Basic Editor
- [ ] Task 2: yrs CRDT Integration
- [ ] Task 3: Local-First Storage
- [ ] Task 4: PyO3 Bridge
- [ ] Task 5: "AI Loop" UI
- [ ] Task 6: Basic Markdown Rendering
