"""Developer-only paired convergence check; no runtime dependency on verse-fmt."""
import argparse
import hashlib
import json
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
    if not isinstance(declared, dict):
        raise SystemExit(f"malformed parity pin {path}: top level must be an object")
    missing = [key for key in PIN_KEYS if key not in declared]
    if missing:
        raise SystemExit(f"incomplete parity pin {path}: missing {', '.join(missing)}")
    corpus_sha = declared["corpusSha256"]
    if not isinstance(corpus_sha, str) or not SHA256_PATTERN.match(corpus_sha):
        raise SystemExit(
            f"malformed parity pin {path}: corpusSha256 must be 64 lowercase hex digits"
        )
    golden_count = declared["goldenCaseCount"]
    if not isinstance(golden_count, int) or isinstance(golden_count, bool) or golden_count < 1:
        raise SystemExit(
            f"malformed parity pin {path}: goldenCaseCount must be a positive integer"
        )
    for key in (
        "formatterGrammarBaseRevision",
        "lastVerifiedFormatterRevision",
        "lastVerifiedLinterRevision",
    ):
        value = declared[key]
        if not isinstance(value, str) or not REVISION_PATTERN.match(value):
            raise SystemExit(
                f"malformed parity pin {path}: {key} must be a 40-character lowercase revision"
            )
    return declared

def run(args, cwd):
    result = subprocess.run(args, cwd=cwd, capture_output=True, timeout=30)
    return result

def require_clean_checkout(path: Path) -> str:
    status = subprocess.run(
        ["git", "-C", str(path), "status", "--porcelain"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=True,
    ).stdout
    if status.strip():
        raise SystemExit(f"paired verification requires clean checkout: {path}")
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
    args = parser.parse_args()
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
    formatter_revision = require_clean_checkout(formatter_root)
    linter_revision = require_clean_checkout(linter_root)
    corpus_bytes = corpus_path.read_bytes()
    corpus_sha = sha256(corpus_bytes)
    if corpus_sha != expected_corpus_sha:
        raise SystemExit(
            f"formatter corpus revision/hash mismatch: {corpus_sha} "
            f"(parity.json declares {expected_corpus_sha})"
        )
    cases = json.loads(corpus_bytes)
    positives = [item for item in cases["cases"] if "expected" in item]
    if len(positives) != expected_cases:
        raise SystemExit(
            f"expected {expected_cases} golden cases from parity.json, got {len(positives)}"
        )
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
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps({k: v for k, v in report.items() if k != "case_results"}, indent=2))

if __name__ == "__main__":
    main()
