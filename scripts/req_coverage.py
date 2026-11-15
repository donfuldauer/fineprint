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
  python3 scripts/req_coverage.py --tests crates tests  # global check: also require each ID in some test file
  python3 scripts/req_coverage.py --milestone "M1 — Two-player engine" --tests crates tests  # whole-milestone check
  python3 scripts/req_coverage.py --plan-file units.md  # check a local draft instead of issues

What `--tests` counts (only test code, never production source):
  a. a file with a path component named tests, test, spec, e2e or __tests__, or whose name
     matches *_test.*, *.test.*, *.spec.*, test_*.py or *Tests.cs, counts in full;
  b. any .pzl file counts in full wherever it is;
  c. any other .rs file counts only its inline test modules: from each `#[cfg(test)]` line to
     the end of the annotated item (brace depth from the first `{` back to zero; an attribute
     with no following `{` is ignored);
  d. any other file does not count.
Files are visited in sorted order; missing roots are skipped silently.

Exit status is 1 when any ID is missing, so CI can use it as a gate.
Standard library plus the `gh` CLI.
"""
import argparse
import fnmatch
import json
import pathlib
import subprocess
import re
import sys

REQ = re.compile(r"\bREQ-[A-Z]+-\d{3}\b")
TEST_SUFFIXES = {".rs", ".cs", ".c", ".h", ".cpp", ".ts", ".js", ".py", ".pzl", ".json", ".toml", ".yaml", ".yml"}
TEST_DIRS = {"tests", "test", "spec", "e2e", "__tests__"}
TEST_NAMES = ("*_test.*", "*.test.*", "*.spec.*", "test_*.py", "*Tests.cs")
CFG_TEST = re.compile(r"^\s*#\[cfg\(test\)\]")


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


def is_test_path(p: pathlib.Path) -> bool:
    """Rule (a): a path component or file name that marks the file as test code."""
    if any(part in TEST_DIRS for part in p.parts[:-1]):
        return True
    return any(fnmatch.fnmatchcase(p.name, pat) for pat in TEST_NAMES)


def cfg_test_text(text: str) -> str:
    """Rule (c): the text of inline `#[cfg(test)]` items, attribute line to closing brace.

    The annotated item ends at the first `;` before any `{` (a brace-less item contributes
    nothing), else where brace depth returns to zero.
    """
    lines = text.splitlines()
    out = []
    i = 0
    while i < len(lines):
        m = CFG_TEST.match(lines[i])
        if not m:
            i += 1
            continue
        start, depth, opened, ended, j = i, 0, False, False, i
        col = m.end()  # scan after the attribute itself; later lines scan from column 0
        while j < len(lines) and not ended:
            for ch in lines[j][col:]:
                if ch == "{":
                    depth += 1
                    opened = True
                elif ch == "}":
                    depth -= 1
                    if opened and depth <= 0:
                        ended = True
                        break
                elif ch == ";" and not opened:
                    ended = True  # brace-less item: ignore the attribute
                    break
            if ended:
                break
            col = 0
            j += 1
        if ended and opened:
            out.extend(lines[start:j + 1])
        i = start + 1 if not (ended and opened) else j + 1
    return "\n".join(out)


def ids_in_test_code(p: pathlib.Path) -> set:
    """REQ IDs in `p` that count as test coverage under the module-docstring rules."""
    if p.suffix == ".pzl" or (p.suffix in TEST_SUFFIXES and is_test_path(p)):
        return ids_in(p)
    if p.suffix == ".rs":
        text = p.read_text(encoding="utf-8", errors="replace")
        return set(REQ.findall(cfg_test_text(text)))
    return set()


def ids_in_tree(roots) -> set:
    found = set()
    for root in roots:
        root = pathlib.Path(root)
        if root.is_file():
            found |= ids_in_test_code(root)
            continue
        for p in sorted(root.rglob("*")):
            if p.is_file():
                found |= ids_in_test_code(p)
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
    # Unknown IDs are an error in every mode, including a milestone-scoped run.
    unknown = sorted(planned - ids_in(srs))
    if args.milestone:
        # A milestone covers a subset; only report what it does trace.
        print(f"{source} in {args.milestone!r}: {len(planned & required)} IDs traced")
    else:
        missing = sorted(required - planned)
        print(f"{source}: {len(required) - len(missing)}/{len(required)} traced")
        for rid in missing:
            print(f"  MISSING in plan: {rid}")
        failed |= bool(missing)
    for rid in unknown:
        print(f"  UNKNOWN in plan (not in SRS): {rid}")
    failed |= bool(unknown)

    if args.tests is not None:
        tested = ids_in_tree(args.tests or ["tests"])
        # With --milestone, compare only the IDs that milestone traces.
        scope = (planned & required) if args.milestone else required
        label = f"tests (milestone {args.milestone!r})" if args.milestone else "tests"
        missing = sorted(scope - tested)
        print(f"{label}: {len(scope) - len(missing)}/{len(scope)} referenced")
        for rid in missing:
            print(f"  MISSING in tests: {rid}")
        failed |= bool(missing)

    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
