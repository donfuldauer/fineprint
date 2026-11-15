# Fine Print — agent instructions

Fine Print is a deterministic, data-driven rules engine. The requirements are `docs/SRS.md`
(SRS-TCG-ENGINE-001). The process every agent follows is `docs/PROCESS.md` (cite it as P1–P8,
rules R1–R12). The plan is `docs/PLAN.md`.

**If `docs/PLAN.md` does not exist yet:** no product code is written. Run the `plan-from-srs`
skill: it gets the architecture and platform decisions from the user, writes them into the SRS as
`REQ-NFR-*` nonfunctional requirements, then generates the plan. This is the only step where the Orchestrator stops to ask a person.

**Once the plan exists:** run the `run-plan` skill to pick up the next ready work and drive it.

- **Model routing (P4).** Every unit of work goes through the `work-classifier` agent first,
  except items that pass the `triage` skill's fast-path checks. Tiers: Haiku 5.5, Sonnet 5.5,
  Opus 5.5. **Fable 5.2 only** for very-high-blast-radius (P3) or extremely hard-to-troubleshoot
  work, with evidence, **and** only when the account's current usage limits leave enough Fable
  allowance. Unknown allowance means no Fable.
- **Agents (P5).** `.claude/agents/`: `work-classifier`, `task-executor`, `fixture-author`,
  `work-verifier`, `issue-investigator`, `rules-researcher`. Pass the classifier's brief verbatim
  and its model as the `model` override. Subagents cannot spawn subagents; you do every spawn.
- **Rules questions** go to `rules-researcher`. Never put CR rule numbers or text into a brief,
  code comment or reply from memory.
- **No self-review (R9).** Do not review code yourself or with a sibling agent. Open a PR against
  `main`, subscribe to its activity, resolve every CodeRabbit finding with a reply, and merge
  without waiting for a person once the P6 merge conditions hold.
- **Non-negotiables:** determinism (R3), no card-specific branches (R4), no card data anywhere in
  the repo, synthetic test cards only (R5), `VERIFIED` only through passing tests (R11).
- Record ambiguous choices in `docs/DECISIONS.md` and `docs/HUMAN_QUEUE.md` and continue (R8).
