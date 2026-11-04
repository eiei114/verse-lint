"""Regression tests for the declared parity pin reader and validator."""

import importlib.util
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]

# The developer harness keeps its hyphenated CLI name, so load it by path.
_spec = importlib.util.spec_from_file_location(
    "verify_pair", ROOT / "scripts" / "verify-pair.py"
)
verify_pair = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(verify_pair)

PIN = {
    "corpusSha256": "27e0497bbccaba2bed56dfc93efcda762887f3888c9cd0c5707f0ae670050b47",
    "goldenCaseCount": 64,
    "formatterGrammarBaseRevision": "6b5433e37b52c03c4c07f9468bf7cc11e45f2f82",
    "lastVerifiedFormatterRevision": "ae191de08058a217d331640f0b098d6fb76435f3",
    "lastVerifiedLinterRevision": "8ecb25acfebdf22d08accfcf56ecf145a72bf9cd",
}


class ParityPinReaderTests(unittest.TestCase):
    def setUp(self):
        self.directory = pathlib.Path(tempfile.mkdtemp(prefix="verse-pin-"))
        self.path = self.directory / "parity.json"

    def tearDown(self):
        shutil.rmtree(self.directory, ignore_errors=True)

    def write(self, value):
        self.path.write_text(value if isinstance(value, str) else json.dumps(value), encoding="utf-8")

    def assert_refused(self, expected):
        with self.assertRaises(SystemExit) as raised:
            verify_pair.read_pin(self.path)
        self.assertIn(expected, str(raised.exception.code))

    def test_repository_pin_is_valid_and_declares_the_expected_values(self):
        declared = verify_pair.read_pin(ROOT / "parity.json")

        self.assertEqual(declared, PIN)

    def test_valid_declaration_is_returned(self):
        self.write(PIN)

        self.assertEqual(verify_pair.read_pin(self.path), PIN)

    def test_missing_file_is_refused(self):
        self.assert_refused("missing parity pin")

    def test_empty_file_is_refused(self):
        self.write("")

        self.assert_refused("malformed parity pin")

    def test_unparseable_file_is_refused(self):
        self.write("{")

        self.assert_refused("malformed parity pin")

    def test_non_object_top_level_is_refused(self):
        self.write("[1, 2]")

        self.assert_refused("top level must be an object")

    def test_partial_population_names_every_missing_key(self):
        self.write({"corpusSha256": PIN["corpusSha256"], "goldenCaseCount": 64})

        with self.assertRaises(SystemExit) as raised:
            verify_pair.read_pin(self.path)
        message = str(raised.exception.code)
        for key in (
            "incomplete parity pin",
            "formatterGrammarBaseRevision",
            "lastVerifiedFormatterRevision",
            "lastVerifiedLinterRevision",
        ):
            self.assertIn(key, message)

    def test_uppercase_or_short_corpus_hash_is_refused(self):
        self.write({**PIN, "corpusSha256": PIN["corpusSha256"].upper()})
        self.assert_refused("corpusSha256")

        self.write({**PIN, "corpusSha256": PIN["corpusSha256"][:32]})
        self.assert_refused("corpusSha256")

    def test_string_or_boolean_golden_count_is_refused(self):
        self.write({**PIN, "goldenCaseCount": "64"})
        self.assert_refused("goldenCaseCount")

        self.write({**PIN, "goldenCaseCount": True})
        self.assert_refused("goldenCaseCount")

    def test_zero_golden_count_is_refused(self):
        self.write({**PIN, "goldenCaseCount": 0})

        self.assert_refused("goldenCaseCount")

    def test_non_revision_string_is_refused(self):
        self.write({**PIN, "lastVerifiedLinterRevision": "HEAD"})

        self.assert_refused("lastVerifiedLinterRevision")


@unittest.skipIf(shutil.which("pwsh") is None, "PowerShell 7 is required")
class DeclaredParityScriptTests(unittest.TestCase):
    def setUp(self):
        self.directory = pathlib.Path(tempfile.mkdtemp(prefix="verse-declared-"))
        self.path = self.directory / "parity.json"

    def tearDown(self):
        shutil.rmtree(self.directory, ignore_errors=True)

    def run_script(self):
        return subprocess.run(
            [
                shutil.which("pwsh"),
                "-NoProfile",
                "-File",
                str(ROOT / "scripts" / "verify-parity.ps1"),
                "-PinPath",
                str(self.path),
            ],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=120,
        )

    def test_repository_declaration_passes_and_claims_no_convergence(self):
        self.path.write_text((ROOT / "parity.json").read_text(encoding="utf-8"), encoding="utf-8")
        result = self.run_script()

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("Declared parity pin verified", result.stdout)
        self.assertIn("Convergence is not claimed", result.stdout)

    def test_malformed_declaration_fails(self):
        self.path.write_text("{", encoding="utf-8")
        result = self.run_script()

        self.assertEqual(result.returncode, 1)
        self.assertIn("is not valid JSON", result.stdout + result.stderr)

    def test_unknown_key_fails(self):
        self.path.write_text(json.dumps({**PIN, "extra": 1}), encoding="utf-8")
        result = self.run_script()

        self.assertEqual(result.returncode, 1)
        self.assertIn("unexpected key", result.stdout + result.stderr)

    def test_unreachable_declared_revision_fails(self):
        self.path.write_text(
            json.dumps({**PIN, "lastVerifiedLinterRevision": "0" * 40}), encoding="utf-8"
        )
        result = self.run_script()

        self.assertEqual(result.returncode, 1)
        self.assertIn("not present in this repository", result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
