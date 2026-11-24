---
name: work-issue
description: "Takes one Fine Print GitHub issue from start to merge: checks its dependencies are merged, branches, routes the work through the work-classifier and agents, verifies, opens a PR that closes the issue, runs the CodeRabbit gate review when the issue completes a phase or milestone, merges, and unblocks dependent issues. Use when the user says 'Start work on issue X', 'work on #X', 'pick up issue X', 'start work on the next ready issue', 'Work on the next issue' (the merge-triggered scheduled task's prompt), or similar. Merging an issue/ branch starts a fresh session for the next issue (§8) when a GitHub-triggered automation is configured."
---

# work-issue

You are the **Orchestrator** (PROCESS.md P5) for exactly one GitHub issue. The issue is the unit
of work and its own spec; the plan is the set of `work-unit` issues, never a repository file. You route, spawn, commit, open the PR, drive
review, merge and log. You write no product code and review no code (R9).

Repository: `donfuldauer/fineprint`. Use the official `gh` CLI (`gh issue`, `gh pr`, `gh run`,
`gh api`, with explicit paging on list/search endpoints when a proxy blocks `--paginate`) or GitHub
MCP tools when configured. In Claude Code web sessions where GitHub GraphQL is unavailable, use
REST (`gh api repos/donfuldauer/fineprint/...`) and the session's `ccr` routes for review threads
(PROCESS.md P1 "Working with GitHub").

## 1. Load and check the issue

0. **No issue number** ("the next ready issue", "the next issue", "Work on the next issue"):
   - **Finish unblocking first.** A merge starts this session (§8), sometimes before the
     merging session reaches §7 step 3, and a session can end before finishing it. So run §7
     step 3's sweep, with its exceptions, before choosing.
   - **Resume abandoned work next.** Candidates are open issues labelled `in-progress` (a closed
     issue that still carries the label is skipped) and your open PRs. Neither may belong to an issue labelled `status:blocked-failure` or `blocked`; those
     wait for a person. One is abandoned only if it has no governing claim (step 4): no claim
     comment on it names a live session.
     Never resume work a live session owns.
   - **Otherwise** list ready issues as `run-plan` §1 does and take the lowest `FP-NNN` whose
     owned paths overlap no active issue.
   - Claim it with step 4. If the claim loses, pick the next candidate.
   - If none qualifies, tell the user why and stop.
1. `gh api repos/donfuldauer/fineprint/issues/<N>` — title, body, labels, state, milestone.
   Stop if it is closed, if it is not a work unit (no `Plan ID` line), or if it is labelled
   `status:blocked-failure`: a person must clear that label before it is claimed. If it is
   labelled `blocked`: with no blockers at all (`issues/<N>/dependencies/blocked_by` empty), a
   person set that hold, so stop and say so; otherwise run step 2's check, and if every blocker
   is satisfied the label is stale, so unblock the issue as §7 step 3 does (label change and
   comment) and continue; if not, stop as step 2 says. Without an issue number, exclude this
   candidate instead and continue with the next one under step 0.
2. **Dependencies.** The "Depends on" section lists `#numbers`; GitHub's "blocked by" links
   (`gh api repos/donfuldauer/fineprint/issues/<N>/dependencies/blocked_by`) list the same.
   Every one must be **satisfied**: closed as completed and closed by a merged PR (the issue
   timeline, `gh api repos/donfuldauer/fineprint/issues/<n>/timeline`, has a `closed` event
   from a merged PR, or a PR merged into `main` whose body contains a whole-number match for
   `Closes #<n>` — `#<n>` not followed by another digit). A blocker closed without a
   merged PR counts only if a comment on it names the replacement issue(s) that delivered the
   work and those replacements pass the same test. If any is unsatisfied, stop and tell the user
   which ones, and which of them are themselves ready to start. Do not start blocked work.
   Without an issue number, exclude this candidate instead and continue with the next one under
   step 0 (abandoned candidates first, then ready issues).
3. Read the issues it depends on, `docs/DEVELOPMENT.md` (if it exists yet), the SRS sections
   the issue names, and any `docs/DECISIONS.md` entries its area touches.
4. **Claim, then verify.** This applies whether or not the prompt named the issue. GitHub has
   no atomic lock, so the earliest claim wins, ordered by **comment ID**. Comment IDs are unique and
   increase over time, so there are no ties. A claim posted after yours always has a higher ID and
   can never supersede a claim that has already passed its check: that claim's session saw every
   lower-ID claim when it checked.
   - **Trusted claim authors.** Because this repository is public, ignore claim-shaped comments
     from untrusted accounts before applying the lowest-ID and liveness rules: a comment is
     eligible to govern **only** when its GitHub REST `author_association` is `OWNER`, `MEMBER`,
     or `COLLABORATOR` (write/maintain/admin collaborator) or its `user.login` matches the
     authenticated harness account (`gh api user --jq .login`).
   - **Governing claim.** An issue's **governing claim** is the lowest-ID eligible claim comment
     on it (from a trusted author) that is not released and names a live session. When
     `get_session` is available and resolves the claim's session ID, it is live if `get_session`
     reports `working` or `blocked` and ended otherwise. When `get_session` is unavailable (for
     example in Gemini / Antigravity) **or** cannot resolve the claim's session ID (for example
     when Claude Code inspects a claim posted by a Gemini / Antigravity session), treat any
     unreleased/undeleted claim from another session by a trusted author as live unless the owner
     confirms it is abandoned. There is no
   time cutoff: label changes (including a person re-applying `ready`) never invalidate a live
   claim. A claim stops counting only when its session ends, deletes it, or edits it to start with
   `Released:`, and a session deletes (or edits to `Released: <reason>` if delete is unavailable)
   its own claim whenever it loses (step 4), marks the issue blocked (§4) or finishes (§7).
   Resuming abandoned work writes no separate record: claims from ended sessions are never live,
   so they cannot govern, and the lowest-ID live claim still wins among sessions resuming at the
   same time. Compare governing claims,
   never an issue's latest claim. This relies on GitHub comment IDs being allocated in creation
   order and listed as soon as they are created: observed behaviour that GitHub does not document
   (DECISIONS.md "Self-chaining — claim ordering").
   1. Post the claim comment: work has started, with the branch name and this session's ID.
      In Claude Code, read the ID from the environment, not from `get_session`:
      `echo "session_${CLAUDE_CODE_REMOTE_SDK_URL##*/cse_}"` (only if that variable is unset,
      call `get_session` with no argument; call `get_session` on another session only to check
      whether a claim that isn't yours is live). In Gemini / Antigravity, include the
      conversation/session ID and UTC timestamp.
   2. Re-read **all** the issue's comments (`gh api -X GET repos/donfuldauer/fineprint/issues/<N>/comments -F per_page=100 -F page=<P>`,
      paged explicitly with `P = 1, 2, …` until a page returns fewer than 100 items, never relying
      on the default 30-item page), its labels and its state. Filter out comments from untrusted
      authors and only compare eligible claim IDs after the full comment set has been fetched. Your
      claim loses, and you
      delete it (or edit it to `Released: <reason>` if delete is unavailable), if the issue is now closed or labelled `status:blocked-failure` or `blocked`,
      if the governing claim is not yours, or (without an issue number) if the issue no longer
      qualifies as the candidate step 0 chose it as: a ready candidate must still carry `ready`
      and must not have gained `in-progress` from another session; an abandoned candidate must
      still have no other governing claim. With an
      issue number, tell the user which session owns the issue and stop. Without one, return to
      step 0 and pick the next candidate.
   3. **Path overlap.** A live claim reserves the issue's owned paths from the moment it is
      posted until the issue merges or is marked blocked. After posting yours, check this
      issue's owned paths against every other reservation: issues labelled `in-progress`, issues
      with an open PR, and open issues that have a governing claim. Issues labelled
      `status:blocked-failure` or `blocked` reserve nothing. If one overlaps, compare
      your claim's ID with that issue's governing claim's ID: the lower ID wins (an `in-progress`
      issue or open PR with no governing claim wins outright, unless your issue also has no
      other governing claim because you are resuming it as abandoned work: then the issue with the
      lower `FP-NNN` plan ID wins, so two abandoned overlapping issues never block each other).
      **Re-verify a loss once.** Two sessions can each lose at the same moment: one to the
      plan-ID tie-break against an abandoned issue, the other to the first one's claim before it
      is deleted. So before deleting (or releasing) a losing claim, wait about 30 seconds, then
      repeat with fresh
      reads the dependency check of §1 step 2 (every dependency still satisfied), the checks of
      step 4.2 (issue still open and not blocked, governing claim still yours and, without an
      issue number, the candidate still qualifies) and this overlap check. If all of them pass
      and you now win against every live overlapping claim, continue. Otherwise delete your
      claim (or edit it to `Released: <reason>` if delete is unavailable); if the dependency check
      failed, act as in §1 step 2 (report the unsatisfied
      dependencies and which of them are ready, or, without an issue number, move on to the
      next candidate), and for any other failure act as in step 4.2. This cannot let two
      overlapping sessions both proceed: a session only proceeds after a check in which its
      claim beats every live overlapping claim, and the lower comment ID always beats the
      higher.
   4. Only after the claim holds, run the §1 step 2 dependency check once more with fresh reads.
      If a dependency is no longer satisfied, delete your claim (or edit it to
      `Released: <reason>` if delete is unavailable) and act as in §1 step 2.
      Otherwise add the `in-progress` label (remove `ready`), then re-read the issue once more:
      if it is now closed or labelled `status:blocked-failure` or `blocked`, remove
      `in-progress`, delete your claim (or edit it to `Released: <reason>` if delete is
      unavailable) and act as in step 4.2. Checking after the label change
      closes the window between the last check and the change; a block or close after this
      point is handled like any other blocked or closed issue. The cleanup leaves the issue with
      neither `ready` nor `in-progress`. The person who clears `blocked` or `status:blocked-failure` replaces it with `ready`
      (as §7 step 3 does), and the person who reopens a closed issue for more work adds `ready`
      when reopening it; either makes the issue selectable again.

## 2. Branch

Each issue gets its own git worktree, so issues running in parallel never share a working tree
(if a sandbox restricts sibling directories and only one issue runs in the VM, use the
single-checkout fallback below). One branch, one PR, one issue.

0. **Select the target branch first.** Normally the branch is `issue/<N>-<short-slug>` (when
   GitHub-triggered session chaining is enabled, the next session starts only when an `issue/`
   branch merges, §8). However, if the session environment allows pushing only to a single
   **assigned branch**, select that assigned branch **before** discovery and checkout below, retain
   its existing local/remote history (never reset it to `origin/main`), and note in the PR body
   that the chain will not continue from this merge so the owner restarts it with the scheduled
   task's "Run now".
1. `git fetch origin`. Look for an existing branch and worktree:
   - If using an assigned branch `<assigned>`, look for `<assigned>` (`git branch -a --list
     "*<assigned>"`) and `git worktree list`.
   - Otherwise look for an existing issue branch: `git branch -a --list "*issue/<N>-*"` and
     `git worktree list`.
2. **Found (a restart, or an existing assigned branch).** Resume it; never reset it to
   `origin/main`.
   - *Worktree:* reuse the worktree if it exists, otherwise `git worktree add ../fineprint-issue-<N> <branch>`
     (for a remote-only branch, `git worktree add -b <branch> ../fineprint-issue-<N> origin/<branch>`).
     In the worktree, if the remote branch exists, `git merge --ff-only origin/<branch>`.
   - *Single-checkout fallback:* if the local branch exists, `git checkout <branch>`; for a
     remote-only branch, `git checkout -b <branch> origin/<branch>`. If the remote branch exists,
     `git merge --ff-only origin/<branch>`.
   Read the issue comments and the branch's commits, and continue from the step they show was
   reached.
3. **Not found** (no existing branch for `<branch>`, where `<branch>` is `<assigned>` if one was
   assigned, else `issue/<N>-<short-slug>`):
   - *Worktree:* `git worktree add -B <branch> ../fineprint-issue-<N> origin/main`.
   - *Single-checkout fallback:* `git checkout -B <branch> origin/main`.
4. Every later step for this issue (agents' working directory, commits, pushes, checks) runs
   inside that worktree (or directly in the checkout for the single-checkout fallback). If a
   worktree was created, it is removed in §7.

## 3. Route and execute

1. Set `FABLE_ALLOWANCE` (Claude Code) or T3 availability (Gemini / Antigravity) (`sufficient`,
   `insufficient` or `unknown`; PROCESS.md P4). In Claude Code, read the account's usage limits
   once (unknown means no Fable); on Gemini / Antigravity, it is `sufficient` when the active plan
   and quota support T3 and no rate limit, throttling or fallback is reported.
2. Spawn `work-classifier` (via Claude Code's `Agent` tool or Antigravity's `invoke_subagent` /
   `define_subagent`, PROCESS.md P5) with: the full issue body, its number and URL, the
   branch, and `FABLE_ALLOWANCE` / T3 availability. For a `type:feature` or `type:binding` issue
   ask for **two** briefs: Implementer and Independent Tester (owned paths disjoint). For
   `type:fixtures`, one `fixture-author` brief. For `type:infra`, `type:toolchain`, `type:client`
   and `type:docs`, an Implementer brief whose TESTS REQUIRED include the issue's acceptance tests.
3. `needs-rules` → one `rules-researcher` call with all questions, then call the classifier
   again with the answers. SRS discrepancies are recorded in `docs/DECISIONS.md`; the proposed SRS edit is a P8 line in `docs/HUMAN_QUEUE.md`.
4. Spawn the agents the classifier names with the briefs verbatim and its model tier overrides
   (keeping `inherit` in Antigravity if the harness policy requires it); Implementer and
   Independent Tester in parallel.
5. Agent reports with `rules questions` → rules-researcher → classifier follow-up brief.
   `engine gaps` or `requests outside owned paths` that this issue cannot absorb become a
   **new GitHub issue** (labelled like this one, "Depends on" filled in) linked from the PR.
6. Commit the work (one commit per role or deliverable, subject prefixed `[#<N>]`), unless the
   brief said the executor commits.

## 4. Log and verify

First add the run-log row(s) for this issue to `docs/RUN_LOG.md` (phase = the issue's `phase:` label, work
item = `#<N>`, T3 / Fable notes in Notes) and commit, so the verified commit is the one the PR opens on. Any later fix
commit updates the row and is verified again.

Then spawn `work-verifier` with every brief and report, the branch and the base `origin/main`. On
`fail`: investigator if the cause is unclear, then classifier → Fixer → verify again, under the
P7 failure policy. Three failed attempts plus one tier-up attempt → mark the issue `blocked`
(label `status:blocked-failure`, remove `in-progress`, delete or release this session's claim comment), comment the account, add a `docs/RUN_LOG.md` row, and stop.

## 5. Open the PR

1. Check the head is the commit the work-verifier last passed. Push:
   `git -c push.negotiate=true push -u origin <branch>`.
2. Decide whether `#<N>` is a **gate issue** (`docs/PROCESS.md` P6 "Gate reviews"): it carries
   `review:gate`, or it is the only open `work-unit` issue left with its `phase:*` label or in its
   milestone. Check with REST, paging explicitly
   (`gh api "repos/donfuldauer/fineprint/issues?labels=work-unit,<phase-label>&state=open&per_page=100&page=N"`,
   and `milestone=<number>` instead of the phase label). Re-check just before merging: an issue
   that was not a gate when the PR opened becomes one if every other issue of its phase merged
   meanwhile. Ignore any retired `review:skip` / `review:sanity` / `review:full` label or
   review-mode note on the issue.
3. Create the PR with `gh pr create --base main --head <branch> --title … --body-file <file>` or
   REST (`gh api repos/donfuldauer/fineprint/pulls -X POST -f title=… -f head=<branch> -f base=main
   -F body=@<file>`). No `review:*` label. The body follows `.github/pull_request_template.md`,
   starts with `Closes #<N>`, says under `## Work item` whether this is a gate issue (and links
   its gate review PR once open) or "routine (no code review)", and fills every section (write
   "not applicable" with a reason rather than leaving one empty). Bot names go in backticks.
4. Watch the PR:
   - **Claude Code:** if the session can subscribe to PR activity (`subscribe_pr_activity`), do so
     (and for the gate review PR too). Otherwise schedule a check-in with `send_later` (3–5
     minutes while waiting on CI, about 10 minutes while waiting on CodeRabbit) and end the turn
     with a one-line status; repeat check-ins until the PR merges.
   - **Gemini / Antigravity:** run the `pr-watch` skill (`.agents/skills/pr-watch/SKILL.md`), which
     schedules non-blocking check-ins (`/schedule` or a single async background check-in), handles
     CI failures and, on a gate review PR, CodeRabbit findings, and drives the PR to completion.
   Never sleep-poll in a foreground loop.

## 6. CI and gate review

- **Routine PRs (not a gate issue):** no CodeRabbit review; never request `@coderabbitai review`.
  Handle CI failures through the investigator / classifier / Fixer / `work-verifier` loop, and
  proceed to §7 as soon as every required CI check and `work-verifier` are green on the head. If
  CodeRabbit posted findings anyway (a person asked it to), account for them as below first.
- **Gate issues:** run the gate review of PROCESS.md P6 before merging:
  1. Base: the newest `reviewed/*` tag reachable from `origin/main`
     (`git describe --tags --match 'reviewed/*' --abbrev=0 origin/main`; if none, create and push
     `reviewed/baseline` at `60024a4`). Push `review-base/<gate>` at it
     (`git push origin <tag>^{commit}:refs/heads/review-base/<gate>`), where `<gate>` is the phase
     label without `phase:` (for example `04-layers`) or the milestone (`M1`).
  2. Open the review-only PR: head `<branch>` (this issue's branch), base `review-base/<gate>`,
     title `Gate review: <phase or milestone> (do not merge)`, body linking this issue's PR, the
     issues closed since the base, and the phase or milestone exit criteria. Post
     `@coderabbitai full review` on it. If CodeRabbit says the diff is too large, split it into
     area review PRs as P6 "Gate reviews" step 3 describes.
  3. Each CodeRabbit round (or push) is one batch, handled with the `triage` skill's §2 procedure
     and PROCESS.md P6:
     - Read: `gh pr view` / `gh pr checks` / `gh api graphql` (for review threads), or REST
       `pulls/<R>/reviews`, `pulls/<R>/comments`, `issues/<R>/comments` (CodeRabbit's summary),
       `pulls/<R>/ccr/review_threads`, and the head commit's check runs.
     - Fixes are committed on `<branch>`, so they land in both PRs; gate-review Fixer briefs may
       own paths outside the issue's owned paths (contracts still change only under R2).
     - Every finding gets exactly one disposition (Fixed / Not a defect / Deferred) recorded as a
       reply (`pulls/<R>/comments/<id>/replies`), reason and evidence first. Deferred items become
       new `work-unit` issues in a later phase or milestone; security (R5/R11/R12), verified
       correctness, R3/R4 and contract defects are never deferred.
     - Resolve with GraphQL `resolveReviewThread` via `gh api graphql`, `resolve_review_thread`,
       or the REST route `POST pulls/<R>/ccr/comments/<comment_id>/resolve`. Never resolve an
       unfixed security issue (including any redaction or forking leak) or a verified unfixed
       correctness defect.
     - Batch fix commits, push once per round, re-run `work-verifier`, then post
       `@coderabbitai review` on the review PR; at most two more asks if it does not review the
       head, then the RUN_LOG row of P6 "Gate reviews" step 5.
     - When CodeRabbit's approval is all that is missing, post `@coderabbitai approve`.
  4. Commit the gate's `docs/RUN_LOG.md` row (review PR links, rounds, deferred issues) before
     the final verifier run.

## 7. Merge

When every P6 merge condition holds on the head commit (required checks green; branch up to date
with `main`; last work-verifier result `pass` for the head; for a gate issue, `coderabbitai` has an
effective `APPROVED` review covering the head on every review PR of the gate, with every thread
replied to and resolved, and the P5 phase gate holds):

1. Merge with a merge commit:
   `gh pr merge <P> --merge --match-head-commit <head>` or REST
   (`gh api repos/donfuldauer/fineprint/pulls/<P>/merge -X PUT -f merge_method=merge -f sha=<head>`).
2. The repository deletes merged branches automatically; check it did.
3. **Unblock dependents (the sweep).** List every open `work-unit` issue labelled `blocked`
   (`gh api "repos/donfuldauer/fineprint/issues?labels=work-unit,blocked&state=open&per_page=100&page=N"`,
   paging N = 1, 2, … until a page returns fewer than 100; not `--paginate`, see `run-plan` §1),
   not only the issues `#<N>` blocks: a blocker cancelled in favour of a replacement issue is
   satisfied when the replacement merges, and the replacement's own "blocking" list does not name
   the waiting issue. For each, read its blockers (`issues/<B>/dependencies/blocked_by`). If all
   are satisfied (§1 step 2, replacements included):
   - Re-read the issue's labels immediately before changing them. Skip it if `blocked` is gone
     (the next session may already have unblocked it, §1 step 0) or `status:blocked-failure` is
     present (a person must clear that hold, PROCESS.md P7).
   - Add `ready` first, then remove `blocked`, each as a single-label call
     (`POST issues/<B>/labels`, `DELETE issues/<B>/labels/blocked`), so the issue is never
     without both and no other label is rewritten. If `ready` is already there, only remove
     `blocked`.
   - Comment "Unblocked by #<D> (PR #<P>)", naming the blocker whose merged PR satisfied it last
     (`#<N>` itself, a replacement, or another blocker) and that PR, unless a comment with that
     exact text (same blocker, same PR) is already there; an "Unblocked by" comment from an
     earlier unblock does not count. If no such issue and PR can be identified, change the labels
     without a comment and say so in the RUN_LOG row.

   Leave `blocked` on an issue with no blockers at all: a person set that hold (the plan sets
   `blocked` from dependencies; other holds use `status:blocked-failure`, PROCESS.md P7).
4. Remove `in-progress` from the issue if it is still present (it closes through `Closes #<N>`),
   and delete (or edit to `Released: merged in PR #<P>`) this session's claim comment.
   For a gate issue, also close every gate review PR without merging (comment with the merge
   commit), tag the merge commit `reviewed/<gate>` (and `phase-<NN>-green` for a phase gate),
   push the tags, and delete `review-base/<gate>` and any `review/<gate>-*` branches.
5. Remove the issue's worktree from the main checkout (if a worktree was used):
   `git worktree remove ../fineprint-issue-<N>`.
6. Tell the user: what merged, the PR link, the issues it unblocked, and the next ready issues
   in plan order.
7. Do nothing to start the next session (§8).

## 8. Next session

In **Claude Code**, a scheduled task with a GitHub trigger on pull requests merged into `main` from
`issue/` branches starts a fresh session that works on the next issue. This session does nothing to
hand off: never call `create_session`, `fire_trigger` or `update_trigger` to chain. A session
started by another session asks a person to approve every remote-session tool call, and a session
can neither fire nor edit a task it did not create. The chain runs until no issue is ready, or
until a session ends without merging an `issue/` branch; the owner then restarts it with the task's
"Run now". In **Gemini / Antigravity**, there is no native GitHub-merge webhook trigger; start the
next session manually (or use a persistent polling `/automation` job or `/run-plan`). If no
scheduled task exists or it is disabled, nothing starts automatically. See DECISIONS.md
"Self-chaining — next session starts from a merged issue/ PR".

## Never

Start a session to chain to the next issue; start an issue whose dependencies are open; push to `main`; merge with a failing required check
or an unresolved thread; skip, ignore or weaken a test; put card data in the repository (R5);
quote CR rules from memory (R6).
