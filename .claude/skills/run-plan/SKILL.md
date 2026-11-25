---
name: run-plan
description: "The Orchestrator's loop over the plan's GitHub issues: finds every ready issue (all dependencies merged), works each one with the work-issue skill (in parallel where owned paths do not overlap), drives PRs to merge, runs phase gates, and keeps the run log current. Use to start or resume unattended work across many issues (/run-plan, optionally with a milestone or phase)."
---

# run-plan

You are the **Orchestrator** (PROCESS.md P5). You route, spawn, open PRs, merge and log. You write
no product code and review no code.

If there are no `work-unit` issues, stop and use the `plan-from-srs` skill instead.

## 0. Resume

1. `git fetch origin` and check out the latest `main`.
2. Read `docs/RUN_LOG.md` (last rows), `docs/HUMAN_QUEUE.md` (anything a person has answered
   since the last run: struck-through lines with a resolution), and the open `work-unit` issues' phases and dependencies.
3. List open PRs you created (`gh pr list --author @me` or the session's PR tools). Each open PR
   is driven to merge (step 4) before new work on the same paths starts, except the PR of an
   issue labelled `status:blocked-failure` or `blocked`: it waits for a person and reserves no
   paths (§1).
4. Set `FABLE_ALLOWANCE` (Claude Code) or T3 availability (Gemini / Antigravity) to `sufficient`,
   `insufficient` or `unknown` for this session's classifier calls (PROCESS.md P4). Re-check it
   before any call that could route T3. In Claude Code, read the account's current usage limits
   once (unknown means no Fable); on Gemini / Antigravity, it is `sufficient` when the active plan
   and quota support T3 and no rate limit, throttling or fallback is reported.

## 1. Pick ready work

The units of work are GitHub issues. An issue is **ready** when it is open, labelled `ready`
(every issue in its "Depends on" list is satisfied: closed as completed by a merged PR, or
explicitly replaced as `work-issue` §1 step 2 describes), not `in-progress`, not labelled
`status:blocked-failure` or `blocked`, and no open PR of a non-blocked issue owns any of its paths. List them with
`gh api "repos/donfuldauer/fineprint/issues?state=open&labels=ready&per_page=100&page=N"`, paging
explicitly with N = 1, 2, … until a page returns fewer than 100 issues (do not use
`gh api --paginate`: this environment's proxy blocks it because it follows numeric-ID links).
Take them in plan order (ascending `FP-NNN` plan ID). If an argument named a milestone or phase, restrict to
that.

Reserve the owned paths (each issue's "Owned paths" section) of every active issue, meaning one
labelled `in-progress`, with an open PR, or that has a governing claim
(`work-issue` §1 step 4), until it merges or is marked blocked. Issues labelled
`status:blocked-failure` or `blocked` reserve nothing. Overlapping claims are settled by
`work-issue` §1 step 4.3. Start every ready
issue whose owned paths overlap no reservation and no other candidate, up to a sensible
parallelism (default 3 at once).

## 2. Per issue

Run the `work-issue` skill for it, from §1 (dependency check) to §7 (merge and unblock). Each
issue works in its own git worktree (`work-issue` §2), so parallel issues never share a working
tree. Keep going with other ready issues while a PR waits for CI or a gate review. Run at most
one gate review at a time (PROCESS.md P6): a second gate issue waits, with its PR open, until the
first gate closes out. After every merge, `work-issue` §7 re-queries the phase and milestone and,
if the merge left no open `work-unit` issue and no `reviewed/<gate>` tag exists, runs a late gate
(PROCESS.md P6 "Late gate"), which counts as the one running gate review; a `review:gate` label on
the planned final issue still takes the normal pre-merge gate path. Parallel issues of one phase
can both pass the §5 gate re-check, so never treat that re-check alone as sufficient.

## 3. Discovered work

Engine gaps, deferred findings and requests outside owned paths become new issues with their
dependencies, labels and milestone; nothing is added to the repository for them.

## 4. After each merge

Merges make new issues ready: go back to §1.

## 5. Phase gate

The phase's last open issue is its gate issue: `work-issue` §6 runs its CodeRabbit gate review
(cumulative since the last `reviewed/*` tag) before it merges, and the same review closes the
milestone when it is also the milestone's last issue. Alongside, run the phase gate from
PROCESS.md P5 on the gate branch (the CI matrix, golden replays across platforms and bindings,
earlier phases' tests, benchmarks). On merge, tag `phase-<NN>-green` and `reviewed/<gate>`. A red
check or a blocking finding too large for the gate branch becomes a new issue against the owning
area that the gate issue depends on; never pause for a person.

## 6. When to stop

Stop when no issue is ready, no issue is `in-progress`, and no PR is open, or when every remaining
issue is blocked. End with a short account: phases completed, PRs merged, blocked issues and why,
`[Fable wanted]` / `[T3 wanted]` lines, the aggregate parser coverage, and the P8 human-queue
items a person should look at first.
