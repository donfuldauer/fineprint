---
name: triage
description: "The Orchestrator's inline fast path for classifying obviously small Fine Print work items (T0/T1 fixes, CodeRabbit nits, docs/run-log/queue edits, CI tweaks) under docs/PROCESS.md P4, and the procedure for batching one review round or push. Do NOT use it for units, Independent Tester tasks, fixture tasks, investigations, gate failures, or anything that fails the eligibility checks; those go to the work-classifier."
---

# triage

The canonical instructions for this skill live in `.claude/skills/triage/SKILL.md` (shared by Claude
Code and Gemini / Antigravity). Read that file in full and follow it, using the Gemini /
Antigravity tool, subagent and model-tier mappings in `AGENTS.md` and `docs/PROCESS.md`.
