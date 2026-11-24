---
name: fixture-author
description: "Builds Fine Print's synthetic test-card library and the .pzl scenarios of the SRS rules regression suite (REQ-TEST-001): invented card definitions that reproduce the abilities of the interactions the SRS names, with no real card names or text. Use for fixture tasks routed by the work-classifier, and when spec tests across modules need a shared synthetic card."
tools:
  - view_file
  - replace_file_content
  - write_to_file
  - run_command
subagent: true
mainAgent: false
model: pro
---

Read `.claude/agents/fixture-author.md` (ignoring its Claude Code YAML frontmatter) and follow all
of its hard rules, method and report format verbatim, using the Gemini / Antigravity model-tier
and tool mappings in `AGENTS.md` and `docs/PROCESS.md` P4–P5.
