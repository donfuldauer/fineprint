# Fine Print

**The rules engine that reads what the card actually says.**

Fine Print is a clean-room, deterministic, data-driven rules engine and state machine for a
collectible trading card game, targeting full parity with that game's Comprehensive Rules. It is a
standalone, UI-agnostic library: a desktop client, a web frontend, a tournament server, an AI
search agent or a headless test runner can all drive a complete match through one typed API.

> **Status: pre-implementation.** The requirements, including the architecture and platform
> decisions, are in [`docs/SRS.md`](docs/SRS.md) (§17 holds the nonfunctional requirements).
> The work is planned as [GitHub issues](https://github.com/donfuldauer/fineprint/issues?q=is%3Aissue+label%3Awork-unit)
> (label `work-unit`, milestones M1–M3), each one reviewable pull request with its dependencies
> linked. No engine code exists yet.

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

## Repository layout

```
docs/SRS.md               requirements (normative)
docs/PROCESS.md           how agents turn the plan into merged PRs (rules R1–R12, P1–P8)
docs/BRIEF_TEMPLATE.md    the self-contained delegation brief every agent receives
docs/DECISIONS.md         decisions made during the run, with rationale
docs/HUMAN_QUEUE.md       items waiting for a person
docs/RUN_LOG.md           one row per unit of work
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

Fine Print is unofficial Fan Content permitted under the Fan Content Policy. Not approved/endorsed
by Wizards. Portions of the materials used are property of Wizards of the Coast. ©Wizards of the
Coast LLC.

This repository and Fine Print's packages contain no card names, rules text, card definitions,
art or set symbols. Clients obtain card data themselves (for example from Scryfall, under its
terms of use) and build their own card databases with the toolchain; anyone who redistributes a
card database is responsible for those terms and the Fan Content Policy. Commercial distribution
of anything that bundles card data requires legal review first (SRS §1.3).
