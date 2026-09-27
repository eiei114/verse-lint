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
