# Narrow line suppression

Only lexer-confirmed line comments whose content (after `#` and leading
whitespace) begins exactly `verse-lint:` are directives. Strings and block
comments never are. An ordinary comment mentioning `verse-lint` without the
colon prefix is not a directive.

```text
# verse-lint: disable-next-line V2001,V2002 -- intentional test data
Print("long text") # verse-lint: disable-line V2001 -- protocol constant
```

`disable-line` requires code before the comment on that same physical line.
`disable-next-line` targets exactly the next physical line, even if it is blank
or another comment. IDs are comma-separated complete IDs with optional
surrounding whitespace. Duplicate IDs collapse. A nonempty reason is mandatory;
`--` must be separated from both IDs and reason by whitespace.

Unknown/empty IDs, missing reasons, unsupported modes and V1002 line suppression
are execution failures, even if the referenced rule was not selected. V1002 is
file-level: use config ignore. No all-rules, file-wide or disable/enable regions.
Directives never suppress I/O, encoding, parser or invariant errors. Invalid
directives prevent every file's fix during preflight.

Only diagnostics from selected, non-ignored rules contribute to `suppressed`.
They are removed from ordinary output, but count toward the 10,000-finding
resource bound. No unused-directive cleanup or show-suppressed option exists.
