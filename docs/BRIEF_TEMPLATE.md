# Delegation Brief Template

The work-classifier fills this in for each delegation (PROCESS.md P4, P5), and the Orchestrator
passes it verbatim to the named agent. A brief must be usable without access to `docs/SRS.md`,
the GitHub issue, `docs/DEVELOPMENT.md` or `docs/PROCESS.md`: everything the agent needs is pasted in.

```text
WORK ITEM <id>: <title>          AGENT: task-executor | fixture-author | work-verifier | issue-investigator | rules-researcher
ROLE: Implementer | Independent Tester | Fixer | Fixture Author | Verifier | Investigator | Researcher
MODEL: <Haiku 5.5 | Sonnet 5.5 | Opus 5.5 | Fable 5.2 — or Flash-Lite | Flash | Pro | T3>   TIER: T0–T3 (+ P4 T3/Fable evidence if T3)
BRANCH: <branch>   BASE: main   PR: <number or "not yet open">

ISSUE: #<n> <url>   PLAN ID: FP-<NNN>
REQUIREMENT IDS: <REQ-… list from the issue>
CR REFERENCES:   <rule numbers, each with its text quoted from the rules-researcher's answer>

OWNED PATHS (you may modify only these):
  <paths>
DO NOT TOUCH:
  <paths, especially contracts owned by other units>
READ-ONLY CONTEXT:
  <public APIs of dependency units, with signatures>

GOAL
  <one paragraph on the result, not the method>

REQUIREMENTS / DELIVERABLES
  <numbered, each testable, each tagged with the REQ-* ID(s) it implements.
   For a CodeRabbit finding: the quoted comment, thread URL, file:lines, and what "fixed" means.
   For a fixture task: the regression scenarios and the abilities each synthetic card needs.>

CONTRACTS YOU MUST HONOUR
  <relevant SRS excerpts (schemas, tables, procedures) and contracts from docs/DEVELOPMENT.md, inlined in full>

RULES
  <PROCESS.md P2 rules R1–R12, inlined in full>

TESTS REQUIRED
  <tests to add or update, including .pzl scenarios; each test names its REQ-* IDs and CR rules;
   existing suites that must still pass>

COMMANDS
  <exact build / format / lint / test / coverage / replay commands from docs/DEVELOPMENT.md,
   with the environment variables for this run>

VERIFICATION CRITERIA
  <the issue's acceptance criteria, verbatim>

GATE / DONE WHEN
  <the unit gate from PROCESS.md P5, inlined, or the checks the work-verifier will run>

WHEN SOMETHING IS UNCLEAR
  Choose the option most consistent with the SRS, the issue and the decision-log excerpts below,
  record it in docs/DECISIONS.md (the recommendation is the decision) and continue. Do not wait.
  Do not add a docs/HUMAN_QUEUE.md line unless it is a PROCESS.md P8 item.
  If the question is what a rule of the game says, stop that item and report it as a
  rules question; the Orchestrator sends it to the rules-researcher.
  <relevant DECISIONS.md excerpts>

ROLE-SPECIFIC
  Implementer:        deliver code + unit tests; do not read the unit's spec tests.
  Independent Tester: you have NOT seen the implementation and must not ask for it. Write
                      black-box, property and .pzl tests from this brief alone.
  Fixer:              smallest change that meets "fixed"; regression test; name the thread(s).
                      Do not reply to or resolve threads; the Orchestrator does.
  Fixture Author:     synthetic cards + scenarios for the listed items only; no real card data (R5).
  Verifier:           re-run every gate step; trace each requirement and REQ-* ID to a passing
                      test; no style or design opinions.
  Investigator:       reproduce, root-cause, verdict, fix plan; change no repository files.
  Researcher:         cite and quote; change no repository files.

REPORT FORMAT
  <the report block from the receiving agent's definition in .claude/agents/ (forwarded by .agents/agents/)>
```
