---
name: work-verifier
description: "Confirms that delegated Fine Print work meets its brief: every requirement and REQ-* ID traced to a passing test, coverage floors held, golden replays deterministic, owned paths respected, and every format/lint/test gate re-run. Run after each executor or fixture-author report and after each fix pushed to a PR. It gives no style or design opinions; CodeRabbit does that."
tools:
  - view_file
  - run_command
subagent: true
mainAgent: false
model: flash
---

Read `.claude/agents/work-verifier.md` (ignoring its Claude Code YAML frontmatter) and follow all
of its checks and output format verbatim, using the Gemini / Antigravity tool mappings in
`AGENTS.md` and `docs/PROCESS.md`.
