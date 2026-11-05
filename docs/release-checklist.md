# Release checklist for a crates.io publish

Nothing in this repository publishes. Every step below is a human action taken
at a release boundary, and each one is a decision — not a routine re-run of CI.
`cargo publish --dry-run` and `cargo package` are read-only local checks and are
the only commands here you may run without deciding anything.

The checklist exists so an approved publish is one operation instead of an
investigation. It does not grant permission: publishing, tagging and creating a
release stay separate approvals, and no workflow in this repository performs
any of them.

## Before you start

- [ ] The commit to publish is on `main`, the working tree is clean, and
      `pwsh -NoProfile -File scripts/verify.ps1` passes on that exact commit.
- [ ] The crate version in `Cargo.toml` is the version you intend to publish;
      the tag and the published version will not be reconciled later.
- [ ] `crates.io` has no `verse-lint` version with that number. A published
      version number is spent permanently.

## Version and its rationale

- [ ] Choose the increment deliberately and record the reason in the release
      note: `0.1.0-alpha.1` is an unreleased alpha with a documented, incomplete
      contract, so nothing here justifies a `1.0.0`. A first publish is
      `0.1.0-alpha.1` as it stands, or a new pre-release if the contract moved.
- [ ] Change only the `version` field in `Cargo.toml`; `Cargo.lock` follows the
      next build. Do not edit the version to prepare a publish you have not
      approved.
- [ ] `rust-version` is still honest: it is the tested minimum (1.97), not a
      claim about earlier compilers.

## Changelog

- [ ] `CHANGELOG.md` has an `Unreleased` section with every user-visible change
      since the previous release, and nothing that is not yet merged.
- [ ] Rename `Unreleased` to the version you are publishing, with a date, and
      open a fresh empty `Unreleased` section in the same commit.
- [ ] The changelog says what is *not* established (UEFN compiler acceptance,
      broad-project readiness, distribution checks) so a reader of the release
      page is not misled by the entry list.

## Licence and attribution

- [ ] `LICENSE-MIT`, `LICENSE-APACHE` and `NOTICE` are the versions you intend
      to ship, and the licence choice in the changelog matches them.
- [ ] `Cargo.toml` declares `license = "MIT OR Apache-2.0"` and does **not** set
      `license-file`. Declaring both is not an error — `cargo package` and
      `cargo publish --dry-run` still succeed — but Cargo warns that only one of
      them is necessary, `license-file` being meant for a non-standard licence.
      Neither shipped text carries the dual wording on its own.
- [ ] `NOTICE` still names the imported formatter revision and the vendored
      Tree-sitter grammar with its copyright holder.
- [ ] Both vendored licence texts (`vendor/tree-sitter-verse/LICENSE`,
      `vendor/tree-sitter-verse/LICENSE-tree-sitter`) are inside the package. A
      published crate must carry them; check them in the `cargo package --list`
      output rather than assuming inclusion.

## Package contents

- [ ] Run `cargo package --list` and read the list. It must contain no `target/`
      directory, no cargo build output, and no `vendor/` source that no build
      step reads.
- [ ] The files the build needs are present: `build.rs`,
      `vendor/tree-sitter-verse/src/parser.c`,
      `vendor/tree-sitter-verse/src/scanner.c` and the three
      `vendor/tree-sitter-verse/src/tree_sitter/*.h` headers that `parser.c`
      includes.
- [ ] The files the code and tests read are present: `docs/rules/*.md`,
      `docs/cli.md`, `docs/parser-design.md`, `docs/rules.md`,
      `docs/syntax-snapshot.md`, `vendor/tree-sitter-verse/hashes.json`,
      `vendor/tree-sitter-verse/upstream.md`, `tests/fixtures/`,
      `tests/schemas/` and `.github/workflows/` (the SARIF upload guard reads
      it).
- [ ] `[package] exclude` in `Cargo.toml` is what you intend: `/scripts` (the
      paired harness needs a sibling `verse-fmt` checkout and is not runnable
      from a published crate), `tests/test_package.py`, and the generated
      grammar inputs `grammar.js`, `tree-sitter.json`, `src/grammar.json`,
      `src/node-types.json` that only regeneration reads. `hashes.json` records
      the full vendored tree, including those four files, in the repository.
- [ ] `cargo package` (without `--allow-dirty`) succeeds and builds the crate
      from the unpacked tarball. A publish fails here if it would fail there.

## Dry run

- [ ] Run `cargo publish --dry-run`. Expect: the same file list you reviewed,
      no upload, and no credential prompt. If your environment has no network
      access to the registry index, skip the step, and record that it was
      skipped rather than treating a network failure as a pass.
- [ ] Capture the file list it prints; it is the artifact this checklist is
      about.

## Tag and release

- [ ] After a successful publish, create an annotated tag `v<VERSION>` matching
      the published version exactly, and push it deliberately.
- [ ] A GitHub Release is a separate decision from the crates.io publish. If one
      is created, its notes are the changelog section, not a new summary.
- [ ] A prebuilt Windows binary is a separate decision again: the archive comes
      from `scripts/package.py` on the same commit, and `docs/packaging-and-mise.md`
      keeps its "not yet available" wording until mise installation has actually
      been tested with a recorded mise version.

## docs.rs

- [ ] `[package.metadata.docs.rs]` stays absent. This package has no library
      target, so docs.rs would build an empty binary crate: no usable API
      documentation, while implying that generated docs are the contract. The
      contract lives in `docs/`, which neither crates.io nor docs.rs serves;
      `README.md` links to it with absolute repository URLs for that reason.

## CI must never publish

- [ ] `.github/workflows/ci.yml` still only verifies on `windows-2025` with
      `contents: read`.
- [ ] No workflow gains a `cargo publish`, a `git tag`, a release, an
      upload-artifact-to-release step, or a registry token. Credentials do not
      belong in this repository, in any form.
- [ ] `tests/sarif-upload-guard.rs` still passes: it fails the build if a
      workflow gains the SARIF upload step or a `security-events` permission,
      and the same reasoning applies to any new publishing step. Adding one is a
      policy change that needs its own decision, not a convenience.
