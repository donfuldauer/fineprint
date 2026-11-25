#!/usr/bin/env bash
# This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
# If a copy of the MPL was not distributed with this file, You can obtain one at
# https://mozilla.org/MPL/2.0/.
#
# Enables the tracked hooks in .githooks/ for this clone and records which human the
# commit-msg hook should credit as co-author on agent-made commits. Idempotent and quiet.
#
# core.hooksPath is set to .githooks only when it is unset (or already .githooks) AND the clone's
# hooks directory (`git rev-parse --git-path hooks`) holds no non-.sample hook. Otherwise it is
# left alone and a warning with the manual command is printed to stderr (this script runs at
# session start and never fails because of it). FINEPRINT_FORCE_HOOKS=1 forces the replacement.
# The co-author below is always resolved and recorded.
#
# Agent harnesses run this automatically (Claude Code: the SessionStart hook in
# .claude/settings.json; Antigravity cloud VMs: .agents/setup-env.sh). Contributors can also run
# it by hand once per clone.
#
# The co-author ("Name <email>") is stored in this clone's `git config fineprint.coauthor` and
# is resolved, first match wins, from:
#   1. $FINEPRINT_COAUTHOR                  e.g. FINEPRINT_COAUTHOR="Jane Doe <jane@example.com>"
#   2. an existing `git config fineprint.coauthor` (set it in ~/.gitconfig to apply to every clone)
#   3. git's user.name/user.email, when that identity is a person rather than an agent
#   4. the GitHub account `gh` is authenticated as, using its no-reply address
#      (<id>+<login>@users.noreply.github.com), which is how cloud agent sessions resolve it
# If none resolves, the hook prints a warning on each commit and adds no trailer.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

current_hooks_path="$(git config --get core.hooksPath || true)"
if [[ "${current_hooks_path}" != ".githooks" ]]; then
  existing=""
  if [[ -n "${current_hooks_path}" ]]; then
    existing="core.hooksPath is already set to '${current_hooks_path}'"
  else
    git_hooks_dir="$(git rev-parse --git-path hooks)"
    if [[ -d "${git_hooks_dir}" ]]; then
      found="$(find "${git_hooks_dir}" -maxdepth 1 -type f ! -name '*.sample' -print | LC_ALL=C sort | tr '\n' ' ')"
      if [[ -n "${found}" ]]; then
        existing="existing hooks in ${git_hooks_dir}: ${found}"
      fi
    fi
  fi
  if [[ -z "${existing}" || "${FINEPRINT_FORCE_HOOKS:-}" == "1" ]]; then
    git config core.hooksPath .githooks
    echo "git hooks: core.hooksPath set to .githooks"
  else
    echo "git hooks: NOT enabling .githooks (${existing})." \
         "To enable them, run: git config core.hooksPath .githooks" \
         "(and call your existing hooks from .githooks to keep them), or re-run with FINEPRINT_FORCE_HOOKS=1." >&2
  fi
fi

# shellcheck source=../.githooks/lib-agent-identity.sh
source .githooks/lib-agent-identity.sh

coauthor="${FINEPRINT_COAUTHOR:-}"
[[ -n "${coauthor}" ]] || coauthor="$(git config --get fineprint.coauthor || true)"

if [[ -z "${coauthor}" ]]; then
  name="$(git config --get user.name || true)"
  email="$(git config --get user.email || true)"
  if [[ -n "${name}" && -n "${email}" ]] && ! is_agent_identity "${name}" "${email}"; then
    coauthor="${name} <${email}>"
  fi
fi

if [[ -z "${coauthor}" ]] && command -v gh >/dev/null 2>&1; then
  gh_user="$(gh api user --jq '"\(.id) \(.login) \(.name // "")"' 2>/dev/null || true)"
  if [[ -n "${gh_user}" ]]; then
    read -r gh_id gh_login gh_name <<<"${gh_user}"
    coauthor="${gh_name:-${gh_login}} <${gh_id}+${gh_login}@users.noreply.github.com>"
  fi
fi

if [[ -n "${coauthor}" ]]; then
  if [[ "$(git config --local --get fineprint.coauthor || true)" != "${coauthor}" ]]; then
    git config --local fineprint.coauthor "${coauthor}"
    echo "git hooks: agent commits in this clone will be co-authored by ${coauthor}"
  fi
else
  echo "git hooks: could not determine a human co-author for agent commits;" \
       "set one with: git config fineprint.coauthor 'Name <email>'" >&2
fi
