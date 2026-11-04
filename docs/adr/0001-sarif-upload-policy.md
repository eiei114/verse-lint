# ADR 0001: Do not upload SARIF to GitHub code scanning

## Status

Accepted (2026-11-04). Supersedes nothing. Closes the open question in
`eiei114/verse-lint#4`.

## Context

`verse-lint . --output-format sarif` emits one deterministic SARIF 2.1.0 run that
is validated against the pinned official OASIS schema. The repository states that
emitting SARIF does not upload anything, but that sentence describes today's
behaviour rather than a decision. Without a recorded decision every future
contributor re-opens the same debate, and a well-meaning pull request can turn
an accidental upload into policy.

**verse-lint is a style linter, not a security tool.** Its five rules are
trailing whitespace (V1001), missing final newline (V1002), tab indentation
(V1003), line length (V2001) and bare `TODO` comments (V2002). None of them
detects a vulnerability, a secret, an injection path or an unsafe API use. This
is a product-boundary statement, not a maturity caveat: the rule set is defined
in `src/rules.rs` `REGISTRY` and `docs/rules.md`.

GitHub code scanning consumes SARIF through `github/codeql-action/upload-sarif`.
That step needs the `security-events: write` workflow permission, and it
publishes the uploaded results as code scanning alerts in the repository's
Security tab for anyone who can read the repository. Per GitHub's
documentation, code scanning on private repositories requires a GitHub Code
Security (formerly Advanced Security) licence.

The measured output volume is relevant, because an upload repeats on every
push. On 2026-11-04 a self-authored, deleted-after-use probe project of 50
Verse files (26,303 bytes) was linted with the development build:

| Selection | Results | SARIF size | Execution-error notifications |
|---|---|---|---|
| Defaults (V1001, V1002, V1003) | 117 (V1001 94, V1003 23) | 111,691 bytes | 11 |
| All five rules | 163 (V1001 94, V2002 27, V1003 23, V2001 19) | 150,365 bytes | 11 |

The repository's own `tests/fixtures/*.verse` produce zero results, so this is
not a complaint about this repository's own cleanliness; it is the shape of
ordinary style drift at project scale. The 11 execution errors in that probe
came from syntax the pinned grammar refuses, which an upload would additionally
publish as failed runs.

## Decision

**Option A: no upload.**

- verse-lint stays a local/CI tool that emits SARIF on stdout.
- This repository ships **no** `upload-sarif` step, **no** code-scanning
  configuration and **no** `security-events` permission in any workflow.
- Uploading is entirely the consumer's own CI step, chosen and written by them,
  in their repository, under their permissions and licence.
- The repository does not publish a copy-paste upload recipe, because a recipe
  in this repository's own docs reads as an endorsement of the practice and
  would re-open the question every review (see "Option B" below).

The existing documentation already implied this; this ADR is what makes it a
decision instead of an omission.

## Trade-offs considered

### What becomes public

An upload publishes every result — rule ID, severity, message, artifact URI and
region — plus each invocation's execution notifications into the repository's
Security tab. For a lint finding the "location" is a line of the consumer's
source, and the alert text names the style rule. Nothing in the current SARIF
output embeds source text (`docs/sarif.md` guarantees no source contents), so an
upload does not leak file bodies. It does disclose file layout and the density
of style problems, and it does so to every account that can read the repository,
including accounts added later.

### The `security-events: write` permission

This is the decisive argument. `security-events: write` is not a lint-scoped
permission: it lets a job create, update and dismiss code scanning alerts. The
current CI workflow declares exactly `permissions: contents: read`
(`.github/workflows/ci.yml`). Adding an upload would widen the token of a job
that currently only checks out and tests. Any third-party action or compromised
dependency in that job would then be able to manipulate security alerts. The
gain is a style report in a security surface; the cost is a security-relevant
write permission in a style linter's pipeline. That trade is not worth taking.

### SARIF result volume from a style linter

117–163 results for 50 files means a real project yields thousands of alerts,
re-uploaded on every push and every pull request. Code scanning is built to keep
that list actionable; a style linter fills it with whitespace. The alerts are
also not stable run to run in a way that matters — fixing trailing whitespace
closes an alert, which reads as "a security issue was remediated".

### False-positive visibility to third parties

Style findings that are legitimately debatable — a long line that a reader
considers fine, a `TODO` that is intentional and unreferenced, tab indentation
in a file the project decided to keep — become alerts attributed to the
repository rather than to the author's own tooling. A false positive in a local
run costs a shrug; a false positive in a shared Security tab costs triage time
and credibility for everyone watching the repository.

### A Security tab that misleads about severity

Severity misplacement is the strongest product argument. Code scanning alerts
are read as security findings. verse-lint reports V1001 (trailing whitespace) as
`error`. Uploaded unchanged, that becomes an error-level code scanning alert on
par with a real vulnerability, and a reader who does not open the alert has no
way to tell the difference. The alternative — re-mapping style severities to
`note` before upload — means the uploaded artifact is no longer the documented
SARIF contract in `docs/sarif.md`, so the tool would be shipping a second,
undocumented severity model purely to fit a surface it never targeted.

## Alternatives rejected

### Option B: opt-in documented recipe

Rejected. It keeps the workflow out of the repository, which is the part that
matters for permissions, but it puts a `security-events: write` snippet in this
repository's own docs. Contributors read a recipe in `docs/` as the maintainers'
recommendation, then file issues when the recommended recipe publishes style
noise as security alerts, or when it fails on a private repository without a
code-security licence. The honest version of "you may do this yourself" is
GitHub's own documentation of the action, which is authoritative and stays
current; a copy here would be a worse-maintained duplicate.

Reconsidering this option later is cheap: it is a docs-only change plus the
guard's allow-list entry, and this ADR records exactly what would have to change.

### Option C: committed upload workflow

Rejected. It takes on all of the costs above and adds a permanent permission
widening in exchange for a style report nobody asked for. The prerequisite for
even considering it would be a security-relevant rule set, which is explicitly
outside this project's scope (`eiei114/verse-lint#4` non-goal: never silently
publish SARIF).

## Enforcement

The decision is enforced, not merely written. `tests/sarif-upload-guard.rs`
reads every file under `.github/workflows/` and fails if any of them contains an
`upload-sarif` step or a `security-events` permission, naming this ADR in the
failure message. `cargo test`, and therefore `scripts/verify.ps1` and the Windows
CI job, run it.

Changing the decision means editing this ADR first and the guard second; a pull
request that only edits the workflow fails verification.

## Consequences

- Positive: no security-relevant write permission in a style linter's CI; the
  question is closed for contributors; `security-events: write` cannot appear by
  accident; consumers keep full freedom in their own repositories.
- Negative: a consumer who wants code scanning must write the workflow
  themselves from GitHub's documentation. That is the intended division: the
  linter owns the SARIF contract, the consumer owns the upload.
- Neutral: `docs/sarif.md` remains the authoritative description of the emitted
  artifact; this ADR adds no output behaviour and changes no rule.

## References

- `docs/sarif.md` — the emitted SARIF contract.
- `README.md` — user-facing statement of the same policy.
- `docs/rules.md` — the style rule set this ADR's severity argument refers to.
- `.github/workflows/ci.yml` — `permissions: contents: read`, unchanged.
- `tests/sarif-upload-guard.rs` — the guard that enforces this decision.
- `eiei114/verse-lint#4` — the upstream question this record answers.
- GitHub code scanning and `github/codeql-action/upload-sarif` documentation.
