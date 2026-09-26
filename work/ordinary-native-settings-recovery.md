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

## Service and display delivery

The process gateway now passes observed settings from successful typed start and
resume responses. The runtime records start observations only after the exact
thread binding succeeds, before admitting inference. Resume observations must
pass the same source checks before the response grants continuation authority.
A failed observation write retains the existing recovery path.

Protocol 2.91 exposes the last observation and the saved original directory in
`ConversationSummary`. Durable and live projections use the same validation.
An observation cannot attach to an unbound conversation, and mismatched local
thread identity is rejected. Current native settings queries remain separate.

The ordinary History context inspector displays model, provider, directory and
effort as last-read facts. Missing observations are explicit. A later submission
uses the selected task's native directory, or its saved original directory when
no observation exists. Missing or unusable directories do not fall back to the
application directory. Runtime validation still owns execution admission.
Model, effort, tier and draft choices are unchanged by observation updates.

Local tests cover wire validation, durable/live projection, directory selection,
stale observation rejection, and the rendered inspector without changing drafts
or execution choices. The installed-native confirmation fixtures also verify
observations in live publications and after reopening the store, while retaining
exactly one inference despite repeated confirmation. These tests use isolated
homes and a synthetic local provider. A separate native runtime fixture checks
creation, same-thread continuation and continuation after service restart. Each
finished turn publishes the expected native model, provider and directory; four
requests correspond to the original fixture input and three explicit runtime
submissions. Metadata reads and recovery do not replay inference.

The ordinary History workspace remains optional and unexposed by current normal
navigation. The rendered GPUI fixture is not signed desktop acceptance. No new
navigation, application installation or public release is part of this change.
R03, remaining shared-file reconciliation and the final desktop boundary remain
open until their separate exit conditions are resolved.
