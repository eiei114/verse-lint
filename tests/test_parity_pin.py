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
    "lastVerifiedFormatterRevision": "27d3dc76a61e569a394a9115d5c0c37a63345626",
    "lastVerifiedLinterRevision": "a9b43c6c137be9f03618869d0c2ec6635dd42e29",
}

# The committed harness report that the repository pin must quote.
EVIDENCE = ROOT / "docs" / "evidence" / "parity-paired-2026-11-05-full.json"
EVIDENCE_SUMMARY = ROOT / "docs" / "evidence" / "parity-paired-2026-11-05.json"


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

    def test_repository_pin_is_structurally_valid(self):
        # Values are asserted by shape only: the pin is expected to move on a
        # real paired update, and that must not need a test edit.
        declared = verify_pair.read_pin(ROOT / "parity.json")

        self.assertEqual(set(declared), set(verify_pair.PIN_KEYS))
        self.assertRegex(declared["corpusSha256"], r"^[0-9a-f]{64}$")
        self.assertGreaterEqual(declared["goldenCaseCount"], 1)

    def test_repository_pin_quotes_the_committed_paired_report(self):
        # The pin must name revisions a committed run actually observed. A pin
        # value quoted only from a discussion comment is an unverified claim.
        declared = verify_pair.read_pin(ROOT / "parity.json")
        report = json.loads(EVIDENCE.read_text(encoding="utf-8"))
        summary = json.loads(EVIDENCE_SUMMARY.read_text(encoding="utf-8"))

        self.assertEqual(declared["corpusSha256"], report["corpus_sha256"])
        self.assertEqual(
            declared["lastVerifiedFormatterRevision"], report["formatter_revision"]
        )
        self.assertEqual(
            declared["lastVerifiedLinterRevision"], report["linter_revision"]
        )
        self.assertEqual(declared["goldenCaseCount"], report["cases_total"])
        self.assertEqual(report["cases_passed"], report["cases_total"])
        self.assertEqual(
            len(report["case_results"]),
            report["cases_total"],
            "every golden must carry a per-case result",
        )
        # The readable summary must agree with the raw harness output it cites.
        for source, mirror in (
            ("corpus_sha256", "corpus_sha256"),
            ("cases_total", "cases_total"),
            ("cases_passed", "cases_passed"),
            ("formatter_revision", "formatter_revision"),
            ("linter_revision", "linter_revision"),
            ("formatter_sha256", "formatter_sha256"),
            ("linter_sha256", "linter_sha256"),
        ):
            self.assertEqual(summary[source], report[mirror], source)
        self.assertEqual(summary["golden_cases"], report["cases_total"])

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


def _golden(case_id):
    return {"id": case_id, "input": "source", "expected": "formatted"}


def _refusal(case_id):
    return {"id": case_id, "input": "rejected", "reject": True}


def _run_report(goldens, *, passed=None):
    return {
        "corpus_sha256": "b" * 64,
        "formatter_revision": "c" * 40,
        "linter_revision": "d" * 40,
        "cases_total": len(goldens),
        "cases_passed": len(goldens) if passed is None else passed,
        "case_results": [{"id": case["id"]} for case in goldens],
    }


class PinUpdateTests(unittest.TestCase):
    """Exercise the pin updater without invoking either formatter binary."""

    def setUp(self):
        self.directory = pathlib.Path(tempfile.mkdtemp(prefix="verse-pin-update-"))
        self.path = self.directory / "parity.json"
        self.original = json.dumps(PIN, indent=2) + "\n"
        self.path.write_text(self.original, encoding="utf-8")
        self.goldens = [_golden("first"), _golden("second")]
        self.corpus = {"cases": self.goldens + [_refusal("reject") ]}

    def tearDown(self):
        shutil.rmtree(self.directory, ignore_errors=True)

    def assert_refused_without_write(self, report, corpus=None, expected=None):
        before = self.path.read_bytes()
        with self.assertRaises(SystemExit) as raised:
            verify_pair.build_pin_update(
                PIN, report, self.corpus if corpus is None else corpus
            )
        message = str(raised.exception.code)
        if expected:
            self.assertIn(expected, message)
        self.assertEqual(self.path.read_bytes(), before)
        return message

    def test_successful_run_rewrites_observed_values(self):
        updated = verify_pair.build_pin_update(PIN, _run_report(self.goldens), self.corpus)
        verify_pair.apply_pin_update(self.path, updated)

        self.assertEqual(verify_pair.read_pin(self.path), updated)
        self.assertEqual(updated["corpusSha256"], "b" * 64)
        self.assertEqual(updated["goldenCaseCount"], 2)
        self.assertEqual(updated["formatterGrammarBaseRevision"], PIN["formatterGrammarBaseRevision"])
        self.assertEqual(updated["lastVerifiedFormatterRevision"], "c" * 40)
        self.assertEqual(updated["lastVerifiedLinterRevision"], "d" * 40)

    def test_dry_run_reports_candidate_without_writing(self):
        updated = verify_pair.build_pin_update(PIN, _run_report(self.goldens), self.corpus)
        summary = verify_pair.describe_pin_update(PIN, updated)

        verify_pair.apply_pin_update(self.path, updated, dry_run=True)

        self.assertIn("corpusSha256", summary)
        self.assertIn("goldenCaseCount", summary)
        self.assertEqual(self.path.read_text(encoding="utf-8"), self.original)

    def test_failed_case_refuses_update(self):
        report = _run_report(self.goldens)
        report["cases_failed"] = 1

        self.assert_refused_without_write(report, expected="a paired case failed")

    def test_incomplete_run_refuses_update(self):
        report = _run_report(self.goldens)
        report["case_results"] = report["case_results"][:1]

        self.assert_refused_without_write(report, expected="incomplete")

    def test_observed_golden_count_must_match_corpus(self):
        report = _run_report(self.goldens)
        report["cases_total"] = 3
        report["cases_passed"] = 3

        self.assert_refused_without_write(report, expected="corpus count 2")

    def test_unclassified_corpus_case_refuses_update(self):
        corpus = {"cases": self.goldens + [{"id": "unknown", "input": "x"}]}

        self.assert_refused_without_write(
            _run_report(self.goldens), corpus=corpus, expected="neither goldens nor explicit refusals"
        )

    def test_dirty_formatter_or_linter_checkout_refuses_update(self):
        for role in ("formatter checkout", "linter checkout"):
            with self.subTest(role=role):
                checkout = self.directory / role.split()[0]
                checkout.mkdir()
                subprocess.run(["git", "init", "--quiet", str(checkout)], check=True)
                subprocess.run(["git", "-C", str(checkout), "config", "user.name", "Pin test"], check=True)
                subprocess.run(["git", "-C", str(checkout), "config", "user.email", "pin-test@example.invalid"], check=True)
                tracked = checkout / "tracked.txt"
                tracked.write_text("clean\n", encoding="utf-8")
                subprocess.run(["git", "-C", str(checkout), "add", "tracked.txt"], check=True)
                subprocess.run(["git", "-C", str(checkout), "commit", "--quiet", "-m", "initial"], check=True)
                tracked.write_text("dirty\n", encoding="utf-8")

                with self.assertRaises(SystemExit) as raised:
                    verify_pair.require_clean_checkout(checkout, role)
                self.assertIn(role, str(raised.exception.code))

    def test_hash_mismatch_still_fails_without_update_mode(self):
        with self.assertRaises(SystemExit) as raised:
            verify_pair.check_corpus_hash("a" * 64, "b" * 64, update_pin=False)
        self.assertIn("formatter corpus revision/hash mismatch", str(raised.exception.code))

    def test_hash_and_count_mismatch_are_allowed_only_for_update_mode(self):
        verify_pair.check_corpus_hash("a" * 64, "b" * 64, update_pin=True)
        verify_pair.check_golden_count(3, 2, update_pin=True)

    def test_dry_run_requires_update_pin(self):
        result = subprocess.run(
            [
                sys.executable,
                str(ROOT / "scripts" / "verify-pair.py"),
                "--formatter", "formatter.exe",
                "--corpus", "corpus.json",
                "--output", "report.json",
                "--dry-run",
            ],
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=15,
        )

        self.assertEqual(result.returncode, 2)
        self.assertIn("--dry-run requires --update-pin", result.stderr)


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

    def test_uppercase_key_name_fails(self):
        # Reproduced defect: PowerShell's -contains/-notcontains ignore case, so a
        # renamed 'CORPUSSHA256' satisfied 'corpusSha256' and the script exited 0.
        # The key is renamed, not added: duplicating it case-insensitively would
        # make ConvertFrom-Json fail before the key check is reached.
        renamed = json.dumps(PIN).replace('"corpusSha256"', '"CORPUSSHA256"')
        self.path.write_text(renamed, encoding="utf-8")
        result = self.run_script()

        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("corpusSha256", result.stdout + result.stderr)

    def test_uppercase_hex_value_fails(self):
        # Same case-insensitivity class: -match accepted an uppercase digest and
        # revision although both are documented as lowercase hex.
        self.path.write_text(
            json.dumps({**PIN, "corpusSha256": PIN["corpusSha256"].upper()}),
            encoding="utf-8",
        )
        result = self.run_script()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("corpusSha256", result.stdout + result.stderr)

        self.path.write_text(
            json.dumps(
                {**PIN, "lastVerifiedLinterRevision": PIN["lastVerifiedLinterRevision"].upper()}
            ),
            encoding="utf-8",
        )
        result = self.run_script()
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("lastVerifiedLinterRevision", result.stdout + result.stderr)

    def test_unreachable_declared_revision_fails(self):
        self.path.write_text(
            json.dumps({**PIN, "lastVerifiedLinterRevision": "0" * 40}), encoding="utf-8"
        )
        result = self.run_script()

        self.assertEqual(result.returncode, 1)
        self.assertIn("not present in this repository", result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
