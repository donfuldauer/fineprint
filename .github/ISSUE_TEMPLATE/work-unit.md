---
name: Work unit
about: One reviewable unit of Fine Print work (one PR). Use for new work found during a unit.
title: "FP-XXX: <title>"
labels: ["work-unit", "blocked"]
---

> **Plan ID:** FP-XXX · **Phase:** <n — name> · **Milestone:** <M1|M2|M3> · **Expected tier:** <T1 (Sonnet 5.5 / Flash) | T2 (Opus 5.5 / Pro)>

<!-- After creating the issue: add type:<feature|binding|toolchain|client|fixtures|infra|docs>,
     area:<…>, phase:<NN-name>, tier:<T1|T2>, and review:gate only if this issue must carry a
     phase/milestone gate review (docs/PROCESS.md P6); replace `blocked` with `ready` if every
     dependency is already satisfied (docs/PROCESS.md P6 routine step 3). -->

## Summary

<what this unit delivers and why, in two or three sentences>

## Depends on

- #<n> FP-… <title>   <!-- or "None: can start immediately" (then label `ready`) -->

## Requirements

- REQ-… — <clause this unit covers> (SRS §…)

## Deliverables

- [ ] …

## Acceptance criteria

- [ ] …
- [ ] The subphase gate in `docs/PROCESS.md` P5 passes.

## Owned paths

- `…`

## Notes

<constraints, related decisions, out of scope>

---
To work this issue, start a Claude Code or Gemini / Antigravity session on this repository and say **"Start work on issue #<n>"** (`work-issue` skill).
