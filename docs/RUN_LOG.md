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
