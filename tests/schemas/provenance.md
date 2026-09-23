# SARIF schema provenance

- Source: https://docs.oasis-open.org/sarif/sarif/v2.1.0/os/schemas/sarif-schema-2.1.0.json
- Standard: SARIF 2.1.0 OASIS Standard, associated document
  https://docs.oasis-open.org/sarif/sarif/v2.1.0/os/sarif-v2.1.0-os.html
- Retrieved: 2026-09-22 UTC. 115,632 bytes, unchanged; no build/runtime fetching.
- SHA-256: `ad6db49878699b091f3eeb765b6e29e92a34bad4da88664d000c923b549c3a25`.
- Draft-07 JSON Schema. The internal `$id` names OASIS's GitHub master schema;
  the downloaded bytes are pinned to the official published Standard URL above.
- Copyright/permissions: complete Standard Notices reproduced in
  [oasis-notices.md](oasis-notices.md). Do not relabel this resource MIT/Apache.

`scripts/verify-json.ps1` checks the hash and validates actual CLI output with
PowerShell 7 Test-Json. It also proves an intentionally invalid schema version
is rejected. This schema is a development/test resource, not a runtime dependency.
No SARIF upload service or GitHub code-scanning integration is enabled.
