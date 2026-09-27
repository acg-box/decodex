# Preserve pasted indentation

The inherited composer accepted Tab characters. The current input filter removed
those characters from ordinary pasted text and rejected native text edits that
contained them. Restore Tab in the existing control-character filter. Keep CRLF
normalization, UTF-8 byte limits and filtering of other control characters.

## Reconciliation

Compare the complete preserved and current `composer_input.rs` files. Retain the
current native prompt owner, marker-safe edits, the native editor size limit,
composition-state query and native-state invalidation on ordinary set and clear.
The removed key-repeat comment has no behavior change. Restore the inherited
paste, undo, redo and wire-text test. Close only this file's review row.

The existing native editor test now inserts a Tab before the text marker and
checks its byte offset, undo, redo, extension metadata and rejected marker edit.
This repair does not add an optional feature or another input owner.

## Evidence

Before the filter repair, the restored test failed: `界\tfirst` became `界first`
and indentation on the next line was also removed. The failure log is
`/tmp/decodex-composer-tabs-before.log`. All 14 focused composer tests pass, including ordinary paste and native marker
editing. Strict GPUI lint passes with all features and targets. Final logs are
`/tmp/decodex-composer-tabs-final.log` and
`/tmp/decodex-composer-tabs-clippy.log`.
Signed desktop acceptance remains part of the final catch-up pass.
