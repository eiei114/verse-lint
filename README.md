# verse-lint

Linter for Epic Games' Verse language and UEFN projects.

> Unreleased, incomplete alpha. The current slice implements V1001 trailing
> whitespace, file/stdin/project input, configuration and text/JSON output.
> The other four planned rules, suppression, safe fixes and SARIF are next;
> requesting them fails with exit 2 instead of pretending they ran. No files
> are modified. This is not the finished v0.1 rule set or a Verse compiler.

## Current usage

```powershell
verse-lint .
verse-lint Content/device.verse --select V1001
verse-lint . --output-format json
verse-lint --show-config
```

No arguments inspects CWD. Exit 0 means inspection completed without selected
errors; 1 means lint violations; 2 means incomplete inspection/config/I/O failure.
Readable-file diagnostics survive failures in other files. Strings/comments are
protected. Unknown/incomplete syntax is an execution error, not a claim that the
UEFN compiler rejects it. Prefer file input in Windows PowerShell 5.1 because
text pipelines can recode bytes before the tool receives them.

See [CLI/config and JSON contract](docs/cli.md), [V1001](docs/rules/v1001.md) and
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
