# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Regression tests for scripts/req_coverage.py `--tests` scanning (REQ-NFR-008).

Run: python3 -m unittest discover -s scripts/tests -v
"""
import importlib.util
import pathlib
import subprocess
import sys
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parent.parent / "req_coverage.py"
_spec = importlib.util.spec_from_file_location("req_coverage", SCRIPT)
rc = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(rc)


def write(root: pathlib.Path, rel: str, text: str) -> None:
    """Create `root/rel` with `text`."""
    p = root / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text, encoding="utf-8")


class TreeCase(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = pathlib.Path(self._tmp.name)

    def ids(self, sub="src"):
        return rc.ids_in_tree([self.root / sub])


class TestLayouts(TreeCase):
    def test_inline_block_counts_production_does_not(self):
        """REQ-NFR-008: inline test module counts; same-file/other-file production IDs do not."""
        write(self.root, "src/a.rs", "// REQ-ZZZ-009\nfn f() {}\n#[cfg(test)]\nmod tests {\n // REQ-ZZZ-001\n}\n")
        write(self.root, "src/p.rs", "// REQ-ZZZ-001 REQ-ZZZ-010\nfn g() {}\n")
        self.assertEqual(self.ids(), {"REQ-ZZZ-001"})

    def test_sibling_file_mod_rs(self):
        """REQ-NFR-008: mod.rs with `#[cfg(test)] mod tests;` counts src/b/tests.rs."""
        write(self.root, "src/b/mod.rs", "fn f() {}\n#[cfg(test)]\nmod tests;\n")
        write(self.root, "src/b/tests.rs", "// REQ-ZZZ-002\n")
        self.assertEqual(self.ids(), {"REQ-ZZZ-002"})

    def test_sibling_file_plain_rs(self):
        """REQ-NFR-008: c.rs declares tests found at c/tests.rs (and c/tests/mod.rs)."""
        write(self.root, "src/c.rs", "#[cfg(test)]\nmod tests;\n#[cfg(test)]\npub(crate) mod more;\n")
        write(self.root, "src/c/tests.rs", "// REQ-ZZZ-003\n")
        write(self.root, "src/c/more/mod.rs", "// REQ-ZZZ-004\n")
        self.assertEqual(self.ids(), {"REQ-ZZZ-003", "REQ-ZZZ-004"})

    def test_path_attribute_both_orders_and_one_line(self):
        """REQ-NFR-008: #[path] resolves relative to the declaring file's directory."""
        write(self.root, "src/d.rs",
              '#[cfg(test)]\n#[path = "custom_tests.rs"]\nmod t1;\n'
              '#[path = "other_tests.rs"]\n#[allow(dead_code)]\n#[cfg(test)]\nmod t2;\n'
              '#[cfg(test)] #[path = "one.rs"] pub mod t3;\n')
        write(self.root, "src/custom_tests.rs", "// REQ-ZZZ-005\n")
        write(self.root, "src/other_tests.rs", "// REQ-ZZZ-006\n")
        write(self.root, "src/one.rs", "// REQ-ZZZ-007\n")
        self.assertEqual(self.ids(), {"REQ-ZZZ-005", "REQ-ZZZ-006", "REQ-ZZZ-007"})

    def test_transitive_child(self):
        """REQ-NFR-008: `mod nested;` inside a counted file counts, even outside the roots."""
        write(self.root, "src/b/mod.rs", "#[cfg(test)]\nmod tests;\n")
        write(self.root, "src/b/tests.rs", "mod nested;\n")
        write(self.root, "src/b/tests/nested.rs", "// REQ-ZZZ-008\n")
        self.assertEqual(self.ids(), {"REQ-ZZZ-008"})
        (self.root / "src/b/tests/nested.rs").unlink()
        write(self.root, "outside/x.rs", "// REQ-ZZZ-011\n")
        write(self.root, "src/b/tests.rs", '#[path = "../../outside/x.rs"]\nmod x;\n')
        self.assertEqual(self.ids(), {"REQ-ZZZ-011"})

    def test_cycle_terminates(self):
        """REQ-NFR-008: declaration cycles terminate."""
        write(self.root, "src/mod.rs", '#[cfg(test)]\n#[path = "t.rs"]\nmod t;\n')
        write(self.root, "src/t.rs", '// REQ-ZZZ-001\n#[path = "t.rs"]\nmod again;\n#[path = "mod.rs"]\nmod up;\n')
        self.assertEqual(self.ids(), {"REQ-ZZZ-001"})

    def test_order_independent(self):
        """REQ-NFR-008: result does not depend on which file sorts first."""
        write(self.root, "src/zz/mod.rs", "#[cfg(test)]\nmod tests;\n")
        write(self.root, "src/zz/tests.rs", "// REQ-ZZZ-002\n")
        write(self.root, "src/aa.rs", "fn f() {}\n")
        self.assertEqual(self.ids(), {"REQ-ZZZ-002"})
        self.assertEqual(rc.ids_in_tree([self.root / "src" / "zz" / "mod.rs", self.root / "src" / "aa.rs"]),
                         {"REQ-ZZZ-002"})


class TestNegatives(TreeCase):
    def test_plain_mod_not_counted(self):
        """REQ-NFR-008: `mod helper;` without cfg(test) in a non-test file does not count."""
        write(self.root, "src/e.rs", "mod helper;\n")
        write(self.root, "src/e/helper.rs", "// REQ-ZZZ-001\n")
        self.assertEqual(self.ids(), set())

    def test_comment_and_string_do_not_declare(self):
        """REQ-NFR-008: attribute/mod text inside comments and strings declares nothing."""
        write(self.root, "src/f/mod.rs",
              '// #[cfg(test)]\n// mod tests;\n/* #[cfg(test)] mod tests; */\n'
              'const S: &str = "#[cfg(test)] mod tests;";\n#[cfg(test)]\n// mod tests;\n')
        write(self.root, "src/f/tests.rs", "// REQ-ZZZ-001\n")
        self.assertEqual(self.ids(), set())

    def test_missing_file_no_crash(self):
        """REQ-NFR-008: a declaration whose file does not exist is ignored."""
        write(self.root, "src/g.rs", '#[cfg(test)]\nmod nope;\n#[cfg(test)]\n#[path = "gone.rs"]\nmod gone;\n')
        self.assertEqual(self.ids(), set())

    def test_directory_target_no_crash(self):
        """REQ-NFR-008: a path attribute naming a directory is ignored without a crash."""
        write(self.root, "src/h.rs", '#[cfg(test)]\n#[path = "sub"]\nmod sub;\n')
        (self.root / "src" / "sub").mkdir()
        self.assertEqual(self.ids(), set())

    def test_orphan_not_counted(self):
        """REQ-NFR-008: an undeclared stray .rs file does not count."""
        write(self.root, "src/orphan.rs", "// REQ-ZZZ-001\n")
        self.assertEqual(self.ids(), set())

    def test_cfg_test_use_ignored(self):
        """REQ-NFR-008: `#[cfg(test)] use x;` contributes nothing, nor does the item after it."""
        write(self.root, "src/i.rs", "#[cfg(test)]\nuse x::y; // REQ-ZZZ-001\nfn prod() {} // REQ-ZZZ-002\n")
        self.assertEqual(self.ids(), set())
        self.assertEqual(rc.cfg_test_text("#[cfg(test)]\nuse x;\n"), "")

    def test_mod_decls_parsing(self):
        """REQ-NFR-008: mod_decls reports name, cfg(test) flag and path attribute."""
        text = '#[cfg(test)] #[path = "a b.rs"] mod x;\npub(crate) mod y;\nmod z { }\n'
        self.assertEqual(rc.mod_decls(text), [("x", True, "a b.rs"), ("y", False, None)])

    def test_resolve_mod_candidates(self):
        """REQ-NFR-008: resolve_mod follows rustc's module-root vs file layout."""
        P = pathlib.Path
        self.assertEqual(rc.resolve_mod(P("s/mod.rs"), "t", None), [P("s/t.rs"), P("s/t/mod.rs")])
        self.assertEqual(rc.resolve_mod(P("s/foo.rs"), "t", None), [P("s/foo/t.rs"), P("s/foo/t/mod.rs")])
        self.assertEqual(rc.resolve_mod(P("s/foo.rs"), "t", "x/y.rs"), [P("s/x/y.rs")])

    def test_declared_test_files_sorted(self):
        """REQ-NFR-008: declared_test_files returns existing files in sorted order."""
        write(self.root, "src/mod.rs", "#[cfg(test)]\nmod b;\n#[cfg(test)]\nmod a;\n")
        write(self.root, "src/a.rs", "")
        write(self.root, "src/b.rs", "")
        got = rc.declared_test_files([self.root / "src" / "mod.rs"])
        self.assertEqual(got, sorted(got))
        self.assertEqual(len(got), 2)


class TestExistingRules(TreeCase):
    def test_rule_a_tests_dir(self):
        """REQ-NFR-008: a file under a tests/ directory counts in full."""
        write(self.root, "src/tests/x.rs", "// REQ-ZZZ-001\n")
        write(self.root, "src/y_test.rs", "// REQ-ZZZ-002\n")
        self.assertEqual(self.ids(), {"REQ-ZZZ-001", "REQ-ZZZ-002"})

    def test_rule_b_pzl(self):
        """REQ-NFR-008: a .pzl file counts in full wherever it is."""
        write(self.root, "src/scenario.pzl", "# REQ-ZZZ-003\n")
        self.assertEqual(self.ids(), {"REQ-ZZZ-003"})

    def test_rule_d_other_files(self):
        """REQ-NFR-008: a .md or non-test .py file does not count."""
        write(self.root, "src/notes.md", "REQ-ZZZ-001\n")
        write(self.root, "src/tool.py", "# REQ-ZZZ-002\n")
        self.assertEqual(self.ids(), set())

    def test_missing_root_skipped(self):
        """REQ-NFR-008: a missing root is skipped silently."""
        self.assertEqual(rc.ids_in_tree([self.root / "nowhere"]), set())


class TestCli(TreeCase):
    def run_cli(self, *extra):
        write(self.root, "srs.md", "REQ-ZZZ-001 REQ-ZZZ-002 REQ-ZZZ-003\n")
        write(self.root, "plan.md", "REQ-ZZZ-001 REQ-ZZZ-002 REQ-ZZZ-003\n")
        return subprocess.run(
            [sys.executable, str(SCRIPT), "--srs", str(self.root / "srs.md"),
             "--plan-file", str(self.root / "plan.md"), "--tests", str(self.root / "src"), *extra],
            capture_output=True, text=True)

    def test_cli_missing_and_exit_status(self):
        """REQ-NFR-008: CLI counts sibling-file tests and reports the rest as MISSING (exit 1)."""
        write(self.root, "src/b/mod.rs", "#[cfg(test)]\nmod tests;\n")
        write(self.root, "src/b/tests.rs", "// REQ-ZZZ-001 REQ-ZZZ-002\n")
        r = self.run_cli()
        self.assertEqual(r.returncode, 1, r.stdout + r.stderr)
        self.assertIn("tests: 2/3 referenced", r.stdout)
        self.assertIn("  MISSING in tests: REQ-ZZZ-003", r.stdout)
        self.assertNotIn("MISSING in tests: REQ-ZZZ-001", r.stdout)
        self.assertNotIn("MISSING in tests: REQ-ZZZ-002", r.stdout)

    def test_cli_all_covered(self):
        """REQ-NFR-008: CLI exits 0 when every ID is covered, and output is deterministic."""
        write(self.root, "src/c.rs", "#[cfg(test)]\nmod tests;\n")
        write(self.root, "src/c/tests.rs", "// REQ-ZZZ-001 REQ-ZZZ-002 REQ-ZZZ-003\n")
        a, b = self.run_cli(), self.run_cli()
        self.assertEqual(a.returncode, 0, a.stdout + a.stderr)
        self.assertIn("tests: 3/3 referenced", a.stdout)
        self.assertEqual(a.stdout, b.stdout)


if __name__ == "__main__":
    unittest.main()
