"""Developer-only paired convergence check; no runtime dependency on verse-fmt."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

EXPECTED_CORPUS_SHA256 = "68918abdf3f7b574ccdc803c4af34bc1f5ebeaf1cc0637e8330fcfa6f1465f28"

def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()

def run(args, cwd):
    result = subprocess.run(args, cwd=cwd, capture_output=True, timeout=30)
    return result

def require_clean_checkout(path: Path) -> str:
    status = subprocess.run(
        ["git", "-C", str(path), "status", "--porcelain"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    if status.strip():
        raise SystemExit(f"paired verification requires clean checkout: {path}")
    return subprocess.run(
        ["git", "-C", str(path), "rev-parse", "HEAD"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--formatter", required=True, type=Path)
    parser.add_argument("--corpus", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    formatter = args.formatter.resolve(strict=True)
    corpus_path = args.corpus.resolve(strict=True)
    lint = Path(__file__).resolve().parents[1] / "target/x86_64-pc-windows-msvc/release/verse-lint.exe"
    if not lint.is_file():
        raise SystemExit(f"missing built linter binary: {lint}")
    # The binary may come from target/release or an external CARGO_TARGET_DIR.
    # The required corpus is a tracked formatter-repository input, so use it to
    # identify the source checkout instead of assuming a target path layout.
    formatter_root = Path(subprocess.run(
        ["git", "-C", str(corpus_path.parent), "rev-parse", "--show-toplevel"],
        capture_output=True, text=True, check=True, timeout=30,
    ).stdout.strip())
    linter_root = Path(__file__).resolve().parents[1]
    formatter_revision = require_clean_checkout(formatter_root)
    linter_revision = require_clean_checkout(linter_root)
    corpus_bytes = corpus_path.read_bytes()
    corpus_sha = sha256(corpus_bytes)
    if corpus_sha != EXPECTED_CORPUS_SHA256:
        raise SystemExit(f"formatter corpus revision/hash mismatch: {corpus_sha}")
    cases = json.loads(corpus_bytes)
    positives = [item for item in cases["cases"] if "expected" in item]
    if len(positives) != 58:
        raise SystemExit(f"expected 58 golden cases, got {len(positives)}")
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
