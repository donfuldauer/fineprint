---
name: triage
description: "The Orchestrator's inline fast path for classifying obviously small Fine Print work items (T0/T1 fixes, CodeRabbit nits, docs/run-log/queue edits, CI tweaks) under PROCESS.md P4, and the procedure for batching one review round or push. Do NOT use it for units, Independent Tester tasks, fixture tasks, investigations, gate failures, or anything that fails the eligibility checks; those go to the work-classifier."
---

# triage (PROCESS.md P4 "Fast path", P5 "Batching")

The Orchestrator runs this skill itself to classify small items without spawning a
work-classifier. It covers only the classification step: the work is still done by a
task-executor and checked by the work-verifier. The fast path never routes T2 or T3.

## 1. Eligibility (ALL must hold)

- The item is a fix, a CodeRabbit finding, a docs/run-log/queue edit or a CI tweak (never a
  unit, an Independent Tester task, a fixture task, an investigation or a gate failure).
- The tier is clearly T0 (Haiku 5.5 / Flash-Lite) or T1 (Sonnet 5.5 / Flash).
- It touches one module, or docs only.
- No contract change (PROCESS.md P3).
- No change to rules evaluation outcomes: nothing in the priority heartbeat, layer evaluator,
  replacement bus, trigger detection, SBA registry, PRNG, state hash or serialization; no golden
  replay hash changes.
- No security boundary: redaction and knowledge model, determinized forking, parsing or
  validating untrusted artifacts and client overrides, the `VERIFIED` check, the card-data
  boundary (R5).
- No change to synthetic test cards shared by other modules' tests (those go to the classifier).
- It does not turn on what a CR rule says (otherwise `rules-researcher` first).
- Validity and cause are clear (otherwise `issue-investigator` first).
- The fix is spelled out by the comment or is obvious.

If validity or cause is unclear, the item goes to the issue-investigator first and is then handled
by its verdict: `valid-defect` is triaged again from its report (fast path if it now passes every
check, else the work-classifier); `valid-but-deferred` or `pre-existing` is **Deferred** (commit
the investigator's new issue or DECISIONS.md entry, reply with it, resolve; no Fixer), unless it is an unfixed
security issue (including any information leak) or a verified unfixed correctness defect, which
stays unresolved and goes to the work-classifier; `not-a-defect` is **Not a defect** (reply with
the evidence and resolve; no Fixer). Any other failing check, or doubt, sends the item to the
work-classifier.

## 2. Batching procedure (one review round, or one push)

1. List every new finding (inline, nitpick, outside-diff and summary items).
2. Triage each as fast-path T0, fast-path T1, rules question, investigator, or classifier.
3. Send all rules questions to one `rules-researcher` call, and findings of unclear validity to
   the issue-investigator (in parallel when independent). Map investigator verdicts as in §1.
4. Move any fast-path item that touches a file also touched by a classifier-bound item into the
   classifier batch. Send all classifier-bound items to ONE work-classifier call.
5. Group the remaining fast-path items into one Fixer brief per (tier, module) group; all
   docs-only items (`docs/`, `.claude/`, `.agents/agents/`, `.agents/skills/`, `AGENTS.md`,
   `CLAUDE.md`, `GEMINI.md`, `README.md`) form one docs group (executable setup files such as
   `.agents/setup-env.sh` and `.agents/environment.json` are not docs-only and keep their code
   verification/regression-test requirement). Items of different tiers on the same file go in one
   brief at the higher tier.
6. Spawn executors, in parallel when owned paths do not overlap.
7. Run ONE work-verifier on the resulting head commit covering every item, whenever the round
   changed the head (a queue-only round uses the verifier's queue-only case). Skip it only when
   the head is unchanged.
8. Reply to and resolve threads per PROCESS.md P6 step 3.

## 3. Fixer brief template

A subset of `docs/BRIEF_TEMPLATE.md`. Pass it to the task-executor with the tier's model override.

```text
WORK ITEM <id>: <title>          AGENT: task-executor
ROLE: Fixer
MODEL: <Haiku 5.5 | Sonnet 5.5 | Flash-Lite | Flash>   TIER: T0 | T1
BRANCH: <branch>   PR: <number>
REQUIREMENT IDS: <REQ-… touched, or none>

OWNED PATHS (you may modify only these):
  <every file the findings touch, plus the regression test file for each code finding>

GOAL
  <one paragraph: the findings in this brief are fixed with the smallest change>

REQUIREMENTS
  Per finding:
  N. Quoted comment: "<comment text>"
     Thread URL: <url>
     File:lines: <path>:<a>-<b>
     Fixed means: <exact observable condition>
  Report each thread URL and what changed. Do not reply to or resolve threads.

TESTS REQUIRED
  A regression test for each finding that changes code; "none (docs only)" otherwise.
  Existing suites that must still pass: <list>.

RULES
  <PROCESS.md P2, R1–R12, inlined in full>

COMMANDS
  <environment block from docs/briefs/COMMON.md, then the exact format / lint / test commands
   from docs/DEVELOPMENT.md for the touched module>

DONE WHEN
  <each "fixed" condition holds; the commands pass; the diff touches only OWNED PATHS>

REPORT FORMAT
  status: done | blocked
  requirements: <each item -> met (test or evidence) | not met>
  gate results: <per command>
  threads: <thread URL -> what changed, commit>
  decisions logged: <list>
```

## 4. Run-log convention

Fast-path rows in `docs/RUN_LOG.md` start the Notes column with `triage`, the tier and a one-line
reason, e.g. `triage T0: README typo fix spelled out by the comment`.
