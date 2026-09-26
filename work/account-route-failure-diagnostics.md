# Account route failure diagnostics

## Incident evidence

The user reported `The Codex authentication file could not be read.` after quitting the external app. Local receipts recorded `auth_file_unreadable` from 2026-09-26 16:14 through 16:30 UTC. The user then manually replaced or edited shared auth; its modification time was 16:38 UTC. The later readable file and a separate `codex_is_running` refusal do not identify the original cause.

The old Route handler mapped initial read, source-account lookup, source-credential reconciliation, final read, target confirmation, and projection readback failures to the same rejection. It discarded their underlying causes. The original failure cannot be reconstructed from that receipt alone.

## Change

Keep synchronous Route behavior and external-app ownership checks unchanged.

- Report an unenrolled current login as `auth_source_account_unknown`.
- Report source reconciliation or confirmation failures as `auth_credential_conflict`.
- Preserve `auth_file_changed` for a read that detects source replacement.
- Save the failure stage and credential-negative cause in the existing command receipt, in the same transaction as completion.
- Update GPUI and menu-bar rejection decoding and messages.

Do not log tokens, authentication documents, or raw provider responses. Do not automatically replace user authentication data to clear an error.

## Validation

An isolated fixture reproduces the unknown-account and expired conflicting-credential failures with external Codex liveness set to quiescent. It verifies distinct rejections, zero auth projection attempts, and unchanged routing. The account-service test group passed 45 tests. A database test verifies that diagnostics survive restart and command replay returns the original result.

This patch fixes error classification and missing diagnostic evidence. It does not claim a verified root cause or a successful live cross-account switch for the original incident.
