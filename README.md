# verse-lint

Linter for Epic Games' Verse language and UEFN projects.

> Unreleased alpha with five style rules, line suppression, file/stdin/project
> input, configuration, text/JSON/SARIF/GitHub annotations and guarded Windows
> safe fixes.
> A paired 64-case formatter convergence harness and PowerShell/cmd smoke matrix
> pass locally. UEFN compiler acceptance, broad-project readiness and
> distribution checks remain pending. This is not a Verse compiler or a
> supported stable release.

## Current usage

```powershell
verse-lint .
verse-lint Content/device.verse --select V1001
verse-lint . --output-format json
verse-lint . --output-format sarif
verse-lint . --output-format github
verse-lint . --fix
verse-lint . --select V2001,V2002 --deny-warnings
verse-lint . --show-suppressed
verse-lint . --no-ignore-file
verse-lint --show-config
```

```toml
# verse.toml: make an opt-in policy rule a hard failure for this project
[lint]
select = ["V1001", "V1002", "V1003", "V2001"] # config select replaces the defaults
[lint.severity]
V2001 = "error"
```

`select`/`ignore` also accept a rule-family prefix, so `--select V2` enables every
registered V2 rule at once; `--show-config` prints the expanded complete IDs.

```text
# verse-lint.ignore, at the directory owning verse.toml (or CWD)
# one relative forward-slash glob per line; blank lines and # comments are skipped
Content/Generated/**
Vendor/**
```

`verse-lint.ignore` is a shared exclusion list for a project that needs the same
skips in several repositories. It combines with `[files] exclude` as a union —
neither overrides the other — and is read only from that one location: no parent
search, no nested ignore files. `--no-ignore-file` skips it for one run, and
`--verbose` names which source excluded a path.

No arguments inspects CWD. Exit 0 means inspection completed without selected
errors; 1 means lint violations; 2 means incomplete inspection/config/I/O failure.
Readable-file diagnostics survive failures in other files. Strings/comments are
protected. Unknown/incomplete syntax is an execution error, not a claim that the
UEFN compiler rejects it. Prefer file input in Windows PowerShell 5.1 because
text pipelines can recode bytes before the tool receives them.

Defaults: V1001 trailing whitespace, V1002 final newline, V1003 tab indentation.
V2001 line length and V2002 bare TODO comments are opt-in warnings. `[lint.severity]`
can promote or demote a registered rule per project (`"error"`/`"warning"` only);
unknown IDs or values exit 2. It changes severity only, so a promoted rule still
has to be selected. Safe fixes
only remove unprotected trailing trivia and add a final newline; they never
guess tab widths, rename symbols or reflow comments. Use version control and
avoid simultaneous editor/UEFN saves during `--fix`.

See [CLI/config and JSON contract](docs/cli.md), [rule index](docs/rules.md),
[safe fixes](docs/fixes.md) and
[pinned independent syntax snapshot](docs/syntax-snapshot.md) and
[paired/shell integration checks](docs/integration-testing.md), plus
[local packaging and future mise usage](docs/packaging-and-mise.md). No
installed formatter, Node.js or runtime network access is needed.

The formatter parity contract is declared in `parity.json` and checked
structurally by `scripts/verify.ps1`; only `scripts/verify-pair.py` can claim
actual convergence. See the [parity checklist](CONTRIBUTING.md) before changing
the grammar snapshot, the corpus or safe-fix bytes.

[SARIF 2.1.0](docs/sarif.md) is validated against the pinned official OASIS
schema. Emitting SARIF writes the report to stdout only; the command has no
network access and uploads nothing.

`--output-format github` is for pull-request review: one
[Actions workflow command](https://docs.github.com/actions/reference/workflow-commands-for-github-actions)
per finding on stdout, `::error` or `::warning` by severity, so a PR job
annotates the diff without code scanning and without a token.

```text
::error file=Content/device.verse,line=12,col=7,endLine=12,endColumn=9,title=V1001 TrailingWhitespace::Trailing whitespace
```

```yaml
- name: Lint Verse
  shell: pwsh
  run: ./verse-lint.exe Content --output-format github
```

GitHub shows at most 10 warning and 10 error annotations per step; findings
beyond that are not attached to the diff, so keep `json` or `sarif` for a
complete report. `file=` is resolved against the repository root, which is where
the configuration root has to sit for annotations to line up — a nested
`verse.toml` still emits config-root-relative paths, exactly as text and JSON do.
That limitation, and why this format does not discover the checkout root, are in
[the CLI reference](docs/cli.md#github-annotations).

Properties escape `%`, CR, LF, `:` and `,` (`%25`, `%0D`, `%0A`, `%3A`, `%2C`),
and a message escapes `%`, CR and LF, so a path or message containing them
stays one annotation. Find suppressed findings with `--show-suppressed` are
reported as `::notice`, which cannot fail a check run, matching the rule that a
suppressed finding never changes the exit code. `--color` is accepted and inert;
a workflow command never carries ANSI. Exit codes are unchanged — 0 clean, 1
violations (or denied warnings), 2 incomplete or configuration failure — and
execution errors stay on stderr, never on stdout as annotations. No network
access, no GitHub API call and no token handling happen in any of these formats.

This project does not upload SARIF to GitHub code scanning and ships no upload
workflow or code-scanning configuration. That is a recorded decision, not a
missing feature: see
[ADR 0001: do not upload SARIF to GitHub code scanning](docs/adr/0001-sarif-upload-policy.md).
Uploading, if you want it, is your own CI step in your own repository, under
your permissions and licence. This is a style linter — its findings are not
security findings, and error-level style alerts in a Security tab would be
misleading.

## Development

Windows x64 is the initial test and distribution target. Rust 1.97.0 (MSVC),
rustfmt and clippy are pinned in `rust-toolchain.toml`. 1.97 is the tested minimum,
not a claim that earlier Rust versions cannot work. Install the MSVC build tools
when building from source. Consumers of future prebuilt binaries will not need Cargo.

```powershell
./scripts/verify.ps1
cargo run -- --help
```

Read [CONTRIBUTING.md](CONTRIBUTING.md) before changing the syntax snapshot, the
formatter corpus or safe-fix bytes.

Tests and the release build do not replace manual UEFN acceptance. CI never
publishes packages or creates releases. `verse-lint.dev` is a planned, unregistered
project domain, not an active homepage. The linter is independent of `verse-fmt`.

The project is community-built and is not affiliated with or endorsed by Epic Games.

## License

Licensed under either of:

- [MIT](LICENSE-MIT)
- [Apache License 2.0](LICENSE-APACHE)

at your option.
