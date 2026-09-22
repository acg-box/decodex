# Repository automation retirement

This change removes Managed Repository and built-in GitHub PR/check-run
orchestration. It is based on remote main `79a4d8de0`, separately from the earlier
uncommitted broad refactor.

## Removed scope

- Core repository allocation, admission, Git registration, worktree readiness,
  commit, reconciliation, and operation-assignment state machines and exports.
- The disabled runtime capability, readiness facade, bootstrap construction,
  unused application field, and constructor argument.
- The doctor component and its CLI and desktop presentation.
- The disconnected GitHub provider abstraction, dispatch receipts, PR/check-run
  write/readback implementation, module declaration, and dormant-code exemption.
- Four repository state-machine tests and five GitHub fake-provider tests whose
  owners no longer have a supported execution path.

Reverse scans of 150 retired identifiers found no active consumers. Neither
capability owns a current database table or migration. No user repository,
worktree, branch, credential, conversation, or application database is deleted.
Radar, Publisher, and repository maintenance automation remain independent.

## Preserved contracts

`RepositoryContentRevision` remains in its own module because context and
supervised validation use it. It still preserves exact, nonempty values of at
most 256 UTF-8 bytes and rejects surrounding whitespace and control characters.
Its regression test covers bounds, Unicode byte length, newline, NUL, and exact
roundtrip. This value grants no Git mutation authority.

The doctor component set changes from 19 to 18. Exact local protocol 2.41 replaces
2.40, and negotiation explicitly rejects 2.40. Update the app and service together.
Ordinary working directories, Chief, Codex Conversations, account ownership,
process supervision, and historical data remain supported.

## Documentation

README and current Wiki guidance explicitly retire both capabilities. Obsolete
execution and delivery requirements were removed from the old authority and gate
pages. Frozen baseline inventories and evidence retain captured identities and
are labelled archival; they are not current requirements or generator inputs.

## Validation repairs

The SQLite gate now includes the already-shipped schema-28 migration. Login
source assertions follow the existing `Self` variants and distinguish public
prompt fetching and timestamp formatting from login authority. Repository Rust
and TOML formatting was applied; parsed manifest values are unchanged. Joining
wrapped lines restores the existing automation prompt length limit with identical
words. No validation check was disabled.

Full evidence is retained with the task's local build artifacts. This change does
not install or release an application bundle.

## Final local results

- Rust workspace: 1,551 passed; 19 skipped. Two stdio-leak markers did not recur
  in the serial three-execution recheck; complete logs retain both observations.
- Strict Rust lint: all 12 packages passed.
- Architecture: 22 passed. Gate contracts: 18 passed. Automation contracts: six passed.
- Rust and TOML format checks passed.
- SQLite initialization, validation, and schema-28 readback gate passed.
- Real CLI diagnostics and `git diff --check` passed.
