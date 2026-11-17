---
name: work-issue
description: "Takes one Fine Print GitHub issue from start to merge: checks its dependencies are merged, branches, routes the work through the work-classifier and agents, verifies, opens a PR that closes the issue, drives the CodeRabbit review to resolution, merges, and unblocks dependent issues. Use when the user says 'Start work on issue X', 'work on #X', 'pick up issue X', 'start work on the next ready issue', or similar; add 'and self-chain' to have it start a fresh session for the next issue after merging."
---

# work-issue

You are the **Orchestrator** (PROCESS.md P5) for exactly one GitHub issue. The issue is the unit
of work and its own spec; the plan is the set of `work-unit` issues, never a repository file. You route, spawn, commit, open the PR, drive
review, merge and log. You write no product code and review no code (R9).

Repository: `donfuldauer/fineprint`. In Claude Code web sessions GitHub GraphQL is unavailable:
use REST (`gh api repos/donfuldauer/fineprint/...`) and the session's `ccr` routes for review
threads.

## 1. Load and check the issue

0. **Self-chain flag.** Set `SELF_CHAIN` to true only if the prompt that started this work asks
   for it in some form ("and self-chain", "chain to the next issue", "keep going in a new
   session"). Otherwise it is false, and §8 does nothing. A request to work an issue is not a
   request to chain.
   **No issue number** ("the next ready issue"):
   - **Resume abandoned work first.** Candidates are issues labelled `in-progress` (never one
     labelled `status:blocked-failure` or `blocked`) and your open PRs. One is abandoned only if
     the session named in its latest claim comment (step 4) is not live: `get_session` on it
     reports a status bucket other than `working` or `blocked`, or the comment names no session.
     Never resume work a live session owns.
   - **Otherwise** list ready issues as `run-plan` §1 does and take the lowest `FP-NNN` whose
     owned paths overlap no active issue.
   - Claim it with step 4. If the claim loses, pick the next candidate.
   - If none qualifies, tell the user why and stop (no chaining).
1. `gh api repos/donfuldauer/fineprint/issues/<N>` — title, body, labels, state, milestone.
   Stop if it is closed, or if it is not a work unit (no `Plan ID` line).
2. **Dependencies.** The "Depends on" section lists `#numbers`; GitHub's "blocked by" links
   (`gh api repos/donfuldauer/fineprint/issues/<N>/dependencies/blocked_by`) list the same.
   Every one must be **satisfied**: closed as completed and closed by a merged PR (the issue
   timeline, `gh api repos/donfuldauer/fineprint/issues/<n>/timeline`, has a `closed` event
   from a merged PR, or a merged PR body contains `Closes #<n>`). A blocker closed without a
   merged PR counts only if a comment on it names the replacement issue(s) that delivered the
   work and those replacements pass the same test. If any is unsatisfied, stop and tell the user
   which ones, and which of them are themselves ready to start. Do not start blocked work.
3. Read the issues it depends on, `docs/DEVELOPMENT.md` (if it exists yet), the SRS sections
   the issue names, and any `docs/DECISIONS.md` entries its area touches.
4. **Claim, then verify.** This applies whether or not the prompt named the issue. GitHub has
   no atomic lock, so the earliest claim wins.
   1. Post the claim comment: work has started, with the branch name and this session's ID
      (`get_session` with no argument returns it).
   2. Re-read the issue's comments. If another claim comment that is earlier than yours, and
      later than the issue's last `ready` label or abandoned-work resume, names a session that is
      live (`get_session` reports `working` or `blocked`), your claim loses: delete it. With an
      issue number, tell the user which session owns the issue and stop. Without one, return to
      step 0 and pick the next candidate.
   3. Only after the claim holds, add the `in-progress` label (remove `ready`).

## 2. Branch

Each issue gets its own git worktree, so issues running in parallel never share a working tree.
One branch, one PR, one issue.

1. `git fetch origin`. Look for an existing branch and worktree for the issue: `git branch -a
   --list "*issue/<N>-*"` and `git worktree list`.
2. **Found (a restart).** Resume it; never reset it to `origin/main`. Reuse the worktree if it
   exists, otherwise `git worktree add ../fineprint-issue-<N> <branch>` (for a remote-only
   branch, `git worktree add -b <branch> ../fineprint-issue-<N> origin/<branch>`). In the
   worktree, if the remote branch exists, `git merge --ff-only origin/<branch>`. Read the issue
   comments and the branch's commits, and continue from the step they show was reached.
3. **Not found.** `git worktree add -B issue/<N>-<short-slug> ../fineprint-issue-<N>
   origin/main`.
4. Every later step for this issue (agents' working directory, commits, pushes, checks) runs
   inside that worktree. It is removed in §7.

If the session may push only to an assigned branch, use it and note that in the PR.

## 3. Route and execute

1. Read the account's usage limits once and set `FABLE_ALLOWANCE` (`sufficient`,
   `insufficient` or `unknown`).
2. Spawn `work-classifier` with: the full issue body, its number and URL, the
   branch, and `FABLE_ALLOWANCE`. For a `type:feature` or `type:binding` issue ask for **two**
   briefs: Implementer and Independent Tester (owned paths disjoint). For `type:fixtures`, one
   `fixture-author` brief. For `type:infra`, `type:toolchain`, `type:client` and `type:docs`,
   an Implementer brief whose TESTS REQUIRED include the issue's acceptance tests.
3. `needs-rules` → one `rules-researcher` call with all questions, then call the classifier
   again with the answers. SRS discrepancies go to `docs/HUMAN_QUEUE.md`.
4. Spawn the agents the classifier names with the briefs verbatim and its models as `model`
   overrides; Implementer and Independent Tester in parallel.
5. Agent reports with `rules questions` → rules-researcher → classifier follow-up brief.
   `engine gaps` or `requests outside owned paths` that this issue cannot absorb become a
   **new GitHub issue** (labelled like this one, "Depends on" filled in) linked from the PR.
6. Commit the work (one commit per role or deliverable, subject prefixed `[#<N>]`), unless the
   brief said the executor commits.

## 4. Log and verify

First add the run-log row(s) for this issue to `docs/RUN_LOG.md` (phase = the issue's `phase:` label, work
item = `#<N>`) and commit, so the verified commit is the one the PR opens on. Any later fix
commit updates the row and is verified again.

Then spawn `work-verifier` with every brief and report, the branch and the base `origin/main`. On
`fail`: investigator if the cause is unclear, then classifier → Fixer → verify again, under the
P7 failure policy. Three failed attempts plus one tier-up attempt → mark the issue `blocked`
(label `status:blocked-failure`), comment the account, add a `docs/HUMAN_QUEUE.md` line, and stop.

## 5. Open the PR

1. Check the head is the commit the work-verifier last passed. Push:
   `git -c push.negotiate=true push -u origin <branch>`.
2. Create the PR with REST (`gh api repos/donfuldauer/fineprint/pulls -X POST -f title=… -f
   head=<branch> -f base=main -F body=@<file>`). The body follows
   `.github/pull_request_template.md`, starts with `Closes #<N>`, and fills every section
   (write "not applicable" with a reason rather than leaving one empty: CodeRabbit checks it).
3. Reply-to-review readiness: watch the PR. If the session can subscribe to PR activity, do so.
   Otherwise schedule a check-in with `send_later` (about 10 minutes) and end the turn with a
   one-line status; never sleep-poll in a loop. Repeat check-ins until the PR merges.

## 6. Review rounds

Each CodeRabbit review round (or push) is one batch, handled with the `triage` skill's §2
procedure and PROCESS.md P6:

- Read: `pulls/<P>/reviews`, `pulls/<P>/comments`, `issues/<P>/comments` (CodeRabbit's summary
  and pre-merge checks), `pulls/<P>/ccr/review_threads`, and the head commit's check runs.
- Every finding gets exactly one disposition (Fixed / Not a defect / Deferred) recorded as a
  reply (`pulls/<P>/comments/<id>/replies`), reason and evidence first.
- Resolve with `POST pulls/<P>/ccr/comments/<comment_id>/resolve`. Never resolve an unfixed
  security issue (including any redaction or forking leak) or a verified unfixed correctness
  defect.
- Failed pre-merge checks (for example the description check) are fixed by editing the PR body
  (`PATCH pulls/<P>`).
- CI failures go through the same loop (investigator if the cause is unclear).
- If CodeRabbit has not reviewed the head commit, post `@coderabbitai review` once; at most two
  more asks, then the HUMAN_QUEUE line of P6 step 4.

## 7. Merge

When every P6 step 5 condition holds on the head commit (required checks green, CodeRabbit
reviewed the head with no outstanding change request, every thread replied to and resolved,
branch up to date with `main`, last work-verifier result `pass` for the head):

1. Merge with a merge commit:
   `gh api repos/donfuldauer/fineprint/pulls/<P>/merge -X PUT -f merge_method=merge -f sha=<head>`.
2. The repository deletes merged branches automatically; check it did.
3. **Unblock dependents.** For each issue this one blocks
   (`issues/<N>/dependencies/blocking`), if all of its blockers are now satisfied (§1 step 2:
   closed as completed by a merged PR, or replaced by issues a comment on the blocker names), replace
   its `blocked` label with `ready` and comment "Unblocked by #<N> (PR #<P>)".
4. Remove `in-progress` from the issue if it is still present (it closes through `Closes #<N>`).
5. Remove the issue's worktree from the main checkout: `git worktree remove
   ../fineprint-issue-<N>`.
6. Tell the user: what merged, the PR link, the issues it unblocked, and the next ready issues
   in plan order.
7. If `SELF_CHAIN` is true, go to §8.

## 8. Self-chain (only when `SELF_CHAIN` is true)

Run this only after a merge in §7. Do not run it if the issue ended `blocked`, or if no issue is
now ready; say why in the final message instead.

1. Start one new session with `create_session`: `source_url`
   `https://github.com/donfuldauer/fineprint`, tag `fineprint-chain`, a title naming the
   next issue, and the prompt `Start work on the next ready issue and self-chain.` Leave the
   environment and model to be inherited.
2. Comment on the merged issue: "Self-chained to session <id>."
3. End the turn. Never start more than one session per merge, and never chain from a session
   whose prompt did not ask for it.

## Never

Self-chain unless the starting prompt asked for it; start an issue whose dependencies are open; push to `main`; merge with a failing required check
or an unresolved thread; skip, ignore or weaken a test; put card data in the repository (R5);
quote CR rules from memory (R6).
