# verse-lint

Linter for Epic Games' Verse language and UEFN projects.

> Unreleased alpha with five style rules, line suppression, file/stdin/project
> input, configuration, text/JSON and guarded Windows safe fixes. SARIF remains
> unimplemented and fails with exit 2. Corpus, UEFN and distribution acceptance
> are still pending. This is not a Verse compiler or a supported stable release.

## Current usage

```powershell
verse-lint .
verse-lint Content/device.verse --select V1001
verse-lint . --output-format json
verse-lint . --fix
verse-lint . --select V2001,V2002 --deny-warnings
verse-lint --show-config
```

No arguments inspects CWD. Exit 0 means inspection completed without selected
errors; 1 means lint violations; 2 means incomplete inspection/config/I/O failure.
Readable-file diagnostics survive failures in other files. Strings/comments are
protected. Unknown/incomplete syntax is an execution error, not a claim that the
UEFN compiler rejects it. Prefer file input in Windows PowerShell 5.1 because
text pipelines can recode bytes before the tool receives them.

Defaults: V1001 trailing whitespace, V1002 final newline, V1003 tab indentation.
V2001 line length and V2002 bare TODO comments are opt-in warnings. Safe fixes
only remove unprotected trailing trivia and add a final newline; they never
guess tab widths, rename symbols or reflow comments. Use version control and
avoid simultaneous editor/UEFN saves during `--fix`.

See [CLI/config and JSON contract](docs/cli.md), [rule index](docs/rules.md),
[safe fixes](docs/fixes.md) and
[pinned independent syntax snapshot](docs/syntax-snapshot.md). No installed
formatter, Node.js or runtime network access is needed.

## Development

Windows x64 is the initial test and distribution target. Rust 1.97.0 (MSVC),
rustfmt and clippy are pinned in `rust-toolchain.toml`. 1.97 is the tested minimum,
not a claim that earlier Rust versions cannot work. Install the MSVC build tools
when building from source. Consumers of future prebuilt binaries will not need Cargo.

```powershell
./scripts/verify.ps1
cargo run -- --help
```

Tests and the release build do not replace manual UEFN acceptance. CI never
publishes packages or creates releases. `verse-lint.dev` is a planned, unregistered
project domain, not an active homepage. The linter is independent of `verse-fmt`.

The project is community-built and is not affiliated with or endorsed by Epic Games.

## License

Licensed under either of:

- [MIT](LICENSE-MIT)
- [Apache License 2.0](LICENSE-APACHE)

at your option.
