# Large native approval storage

Classification: core compatibility for native approval requests. This is the
storage batch of the complete approval reader and reply adaptation.

## Source and authority

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Commit `9c4879f3a5bfdc7fb5401b7d3ebcdc9a77c27aa2` preserves complete Guardian
actions and sends optional review budget failures to explicit user approval.
Native review policy and execution remain in Codex.

The fixed CommandExecutionRequestApprovalParams schema and the schema generated
from installed Codex 0.155.0-alpha.16.4 both expose the complete command and exact
thread, turn and item identities. The existing Chief transport accepts 8 MiB
frames. This batch names that existing bound in decodex-core and reuses it for
transport and approval storage. Ordinary conversation transport keeps its own
existing bound.

SQLite source authority is versioned-first in database/migrations. Migration39
adds chief_request_payloads under the existing store migration owner. It does not
change old migrations, rewrite inbox records or mutate the user's live database.
The inherited migration41 is adapted to the current sequence.

## Persistence behavior

Only large command, file, permission and MCP elicitation approvals can use the
complete payload table. Other event kinds retain the 64 KiB bound. The complete
JSON must fit within 8 MiB of UTF-8 bytes. The immutable payload references its
exact inbox event with a cascading foreign key.

Inbox scans retain ID, method, thread, turn, item and owner routing fields. They do
not load large command text or schemas. Exact event reads hydrate the complete
original envelope. Repeated insertion compares complete bytes, including action
suffixes. Compact metadata and complete content commit in one transaction.

## Validation scope

Database fixtures exercise upgrades from schema38, existing inbox preservation,
foreign keys, JSON validity, UTF-8 size limits and immutable payloads. Store tests
exercise rollback on detail-write failure, compact scans, complete reads after
reopen, duplicate replay, changed-content conflict and event-kind restrictions.
A coordinator fixture receives a 300 KB Unicode command and large MCP approval,
then verifies complete original params and cold database readback.

## Remaining delivery

The bounded query in this storage batch returned unavailable for oversized
selected content. [The page transport batch](large-native-approval-pages.md) adds
identity-bound pages, native liveness and complete client assembly. Large explicit
decisions and desktop inspection controls remain. Pending native file-change
evidence must also be retained before native history contains it.

This storage batch does not establish complete large-approval UI, accept/decline
or signed desktop acceptance. Scheduled automation remains paused.

Validation results: 127 database unit tests passed. The dedicated coordinator
fixture passed; the runtime request regression selection passed 26 tests with
three explicit opt-in skips. Strict core, adapter, database and runtime Clippy
passed for all targets and features. No new installed-native or signed desktop
acceptance run was performed for this storage batch.
