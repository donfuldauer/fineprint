# Fine Print — Claude Code instructions

@AGENTS.md

## Claude Code notes

- **Settings:** shared permission defaults live in `.claude/settings.json`; put personal overrides
  in `.claude/settings.local.json` (git-ignored).
- **Agents and skills:** custom subagents live in `.claude/agents/` and skills in
  `.claude/skills/`.
- **GitHub:** in Claude Code web sessions where GitHub GraphQL is unavailable, use REST
  (`gh api repos/donfuldauer/fineprint/...`, paging list/search endpoints explicitly without
  `--paginate`) and the session's `ccr` routes for review threads (`docs/PROCESS.md` P1, P6).
