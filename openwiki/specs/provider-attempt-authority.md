# XY-1401 ProviderAttempt authority

Status: source-only implementation candidate. Executable acceptance is deferred by the core
freeze.

The accepted XY-1401 decision memo
`134844d5-ad31-484f-9409-4591638c966d`, V3 product authorization
`099fb36d-9a48-407e-abdd-80dd56d13051`, V3 skeptic receipt
`02d37443-f3cf-4c11-a462-e3da80c40481`, and XY-1400 implementation authorization
`a0f34b3e-033e-4106-9007-c9b21d23ae57` define this boundary.

## Architecture result

One authority serves both consumer types. V24 uses one `provider_attempts` relation and one
atomic preparation command. The consumer is a closed union:

- one existing Conversation and one reserved exact Turn identity; or
- one ManagedRun revision and one distinct execution identity.

The union does not make a Conversation depend on ManagedRun. It does not give
ProviderAttemptService authority to change either domain. It lets the same external-effect
machine retain one exact consumer reference. A V24 Turn guard rejects later materialization that
does not match the reserved Conversation and RuntimeSession identities. The guard does not write
the Conversation domain. It runs as the migration owner with a fixed search path because runtime
owns Turn DML but has no ProviderAttempt relation privilege. Runtime and PUBLIC cannot execute
the trigger helper directly. Unreserved Turn writes pass the empty lookup.

`ProviderAttemptService` is the sole product writer for external Codex turn attempts.
`ExecutionCoordinator` remains stateless. V16 continues to own account selection. V17
continues to own the accepted existing RuntimeSession or the atomic Context Pack and fallback
RuntimeSession. ProcessSupervisor continues to own ProcessGeneration. ProviderAttemptService
consumes those accepted records. It creates none of them.

This result does not trigger the architecture falsifier. One PostgreSQL transaction can bind
all required authorities without a second writer or coordinator ledger.

## Durable data

Forward-only migration `V24__provider_attempt_authority.sql` adds:

| Relation | Purpose |
| --- | --- |
| `provider_attempts` | Current immutable authority binding and optimistic state. |
| `provider_attempt_positive_evidence` | Append-only positive provider proof for an original attempt. |
| `provider_attempt_transitions` | Append-only state history. |

A prepared row binds these values in one transaction:

- one exact consumer intent;
- one immutable V17 continuation plan and its V16 routing decision;
- the accepted RuntimeSession identity and revision;
- the V16-selected account;
- one live ready ProcessGeneration, its exact ready transition revision, and execution epoch;
- one request identity and SHA-256 request digest;
- at least one provider idempotency or correlation key; and
- an original-intent disposition or one exact unknown predecessor plus a durable
  duplicate-risk acknowledgement digest.

The service never accepts a caller-selected account or RuntimeSession. PostgreSQL derives them
from the accepted V16 and V17 records. It verifies that the ready ProcessGeneration belongs to
that account and that its execution epoch is active. A Conversation consumer reserves a canonical
Turn identity in the accepted RuntimeSession. If the Conversation owner has already materialized
that Turn, it must be active and match the same Conversation and RuntimeSession. This prospective
identity permits V17's accepted fallback RuntimeSession to remain `starting`; XY-1402 owns later
Turn materialization and must consume the reserved identity. A ManagedRun consumer must reference
the exact V17 ManagedRun revision.

One provider idempotency key can belong to only one attempt for an account. Correlation keys are
indexed but are not unique because one provider thread can correlate several turns.

## State authority

The ordinary state machine is:

```text
prepared -> canceled | dispatch_authorized
dispatch_authorized -> succeeded | failed_definitive | not_submitted | unknown
unknown -> succeeded | failed_definitive | not_submitted
```

The last transition group requires positive evidence. `canceled`, `succeeded`,
`failed_definitive`, and `not_submitted` are terminal. An `unknown` attempt has no automatic
retry transition.

Restore has one explicit fail-closed projection. It changes every present `prepared` or
`dispatch_authorized` row to `unknown` with reason `restore_projection`. This exceptional
projection is required because a restored prepared row can be older than a dispatch
authorization that occurred after the backup. It does not authorize dispatch or claim that a
request was submitted.

The preparation and dispatch-authorization commands share the existing restore advisory gate.
The restore projection takes the exclusive gate. Consumer-scoped serialization prevents a new
intent from racing an unknown transition and bypassing duplicate-risk acknowledgement.

## Positive-only reconciliation

The closed evidence sources are:

- exact provider terminal receipt;
- positive idempotency lookup;
- exact turn readback;
- exact thread and turn readback; or
- positive non-submission receipt.

Evidence binds the original attempt, pre-transition attempt revision, request identity, one
retained provider key, outcome, positive identities, and a SHA-256 witness digest. Exact thread
readback must match the accepted RuntimeSession thread. A missing RuntimeSession thread identity
is inconclusive.

There is no timeout, absence, negative-search, exhausted-list, missing-event, process-death,
kqueue, boot-change, EOF, restart, lease-expiry, or row-absence evidence type. Those observations
cannot produce `not_submitted` or another terminal state.

A late positive result updates its original attempt even after the bound ProcessGeneration is
dead. A replacement service reads and reconciles the original row. It cannot recreate the fresh
prepared capability or the one-time dispatch fence, so it cannot replay the request.

The runtime performs the restore projection and one bounded reconciliation pass during startup.
It then continues bounded background passes. Each pass reads at most one page for each
reconcilable state and advances a persistent state-specific cursor, so a large unresolved prefix
cannot exclude later attempts. One evidence-source error affects only that item. The current
composition installs an inconclusive source and also exposes an exact positive receipt operation.
It installs no provider gateway.

## Authority and observability

Runtime has no relation DML on the three V24 relations. It receives enum `USAGE` and only seven
closed `SECURITY DEFINER` entry points: prepare, authorize dispatch, cancel, mark unknown,
record positive evidence, restore projection, and bounded read.
The Turn-reservation trigger is one additional migration-owner `SECURITY DEFINER` function.
Its fixed search path and direct-execution revokes let runtime mutate Turns without receiving
ProviderAttempt relation access.

Current diagnostics expose consumer identity, V17 plan, V16 decision, accepted RuntimeSession
and revision, selected account, bound ProcessGeneration revision and execution epoch, request
identity, key-presence flags, state, reason, evidence identity, revision, and update time.
Provider keys and request digests are not representable in the diagnostic type. Core and
PostgreSQL debug projections redact exact provider keys.

The fresh dispatch fence contains no provider transport, credentials, request bytes, retry
operation, or public consumer. The current `CodexAdapter` remains unavailable. No protocol, CLI,
scheduler, UI, remote-auth, or live provider-effect path can consume the fence.

## V12 and XY-1402 boundary

V24 does not consult V12 submitted-turn receipts, safety inputs, or effect barriers to establish
attempt submission or outcome. ProviderAttempt is the only external-turn authority after this
cutover. The existing V12 relations can remain as inert historical ManagedRun data until XY-1402.
They are not a second ProviderAttempt ledger.

XY-1402 owns consumer integration and V12 retirement. It can adapt the currently
ManagedRun-specific V16 and V17 inputs for ordinary Conversation lineage. It must not move
attempt persistence into ManagedRun, add a submitted-turn receipt, add coordinator persistence,
or let ProviderAttempt create routing decisions or RuntimeSessions.

## Deferred acceptance matrix

The integrated core is not frozen. XY-1401 runs no executable validation. The later unified gate
must run these cases against the exact committed tree:

| Boundary | Deferred acceptance cases |
| --- | --- |
| Rust quality | Run repository formatting, compile, static analysis, lint, documentation, unit, integration, and nextest gates on supported hosts. |
| Migration | Prove clean V1-to-V24 initialization and V23-to-V24 forward upgrade. Prove exact migration ledger identity and checksum. |
| Manifest refreeze | Capture PostgreSQL 18 source S0, restore R1, and second restore R2. Require S0=R1 and R1=R2. Regenerate and accept the complete V24 schema and configured-authority digests. Remove the temporary `process_generation` and `provider_attempt` exclusions. |
| Catalog and ACL | Prove the expected 84 relations, 184 functions, 75 safety functions, 154 triggers, 69 runtime-callable functions, ten post-V22 enums, 60 security definers, exact ownership, no PUBLIC authority, no runtime relation DML, no grant option, closed dependencies, hostile `search_path`, overload rejection, direct Turn writes with trigger-only reservation reads, and populated restore parity. |
| Atomic binding | Exercise both consumer shapes, every partial or cross-linked shape, stale V16/V17/RuntimeSession/generation/epoch authority, transaction rollback, lost response, replay, changed-intent conflict, and concurrent preparation. |
| State machine | Exercise each ordinary transition and the restore-only projection. Reject every other transition, immutable-authority rewrite, stale revision, history rewrite, delete, and truncate. |
| Positive evidence | Exercise every positive source and outcome shape, exact request/key/thread binding, evidence replay, late success, late definitive failure, positive non-submission, evidence-ID conflict, and malformed witness. |
| Negative observations | Prove that process death, kqueue, boot change, EOF, timeout, restart, lease expiry, row absence, missing events, exhausted lists, and negative search cannot produce a terminal attempt state. |
| Replacement | Prove startup projection, replacement reconciliation without replay, original-attempt attribution after generation death, and persistence across repeated restart and restore. |
| Duplicate risk | Exercise one unknown predecessor, exact acknowledgement, an unrelated predecessor, multiple unresolved predecessors, concurrent unknown transition, a distinct successor intent, and no mutation of the original unknown attempt. |
| Background progress | Prove bounded paging, repeated passes, item-error isolation, control drop, unrelated-account progress, and no automatic retry. |
| Production isolation | Prove no live dispatch consumer, available Codex adapter, protocol/CLI/UI writer, routing or RuntimeSession creation, second ledger, coordinator persistence, credential path, remote-auth path, provider effect, or enabled dispatch flag. |
| XY-1402 handoff | Prove that Conversation and ManagedRun consume the same attempt authority and that retained V12 turn ledgers are not authoritative or writable through a second product path. |

No formatter, compiler, build, static check, migration or SQL parser, migration execution, test,
fixture, validation wrapper, generator, service, VM, UI or accessibility check, live Codex
operation, account operation, or provider effect is part of this source-only candidate. The only
test-file changes allowed in this slice are existing authority-manifest inventory updates required
to describe V24.
