"""Developer-only paired convergence check; no runtime dependency on verse-fmt."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

LINTER_ROOT = Path(__file__).resolve().parents[1]
PIN_PATH = LINTER_ROOT / "parity.json"
PIN_KEYS = (
    "corpusSha256",
    "goldenCaseCount",
    "formatterGrammarBaseRevision",
    "lastVerifiedFormatterRevision",
    "lastVerifiedLinterRevision",
)
SHA256_PATTERN = re.compile(r"^[0-9a-f]{64}$")
REVISION_PATTERN = re.compile(r"^[0-9a-f]{40}$")

def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()

def validate_pin(declared, source) -> dict:
    """Validate a declaration or a candidate update without writing it."""
    if not isinstance(declared, dict):
        raise SystemExit(f"malformed parity pin {source}: top level must be an object")
    missing = [key for key in PIN_KEYS if key not in declared]
    if missing:
        raise SystemExit(f"incomplete parity pin {source}: missing {', '.join(missing)}")
    corpus_sha = declared["corpusSha256"]
    if not isinstance(corpus_sha, str) or not SHA256_PATTERN.match(corpus_sha):
        raise SystemExit(
            f"malformed parity pin {source}: corpusSha256 must be 64 lowercase hex digits"
        )
    golden_count = declared["goldenCaseCount"]
    if not isinstance(golden_count, int) or isinstance(golden_count, bool) or golden_count < 1:
        raise SystemExit(
            f"malformed parity pin {source}: goldenCaseCount must be a positive integer"
        )
    for key in (
        "formatterGrammarBaseRevision",
        "lastVerifiedFormatterRevision",
        "lastVerifiedLinterRevision",
    ):
        value = declared[key]
        if not isinstance(value, str) or not REVISION_PATTERN.match(value):
            raise SystemExit(
                f"malformed parity pin {source}: {key} must be a 40-character lowercase revision"
            )
    return declared

def read_pin(path: Path = PIN_PATH) -> dict:
    """Read the declared parity pin; never fall back to built-in defaults."""
    try:
        text = path.read_text(encoding="utf-8")
    except FileNotFoundError:
        raise SystemExit(f"missing parity pin: {path}")
    except OSError as error:
        raise SystemExit(f"unreadable parity pin {path}: {error}")
    try:
        declared = json.loads(text)
    except json.JSONDecodeError as error:
        raise SystemExit(f"malformed parity pin {path}: {error}")
    return validate_pin(declared, path)

def build_pin_update(declared: dict, report: dict, corpus: dict) -> dict:
    """Return a pin justified by a complete successful run, or refuse it."""
    cases = corpus.get("cases") if isinstance(corpus, dict) else None
    if not isinstance(cases, list):
        raise SystemExit("refusing to update parity pin: corpus has no case list")
    goldens = []
    unclassified = []
    for case in cases:
        if not isinstance(case, dict):
            unclassified.append("<non-object case>")
        elif "expected" in case:
            goldens.append(case)
        elif case.get("reject") is not True:
            unclassified.append(str(case.get("id", "<unnamed case>")))
    if unclassified:
        raise SystemExit(
            "refusing to update parity pin: corpus contains cases that are neither "
            "goldens nor explicit refusals: " + ", ".join(unclassified)
        )

    if report.get("cases_failed", 0):
        raise SystemExit(
            "refusing to update parity pin: a paired case failed; the pin is unchanged"
        )
    total = report.get("cases_total")
    passed = report.get("cases_passed")
    if total != len(goldens):
        raise SystemExit(
            "refusing to update parity pin: observed golden count "
            f"{total!r} does not match the corpus count {len(goldens)}"
        )
    if passed != total:
        raise SystemExit(
            "refusing to update parity pin: paired run is incomplete; "
            f"{passed!r} of {total!r} golden cases passed"
        )
    results = report.get("case_results")
    if not isinstance(results, list) or len(results) != total:
        result_count = len(results) if isinstance(results, list) else "no"
        raise SystemExit(
            "refusing to update parity pin: paired run is incomplete; "
            f"it recorded {result_count} per-case results for {total} goldens"
        )
    expected_ids = [case.get("id") for case in goldens]
    observed_ids = [result.get("id") if isinstance(result, dict) else None for result in results]
    if observed_ids != expected_ids:
        raise SystemExit(
            "refusing to update parity pin: paired run is incomplete; its results "
            "skip or duplicate corpus goldens"
        )

    updated = {
        "corpusSha256": report["corpus_sha256"],
        "goldenCaseCount": total,
        "formatterGrammarBaseRevision": declared["formatterGrammarBaseRevision"],
        "lastVerifiedFormatterRevision": report["formatter_revision"],
        "lastVerifiedLinterRevision": report["linter_revision"],
    }
    return validate_pin(updated, "computed pin update")

def select_goldens(corpus) -> list:
    """Return the corpus goldens, refusing a malformed corpus before any case runs."""
    if not isinstance(corpus, dict) or not isinstance(corpus.get("cases"), list):
        raise SystemExit("malformed formatter corpus: expected an object with a case list")
    malformed = [
        str(index) for index, case in enumerate(corpus["cases"]) if not isinstance(case, dict)
    ]
    if malformed:
        raise SystemExit(
            "malformed formatter corpus: every case must be an object; "
            "non-object entries at index " + ", ".join(malformed)
        )
    return [case for case in corpus["cases"] if "expected" in case]

def check_corpus_hash(observed: str, declared: str, update_pin: bool) -> None:
    """A normal paired run remains strict; update mode takes the observed hash."""
    if observed != declared and not update_pin:
        raise SystemExit(
            f"formatter corpus revision/hash mismatch: {observed} "
            f"(parity.json declares {declared})"
        )

def check_golden_count(observed: int, declared: int, update_pin: bool) -> None:
    """A normal run uses the pin; update mode counts the actual corpus goldens."""
    if observed != declared and not update_pin:
        raise SystemExit(
            f"expected {declared} golden cases from parity.json, got {observed}"
        )

def describe_pin_update(declared: dict, updated: dict) -> str:
    lines = [
        f"  {key}: {declared[key]} -> {updated[key]}"
        for key in PIN_KEYS
        if declared[key] != updated[key]
    ]
    return "\n".join(lines) if lines else "  no pin values changed"

def write_pin(path: Path, pin: dict) -> None:
    """Atomically replace parity.json so an interrupted write cannot truncate it."""
    contents = json.dumps(pin, indent=2, ensure_ascii=False) + "\n"
    descriptor, temporary = tempfile.mkstemp(
        prefix=f".{path.name}.", suffix=".tmp", dir=path.parent
    )
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as output:
            output.write(contents)
        os.replace(temporary, path)
    except Exception:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
        raise

def apply_pin_update(path: Path, updated: dict, dry_run: bool = False) -> None:
    """Write a validated candidate unless the caller requested a dry run."""
    if not dry_run:
        write_pin(path, updated)

def run(args, cwd):
    result = subprocess.run(args, cwd=cwd, capture_output=True, timeout=30)
    return result

def require_clean_checkout(path: Path, role: str = "checkout") -> str:
    status = subprocess.run(
        ["git", "-C", str(path), "status", "--porcelain"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=True,
    ).stdout
    if status.strip():
        raise SystemExit(f"paired verification requires clean {role}: {path}")
    return subprocess.run(
        ["git", "-C", str(path), "rev-parse", "HEAD"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=True,
    ).stdout.strip()

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--formatter", required=True, type=Path)
    parser.add_argument("--corpus", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument(
        "--update-pin",
        action="store_true",
        help="update parity.json after a complete, successful paired run",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="show the pending parity.json update without writing it (requires --update-pin)",
    )
    args = parser.parse_args()
    if args.dry_run and not args.update_pin:
        parser.error("--dry-run requires --update-pin")
    # Read the declared pin first, so a malformed declaration fails with its own
    # message instead of hiding behind a checkout or binary problem.
    pin = read_pin()
    expected_corpus_sha = pin["corpusSha256"]
    expected_cases = pin["goldenCaseCount"]
    formatter = args.formatter.resolve(strict=True)
    corpus_path = args.corpus.resolve(strict=True)
    lint = LINTER_ROOT / "target/x86_64-pc-windows-msvc/release/verse-lint.exe"
    if not lint.is_file():
        raise SystemExit(f"missing built linter binary: {lint}")
    # The binary may come from target/release or an external CARGO_TARGET_DIR.
    # The required corpus is a tracked formatter-repository input, so use it to
    # identify the source checkout instead of assuming a target path layout.
    formatter_root = Path(subprocess.run(
        ["git", "-C", str(corpus_path.parent), "rev-parse", "--show-toplevel"],
        capture_output=True, text=True, encoding="utf-8", check=True, timeout=30,
    ).stdout.strip())
    linter_root = LINTER_ROOT
    formatter_revision = require_clean_checkout(formatter_root, "formatter checkout")
    linter_revision = require_clean_checkout(linter_root, "linter checkout")
    corpus_bytes = corpus_path.read_bytes()
    corpus_sha = sha256(corpus_bytes)
    check_corpus_hash(corpus_sha, expected_corpus_sha, args.update_pin)
    cases = json.loads(corpus_bytes)
    positives = select_goldens(cases)
    check_golden_count(len(positives), expected_cases, args.update_pin)
    report = {
        "corpus_sha256": corpus_sha,
        "formatter_version": run([str(formatter), "--version"], corpus_path.parent).stdout.decode().strip(),
        "linter_version": run([str(lint), "--version"], Path.cwd()).stdout.decode().strip(),
        "formatter_sha256": sha256(formatter.read_bytes()),
        "linter_sha256": sha256(lint.read_bytes()),
        "formatter_revision": formatter_revision,
        "linter_revision": linter_revision,
        "cases_total": len(positives),
        "cases_passed": 0,
        "case_results": [],
    }
    with tempfile.TemporaryDirectory(prefix="verse-pair-") as temporary:
        temp_root = Path(temporary)
        for index, case in enumerate(positives):
            directory = temp_root / f"case-{index:03}"
            directory.mkdir()
            path = directory / "source.verse"
            original = case["input"].encode("utf-8")
            expected = case["expected"].encode("utf-8")
            path.write_bytes(original)
            try:
                one = run([str(lint), "--fix", "--output-format", "json", str(path)], directory)
                if one.returncode not in (0, 1):
                    raise RuntimeError(f"{case['id']}: first lint/fix exit={one.returncode}: {one.stderr.decode(errors='replace')}")
                lint_one = json.loads(one.stdout)
                if not lint_one["summary"]["complete"]:
                    raise RuntimeError(f"{case['id']}: first lint/fix incomplete")
                fmt_one = run([str(formatter), "--write", str(path)], directory)
                if fmt_one.returncode != 0:
                    raise RuntimeError(f"{case['id']}: first format exit={fmt_one.returncode}: {fmt_one.stderr.decode(errors='replace')}")
                after_first = path.read_bytes()
                if after_first != expected:
                    raise RuntimeError(f"{case['id']}: first fix→format did not match formatter golden (sha {sha256(after_first)})")

                two = run([str(lint), "--fix", "--output-format", "json", str(path)], directory)
                if two.returncode not in (0, 1):
                    raise RuntimeError(f"{case['id']}: second lint/fix exit={two.returncode}: {two.stderr.decode(errors='replace')}")
                lint_two = json.loads(two.stdout)
                if not lint_two["summary"]["complete"] or lint_two["summary"]["filesChanged"] != 0:
                    raise RuntimeError(f"{case['id']}: second lint/fix changed source or became incomplete")
                fmt_two = run([str(formatter), "--write", str(path)], directory)
                if fmt_two.returncode != 0 or path.read_bytes() != after_first:
                    raise RuntimeError(f"{case['id']}: second format cycle did not converge")
            except Exception as failure:
                suffix = (
                    "\nrefusing to update parity pin after a failed paired case"
                    if args.update_pin
                    else ""
                )
                raise SystemExit(f"paired convergence failed: {failure}{suffix}")
            report["case_results"].append({
                "id": case["id"],
                "input_sha256": sha256(original),
                "output_sha256": sha256(path.read_bytes()),
                "lint_exit_codes": [one.returncode, two.returncode],
                "formatter_exit_codes": [fmt_one.returncode, fmt_two.returncode],
                "first_files_changed": lint_one["summary"]["filesChanged"],
                "second_files_changed": lint_two["summary"]["filesChanged"],
            })
            report["cases_passed"] += 1
    updated_pin = build_pin_update(pin, report, cases) if args.update_pin else None
    print(json.dumps({k: v for k, v in report.items() if k != "case_results"}, indent=2))
    if not args.dry_run:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    if updated_pin is not None:
        print(f"\nparity.json update:\n{describe_pin_update(pin, updated_pin)}")
        if args.dry_run:
            print("dry run: no files written")
        else:
            apply_pin_update(PIN_PATH, updated_pin)
            print(f"wrote {PIN_PATH}; commit the updated pin")

if __name__ == "__main__":
    main()
