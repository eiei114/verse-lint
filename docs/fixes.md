# Safe Windows fixes

`--fix` changes only unsuppressed V1001/V1002 findings. It is not a formatter;
indentation, operator spacing, identifiers, comments and line wrapping stay as-is.
Stdin fixing is prohibited. Use version control and stop simultaneous saves.

Each edit has its original UTF-8 byte range, expected old span and SHA-256 of
the complete original source. Before applying edits, reject stale hashes/spans,
overlap and duplicate/same-start positions. Adjacent deletion and EOF insertion
are allowed. Apply edits in descending byte order. Reparse the candidate;
compare CST shape and every non-trivia token, including protected-span bytes.
Re-lint and require no additional safe edits in one pass. BOM and LF/CRLF remain;
an existing terminal line comment can gain a newline without changing its bytes.

All source analysis and candidate validation must succeed before any writes.
Then stage/lock every changed file before committing the first. Windows adapter:
same-directory synced candidate and recovery copy, original DACL applied to
scratch files before source bytes, identity/attributes/byte revalidation,
read/DELETE handle denying writers, backup-backed ReplaceFileW, no ACL-ignore
flags and no delete-before-rename fallback. Existing handles denying deletion
are detected at preflight, not after earlier files have changed.

Read-only files, hardlinks and reparse paths are refused for changed sources.
Unchanged files are not written, preserving mtime. Backup restoration never
overwrites an existing competing destination. If safe restoration is uncertain,
retain recovery paths and report them instead of erasing evidence. Cleanup
failure after successful replacement warns and preserves remaining recovery.

This is not a multi-file transaction. Later commit failure reports completed
and not-attempted paths with exit 2. Only successfully saved files count in
`filesChanged` and acquire candidate diagnostic positions. Uncommitted files
retain inspected-original diagnostics, explicitly marked as such in the
execution failure message. No candidate-only success is counted. Previous
successful writes are not rolled back over possible editor changes.

Tests include original hash/span conflicts, overlapping edits, BOM/CRLF/Unicode,
EOF comments, suppression, remaining tabs, one-pass convergence, unchanged
mtime, all-input abort, actual Windows share locks, read-only/hardlinks, custom
DACL/creation time/NTFS stream, injected staging/replacement/recovery failures
and injected mid-batch failure with persisted-only accounting.

Support is trusted local Windows developer directories, not a hostile-filesystem
sandbox. Parent namespace races, unusual owner/group metadata, network/synced
filesystems and power-loss durability remain unverified. Non-Windows write
adapter fails explicitly. UEFN/clean-machine/release acceptance remains pending.

## Preview with `--fix --diff`

`--fix --diff` runs the same analysis, validation and convergence pipeline
described above and then stops: no file is opened for replacement, no staging or
backup file is created, and no byte of the working tree changes. The preview is
therefore safe to run in an unfamiliar checkout or in CI, and it reports the edit
a real `--fix` would make even for a read-only, hard-linked or locked file that a
write run would refuse at preflight. `summary.filesChanged` stays `0`, and the
remaining diagnostics describe the candidate source exactly as a successful save
would, so an unfixable tab line is still reported at its post-fix position.

`--diff` is valid only with `--fix`. `--diff` alone, `--diff` with stdin (which
`--fix` already rejects), `--fix --diff --show-config` and
`--fix --diff --output-format json|sarif|github` each exit 2 with a message and
change nothing; the diff has no JSON representation in this release, and stdout
is never two formats at once.

The preview owns stdout, so in this one mode the text diagnostics that normally
print there move to stderr. That keeps the rule that stdout carries exactly one
format: `path:line:column: severity rule-id message` lines go to stderr while
stdout is only the diff. Exit codes are unchanged — remaining errors exit 1,
incomplete inspection exits 2, and a failed plan prints no partial diff.

Output is a standard unified diff with 3 lines of context, one `--- a/<path>` /
`+++ b/<path>` pair per changed file, `/`-separated config-root-relative paths,
and no timestamps, index hashes or `diff --git` line. Hunk numbering, context
merging and the `\ No newline at end of file` marker follow the usual `diff -U3`
convention; that marker is what makes a final-newline-only fix visible, because
the two lines otherwise read the same.

Two details are display-only and change nothing on disk. A CRLF source prints
without its CR (the file keeps CRLF, as above), and a leading BOM is not printed
(again, the file keeps it). Every other control character except TAB is escaped
as `\u{XXXX}`, so a comment cannot smuggle a terminal escape sequence into the
preview. TAB stays literal because a trailing tab is the subject of a V1001 fix.
