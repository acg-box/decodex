# Ordinary native settings observations

Classification: data preservation for the optional, currently unexposed ordinary
History workspace. This feature does not add a workspace entry point. Current
Chief model settings keep their existing owner.

## Missing inherited behavior

The preserved snapshot retained native model, provider, directory and effort from
successful ordinary thread start and resume responses. It stored these facts with
an exact session, thread, process generation and account source. Current main
removed the response provider projection, observation table, service persistence
and public summary projection. Existing model inheritance does not replace this
observation path.

The fixed upstream commit `595cc91e8cbb1c2ca822d0311dcf12709410c582` defines
`model_provider` on `ThreadStartResponse` and `ThreadResumeResponse` in
`app-server-protocol/src/protocol/v2/thread.rs`. Generated schemas from installed
Codex 0.158.0-alpha.2 also require a string `modelProvider`. Retain the top-level
current-session provider; the nested thread metadata can describe its original
provider. A provider label is not authentication or submission authority.

## Adapter and store foundation

Restore bounded provider facts in the existing typed response adapter. Tests
cover different current and historical provider labels, empty and oversized
labels, controls and malformed wire values. Preserve current model, effort and
service-tier inheritance APIs.

The database remains versioned-first under `database/migrations`, registered by
`database/src/migrations.rs` and applied by the existing `SqliteStore` owner.
Migration 47 restores the inherited additive observation table. Existing migration
38 and all applied history remain unchanged. There is no backfill or live database
mutation in this delivery.

The store accepts a response only while the exact session, thread, ready process
and account revision still own it. Response IDs order observations within a
process. An identical response is idempotent; conflicting or older responses
cannot overwrite it. A replacement process requires recorded death evidence for
the previous process. The saved user request stays unchanged. The effort bound is
128 bytes, consistent with the current conversation protocol.

Database readback exposes the last observation as historical data. Presence does
not imply that its process remains live. The restart test checks identity and
revision rejection, response ordering, idempotency, original request retention
and durable readback. A migration test checks upgrade from schema 46, unchanged
existing schema and preferences, an empty observation table and repeated startup.

## Remaining delivery

The process gateway must pass these response facts to the runtime. The runtime
must record them after exact start/resume binding and project them through the
public summary and ordinary History display. Restore and adapt those inherited
consumers in the next batch. This foundation alone does not close R03, the shared
file audit or desktop acceptance. Do not count it as complete end-to-end behavior.
