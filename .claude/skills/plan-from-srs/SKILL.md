---
name: plan-from-srs
description: "Turns docs/SRS.md into Fine Print's plan, which is a set of GitHub issues (one reviewable PR each) with dependency links, labels and milestones: first gets any missing architecture and platform decisions from the user and writes them into the SRS as REQ-NFR-* requirements, then creates the work-unit issues with full REQ-ID traceability. Use when there are no work-unit issues yet, or when an SRS or architecture change needs new or changed issues."
---

# plan-from-srs

The plan turns the SRS into units of work the agents in `.claude/agents/` can execute under
`docs/PROCESS.md`. **The plan is GitHub issues, never a repository file**: each `work-unit` issue
is one unit, sized as one reviewable PR, and its body is the unit's spec. Every later brief is
built from an issue, so issues must be concrete: owned paths, deliverables, acceptance criteria.

This is the one workflow in which the Orchestrator stops to ask a person (PROCESS.md P8).

## 1. Architecture and platform decisions (ask before planning)

Read `docs/SRS.md` and `docs/DECISIONS.md`. Every decision below must be made by the user and
written into the SRS as a **nonfunctional requirement** before step 2. Issues are derived from
requirements, never from chat or from the decision log alone.

1. **Ask.** For each decision with no `REQ-NFR-*` requirement in the SRS, ask the user with
   AskUserQuestion (at most four per call; SRS default first, marked Recommended, one-line
   consequence per option). If the user is unreachable, stop here: unlike every other step in
   this repository, these decisions are never made by default.
2. **Write the SRS section.** Add or update `## 17. Nonfunctional Requirements: Architecture,
   Platforms & Toolchain` in `docs/SRS.md`. Each decision becomes one or more requirements with
   stable IDs `REQ-NFR-<NNN>` (SHALL/SHOULD language, testable, citing the functional REQ-IDs they
   constrain). Where a decision changes or narrows an existing requirement (for example REQ-API-001
   names the bindings, REQ-CORE-006 names the binding languages), revise that requirement and keep it
   consistent with the new requirement. The SRS is not versioned (git is its history): never add
   version numbers or change markers; a removed requirement keeps its ID as a one-line
   `[WITHDRAWN]` entry. Record each decision's rationale and alternatives in `docs/DECISIONS.md` as
   `## Architecture — <topic>` linking its `REQ-NFR-*` IDs.
3. **Propagate.** Update everything in the repository that the decisions make concrete:
   `.gitignore` and `.coderabbit.yaml` (language-specific paths and lockfiles), `docs/PROCESS.md`
   only where a rule depended on the open choice (it otherwise stays language-neutral), the
   verifier's and executor's command expectations if a tool is fixed (formatter, linter, coverage
   tool), `README.md` (status and architecture summary), and `docs/HUMAN_QUEUE.md` (strike the
   setup line). The SRS change is a person's change (PROCESS.md P8): the user approves the diff
   before it is committed.
4. **Sync the source of truth.** If the session has the claude.ai Project the SRS came from,
   write the updated SRS back to it so both copies match.

| # | Decision | SRS anchor | SRS default, if any |
|---|---|---|---|
| A1 | Core implementation language and toolchain pin | §13.1, REQ-ARCH-003 | Rust core (`fineprint-core`) |
| A2 | Bindings in scope for the first release, and which are *core* vs *optional* (P7) | §13.1, REQ-CORE-006 | C ABI (`fineprint.h`), .NET (`FinePrint.Core`), WASM (`fineprint.wasm`) |
| A3 | Supported host platforms and the CI matrix (OS × arch × binding runtime, browsers for WASM) | REQ-CORE-006 | none: must ask |
| A4 | Who maintains card definitions/overrides, and where card data may live (R5) | §1.3, §16 | decided: clients supply overrides; no card data in the repo |
| A5 | Toolchain for `fineprint-ingest`: same language as core or not; grammar technology (PEG/GLR library) | REQ-ING-006 | none: must ask |
| A6 | Serialization and hashing libraries: canonical JSON, MessagePack, sha256, incremental state hash (Zobrist or Merkle) | §14.1, REQ-LOOP-001 | as SRS |
| A7 | CR source: vendor the baseline CR text in the repo, or fetch in CI only (REQ-DOC-001) | §0.2 | none: must ask |
| A8 | Scope of the first milestone: which formats (two-player constructed only? Commander?), which variants and extensions (`ext.digital`, subgames, Un-sets) are deferred | §11 | none: must ask |
| A9 | Coverage floors per module, benchmark tolerance, and the reference hardware for REQ-LAYER-005 | §6.5 | suggest 85 % rules core, 70 % bindings/tooling |
| A10 | Card-data distribution for development: may Scryfall bulk files be cached in the repository or CI cache; is the repository public | §1.3, REQ-ING-014 | not committed; legal review before any public distribution |
| A11 | Review and CI: CodeRabbit installed; branch protection; required checks | PROCESS.md P6 | yes |

## 2. Design the work units

Before creating anything, write the breakdown out (in the session's scratch space, not the
repository) and check it.

**Sizing.** One unit is one PR a person would review in one sitting: one coherent change in one
area, usually under about 800 changed lines excluding generated files and fixtures. Split
anything larger.

**Dependencies.** Only real prerequisites (code the unit calls, a contract it implements
against, a tool it runs), so independent units can run in parallel. The graph must be acyclic,
and units are numbered `FP-NNN` in a topological order (that is the plan order).

**Issue body** (`.github/ISSUE_TEMPLATE/work-unit.md`): plan ID line (phase, milestone, expected
tier); summary; **Depends on** (issue numbers, or "None"); **Requirements** (each `REQ-*` ID with
the clause it covers and the SRS section; never quote CR text from memory, cite SRS sections);
**Deliverables** (checkboxes); **Acceptance criteria** (testable, checkboxes, ending with the P5
gate); **Owned paths**; **Notes** (out of scope, decisions to record). The first infrastructure
issue owns `docs/DEVELOPMENT.md` (layout, commands, gates, contracts, fixture and cache paths).

**Labels:** `work-unit`; `ready` / `blocked` (later `in-progress`); `type:` infra, feature,
binding, toolchain, client, fixtures, docs; `area:` the SRS area; `phase:NN-name`; `tier:T1` /
`tier:T2` (expected; T3 is never planned). **Milestones:** M1 per REQ-NFR-010, then the deferred
features in later milestones.

**Suggested phase order** (adapt to the decisions): repository and CI → foundations (numbers,
IDs, PRNG, hashing, DSL schema, lints, synthetic card library) → state, zones, objects and
layouts → events, choices, replacement and triggers → layers → state-based actions → turn
structure and priority → casting and resolution → DSL runtime effects → combat → keywords →
loops → engine API, views and replays → card toolchain → bindings → test clients → regression
fixtures → hardening and release → later milestones.

## 3. Check the breakdown

1. Every `REQ-*` ID in the SRS (except `[WITHDRAWN]`) appears in at least one unit. After the
   issues exist, `python3 scripts/req_coverage.py` checks this against the live issues.
2. The dependency graph is acyclic and every dependency comes earlier in plan order.
3. Owned paths of units that can run at the same time do not overlap.
4. Every contract in PROCESS.md P3 has an owning unit that comes before its first consumer.

## 4. Create the issues

0. List the existing `work-unit` issues (open and closed) and match each drafted unit to one by
   its `Plan ID`. Leave unaffected units alone. Edit a changed unit in place (body, labels,
   blocked-by links) only while it is open. Never edit a closed (completed) unit to carry new
   work: `work-issue` stops on a closed issue. When an SRS change affects a completed unit,
   create a new follow-up unit instead (next free `FP-NNN`) that depends on the completed one
   and references it. Create only units with no match, using the next free `FP-NNN` numbers.
   Close units dropped from the breakdown as "not planned", with a comment naming what replaced
   them.
1. Ensure the labels and milestones exist.
2. Open the new issues in plan order, so every "Depends on" entry names an existing issue
   number. Pace the calls (about one per second) to stay under GitHub's content-creation limits.
3. Link each dependency with GitHub's "blocked by" API
   (`POST /repos/{owner}/{repo}/issues/{n}/dependencies/blocked_by` with the blocker's numeric
   `issue_id`). Label a unit `ready` only when every dependency is complete under the merged-PR rule
   (PROCESS.md P6 step 5, `work-issue` §1 step 2); label the rest `blocked`. An open
   dependency, or one closed as not planned without a qualifying replacement, is not complete.
4. Run `python3 scripts/req_coverage.py` and fix any gap with an issue edit.
5. Nothing is committed for the plan itself. Record any decisions made while planning in
   `docs/DECISIONS.md` through a normal PR. Then: "Start work on issue X" (`work-issue`) or
   `run-plan`.
