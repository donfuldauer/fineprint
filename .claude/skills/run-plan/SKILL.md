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
   is driven to merge (step 4) before new work on the same paths starts.
4. Read the account's current usage limits once and set `FABLE_ALLOWANCE` to `sufficient`,
   `insufficient` or `unknown` for this session's classifier calls. Re-read it before any call
   that could route T3.

## 1. Pick ready work

The units of work are GitHub issues. An issue is **ready** when it is open, labelled `ready`
(every issue in its "Depends on" list is satisfied: closed as completed by a merged PR, or
explicitly replaced as `work-issue` §1 step 2 describes), not `in-progress`, and no open PR owns any of
its paths. List them with
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
tree. Keep going with other ready issues while a PR waits for review.

## 3. Discovered work

Engine gaps, deferred findings and requests outside owned paths become new issues with their
dependencies, labels and milestone; nothing is added to the repository for them.

## 4. After each merge

Merges make new issues ready: go back to §1.

## 5. Phase gate

When every issue of the phase is complete under the merged-PR rule (closed by a merged PR, or
replaced as `work-issue` §1 step 2 describes; or optional and blocked), run the phase gate from
PROCESS.md P5 on `main` (the CI matrix, golden replays across platforms and bindings, earlier
phases' tests, benchmarks). Green: tag `phase-<NN>-green` and log it in the next PR. Red: each
failure becomes a new issue against the owning area; never pause for a person.

## 6. When to stop

Stop when no issue is ready, no issue is `in-progress`, and no PR is open, or when every remaining
issue is blocked. End with a short account: phases completed, PRs merged, blocked issues and why, `[Fable wanted]` lines, the
aggregate parser coverage, and the human-queue items a person should look at first.
