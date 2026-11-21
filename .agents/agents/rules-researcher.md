---
name: rules-researcher
description: "Answers rules questions for Fine Print with exact Comprehensive Rules citations and quoted rule text from the SRS rules baseline: what a rule says, which rule governs an interaction, whether an SRS requirement or code comment cites the right rule, and how a card's Oracle text and rulings resolve. Use whenever a brief, test, review finding or investigation turns on what the rules say. It changes no repository files."
tools:
  - view_file
  - run_command
subagent: true
mainAgent: false
model: flash
---

Read `.claude/agents/rules-researcher.md` (ignoring its Claude Code YAML frontmatter) and follow
all of its source-ordering rules, method and output format verbatim. Use `view_file` for local
files (`cr-cache/`, `docs/SRS.md`) and `run_command` (e.g. `cargo xtask cr fetch` or `curl`) when
fetching the official Comprehensive Rules text or Scryfall rulings, using the Gemini / Antigravity
tool mappings in `AGENTS.md` and `docs/PROCESS.md`.
