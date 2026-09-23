# Local Windows package and mise example

`scripts/package.py` creates a deterministic local ZIP from a pinned Windows
release binary. It does not create a tag, GitHub Release, crate, or published
artifact. It refuses dirty source trees and existing outputs. From a committed
checkout, build/verify first and then package:

```powershell
pwsh -NoProfile -File scripts/verify.ps1
python scripts/package.py
```

The archive is written under ignored `target/packages/` and named
`verse-lint-v<VERSION>-x86_64-pc-windows-msvc.zip`. It contains
`verse-lint.exe` at ZIP root, `README.md`, `BUILD-INFO.txt`, licenses,
`NOTICE`, and the linter documentation. A sibling `.sha256` is generated and
verified against the exact ZIP. Entry order and timestamps are normalized;
the script checks CRC, entry list, every archived byte, and the checksum.
The checksum identifies the artifact but is not a signature or trust claim.

The linter archive contains no installed Formatter, Rust toolchain, or runtime
network requirement. Its syntax snapshot and runtime dependencies are built
into the executable. The paired-test Python harness is a developer tool, not
part of runtime operation.

The following UEFN-project `mise.toml` is illustrative only. Version
`0.1.0-alpha.1` has not been released, and real asset selection/checksum
behavior has not been tested with mise. Do not run it expecting installation
to work before an approved GitHub Release exists.

```toml
[tools]
"github:eiei114/verse-lint" = "0.1.0-alpha.1"

[tasks.lint]
run = "verse-lint ."

[tasks.check]
run = "verse-lint . --output-format text"
```

Pin an exact version; do not use `latest`. `verse.toml` configures linting
separately from mise. After any release is separately approved, test the exact
asset with a recorded mise version, including install, pin, reinstall and
checksum behavior. Until then, mise integration and clean-machine installation
remain unverified.
