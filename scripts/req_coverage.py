#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Check that every requirement ID in the SRS is traced by the plan (and optionally by tests).

The plan is the set of GitHub issues labelled `work-unit` (docs/PROCESS.md P1); this script reads
them with the `gh` CLI.

Usage (from the repository root):
  python3 scripts/req_coverage.py                      # SRS vs work-unit issues
  python3 scripts/req_coverage.py --milestone "M1 — Two-player engine"
  python3 scripts/req_coverage.py --allow REQ-FMT-004 REQ-FMT-005
  python3 scripts/req_coverage.py --tests crates tests  # also require each ID in some test file
  python3 scripts/req_coverage.py --plan-file units.md  # check a local draft instead of issues

Exit status is 1 when any ID is missing, so CI can use it as a gate.
Standard library plus the `gh` CLI.
"""
import argparse
import json
import pathlib
import subprocess
import re
import sys

REQ = re.compile(r"\bREQ-[A-Z]+-\d{3}\b")
TEST_SUFFIXES = {".rs", ".cs", ".c", ".h", ".cpp", ".ts", ".js", ".py", ".pzl", ".json", ".toml", ".yaml", ".yml"}


def ids_in(path: pathlib.Path) -> set:
    return set(REQ.findall(path.read_text(encoding="utf-8", errors="replace")))


def withdrawn_in(path: pathlib.Path) -> set:
    """IDs whose defining line is marked WITHDRAWN; they need no tracing."""
    out = set()
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        ids = REQ.findall(line)
        if "WITHDRAWN" in line and ids:
            out.add(ids[0])  # the ID the line defines; later IDs are cross-references
    return out


def ids_in_issues(repo: str, milestone) -> set:
    """REQ IDs in the bodies of all work-unit issues (open and closed)."""
    found = set()
    page = 1
    while True:
        # Explicit pages: `gh api --paginate` follows numeric-ID links that some proxies block.
        out = subprocess.run(
            ["gh", "api",
             f"repos/{repo}/issues?state=all&labels=work-unit&per_page=100&page={page}"],
            check=True, capture_output=True, text=True).stdout
        issues = json.loads(out)
        for issue in issues:
            if "pull_request" in issue:
                continue
            if milestone and (issue.get("milestone") or {}).get("title") != milestone:
                continue
            found |= set(REQ.findall(issue.get("body") or ""))
        if len(issues) < 100:
            break
        page += 1
    return found


def ids_in_tree(roots) -> set:
    found = set()
    for root in roots:
        root = pathlib.Path(root)
        if root.is_file():
            found |= ids_in(root)
            continue
        for p in sorted(root.rglob("*")):
            if p.is_file() and p.suffix in TEST_SUFFIXES:
                found |= ids_in(p)
    return found


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--srs", default="docs/SRS.md")
    ap.add_argument("--repo", default="donfuldauer/fineprint")
    ap.add_argument("--milestone", default=None, help="only issues in this milestone")
    ap.add_argument("--plan-file", default=None, help="check a local draft file instead of issues")
    ap.add_argument("--allow", nargs="*", default=[], help="IDs deliberately deferred by a recorded decision")
    ap.add_argument("--tests", nargs="*", default=None, help="test roots that must mention every ID")
    args = ap.parse_args()

    srs = pathlib.Path(args.srs)
    if not srs.exists():
        print(f"error: {srs} not found", file=sys.stderr)
        return 2
    withdrawn = withdrawn_in(srs)
    required = ids_in(srs) - set(args.allow) - withdrawn
    print(f"{len(required)} requirement IDs in {srs} "
          f"({len(args.allow)} allowed as deferred, {len(withdrawn)} withdrawn)")

    failed = False
    if args.plan_file:
        planned, source = ids_in(pathlib.Path(args.plan_file)), args.plan_file
    else:
        planned, source = ids_in_issues(args.repo, args.milestone), "work-unit issues"
    if args.milestone:
        # A milestone covers a subset; only report what it does trace.
        print(f"{source} in {args.milestone!r}: {len(planned & required)} IDs traced")
    else:
        missing = sorted(required - planned)
        unknown = sorted(planned - ids_in(srs))
        print(f"{source}: {len(required) - len(missing)}/{len(required)} traced")
        for rid in missing:
            print(f"  MISSING in plan: {rid}")
        for rid in unknown:
            print(f"  UNKNOWN in plan (not in SRS): {rid}")
        failed |= bool(missing or unknown)

    if args.tests is not None:
        tested = ids_in_tree(args.tests or ["tests"])
        missing = sorted(required - tested)
        print(f"tests: {len(required) - len(missing)}/{len(required)} referenced")
        for rid in missing:
            print(f"  MISSING in tests: {rid}")
        failed |= bool(missing)

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
