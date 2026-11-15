# Run Log

Maintained by the Orchestrator (PROCESS.md P5). One row per unit-of-work attempt; phase gates and
merges get their own rows. Fable uses carry the classifier's evidence in Notes.

| Phase | Work item | Role | Model | Attempt | Status | Gate results | Notes |
|---|---|---|---|---|---|---|---|
| — | PR #1 | Orchestrator | Opus 5.5 | 1 | merged | CodeRabbit approved; 1 finding fixed (T0 Haiku 5.5 Fixer, Sonnet 5.5 verifier) | Relicense under MPL-2.0; merge 34f103b |
| — | plan | Orchestrator | Opus 5.5 | 1 | done | 139 work-unit issues (#2–#46, #48–#141), all SRS REQ IDs traced | Plan created as GitHub issues with blocked-by links; this PR carries only the issue-driven tooling |
| 00-repository | #2 | Implementer | Sonnet 5.5 | 1 | done | fmt, clippy, build, test clean; header, forbid and metadata checks pass | FP-001 workspace skeleton; classifier brief T1, no Fable (allowance unknown) |
| 00-repository | #3 | Implementer | Sonnet 5.5 | 1 | done | fmt, clippy, test clean locally; Actions runs pending | FP-002 CI workflow; classifier brief T1, no Fable (allowance unknown) |
| 00-repository | #4 | Implementer | Sonnet 5.5 | 1 | done | fmt, clippy, test clean; xtask coverage 92.27%; guard all passes; 3 scratch violations rejected | FP-003 coverage gates and guards; classifier brief T1, no Fable (allowance unknown) |
| 00-repository | #4 (PR #145 review round 1) | Fixer | Sonnet 5.5 | 1 | done | 7 CodeRabbit findings fixed with tests; xtask coverage 95.00%; guard all passes | T1 fix batch; classifier brief, no Fable (allowance unknown) |
| 00-repository | #4 (PR #145 review round 2) | Fixer | Sonnet 5.5 | 1 | done | 1 CodeRabbit finding fixed with test; xtask coverage 95.02%; guard all passes | T1 fast-path (triage) fix of crate_of xtask arm |
| 00-repository | #4 (PR #145 review round 3) | Fixer | Sonnet 5.5 | 2 | done | crate_of anchored to workspace root; coverage fails on root mismatch; xtask coverage 94.72%; guard all passes | T1; second attempt on the same finding after a deeper-nesting follow-up; no Fable (allowance unknown) |
| 00-repository | #4 (PR #145 review round 4) | Fixer | Sonnet 5.5 | 1 | done | regression test for a checkout directory named src; xtask coverage 94.94%; guard all passes | T1 test-only; requested by CodeRabbit follow-up |
| 00-repository | #6 | Implementer | Opus 5.5 | 1 | done | fmt, clippy, tests clean locally on Linux; Spikes workflow runs pending | FP-005 binding spike; classifier brief T2, no Fable (allowance unknown); CodeRabbit round 1: 2 fixed (T1 Sonnet 5.5 Fixer), 1 deferred to #4 |
| 00-repository | #4 (PR #145 review round 5) | Fixer | Sonnet 5.5 | 1 | done | relative --root resolved against cwd; main merged (3 append-only log conflicts, both sides kept); xtask coverage 94.88%; guard all passes (82 files) | T1; no Fable (allowance unknown) |
| 00-repository | #4 (PR #145 review round 6) | Fixer | Sonnet 5.5 | 1 | done | UNC prefix preserved in resolve_root; run-level test for relative --root; xtask coverage 95.33%; guard all passes (82 files) | T1; two Fixer briefs (UNC fix, run-level test); no Fable (allowance unknown) |
| 00-repository | #5 | Implementer | Opus 5.5 | 1 | done | fmt, clippy, test --locked clean locally; Windows/macOS/wasm32/coverage pending CI | FP-004 crate spike; classifier brief T2, no Fable (allowance unknown) |
| 00-repository | PR #146 r1 | Fixer | Opus 5.5 | 1 | done | fmt, clippy, test --locked clean locally | CodeRabbit round 1: 3 findings (2 docs, JCS duplicate-key rejection); classifier T2 |
| 00-repository | #6 | Implementer | Opus 5.5 | 1 | done | fmt, clippy, tests clean locally on Linux; Spikes workflow runs pending | FP-005 binding spike; classifier brief T2, no Fable (allowance unknown); CodeRabbit round 1: 2 fixed (T1 Sonnet 5.5 Fixer), 1 deferred to #4 |
| 00-repository | PR #146 r2 | Fixer | Sonnet 5.5 | 1 | done | docs only; fmt clean | CodeRabbit round 2: remove bench.rs coverage exemption (REQ-NFR-008); triage fast path T1 |
| 00-repository | PR #146 r3 | Fixer | Sonnet 5.5 | 1 | done | docs only; fmt clean | CodeRabbit round 3: spike deletion needs replacement golden tests first; triage fast path T1 |
| 00-repository | #4 (PR #145 merge of main, #5 spike) | Fixer | Sonnet 5.5 | 1 | done | main merged twice (append-only log conflicts, both sides kept); spikes/libs measured against the 75% floor (98.52%); xtask 95.51%; guard all passes (96 files) | T1; required by the FP-004 decision for #4; no Fable (allowance unknown) |
| 00-repository | #4 (PR #145 review round 7) | Fixer | Sonnet 5.5 | 1 | done | ancestor-root vacuous pass fixed; tests/examples now measured (spikes/libs 82.89%, xtask 95.67%); docs/ JSON key finding answered as per-spec and accepted by CodeRabbit; guard all passes (96 files) | T1; one Fixer, two commits; no Fable (allowance unknown) |
| 00-repository | #4 (PR #145 review round 8) | Fixer | Haiku 5.5 | 1 | done | CI table row in DEVELOPMENT.md back to 2 cells (regex pipes escaped); guard all passes (96 files) | T0 docs-only fast path (triage); no Fable (allowance unknown) |
