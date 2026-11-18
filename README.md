# Fine Print

**The rules engine that reads what the card actually says.**

Fine Print is a clean-room, deterministic, data-driven rules engine and state machine for a
collectible trading card game, targeting full parity with that game's Comprehensive Rules. It is a
standalone, UI-agnostic library: a desktop client, a web frontend, a tournament server, an AI
search agent or a headless test runner can all drive a complete match through one typed API.

> **Status: foundations in progress.** The requirements, including the architecture and platform
> decisions, are in [`docs/SRS.md`](docs/SRS.md) (§17 holds the nonfunctional requirements). The
> work is planned as [GitHub issues](https://github.com/donfuldauer/fineprint/issues?q=is%3Aissue+label%3Awork-unit)
> (label `work-unit`, milestones M1–M3), one reviewable pull request each, with dependencies
> linked. The workspace, CI, the deterministic base library and the card DSL's data schema have
> merged. The rules engine itself (game state, events, layers, turns, casting, combat) has not
> been started, so Fine Print cannot play a game yet. See [Project status](#project-status).

## Design in one screen

- **Deterministic.** The whole match is an immutable state tree; every transition is
  `S(t+1) = f(S(t), action)`, with the PRNG inside the state. No floating point and no
  unordered iteration in rules paths, so every language binding replays to identical state hashes.
- **Event-sourced.** Every atomic change is an event routed through one replacement/prevention bus
  and one trigger detector, so card interactions nobody anticipated still compose correctly.
- **Cards are data, and the data is yours.** Card behavior is a declarative AST. The client runs
  the toolchain (`fineprint-ingest`) on card data it obtains itself; the parser compiles current
  Oracle text into definitions. Fine Print ships no card data and maintains no card definitions,
  and the engine never branches on a specific card.
- **The engine tells you what it can't do.** Every card carries a support status (`VERIFIED`,
  `AUTO_PARSED`, `PARTIAL`, `UNSUPPORTED`). A support report lists every card that needs an
  override, with the unparsed text spans; the client supplies overrides, which the engine
  validates and smoke-tests. A card is `VERIFIED` only when the client's behavioral tests for it
  pass. The engine never silently runs a card it cannot execute correctly.
- **Embeddable.** A Rust core with a C ABI (`fineprint.h`), a .NET 10 package and a WebAssembly
  npm package with TypeScript types, tested on Linux, Windows, macOS and browsers. Player-redacted
  views, sandbox controls, replay logs and determinized forking for AI search.
- **Milestone 1:** two-player constructed play. Multiplayer, Commander and the casual variants
  follow, and the data model is built for N players from the start.

| Artifact | Name |
|---|---|
| Core library | `fineprint-core` / `FinePrint.Core` / `fineprint.h` / `fineprint.wasm` |
| Card toolchain | `fineprint-ingest` |
| Card database (built by the client) | `*.fpdb` |
| Scenario tests | `*.pzl` |
| Replays | `*.fpreplay` |

## Project status

As of plan item FP-013, 13 of the 141 `work-unit` issues have merged. They cover the
workspace and CI (FP-001 to FP-006), the deterministic base library (FP-007 to FP-009) and the
first four DSL schema units (FP-010 to FP-013). Together that is about 50,000 lines of Rust
and about 700 tests (368 of them black-box spec tests).

### Implemented

**Workspace, CI and guards (FP-001 to FP-006)**

- A Cargo workspace of nine product crates (`fineprint-base`, `-dsl`, `-core`, `-ingest`, `-ffi`,
  `-wasm`, `-bot`, `clients/cli`, `xtask`) plus the spike crate `spikes/libs`, with Rust pinned
  to 1.97.0, edition 2024 and MPL-2.0 headers on every source file. Dependencies point downward
  only (base → dsl → core → bindings and tools). Floats, `HashMap` and `HashSet` are banned in the rules crates through
  `clippy.toml`, and unsafe code is forbidden everywhere except `fineprint-ffi`.
- CI on Linux, Windows and macOS: `cargo fmt`, `clippy -D warnings` and `cargo test`. It also
  enforces per-crate line-coverage floors (`cargo xtask coverage`) and runs repository guards
  (`cargo xtask guard all`: license headers, no card data, no card-specific branches). A
  requirement-traceability check (`scripts/req_coverage.py`) confirms that a work-unit issue
  covers every SRS requirement.
- Comprehensive Rules citation checking (`cargo xtask cr fetch|index|check`, the
  `CR citations` workflow). CI fetches the official rules text and caches it outside the
  repository. Every CR number cited in the SRS or the code is checked against it, and
  `docs/cr-citations.json` stores only rule numbers and SHA-256 hashes, never rule text.
- Spikes that de-risked the library choices and the bindings (`spikes/libs`, `spikes/bindings`):
  PRNG, hashing, canonical JSON, MessagePack and `pest`, plus cbindgen, .NET 10 P/Invoke,
  wasm-bindgen and headless browsers.

**`fineprint-base`: deterministic primitives (FP-007 to FP-009)**

- Checked integer arithmetic, scale-2 fixed-point numbers, identifier newtypes with monotonic
  generators, and ordered collections with a stable `(ObjectId, Timestamp)` sort.
- A PCG64-DXSM generator with SplitMix64 seed expansion, unbiased sampling and shuffles, pinned
  by test vectors.
- An injective canonical binary encoding, SHA-256, and an incremental Merkle state-hash
  framework. These are the basis of replay and loop detection.

**`fineprint-dsl`: the card definition schema (FP-010 to FP-013)**

- Card definitions and faces, structured mana costs, support status, provenance, `dslVersion`
  checks and a `definitionHash` computed over RFC 8785 canonical JSON.
- References, selectors, predicates, integer value expressions, rules text as rewritable text
  tokens, and `ChoiceSpec` decision descriptions.
- The ability envelope and all six ability kinds: triggered abilities and their events,
  activated abilities and costs, static abilities and replacement effects.
- Effect instructions: a closed set of 72 ops covering durations, control flow, delayed and
  reflexive triggers and the `NATIVE_EFFECT` node. Decoding is strict, nesting depth is
  bounded, and every type round-trips through the canonical wire format.

These are **data types and wire contracts only**. Nothing executes a card definition yet.

### Still to do

The crates `fineprint-core`, `-ingest`, `-ffi`, `-wasm`, `-bot` and `clients/cli` are still
empty stubs. The remaining 128 issues, by phase:

| Phase | Plan items | What it delivers |
|---|---|---|
| 01 Foundations (finishing) | FP-014 to FP-016, FP-140, FP-141 | Rules vocabulary artifact, definition lints, synthetic test-card library, wiring the full AST into card definitions |
| 02 State and objects | FP-017 to FP-025 | Game state tree, zones, permanents, characteristics, card layouts, last known information, knowledge/redaction core, checkpoints and rollback |
| 03 Events | FP-026 to FP-030 | Game events and history ledger, choice protocol, replacement/prevention bus, trigger detection, delayed and reflexive triggers |
| 04 Layers | FP-031 to FP-039 | Layer pipeline, timestamps and dependencies, lock-in, RuleSet view, layer cache, text-changing, copy and face-down effects, ETB look-ahead |
| 05 State-based actions | FP-040 to FP-042 | The SBA registry and every SBA family |
| 06 Turn structure | FP-043 to FP-049 | Priority, turn sequencer, deck validation, game start and mulligans, legal-action generation, extra turns, the `.pzl` scenario runner |
| 07 Casting | FP-050 to FP-054 | Casting procedure, cost determination, mana payment, auto-tap solver, activated and loyalty abilities |
| 08 DSL runtime | FP-055 to FP-064 | The DSL interpreter, resolution, every effect family, designations, native-effect registry, copies of spells and abilities |
| 09 Combat | FP-065 to FP-067 | Attack and block constraint solvers, combat damage |
| 10 Keywords | FP-068 to FP-074 | Keyword macro registry, keyword families, keyword actions, coverage audit |
| 11 Loops | FP-075 to FP-076 | Loop detection and the shortcut protocol |
| 12 Engine API | FP-077 to FP-084 | `IRulesEngine` facade, redacted views and wire schema, event stream, inspection, replays, sandbox, determinized forking, card support reports and overrides |
| 13 Card toolchain | FP-085 to FP-099 | Card-data import, Oracle text tokenizer, grammar templates, status assignment, smoke tests, `*.fpdb` build and coverage report |
| 14 Bindings | FP-100 to FP-102, FP-104 | C ABI and `fineprint.h`, .NET 10 package, WebAssembly npm package, cross-binding golden-replay determinism CI |
| 15 Clients | FP-103, FP-105 to FP-109 | Random-legal player, synthetic demo decks, terminal, C, .NET and web demo clients |
| 16 Regression | FP-110 to FP-116 | `.pzl` regression suites for layers, replacement and triggers, casting, combat, layouts, SBAs and loops |
| 17 Hardening | FP-117 to FP-121 | Fuzzer, benchmarks, no-card-branch audit, release packaging, **Milestone M1 acceptance** (two-player constructed) |
| 18 Multiplayer | FP-122 to FP-130 | N-player turns, leaving the game, monarch and initiative, Commander, community formats |
| 19 Variants | FP-131 to FP-139 | Subgames, Two-Headed Giant, Planechase, Archenemy, Vanguard, Conspiracy, ante, the Un-set and digital extensions |

The issue tracker is the source of truth. Issues labelled `ready` can start now (currently
FP-017, FP-140 and FP-141), and FP-014 is in progress.

## Building and testing

```bash
cargo build --workspace
cargo test --workspace
cargo xtask guard all
```

[`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) lists every command, CI job, coverage floor and
repository guard.

## Repository layout

```
docs/SRS.md               requirements (normative)
docs/PROCESS.md           how agents turn the plan into merged PRs (rules R1–R12, P1–P8)
docs/BRIEF_TEMPLATE.md    the self-contained delegation brief every agent receives
docs/DECISIONS.md         decisions made during the run, with rationale
docs/HUMAN_QUEUE.md       items only a person can do (PROCESS.md P8)
docs/RUN_LOG.md           one row per unit of work
docs/DEVELOPMENT.md       workspace layout, commands, CI gates, coverage floors, owned paths
docs/cr-citations.json    index of cited CR rule numbers and hashes (no rule text)
crates/fineprint-base     deterministic primitives: numbers, ids, PRNG, encoding, hashing
crates/fineprint-dsl      card definition schema and canonical JSON
crates/fineprint-core     the rules engine (stub)
crates/fineprint-ingest   offline card toolchain (stub)
crates/fineprint-ffi      C ABI binding (stub)
crates/fineprint-wasm     WebAssembly binding (stub)
crates/fineprint-bot      reference bots (stub)
clients/cli               terminal client (stub)
xtask/                    repository automation: coverage, guards, CR citation check
spikes/                   throwaway spikes that chose the libraries and binding toolchains
contracts/                change requests for shared contracts
scripts/req_coverage.py   checks every SRS requirement is traced by the work-unit issues and tests
.claude/agents/           work-classifier, task-executor, fixture-author, work-verifier,
                          issue-investigator, rules-researcher
.claude/skills/           work-issue, run-plan, plan-from-srs, triage, pzl-test
CLAUDE.md                 entry point for Claude Code sessions
```

## How the work gets done

Development is run by Claude Code agents under [`docs/PROCESS.md`](docs/PROCESS.md):

1. **The plan is GitHub issues.** Each `work-unit` issue is one reviewable pull request, with
   its requirements, deliverables, acceptance criteria and "blocked by" links to the issues it
   depends on. Issues labelled `ready` can start now.
2. **"Start work on issue X"** in a Claude Code session on this repository runs the `work-issue`
   skill: it checks the dependencies are merged, routes the work to the cheapest capable model
   tier, implements and independently tests it, verifies it against the issue, opens a PR that
   closes the issue, resolves CodeRabbit's review, merges, and marks newly unblocked issues
   `ready`. `/run-plan` does the same across every ready issue.
3. A person is needed only for architecture decisions, SRS changes, legal questions and
   publishing releases.

## License

Fine Print is licensed under the [Mozilla Public License 2.0](LICENSE) (`MPL-2.0`). You may use,
modify and redistribute it, and embed it in applications under any license. Distributed changes to
Fine Print's own files must stay available under MPL-2.0 with the original notices kept.

## Legal

**Trademarks.** *Magic: The Gathering* is a trademark of Wizards of the Coast LLC. It is named
here only to describe what Fine Print is compatible with. Fine Print is an independent project,
and Wizards of the Coast has not approved, endorsed or sponsored it and is not affiliated with it.

**What this repository contains.** Original code and documentation only: no card names, Oracle text,
card definitions, art, set symbols or logos, and not the Comprehensive Rules text. Design notes
quote at most a few words of a rule where a decision turns on its wording. Fine Print's name and branding use no
third-party trademark, and the test suite uses invented synthetic cards. The Comprehensive Rules
text is never committed. CI downloads the official file into the git-ignored `cr-cache/`
directory and its build cache to check citations. Only rule numbers and hashes are committed,
in `docs/cr-citations.json`. Because Fine Print uses no Wizards of the Coast
materials, it does not rely on the Fan Content Policy and does not carry that policy's disclaimer.

**Card data.** Clients obtain card data themselves (for example from Scryfall, under its terms
of use) and build their own card databases with the toolchain. A client that does this uses the
data under Scryfall's terms and the Wizards of the Coast Fan Content Policy, and is responsible
for meeting them, including the policy's disclaimer. Commercial distribution of anything that
bundles card data requires legal review first (SRS §1.3).

This section is not legal advice.
