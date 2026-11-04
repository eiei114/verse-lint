# Test source provenance

The original `.verse` fixtures were authored for verse-fmt and imported with
the independent snapshot. Later linter inline tests, including range-binding
refusal/fix tests, are independently authored; the original structural
range-binding regressions are retained in both tools. They use public Verse
syntax and API names; no Epic
sample project, assets, generated digest, or proprietary source was copied.
They use the project's MIT OR Apache-2.0 license.

`device.input.verse` is a minimal self-authored creative-device example.
`device.expected.verse` is its intended formatting, not compiler output.

`baseline/*.verse` are self-authored inputs for the pre-slice-versus-HEAD output
parity sweep in `scripts/verify-baseline-parity.ps1`. They cover one input per
rule, clean input, suppression, string and comment protection, Unicode literals,
and the grammar refusals the linter documents. They are test inputs, not
compiler output and not a code-quality sample.

UEFN compile and runtime acceptance have not yet been performed. Parser and
formatter test success must not be presented as UEFN acceptance.
