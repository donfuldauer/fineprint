# Fine Print — Agent Execution Process

This file is the stable operating procedure for every agent that works on Fine Print. It does not
change when the plan changes. The **plan** is the set of **GitHub issues** labelled `work-unit`
(milestones M1–M3): each issue is one unit of work, with its dependencies, requirements, owned
paths and acceptance criteria. The plan is never written into the repository. `docs/DEVELOPMENT.md`
(created by the workspace issue) holds the workspace layout, commands, gates and contracts. This
file says *how* an issue becomes a merged pull request.

Section numbers below (P1–P8) are what agents, briefs and replies cite.

---

## P1. Sources of truth

| Source | Role | Who writes it |
|---|---|---|
| `docs/SRS.md` | Requirements (`REQ-<AREA>-<NNN>`), the rules baseline, and CR citations. Normative. | A person only. Agents propose SRS changes in `docs/HUMAN_QUEUE.md`. |
| GitHub issues (`work-unit`) | **The plan.** One issue per reviewable PR: plan ID (`FP-NNN`, also the plan order), summary, `Depends on` list (also linked as GitHub "blocked by" dependencies), REQ IDs, deliverables, acceptance criteria, owned paths, expected tier. Labels: `ready` / `blocked` / `in-progress`, `type:*`, `area:*`, `phase:*`, `tier:*`, and review mode `review:skip` / `review:sanity` / `review:full` (P6); milestones M1–M3. | `plan-from-srs`; new issues for discovered work by any Orchestrator, with dependencies filled in. |
| `docs/DEVELOPMENT.md` | Workspace layout, module ownership, exact commands and environment, coverage floors per crate, contracts and their owners, card-data and fixture paths. | The workspace issue creates it; later issues update it in their own PRs. |
| `docs/PROCESS.md` | This file. | A person, or a reviewed PR the person asked for. |
| `docs/DECISIONS.md` | Every ambiguity or judgement call an agent resolved, with rationale. The recorded recommendation is the decision (R8). | Any agent (rule R8). |
| `docs/HUMAN_QUEUE.md` | Only the P8 items: things a person must do or decide. Never judgement calls (those are decisions, R8) and never a request to close a review thread. | Any agent. |
| `docs/RUN_LOG.md` | One row per unit-of-work attempt, plus phase gates and merges. | The Orchestrator. |
| `contracts/CHANGE_REQUESTS.md` | Requests to change a contract (P3) after its owning unit merged. | Any agent. |
| `docs/briefs/` | Delegation briefs, when they are worth keeping (phase-wide `COMMON.md`, long briefs). | Work-classifier output, saved by the Orchestrator. |

When the SRS and an issue disagree, the SRS wins and the disagreement is recorded
in `docs/DECISIONS.md`. When the SRS and the Comprehensive Rules (CR) of the SRS's rules baseline
disagree, the CR wins, the work follows the CR, the erratum is recorded in `docs/DECISIONS.md`,
and, because only a person edits the SRS (P8), the proposed SRS edit is one line in
`docs/HUMAN_QUEUE.md` (that is how Appendix A of the SRS was built).

### Working with GitHub

Sessions interact with `donfuldauer/fineprint` through the official `gh` CLI (`gh issue`, `gh pr`,
`gh run`, `gh api`) or GitHub MCP tools when configured. On list and search REST endpoints
(`repos/donfuldauer/fineprint/issues`, `issues/<N>/comments`, `search/issues`), page explicitly
with `per_page=100` and `page=1, 2, …` until a page returns fewer than 100 items (never
`gh api --paginate`: a session proxy can block the numeric-ID links it follows). In Claude Code
web sessions where GitHub GraphQL is unavailable, use REST (`gh api repos/donfuldauer/fineprint/...`)
and the session's `ccr` routes for review threads; in Gemini / Antigravity sessions with the
GitHub plugin, `gh api graphql` and the `gh` subcommands work directly.

## P2. Execution rules (R1–R12)

These apply to every change, by every agent. Briefs inline them in full.

1. **R1 — Module boundaries.** An agent edits only the paths its brief lists as OWNED PATHS. If
   two units share a module in one phase, each owns a sub-directory and the earlier unit
   pre-stubs the module declarations.
2. **R2 — Contract-change protocol.** The contracts (P3) are written freely only by the unit
   that owns them. Any later change is appended to `contracts/CHANGE_REQUESTS.md` and lands as a
   separate contract-only PR. Additive changes merge immediately; breaking changes merge only
   between phases.
3. **R3 — Determinism (REQ-CORE-001, REQ-CORE-006).** Rules code, card ASTs and native effects
   read and write nothing outside the state `S_t`: no wall clock, no global mutable state, no I/O,
   no environment variables, no threads that affect results. No floating point in rules paths
   (signed 64-bit integers; fixed-point scale 2 for half values). No iteration over hash-ordered
   containers where order can affect the outcome: use ordered containers or sort on a stable key
   (ObjectId, then timestamp). All randomness comes from the PRNG stored in the state.
4. **R4 — No card-specific branches (REQ-DSL-013).** No engine procedure branches on a
   `cardDefId`, a card name, or anything else card-specific. Cards contribute effects only at the
   query points of SRS §14.6. A card that cannot be expressed uses a registered native effect
   (REQ-DSL-015), never a special case.
5. **R5 — No card data, anywhere (REQ-ARCH-001, REQ-NFR-004).** Nothing in this repository
   (source, tests, fixtures, generated files, CI artifacts) contains card names, Oracle text,
   rulings, flavor text, mana-symbol art, artwork, set icons or card definitions. Rules vocabulary
   the CR itself defines (keywords, CR 111.10 tokens, counter kinds, designations) is allowed.
   Tests use **synthetic cards**: invented definitions with neutral names (`test.flier_2_2`,
   `test.global_humility_like`). The only exception is design documentation (`docs/SRS.md`,
   and GitHub issues), which may name a card to identify an interaction. Scryfall bulk files live
   only in git-ignored caches. Product names and branding never contain third-party game names or
   marks; documentation that names the supported game carries the trademark notice from `README.md`.
6. **R6 — Cite the rules.** Every rules procedure, SBA registry entry, keyword macro and rules
   test cites the CR rule it implements (`// CR 613.8b`), using the numbering of the SRS rules
   baseline. Every test names the `REQ-*` IDs it covers. Unknown or uncertain rule numbers go to
   the `rules-researcher` agent, never from memory.
7. **R7 — Simulated time.** Tests never sleep and never assert on wall-clock time. Performance
   targets (REQ-LAYER-005) are measured by benchmarks, not asserted in unit tests.
8. **R8 — Never wait on a person.** When a brief is ambiguous, or a judgement call is needed,
   choose the option most consistent with the SRS, the issue and `docs/DECISIONS.md`; record it
   in `docs/DECISIONS.md` as `## <work item> — <title>` with **Decision**, **Rationale**,
   **Alternatives**; continue. The recommendation is the decision: it is final, needs no
   confirmation, and is not added to `docs/HUMAN_QUEUE.md`. Only a later `docs/DECISIONS.md`
   entry (or the repository owner) changes it. The only exceptions are the P8 items.
9. **R9 — No self-review.** No agent reviews code for style or design, its own or another
   agent's. The work-verifier checks objective facts only. Code review happens on pull requests by
   CodeRabbit, scheduled according to the PR's review mode (P6). On every reviewed PR, the
   Orchestrator resolves every review thread itself when justified and never leaves one for a
   person (P6 step 3).
10. **R10 — Tests are evidence, not obstacles.** Never delete, skip, ignore or weaken a test to
    get green. A test that contradicts the brief is corrected to assert exactly what the brief
    requires (never less), and the report names the brief clause and the evidence.
11. **R11 — The project authors no real card definitions (REQ-ING-007, REQ-ING-008).** Overrides
    for real cards are the client's job. No code path assigns `VERIFIED` by declaration: only
    passing client-supplied behavioral tests earn it, and nothing may bypass that check. The engine
    never silently runs a missing or incomplete definition. Agents write synthetic test cards only
    (R5).
12. **R12 — Untrusted inputs stay untrusted.** Card database artifacts, client overrides and
    their tests, `.pzl` files, replays and Scryfall/MTGJSON bulk data are parsed defensively: every parser survives a
    bounded random-bytes run without panicking, and no input can make the engine leak hidden
    information through a redacted view (REQ-CORE-004).

## P3. Contracts and blast radius

### Contracts
A *contract* is anything that more than one module, binding or artifact depends on.
`docs/DEVELOPMENT.md` names the issue that owns each. At minimum:

- the public engine API (`IRulesEngine`, SRS §13.2) and its language bindings;
- the card DSL schema and its `dslVersion` (SRS §14);
- the state model and the canonical state hash (SRS §2, REQ-LOOP-001);
- the replay format `*.fpreplay` and its header (REQ-API-005);
- the card database artifact `*.fpdb` and `RulesVocabulary` (REQ-ING-014);
- the override submission format and the card support report (REQ-ING-007, REQ-ING-016);
- the synthetic test-card library that spec tests across modules share;
- the `.pzl` scenario format (REQ-TEST-001);
- the wire protocol and `PendingChoiceRequest` taxonomy (SRS §15);
- the native-effect registry IDs (`id@version`, REQ-DSL-015);
- the PRNG algorithm and seed derivation (REQ-CORE-001, REQ-API-004).

### Blast radius
- `local`: one module, covered by its own tests.
- `module`: several files in one module, or a public API used elsewhere.
- `very-high`: at least one of
  - a breaking change to a contract above that three or more modules or bindings depend on;
  - a change to the outcome of rules evaluation for every game: the priority heartbeat, the
    layer evaluator, the replacement/prevention bus, trigger detection, the SBA loop, the PRNG,
    or the canonical state hash (golden replay hashes change);
  - a change at a security boundary: player-view redaction and the knowledge model, determinized
    forking (REQ-API-004), parsing or validating untrusted artifacts and client overrides, the
    `VERIFIED` check, or the card-data boundary of R5;
  - a change one revert cannot undo: format or data migrations that clients' databases depend
    on, published packages (crates, NuGet, npm), release packaging.

## P4. Model routing

Every unit of work (a unit, an Independent Tester task, a fixture task, a fix task, a
CodeRabbit finding, a CI or gate failure) goes through the **work-classifier** before anyone works
on it, except items that take the `triage` fast path.

| Tier | Claude Code (`model` override) | Gemini / Antigravity (`Model` override) | Typical work |
|---|---|---|---|
| T0 | Haiku 5.5 (`haiku`) | Flash-Lite (`flash_lite`) | Docs-only edits, run-log and queue lines, formatting, renames, a review nit whose fix the comment spells out exactly |
| T1 | Sonnet 5.5 (`sonnet`) | Flash (`flash`) | Well-specified changes in one module, adding tests, CI tweaks, most review findings, keyword macros or synthetic test cards from a clear template, verification runs |
| T2 | Opus 5.5 (`opus`) | Pro (`pro`, or `inherit` on a Pro/reasoning session) | New unit implementations; the layer evaluator, dependency solver, replacement bus, combat solvers, loop detection, determinized forking; grammar work; cross-module changes; anything ambiguous; every investigation |
| T3 | Fable 5.2 (`fable`) | High-tier escalation (`pro` at maximum reasoning) | Only under the T3 / Fable rule below |

Verification runs on T1 (`Sonnet 5.5` / `Flash`) and investigation runs on T2 (`Opus 5.5` / `Pro`)
by default; neither needs to differ from the implementing model. In Antigravity sessions where the
user has not requested switching model tiers, `invoke_subagent` may keep `inherit` when the harness
policy requires it.

**T3 / Fable rule.** T3 (Fable 5.2 on Claude Code; maximum-reasoning `pro` on Gemini / Antigravity)
is used only when all three hold:
1. **Need.** Blast radius is `very-high` (P3), **or** the work is extremely hard to troubleshoot:
   two full issue-investigator passes at T2 (`Opus 5.5` / `Pro`) found no reproducible root cause,
   or T2 used up the fix attempts of P7, or the failure is non-deterministic below the code's
   control (compiler, platform, binding runtime) and two T2 passes did not isolate it.
2. **T2 is not enough.** The classifier names the T2 attempt that failed, or says exactly why
   one would.
3. **Allowance / quota.**
   - **Claude Code:** Just before routing, the Orchestrator reads the account's current usage
     limits and finds enough Fable allowance for the task, with headroom, without going into
     overage. An allowance that cannot be read counts as not enough.
   - **Gemini / Antigravity:** T3 escalation requires the active plan and quota to support it (no
     rate limit, throttling or fallback). If unavailable or unconfirmed, run at T2.

If 1 and 2 hold but 3 does not, the work runs at T2 (`Opus 5.5` / `Pro`) and a `[Fable wanted]`
(Claude Code) or `[T3 wanted]` (Gemini / Antigravity) note goes in that unit's `docs/RUN_LOG.md`
row. The run never waits for allowance. Every T3 use is logged in `docs/RUN_LOG.md` with the
evidence for 1–3. When torn between two tiers, take the higher one, but never move up into T3 that
way.

**Fast path.** The Orchestrator classifies an obviously small item inline with the `triage` skill
(`.claude/skills/triage/SKILL.md`, forwarded by `.agents/skills/triage/SKILL.md`) instead of
spawning a work-classifier, under the eligibility checks in that skill. The fast path never routes
T2 or T3, and never handles units, Independent Tester tasks, fixture tasks, investigations or gate
failures.

## P5. Agents and the flow of one unit of work

Canonical agent prompts live in `.claude/agents/` (loaded directly by Claude Code); Antigravity
custom subagents live in `.agents/agents/` and forward to them so role instructions stay in one
place. In Antigravity environments that do not auto-load `.agents/agents/`, the Orchestrator
registers each role with `define_subagent` (using the prompt from `.claude/agents/<name>.md`, with
write tools enabled only for `task-executor` and `fixture-author`) and launches it with
`invoke_subagent`. (Standalone Gemini CLI does not discover `.agents/agents/`; see `GEMINI.md`.)
Subagents never spawn subagents (enforced by Claude Code and by policy in Gemini / Antigravity), so
the Orchestrator (the main session) does every spawn, passing the classifier's brief **verbatim**
and its model tier override. The Orchestrator writes no product code and reviews no code.

| Agent | Default tier (model) | Does | Never does |
|---|---|---|---|
| `work-classifier` | T1 (`Sonnet 5.5` / `Flash`) | Rates blast radius and difficulty, picks tier and agent, writes the self-contained brief (`docs/BRIEF_TEMPLATE.md`) | Edit files, implement, review |
| `task-executor` | from the classifier | Implementer, Independent Tester or Fixer, strictly from the brief | Leave owned paths, review others' code |
| `fixture-author` | from the classifier | Builds the shared synthetic test-card library and the `.pzl` scenarios of the SRS regression suite (REQ-TEST-001) | Use real card names or text, edit engine code |
| `work-verifier` | T1 (`Sonnet 5.5` / `Flash`) | Re-runs every gate; traces each requirement and `REQ-*` ID to a passing test | Give style or design opinions |
| `issue-investigator` | T2 (`Opus 5.5` / `Pro`) | Reproduces and root-causes failures; judges unclear review findings | Change repository files |
| `rules-researcher` | T1 (`Sonnet 5.5` / `Flash`) | Answers rules questions with exact CR citations and quoted text; checks SRS/CR consistency | Change repository files |

**Flow.** classify (inline via `triage` for eligible small items) → execute (Implementer and
Independent Tester in parallel for a unit) → verify → on `fail`, investigate if the cause is
unclear, then classify and execute the fix → verify again → open the PR (P6) → CodeRabbit
findings go through the same loop until the merge conditions hold.

**Rules questions.** When a brief needs CR text the classifier does not have, or two agents
disagree about what a rule says, the Orchestrator spawns `rules-researcher` and pastes its answer
into the brief's CONTEXT. Rule text is never paraphrased from memory into a brief.

**Batching.** One CodeRabbit review round or one push is one batch: the Orchestrator lists every
new finding; sends findings of unclear validity or cause to the issue-investigator first; takes
the obviously small ones through `triage`; moves any fast-path item that shares a file with a
classifier-bound item into the classifier batch; sends all classifier-bound items to one
work-classifier call; runs one Fixer brief per (tier, module) group, with docs-only items as one
docs group; and runs one work-verifier on the resulting head commit whenever the head changed.

**Branches.** `main` is protected. Each unit of work is one GitHub issue and runs on its own
branch from the latest `main` (`issue/<N>-<slug>` in its own worktree `../fineprint-issue-<N>`, or
directly in the checkout when a sandbox restricts sibling directories and only one issue runs in the
VM, or the branch the session assigns) and reaches `main` only through its own PR, which closes the
issue. Work found along the way that the issue cannot absorb becomes a new issue with its
dependencies filled in. If the session can push only to one assigned branch, record that in
`docs/DECISIONS.md` and prefix commit subjects with the work item (`[sp 3.2] …`).

**Unit gate (definition of done).** All must hold:
1. The format and lint commands in `docs/DEVELOPMENT.md` are clean (warnings are errors).
2. Implementer tests and every acceptance criterion in the issue pass.
3. The Independent Tester's spec tests pass. A disagreement is resolved against the brief.
4. Line coverage meets the floor for each touched module (REQ-NFR-008: 90% rules core, 75%
   everything else).
5. Property tests cover every stated invariant; every touched parser survives a bounded
   random-bytes run.
6. Determinism: the golden replay corpus replays to identical state hashes (10 runs, and on every
   binding and platform of REQ-NFR-003); cached and full-recompute layer modes agree
   (REQ-LAYER-005).
7. Every `REQ-*` ID the unit claims has at least one test that names it.
8. An automated path check confirms the diff touches only owned paths plus the shared files
   `docs/RUN_LOG.md`, `docs/DECISIONS.md`, `docs/HUMAN_QUEUE.md` and
   `contracts/CHANGE_REQUESTS.md`.
9. The work-verifier reports `pass`. The unit is done only when its PR merges under P6.

**Phase gate.** Full test suite on every platform in REQ-NFR-003; golden replay hashes identical
across platforms and bindings; every earlier phase's tests still pass; benchmarks have not
regressed by more than 10% (REQ-NFR-009). The phase's last merge commit is tagged
`phase-<NN>-green`.

## P6. Pull request, review and merge

Agents do not review their own code (R9). Review comes from CodeRabbit on pull requests, scheduled
according to the repository's review phase and the PR's review label, and the Orchestrator drives
every PR to a merge without a person.

### Review modes (initial usable build vs. post-usable build)

During an **initial-usable-build phase** (currently Milestone M1 — Two-player engine, until `#219`
/ FP-161 merges), per-PR CodeRabbit review is opt-in in `.coderabbit.yaml`
(`auto_review.enabled: false`, `profile: quiet`, `auto_pause_after_reviewed_commits: 2`) so
vertical-slice work stays green on tests, coverage floors, repository guards, differential layer
checks, `work-verifier` and CI without burning multi-round polish loops before the end-to-end
two-player flow runs:

- **`review:skip`** (default for routine M1 vertical-slice chunks: individual SBA additions, effect
  families, keyword families, grammar template slices, demo decks/clients, `.pzl` regression packs,
  benchmarks, and scoped follow-ups on already-reviewed spines): no automatic CodeRabbit review.
  Every merge is still gated by Implementer + Independent Tester tests, coverage floors (90% rules
  core / 75% others, REQ-NFR-008), `cargo xtask guard all`, differential cached vs. full-recompute
  layer digests (REQ-LAYER-005), `work-verifier` (`pass`, P5 unit gate), and all required multi-OS
  CI checks. (If a review is triggered manually with `@coderabbitai review`, any findings posted
  are still accounted for under step 3 before merge.)
- **`review:sanity`** (short, targeted mid-stream sanity review for anything expensive to change
  later): required on any issue or PR that defines or changes
  - public interfaces, schemas, wire/event contracts, or bindings (any P3 contract owner in
    `docs/DEVELOPMENT.md`, `contracts/CHANGE_REQUESTS.md`, `fineprint-dsl` wire schema, `.pzl` /
    `*.fpreplay` / `*.fpdb` formats, `IRulesEngine`, wire protocol, native-effect or keyword macro
    registries, C ABI / .NET / WASM bindings);
  - state machines, deterministic core rules loops, or `very-high` blast-radius rules evaluation
    (priority/SBA heartbeat, turn sequencer, layer evaluator & `GameState` state-hash integration,
    casting/cost-lock-in/payment state machine, DSL interpreter core & stack resolution, combat
    constraint solvers, mandatory loop detection & shortcut protocol);
  - security or compliance-critical paths (R5 card-data boundary including CI aggregate-only
    reporting, R11 override validation & `VERIFIED` enforcement, R12 hidden-information redaction,
    determinized forking, and untrusted-input parsers for Scryfall/MTGJSON/Oracle
    text/`.pzl`/`*.fpreplay`/`*.fpdb`); or
  - automated milestone determinism, conformance, or release gates (golden replay
    cross-platform/cross-binding CI `#106`, query-point conformance audit `#121`, release packaging
    `#122`).
  In `review:sanity`, CodeRabbit reviews in `quiet` profile. Fix blocking boundary, contract,
  schema, state-machine, determinism (R3), no-card-branch (R4), security/compliance (R5/R11/R12)
  and verified correctness defects; defer cosmetic style, naming or non-blocking refactor nitpicks
  to the M1 cohesive review pass (`#219` FP-161) or a new M2 `work-unit` issue instead of running
  multi-round polish loops.
- **`review:full`** (full CodeRabbit review and approval): used for the M1 cohesive full review
  pass (`#219` FP-161, once M1 works end-to-end), Milestone M1 acceptance (`#123` FP-121), and
  explicit opt-in PRs.

**Mutual exclusivity and precedence.** A PR carries at most one `review:*` label. Upgrading a
`review:skip` issue or PR to `review:sanity` or `review:full` removes `review:skip`. If
`review:skip` ever coexists with `review:sanity` or `review:full` on a PR, remove `review:skip`
and treat the stronger review label (`review:sanity` / `review:full`) as authoritative.

Once `#219` (FP-161) completes the cohesive review of the working M1 build, it flips
`.coderabbit.yaml` to **post-usable-build mode** (`auto_review.enabled: true`, `profile: chill`,
`labels: ["!review:skip"]`, `auto_pause_after_reviewed_commits: 2`) so every chunk PR (including
M2 and M3) gets tighter per-PR review by default unless explicitly opted out with `review:skip`
(for example mechanical run-log or housekeeping edits).

1. **Open.** The Orchestrator commits the unit's `docs/RUN_LOG.md` row(s) before the
   work-verifier's final run, so the verified commit is the PR head. When the verifier passes, it
   pushes the branch and opens a PR against `main` (`gh pr create --label <review:*>` or REST plus
   label edit) carrying the issue's `review:skip` / `review:sanity` / `review:full` label
   (upgrading to `review:sanity` — and removing `review:skip` — if the diff touches any
   `review:sanity` category above even when the issue lacked the label). The body follows
   `.github/pull_request_template.md`, starts with `Closes #<issue>`, states the review mode and
   reason under `## Work item`, and fills every section (write "not applicable" with a reason
   rather than leaving one empty). Bot names in the body go in backticks so nobody is notified by
   accident.
2. **PR watch / auto-fix.** Right after opening:
   - **Claude Code:** subscribe to the PR's activity (`subscribe_pr_activity`: comments, reviews,
     CI) if the session offers that; otherwise schedule `send_later` check-ins (about every 10
     minutes, or 3–5 minutes for a `review:skip` PR waiting only on CI) until the PR merges.
   - **Gemini / Antigravity:** Antigravity has no built-in PR auto-fix or GitHub-webhook
     subscription. Run the `pr-watch` skill (`.agents/skills/pr-watch/SKILL.md`), which schedules
     non-blocking check-ins (`/schedule` or a single async background check-in, or a persistent
     `/automation` job across sessions), handles CI failures and — on reviewed PRs (`review:sanity`,
     `review:full`, post-usable default without `review:skip`, or whenever CodeRabbit posted a
     review) — addresses CodeRabbit findings, resolves review threads itself, and repeats until
     the PR is ready to merge.

   Every CodeRabbit comment and CI failure is handled as it arrives or on check-in; nobody
   sleep-polls in a foreground loop. On an unreviewed PR excluded from automatic review
   (`review:skip` in either phase without `review:sanity`/`review:full`, or no opt-in label while
   `auto_review.enabled: false`), do not request `@coderabbitai review`.
3. **Every finding is accounted for** (on reviewed PRs, or whenever CodeRabbit posted comments),
   batch by batch (P5), each ending in exactly one disposition recorded as a reply on its thread:
   - **Fixed:** an executor commit addresses it and the verifier passed; the reply names the
     commit; the thread is resolved.
   - **Not a defect:** the reply gives the evidence (test output, commit, CR rule and its text,
     SRS requirement, DECISIONS.md section, `.coderabbit.yaml` setting); the thread is resolved.
     A rules disagreement is settled by the `rules-researcher`'s citation, not by argument.
   - **Deferred:** real but out of scope (or, on a `review:sanity` PR, a non-blocking style, naming
     or minor refactor nitpick saved for the cohesive pass). It links `#219` (FP-161) when it is
     an M1 cohesive-review item, or becomes a new `work-unit` GitHub issue (with `Depends on`
     filled in) or a `docs/DECISIONS.md` entry that records why it is not done now; the reply
     links it; the thread is resolved.

   No thread is resolved without a reply. On `review:full` / post-usable PRs, nitpicks are fixed
   when cheap (T0/T1), otherwise answered as above; on `review:sanity` PRs, prefer deferring
   cosmetic or refactor nitpicks over extra fix rounds.

   **Resolving.** The Orchestrator resolves threads itself, on its own judgment, once the reply
   states the reason and evidence first. It does not wait for CodeRabbit to agree. Resolve with
   GraphQL `resolveReviewThread` via `gh api graphql`, a GitHub MCP `resolve_review_thread` tool
   if connected, or — in Claude Code web sessions where GitHub GraphQL is unavailable — the
   session's REST routes (`POST /repos/{owner}/{repo}/pulls/{n}/ccr/comments/{comment_id}/resolve`,
   list with `GET …/pulls/{n}/ccr/review_threads`).
   **Never resolved:** a thread reporting an unfixed security issue (including any information
   leak through redaction or determinized forking, R12) or a verified correctness defect that is
   not yet fixed. A person is never asked to resolve a thread. When every other merge condition
   holds on a reviewed PR and CodeRabbit's approval is all that is missing, post
   `@coderabbitai approve`.
4. **Re-review (reviewed PRs only).** Batch fix commits locally before pushing; `.coderabbit.yaml`
   sets `auto_pause_after_reviewed_commits: 2` so incremental reviews pause after two reviewed
   commits rather than burning review allowance on every push. New findings go through step 3. If
   CodeRabbit has not reviewed the head commit on a PR that requires review (for example it paused
   at the commit limit or was rate-limited), ask once with `@coderabbitai review`, wait as long as
   it says, and ask at most twice more. If it still has not reviewed, the PR stays open, a
   `docs/RUN_LOG.md` row records it, and the run continues with independent work.
5. **Merge conditions**, all on the current head commit:
   - every required CI check is green;
   - **CodeRabbit review (conditional on review mode):**
     - if the PR requires review (`review:sanity`, `review:full`, post-usable default without
       `review:skip`), or if CodeRabbit posted a review on the PR anyway: CodeRabbit has reviewed
       the head with nothing actionable unanswered and no outstanding change request, and every
       thread has a disposition reply and is resolved (except types that cannot be resolved);
     - if the PR is excluded from automatic review (`review:skip` in either phase without
       `review:sanity`/`review:full`, or no opt-in label while `auto_review.enabled: false`) and
       no CodeRabbit review was posted: no CodeRabbit review or approval is required;
   - the branch is up to date with `main`;
   - the work-verifier's last result for the head is `pass`.
   Then merge with a merge commit (`gh pr merge <P> --merge --match-head-commit <sha>`
   or REST; the repository deletes the branch), unsubscribe if subscribed, and unblock dependents:
   every issue whose blockers are now all satisfied swaps its `blocked` label for `ready`. A
   blocker is satisfied only if it is closed as completed by a merged PR (the issue timeline shows
   a `closed` event from a merged PR, or a PR merged into `main` whose body contains a whole-number
   match for `Closes #<n>` — `#<n>` not followed by another digit), or a comment on it names the
   replacement issue(s) that delivered the work and those pass the same test. The run-log row was
   committed in the PR itself, so nothing is pushed to `main` directly.
6. **Never**, to get green: skip, ignore, delete or weaken a test; force-push or rewrite history
   on a branch the agent did not create; push an empty commit; close and reopen a PR to restart
   CI; merge with a failing required check.

## P7. Failure policy

1. The executor gets up to three fix attempts, each from a classifier brief built on the
   verifier's failures, the investigator's report or the review findings.
2. Then the issue-investigator makes a fresh pass and the classifier routes one more attempt one
   tier up from a fresh brief plus the failure history (T3 / Fable only under P4).
3. Then the unit is marked `blocked` in `docs/RUN_LOG.md`, and the issue gets the
   `status:blocked-failure` label and a comment with the failure history.
4. A blocked issue outside milestone M1 (REQ-NFR-010) is
   skipped with its dependents; the run continues.
5. A blocked *core* unit halts only its dependents. Independent work continues, and the run
   ends with a partial result and a precise account of what is missing.
6. Parser coverage is a measured number, not a gate: a card template the grammar cannot handle
   yet is reported as residue (REQ-ING-013) and becomes grammar work, never a blocked run.

## P8. What does wait for a person

The run is unattended except for these, which are queued in `docs/HUMAN_QUEUE.md` and never
block independent work. They are the only items ever added to that file; judgement calls are
decisions (R8):

- architecture and platform decisions, which become `REQ-NFR-*` requirements (`plan-from-srs` §1);
- changes to `docs/SRS.md` or this file;
- anything with legal weight: distribution of card data, licensing, commercial use (SRS §1.3);
- publishing a release (crates, NuGet, npm packages);
- repository settings an agent cannot change (for example branch-protection required checks).
