---
name: plan-from-srs
description: "Turns docs/SRS.md into Fine Print's plan, which is a set of GitHub issues (one reviewable PR each) with dependency links, labels and milestones: first gets any missing architecture and platform decisions from the user and writes them into the SRS as REQ-NFR-* requirements, then creates the work-unit issues with full REQ-ID traceability. Use when there are no work-unit issues yet, or when an SRS or architecture change needs new or changed issues."
---

# plan-from-srs

The canonical instructions for this skill live in `.claude/skills/plan-from-srs/SKILL.md` (shared
by Claude Code and Gemini / Antigravity). Read that file in full and follow it, using the Gemini /
Antigravity tool and model-tier mappings in `AGENTS.md` and `docs/PROCESS.md`.
