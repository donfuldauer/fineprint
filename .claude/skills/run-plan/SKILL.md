---
name: run-plan
description: "The Orchestrator's loop for executing docs/PLAN.md under docs/PROCESS.md: works out which subphases and fixture tasks are ready, routes each through the work-classifier, spawns executors and testers in parallel, verifies, opens and drives PRs to merge, runs wave gates, and keeps the run log current. Use to start or resume unattended work once the plan exists (/run-plan, optionally with a wave or subphase id)."
---

# run-plan

You are the **Orchestrator** (PROCESS.md P5). You route, spawn, open PRs, merge and log. You write
no product code and review no code.

If `docs/PLAN.md` does not exist, stop and use the `plan-from-srs` skill instead.

## 0. Resume

1. `git fetch origin` and check out the latest `main`.
2. Read `docs/RUN_LOG.md` (last rows), `docs/HUMAN_QUEUE.md` (anything a person has answered
   since the last run: struck-through lines with a resolution), and the plan's §5 wave matrix.
3. List open PRs you created (`gh pr list --author @me` or the session's PR tools). Each open PR
   is driven to merge (step 4) before new work on the same paths starts.
4. Read the account's current usage limits once and set `FABLE_ALLOWANCE` to `sufficient`,
   `insufficient` or `unknown` for this session's classifier calls. Re-read it before any call
   that could route T3.

## 1. Pick ready work

A subphase is **ready** when every subphase it depends on is merged to `main`, it is not
`blocked` in the run log, and no open PR owns any of its paths. If an argument named a wave or
subphase, restrict to that. Fixture tasks are ready when the DSL schema and `.pzl` format they
use are merged.

Start every ready unit whose owned paths do not overlap, up to a sensible parallelism (default 3
at once).

## 2. Per subphase

1. **Classify.** Spawn `work-classifier` twice in parallel: once for the Implementer task, once
   for the Independent Tester task, each with `FABLE_ALLOWANCE`. If either returns `needs-rules`,
   spawn `rules-researcher` with all the questions in one call, then call the classifier again
   with the answers. Add both `run log line`s to `docs/RUN_LOG.md` and any `human queue line`.
2. **Execute.** Create the branch `sp/<id>` from `main`. Spawn the Implementer and the Independent
   Tester **in parallel**, each with its brief verbatim and its model as the `model` override.
   Their owned paths are disjoint by construction.
3. **Rules questions** in either report: one `rules-researcher` call, then a classifier call for a
   follow-up brief. SRS discrepancies go to `docs/HUMAN_QUEUE.md`.
4. **Verify.** Commit their work (one commit per role, `[sp <id>] …`) unless the briefs said the
   executor commits. Spawn `work-verifier` with both briefs and both reports.
5. **On fail.** For each failure: if the cause is not obvious, `issue-investigator` first; then
   the classifier (batched) and a Fixer; verify again. Apply the P7 failure policy and count
   attempts in the run log.
6. **Open the PR** (PROCESS.md P6 step 1) using the PR template, subscribe to its activity, and
   continue with other ready work. Do not sleep or poll.

## 3. Fixture tasks and parser coverage

- **Fixture tasks** (synthetic cards and regression scenarios, REQ-TEST-001) are classified like
  subphases, run by `fixture-author` on a `fixtures/<id>` branch, verified, and merged through a
  PR. Schedule each SRS area's fixtures alongside or just before the subphase that implements that
  area, so its Independent Tester can use them. `engine gaps` they report become fix tasks for the
  classifier against the owning module.
- **Parser coverage** is grammar work like any other subphase. Pick the next templates from the
  aggregate residue patterns that CI measures on its cached card data (REQ-ING-013,
  REQ-NFR-004). Record the before/after aggregate numbers in the PR and the run log. Never paste
  card names or text into briefs, PRs or logs.

## 4. Driving a PR to merge

Handle each review round or push as one batch with the `triage` skill's procedure (§2 of that
skill). Dispositions, resolving and merge conditions are in PROCESS.md P6; rules disputes are
settled by a `rules-researcher` citation. After merging: delete the branch, unsubscribe, add the
run-log row with the PR number and merge commit, and go back to step 1, since merges make new work
ready.

## 5. Wave gate

When every subphase of a wave is merged (or blocked and optional), run the wave gate from
PROCESS.md P5 on `main` (the plan's CI matrix, golden replays across platforms and bindings,
earlier waves' tests, benchmarks). Green: tag `wave-N-green` and log it. Red: each failure becomes
a fix task through the classifier against the owning subphase; never pause for a person.

## 6. When to stop

Stop when no work is ready and no PR is open, or when every remaining unit is blocked. End with a
short account: waves completed, PRs merged, blocked units and why, `[Fable wanted]` lines, the
aggregate parser coverage, and the human-queue items a person should look at first.
