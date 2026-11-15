---
name: plan-from-srs
description: "Generates docs/PLAN.md for Fine Print from docs/SRS.md: first gets the architecture and supported-platform decisions from the user and writes them into the SRS as REQ-NFR-* nonfunctional requirements (propagating them through the repo), then writes the module layout, commands, gates, contracts, phased subphases, wave matrix and full REQ-ID traceability. Use when docs/PLAN.md does not exist, or when the SRS version or an architecture decision changes and the plan must be regenerated."
---

# plan-from-srs

The plan turns the SRS into units of work the agents in `.claude/agents/` can execute unattended
under `docs/PROCESS.md`. Every later brief is built from it, so it must be concrete: exact paths,
exact commands, exact gates.

This is the one workflow in which the Orchestrator stops to ask a person (PROCESS.md P8).

## 1. Architecture and platform decisions (ask before planning)

Read `docs/SRS.md` and `docs/DECISIONS.md`. Every decision below must be made by the user and
written into the SRS as a **nonfunctional requirement** before step 2. The plan is derived from
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

## 2. Write `docs/PLAN.md`

Use this structure. Fill every section; write "none" rather than omitting one.

```
# Fine Print — Implementation Plan (from SRS at commit <sha>)
## 0. Decisions             (table: D-id | topic | decision | REQ-NFR-* | DECISIONS link)
## 1. Summary               (scope of this plan; what is deferred, from A8)
## 2. Workspace
### 2.1 Module layout        (module → path → owned by subphase → coverage floor)
### 2.2 Commands             (exact build, format, lint, test, coverage, .pzl runner, golden
                              replay check, IP/card-branch guard, req_coverage; environment
                              variables; per binding)
### 2.3 CI matrix            (from A3; which jobs are required checks)
### 2.4 Contracts            (each contract from PROCESS.md P3 → owning subphase → path)
### 2.5 Fixtures & caches     (synthetic test-card library path; git-ignored card-data and CR caches)
## 3. Contracts (normative excerpts the briefs paste: state types, IDs, the DSL schema files,
                 the .pzl schema, the replay header, the artifact layout)
## 4. Phases & subphases
## 5. Wave & dependency matrix (wave | subphases | depends on | expected tier)
## 6. Traceability            (every REQ-* ID → subphase(s) → test location)
## 7. Parser coverage roadmap  (SRS §16.5 order; how CI measures aggregate coverage)
## Appendix: open questions    (also mirrored in docs/HUMAN_QUEUE.md)
```

### Each subphase

```
#### Subphase <P.N>: <title>
- Requirements: REQ-… (every ID it fully or partly satisfies; "partly" says which clause)
- CR references: <rule numbers; text comes from the rules-researcher at brief time>
- Depends on: <subphases>      Optional: yes/no (P7)
- Owned paths: <paths>         Spec-test paths: <paths>
- Deliverables: <numbered>
- Verification criteria: <numbered, testable; include the REQ-TEST-001 scenarios that belong here>
- Expected tier: T1 | T2 (T3 never planned in advance)
- Independent Tester notes: <what to test black-box>
```

Keep subphases small enough for one executor brief: one module, one coherent procedure, roughly a
day of focused work. Split anything larger.

### Suggested phase skeleton (adapt to the decisions)

Dependency order matters more than the names:

0. **Spikes and pins:** toolchain, bindings build on every A3 platform, PRNG crate choice, hashing,
   grammar library; record results in DECISIONS.md.
1. **Foundation:** state tree and identities (§2.1–2.2), PRNG, canonical serialization and state
   hash, LKI records, per-object memory and linked store, checkpoint/rollback (§2.5), citation
   index and CI (REQ-DOC-001), CI matrix, IP/card-branch guard, golden-replay harness skeleton.
2. **Objects and zones:** zones (§3), characteristics and mana value (§4.1), layout behaviours
   (§4.2) one family per subphase, face-down (§4.3).
3. **Events:** the replacement/prevention bus (§8.1), trigger detection incl. look-back (§8.2),
   SBA registry (§9).
4. **Layers:** pipeline, CDA-first, dependency and loops, lock-in, post-layer passes, caching with
   differential testing (§6).
5. **Turn and actions:** heartbeat and priority (§5.1), steps (§5.2), casting/activation and
   resolution (§7), combat solvers and damage (§5.2), turn control (§5.3).
6. **DSL runtime:** interpreter for §14.4 node families, choice specs → pending choices, text
   tokens and layer 3, query points (§14.6), keyword macro registry (§14.7) in batches, native
   effects (§14.8).
7. **API and protocol:** `IRulesEngine` (§13), sandbox, determinized forking, replay integrity,
   redaction and the wire protocol (§15), `.pzl` runner (REQ-TEST-001).
8. **Ingestion toolchain** (§16): import/normalize, layout mapping, tokenizer, macro binding,
   grammar (by template frequency), overrides, validation, test generation, status, artifact and
   coverage report.
9. **Formats and multiplayer:** FormatConfig, Commander, leaving the game, designations, loops
   and shortcuts (§11, §12, §10.3); optional: community formats, `ext.digital`, subgames.
10. **Bindings and cross-platform determinism:** C ABI, .NET, WASM per A2; golden replays on every
    binding and platform.
11. **Parser coverage waves:** grammar work ordered by CI's aggregate residue patterns (§16.5);
    fixture tasks for the regression suite run alongside the subphases they test.
12. **Hardening and release:** fuzzing (REQ-TEST-001.5), performance (REQ-LAYER-005), coverage
    report publication, release packaging (a person signs off, P8).

Phases 1–7 are mostly sequential; 8 can start after 6's schema is fixed; 9–10 overlap 8; 11 starts
when 8 can parse. Work outside milestone 1 (REQ-NFR-010) is planned as optional subphases.

## 3. Check the plan

1. Run `python3 scripts/req_coverage.py` from the repository root. Every `REQ-*` ID in the SRS
   must appear in §6 of the plan. Fix the plan until it reports no missing IDs; IDs deliberately
   deferred are listed in §1 with the decision that defers them, and the script's `--allow`
   option names them.
2. Every owned path belongs to exactly one subphase per wave.
3. Every contract in PROCESS.md P3 has an owning subphase that comes before its first consumer.
4. Every command in §2.2 has been run once (or the subphase that makes it runnable is named).
5. Send the CR references of the first two waves to the `rules-researcher` in one batch, and fix
   any rule number it reports as wrong or renumbered before committing.

## 4. Land it

Create `docs/briefs/` with a `COMMON.md` holding the environment block from §2.2. Add a
`docs/RUN_LOG.md` row (`plan | — | Orchestrator | <model> | 1 | done | req_coverage clean | …`),
commit on a `plan/v1` branch, and open a PR under PROCESS.md P6. The plan is in force once it
merges; then use the `run-plan` skill.
