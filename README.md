# verse-lint

Linter for Epic Games' Verse language and UEFN projects.

> Unreleased alpha with seven lint rules, line suppression, file/stdin/project
> input, configuration, text/JSON/SARIF/GitHub annotations and guarded Windows
> safe fixes.
> A paired 64-case formatter convergence harness and PowerShell/cmd smoke matrix
> pass locally. UEFN compiler acceptance, broad-project readiness and
> distribution checks remain pending. This is not a Verse compiler or a
> supported stable release.

## Installation

verse-lint is **not published anywhere yet**: there is no crates.io crate and no
released binary. Read the labels below before running a command — two of the
three paths do not work today.

### Prebuilt Windows binary via mise — not yet available

The intended path is a version-pinned `mise` entry against a future GitHub
Release asset. No such release exists, so this path cannot install anything yet.
The illustrative `mise.toml` in [the packaging note][packaging] is unverified:
asset selection, checksum checks and clean-machine installation have not been
tested with mise. Do not run it expecting an install.

### `cargo install verse-lint` — not yet published

The crate metadata is prepared for a future publish, but nothing has been
uploaded. `cargo install verse-lint` fails today because crates.io has no such
crate. Do not treat the command as working until a published version is visible
on crates.io.

### Building from source — available today

This is the only path that works now. Requirements: Windows x64 (the initial
target), Rust 1.97.0 with the MSVC toolchain — pinned in `rust-toolchain.toml` —
the MSVC build tools, and the C compiler they ship, because `build.rs` compiles
the vendored Tree-sitter grammar.

```powershell
git clone https://github.com/eiei114/verse-lint
Set-Location verse-lint
cargo build --release --locked --target x86_64-pc-windows-msvc
./target/x86_64-pc-windows-msvc/release/verse-lint.exe --version
```

`cargo install --path .` installs the same build into your Cargo bin directory
from a local checkout. Either way the binary is local and unpublished, so do not
redistribute it as a release.

## Current usage

```powershell
verse-lint .
verse-lint Content/device.verse --select V1001
verse-lint . --output-format json
verse-lint . --output-format sarif
verse-lint . --output-format github
verse-lint . --fix
verse-lint . --fix --diff
verse-lint . --select V2001,V2002 --deny-warnings
verse-lint . --select V3001
verse-lint . --select V3002
verse-lint . --show-suppressed
verse-lint . --no-ignore-file
verse-lint --show-config
verse-lint Content/device.verse --dump-scopes
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

`--dump-scopes` prints the model's scope graph as JSON for rule authors, runs no
rule, and is an **unstable** surface rather than a public contract; see
[the semantic model][semantic].

No arguments inspects CWD. Exit 0 means inspection completed without selected
errors; 1 means lint violations; 2 means incomplete inspection/config/I/O failure.
Readable-file diagnostics survive failures in other files. Strings/comments are
protected. Unknown/incomplete syntax is an execution error, not a claim that the
UEFN compiler rejects it. Prefer file input in Windows PowerShell 5.1 because
text pipelines can recode bytes before the tool receives them.

Defaults: V1001 trailing whitespace, V1002 final newline, V1003 tab indentation.
V2001 line length and V2002 bare TODO comments are opt-in warnings. V3001
unused-declaration is an opt-in warning that reads the
[scope graph][semantic] rather than the token list, and reports nothing at all
for a file whose model is incomplete — an `unused` claim the tool could not prove
would tell you to delete live code. It never reports an exported, `public`,
`protected` or `_`-prefixed name, a `using` import or a `for`/`if` binding, and
it does not report parameters unless you set `unused-parameters = true` under
`[lint]`. V3002 unresolved-reference is also opt-in; without a Verse/UEFN API
database, a missing local name may still come from an API or another file, so a
false positive is a bug. Start with a **report-only job using `--select V3002`**
and add confirmed project externals to `[lint] external-names`. The built-in
allowlist is intentionally short; see [V3002][v3002].
Both rules report nothing at all for files whose scope graph is incomplete.
`[lint.severity]`
can promote or demote a registered rule per project (`"error"`/`"warning"` only);
unknown IDs or values exit 2. It changes severity only, so a promoted rule still
has to be selected. Safe fixes
only remove unprotected trailing trivia and add a final newline; they never
guess tab widths, rename symbols or reflow comments. Use version control and
avoid simultaneous editor/UEFN saves during `--fix`. `--fix --diff` prints the
unified diff of exactly those edits and writes nothing, so the preview can run
in CI or an unfamiliar checkout; stdout then carries only the diff and the
remaining diagnostics go to stderr.

See [CLI/config and JSON contract][cli], [rule index][rules], [safe fixes][fixes],
[pinned independent syntax snapshot][snapshot],
[the scope graph and semantic model][semantic],
[paired/shell integration checks][integration] and
[local packaging and future mise usage][packaging]. No installed formatter,
Node.js or runtime network access is needed.

The formatter parity contract is declared in `parity.json` and checked
structurally by `scripts/verify.ps1`; only `scripts/verify-pair.py` can claim
actual convergence. See the [parity checklist][contributing] before changing
the grammar snapshot, the corpus or safe-fix bytes.

[SARIF 2.1.0][sarif] is validated against the pinned official OASIS
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
[the CLI reference][cli-github].

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
missing feature: see [ADR 0001: do not upload SARIF to GitHub code scanning][adr].
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

Read [CONTRIBUTING.md][contributing] before changing the syntax snapshot, the
formatter corpus or safe-fix bytes.

Tests and the release build do not replace manual UEFN acceptance. CI never
publishes packages or creates releases. `verse-lint.dev` is a planned, unregistered
project domain, not an active homepage. The linter is independent of `verse-fmt`.
Neither the crate nor a binary is published; [the release checklist][checklist]
lists the human-owned steps a publish would need, and the reasons each one is a
decision rather than a re-run of CI.

The project is community-built and is not affiliated with or endorsed by Epic Games.

## License

Licensed under either of:

- [MIT](https://github.com/eiei114/verse-lint/blob/main/LICENSE-MIT)
- [Apache License 2.0](https://github.com/eiei114/verse-lint/blob/main/LICENSE-APACHE)

Links in this file are absolute and mostly written as reference definitions at
the bottom, because crates.io renders this file without serving the
repository's `docs/` directory: a relative link is dead there. They point at
`main`, which may be newer than a published version.

[cli]: https://github.com/eiei114/verse-lint/blob/main/docs/cli.md
[rules]: https://github.com/eiei114/verse-lint/blob/main/docs/rules.md
[fixes]: https://github.com/eiei114/verse-lint/blob/main/docs/fixes.md
[snapshot]: https://github.com/eiei114/verse-lint/blob/main/docs/syntax-snapshot.md
[semantic]: https://github.com/eiei114/verse-lint/blob/main/docs/semantic-model.md
[v3002]: https://github.com/eiei114/verse-lint/blob/main/docs/rules/v3002.md
[integration]: https://github.com/eiei114/verse-lint/blob/main/docs/integration-testing.md
[packaging]: https://github.com/eiei114/verse-lint/blob/main/docs/packaging-and-mise.md
[sarif]: https://github.com/eiei114/verse-lint/blob/main/docs/sarif.md
[adr]: https://github.com/eiei114/verse-lint/blob/main/docs/adr/0001-sarif-upload-policy.md
[checklist]: https://github.com/eiei114/verse-lint/blob/main/docs/release-checklist.md
[cli-github]: https://github.com/eiei114/verse-lint/blob/main/docs/cli.md#github-annotations
[contributing]: https://github.com/eiei114/verse-lint/blob/main/CONTRIBUTING.md
