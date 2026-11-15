# Run Log

Maintained by the Orchestrator (PROCESS.md P5). One row per unit-of-work attempt; phase gates and
merges get their own rows. Fable uses carry the classifier's evidence in Notes.

| Phase | Work item | Role | Model | Attempt | Status | Gate results | Notes |
|---|---|---|---|---|---|---|---|
| — | PR #1 | Orchestrator | Opus 5.5 | 1 | merged | CodeRabbit approved; 1 finding fixed (T0 Haiku 5.5 Fixer, Sonnet 5.5 verifier) | Relicense under MPL-2.0; merge 34f103b |
| — | plan | Orchestrator | Opus 5.5 | 1 | done | 139 work-unit issues (#2–#46, #48–#141), all SRS REQ IDs traced | Plan created as GitHub issues with blocked-by links; this PR carries only the issue-driven tooling |
| 00-repository | #2 | Implementer | Sonnet 5.5 | 1 | done | fmt, clippy, build, test clean; header, forbid and metadata checks pass | FP-001 workspace skeleton; classifier brief T1, no Fable (allowance unknown) |
| 00-repository | #3 | Implementer | Sonnet 5.5 | 1 | done | fmt, clippy, test clean locally; Actions runs pending | FP-002 CI workflow; classifier brief T1, no Fable (allowance unknown) |
| 00-repository | #5 | Implementer | Opus 5.5 | 1 | done | fmt, clippy, test --locked clean locally; Windows/macOS/wasm32/coverage pending CI | FP-004 crate spike; classifier brief T2, no Fable (allowance unknown) |
| 00-repository | PR #146 r1 | Fixer | Opus 5.5 | 1 | done | fmt, clippy, test --locked clean locally | CodeRabbit round 1: 3 findings (2 docs, JCS duplicate-key rejection); classifier T2 |
| 00-repository | #6 | Implementer | Opus 5.5 | 1 | done | fmt, clippy, tests clean locally on Linux; Spikes workflow runs pending | FP-005 binding spike; classifier brief T2, no Fable (allowance unknown); CodeRabbit round 1: 2 fixed (T1 Sonnet 5.5 Fixer), 1 deferred to #4 |
| 00-repository | PR #146 r2 | Fixer | Sonnet 5.5 | 1 | done | docs only; fmt clean | CodeRabbit round 2: remove bench.rs coverage exemption (REQ-NFR-008); triage fast path T1 |
