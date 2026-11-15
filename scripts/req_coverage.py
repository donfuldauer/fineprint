#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Check that every requirement ID in the SRS is traced by the plan (and optionally by tests).

Usage (from the repository root):
  python3 scripts/req_coverage.py                    # SRS vs docs/PLAN.md
  python3 scripts/req_coverage.py --allow REQ-FMT-004 REQ-FMT-005
  python3 scripts/req_coverage.py --tests tests cards  # also require each ID in some test file

Exit status is 1 when any ID is missing, so CI can use it as a gate.
Standard library only.
"""
import argparse
import pathlib
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
    ap.add_argument("--plan", default="docs/PLAN.md")
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
    plan = pathlib.Path(args.plan)
    if plan.exists():
        planned = ids_in(plan)
        missing = sorted(required - planned)
        unknown = sorted(planned - ids_in(srs))
        print(f"plan: {len(required) - len(missing)}/{len(required)} traced")
        for rid in missing:
            print(f"  MISSING in plan: {rid}")
        for rid in unknown:
            print(f"  UNKNOWN in plan (not in SRS): {rid}")
        failed |= bool(missing or unknown)
    else:
        print(f"plan: {plan} not found (run the plan-from-srs skill)")
        failed = True

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
