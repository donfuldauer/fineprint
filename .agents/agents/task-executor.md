---
name: task-executor
description: "Executes one delegated unit of Fine Print work strictly from its brief: implementing a unit (Implementer), writing black-box spec and .pzl tests without seeing the implementation (Independent Tester), or applying a fix or CodeRabbit finding (Fixer). Use after the work-classifier has routed the work; the Orchestrator passes the classifier's model tier override."
tools:
  - view_file
  - replace_file_content
  - write_to_file
  - run_command
subagent: true
mainAgent: false
model: pro
---

Read `.claude/agents/task-executor.md` (ignoring its Claude Code YAML frontmatter) and follow all
of its instructions and report format verbatim, using the Gemini / Antigravity model-tier and tool
mappings in `AGENTS.md` and `docs/PROCESS.md` P4–P5.
