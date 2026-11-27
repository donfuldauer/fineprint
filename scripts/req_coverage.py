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
     with no following `{` is ignored). The scan is lexically aware: braces and semicolons
     inside comments, strings, raw strings and char literals are ignored, and an attribute
     inside a comment or string is not an attribute;
  c2. a file declared by a brace-less `#[cfg(test)] mod name;` (other attributes such as
     `#[path = "x.rs"]` and a `pub` visibility may accompany it) counts in full. It is resolved
     as rustc does, through the declaration's enclosing inline modules (`mod a { mod b { ... } }`
     in the same file, tracked by brace depth on the comment- and string-blanked text): the
     module directory is `<dir>/a/b` in a module root (mod.rs, lib.rs, main.rs) and
     `<dir>/foo/a/b` in any other `foo.rs` (just `<dir>` and `<dir>/foo` at the top level of the
     file). `#[path = "rel"]` gives `<dir of declaring file>/rel` at the top level of the file
     and `<module directory>/rel` inside an inline module; otherwise the candidates are
     `<module directory>/name.rs` and `<module directory>/name/mod.rs`. So in `src/lib.rs`,
     `mod outer { #[cfg(test)] mod checks; }` names `src/outer/checks.rs`, not `src/checks.rs`.
     (A `#[path]` on the inline module itself is not modelled.) A file that does not exist is
     ignored. Plain `mod child;` declarations inside such a file count in full too (transitively,
     cycles terminate), even when the child lies outside the `--tests` roots;
  d. any other file does not count.
Files are visited in sorted order; missing roots are skipped silently.

Exit status is 1 when any ID is missing, so CI can use it as a gate.
Standard library plus the `gh` CLI.
"""
import argparse
import fnmatch
import json
import os
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


def code_mask(text: str) -> list:
    """Per character: True where `text` is Rust code, False inside comments, strings or chars.

    One forward scan. Skipped regions: `//` comments, nested `/* */` comments, `"..."` and
    `b"..."` strings (backslash escapes), raw strings `r#"..."#` (any number of `#`, with or
    without a `b` prefix), and char/byte-char literals. A `'` that does not open a char literal
    is a lifetime or label and stays code.
    """
    n = len(text)
    mask = [True] * n
    ident = lambda c: c.isalnum() or c == "_"
    i = 0
    while i < n:
        c = text[i]
        end = None  # exclusive end of a skipped region starting at i
        if c == "/" and text.startswith("//", i):
            end = text.find("\n", i)
            end = n if end < 0 else end
        elif c == "/" and text.startswith("/*", i):
            depth, k = 1, i + 2
            while k < n and depth:
                if text.startswith("/*", k):
                    depth, k = depth + 1, k + 2
                elif text.startswith("*/", k):
                    depth, k = depth - 1, k + 2
                else:
                    k += 1
            end = k
        elif c == '"':
            k = i + 1
            while k < n and text[k] != '"':
                k += 2 if text[k] == "\\" else 1
            end = min(k + 1, n)
        elif c in "rb" and (i == 0 or not ident(text[i - 1])):
            k = i + 1
            if c == "b" and k < n and text[k] == "r":
                k += 1
            if text[k - 1] == "r":  # `r` or `br`: raw string if `#`* then `"`
                h = k
                while h < n and text[h] == "#":
                    h += 1
                if h < n and text[h] == '"':
                    close = '"' + "#" * (h - k)
                    e = text.find(close, h + 1)
                    end = n if e < 0 else e + len(close)
        elif c == "'":
            if i + 1 < n and text[i + 1] == "\\":
                k = i + 3  # skip the escaped character, then find the closing quote
                while k < n and text[k] != "'" and text[k] != "\n":
                    k += 1
                end = min(k + 1, n)
            elif i + 2 < n and text[i + 2] == "'" and text[i + 1] != "'":
                end = i + 3
        if end is None:
            i += 1
        else:
            for k in range(i, end):
                mask[k] = False
            i = end
    return mask


def cfg_test_text(text: str) -> str:
    """Rule (c): the text of inline `#[cfg(test)]` items, attribute line to closing brace.

    The annotated item ends at the first `;` before any `{` (a brace-less item contributes
    nothing), else where brace depth returns to zero. Braces and semicolons inside comments,
    strings and char literals are ignored (see `code_mask`), and an attribute inside a comment
    or string is not an attribute. The returned text is the full source span, comments included.
    """
    lines = text.split("\n")
    offs, pos = [], 0
    for ln in lines:
        offs.append(pos)
        pos += len(ln) + 1
    mask = code_mask(text)
    out = []
    i = 0
    while i < len(lines):
        m = CFG_TEST.match(lines[i])
        if not m or not mask[offs[i] + m.end() - 1]:
            i += 1
            continue
        start, depth, opened, ended, j = i, 0, False, False, i
        col = m.end()  # scan after the attribute itself; later lines scan from column 0
        while j < len(lines) and not ended:
            for k in range(col, len(lines[j])):
                if not mask[offs[j] + k]:
                    continue
                ch = lines[j][k]
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


MOD_DECL = re.compile(r"(?<![\w])(?:pub\s*(?:\([^()]*\)\s*)?)?mod\s+([A-Za-z_]\w*)\s*;")
INLINE_MOD = re.compile(r"(?<![\w])(?:pub\s*(?:\([^()]*\)\s*)?)?mod\s+([A-Za-z_]\w*)\s*\{")
ATTR_CFG_TEST = re.compile(r"#\[\s*cfg\s*\(\s*test\s*\)\s*\]")
ATTR_PATH = re.compile(r"#\[\s*path\s*=\s*\]")
STRING_LIT = re.compile(r'"((?:\\.|[^"\\])*)"', re.S)
MODULE_ROOTS = {"mod.rs", "lib.rs", "main.rs"}


def _read(p: pathlib.Path) -> str:
    """File text, or "" when the file cannot be read (the scan never crashes on odd input)."""
    try:
        return p.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""


def _inline_paths(blank: str, positions) -> list:
    """Enclosing inline-module path (tuple of names, outermost first) at each of `positions`.

    `blank` is comment- and string-blanked source; `positions` must be ascending. Every `{`
    opens a scope, named when it is the body of `mod name {`, and every `}` closes the innermost
    one (an unbalanced `}` is ignored), so the path lists the named scopes open at a position.
    """
    named = {m.end() - 1: m.group(1) for m in INLINE_MOD.finditer(blank)}
    stack, out, i = [], [], 0
    for pos in positions:
        while i < pos:
            c = blank[i]
            if c == "{":
                stack.append(named.get(i))
            elif c == "}" and stack:
                stack.pop()
            i += 1
        out.append(tuple(n for n in stack if n is not None))
    return out


def mod_decls(text: str) -> list:
    """Brace-less `mod name;` declarations in source order, each as
    (name, is_cfg_test, path_attr_or_None, inline_path).

    `inline_path` is the tuple of the enclosing inline module names (outermost first; empty at
    the top level of the file), computed by brace depth on the blanked text (see
    `_inline_paths`). Lexically aware (see `code_mask`): declarations, attributes and braces
    inside comments or strings are ignored. Attributes are collected backwards from the `mod`, so
    a `#[cfg(test)]` and a `#[path = "..."]` may come in either order, with other attributes
    between them.
    """
    mask = code_mask(text)
    blank = "".join(c if m else " " for c, m in zip(text, mask))
    out = []
    matches = list(MOD_DECL.finditer(blank))
    inline = _inline_paths(blank, [m.start() for m in matches])
    for m, inline_path in zip(matches, inline):
        k, is_test, path = m.start(), False, None
        while True:
            while k > 0 and blank[k - 1].isspace():
                k -= 1
            if k == 0 or blank[k - 1] != "]":
                break
            depth, j = 0, k - 1
            while j >= 0:
                if blank[j] == "]":
                    depth += 1
                elif blank[j] == "[":
                    depth -= 1
                    if depth == 0:
                        break
                j -= 1
            if j < 1 or blank[j - 1] != "#":
                break
            attr_blank, attr_text = blank[j - 1:k], text[j - 1:k]
            if ATTR_CFG_TEST.fullmatch(attr_blank):
                is_test = True
            elif ATTR_PATH.fullmatch(attr_blank):
                lit = STRING_LIT.search(attr_text)
                path = lit.group(1) if lit else None
            k = j - 1
        out.append((m.group(1), is_test, path, inline_path))
    return out


def resolve_mod(declaring: pathlib.Path, name: str, path_attr, inline=()) -> list:
    """Candidate files for `mod name;` declared in `declaring` inside the inline modules
    `inline` (outermost first), in rustc's lookup order."""
    base = declaring.parent
    if path_attr is not None and not inline:
        return [pathlib.Path(os.path.normpath(base / path_attr))]
    if declaring.name not in MODULE_ROOTS:
        base = base / declaring.stem
    for part in inline:
        base = base / part
    if path_attr is not None:
        return [pathlib.Path(os.path.normpath(base / path_attr))]
    return [pathlib.Path(os.path.normpath(base / f"{name}.rs")),
            pathlib.Path(os.path.normpath(base / name / "mod.rs"))]


def declared_test_files(rs_files) -> list:
    """Rule (c2): existing files reachable from `#[cfg(test)] mod name;` in `rs_files`.

    Plain `mod child;` inside an already-found file is followed transitively. Sorted result.
    """
    found, queue = set(), []

    def add(declaring, name, path_attr, inline):
        for cand in resolve_mod(declaring, name, path_attr, inline):
            if cand.is_file():
                if cand not in found:
                    found.add(cand)
                    queue.append(cand)
                return

    for f in rs_files:
        for name, is_test, path_attr, inline in mod_decls(_read(f)):
            if is_test:
                add(f, name, path_attr, inline)
    while queue:
        f = queue.pop()
        if f.suffix == ".rs":
            for name, _, path_attr, inline in mod_decls(_read(f)):
                add(f, name, path_attr, inline)
    return sorted(found)


def ids_in_tree(roots) -> set:
    """REQ IDs that count as test coverage under `roots` (rules a-d and c2)."""
    files = []
    for root in roots:
        root = pathlib.Path(root)
        if root.is_file():
            files.append(root)
        else:
            files.extend(p for p in sorted(root.rglob("*")) if p.is_file())
    found = set()
    for p in files:
        found |= ids_in_test_code(p)
    for p in declared_test_files([f for f in files if f.suffix == ".rs"]):
        found |= set(REQ.findall(_read(p)))
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
