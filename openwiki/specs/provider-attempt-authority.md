# ProviderAttempt Authority

Status: normative current external-turn effect authority. Candidate-5 initial-thread
binding is an accepted target and remains subject to its integrated gate.

ProviderAttempt is a current domain integrity record. It is not a schema-history ledger,
bootstrap receipt, or database migration mechanism. The one latest schema creates its
final objects directly.

## Owner boundary

`ProviderAttemptService` is the sole product writer for external Codex turn attempts.
One authority serves two closed consumer shapes:

- `conversation_turn`: one existing Conversation plus one exact reserved Turn identity;
  or
- `managed_run_execution`: one exact ManagedRun revision plus one distinct managed
  execution identity.

The union does not make Conversation depend on ManagedRun. ProviderAttemptService cannot
change either consumer domain. It retains one exact consumer reference while Conversation
and ManagedRun keep their own lifecycle authority.

A Turn-reservation trigger rejects later materialization that does not match the reserved
Conversation and RuntimeSession. It does not write Conversation state. The final trigger
helper has a fixed safe search path and runs with schema-owner authority because runtime
owns Turn DML but has no ProviderAttempt relation privilege. Runtime and PUBLIC cannot
execute that helper directly. An unreserved Turn follows the ordinary Conversation path.

Routing Decision remains the sole account selector. Continuation Plan remains the sole
owner of an accepted existing RuntimeSession, first RuntimeSession, or atomic Context Pack
and fallback RuntimeSession. ProcessSupervisor remains the ProcessGeneration owner.
ProviderAttemptService consumes these accepted records and creates none of them.

ExecutionCoordinator remains stateless. It adds no attempt relation, receipt, lifecycle,
or second provider-effect ledger.

## Durable data

The latest schema defines:

| Relation | Purpose |
| --- | --- |
| `provider_attempts` | Current immutable authority binding and optimistic state. |
| `provider_attempt_positive_evidence` | Append-only positive provider proof for the original attempt. |
| `provider_attempt_transitions` | Append-only state history. |

One preparation transaction binds:

- one exact consumer intent;
- one immutable Continuation Plan and its Routing Decision;
- the accepted RuntimeSession identity and revision;
- the selected account;
- one live ready ProcessGeneration, exact ready revision, and active execution epoch;
- one request identity and SHA-256 request digest;
- at least one provider idempotency or correlation key; and
- original-intent disposition, or one exact unknown predecessor plus a durable
  duplicate-risk acknowledgement digest.

The service never accepts a caller-selected account or RuntimeSession. former server store derives
both from accepted route and plan records, then verifies that the ready generation belongs
to that account and its execution epoch is active.

A Conversation consumer reserves a canonical Turn identity in the accepted
RuntimeSession. If Conversation authority already materialized that Turn, the row must be
active and match the same Conversation and RuntimeSession. A ManagedRun consumer must
reference its exact accepted revision and managed execution identity.

One provider idempotency key can belong to only one attempt for one account. Correlation
keys are indexed but not unique because several turns may use one provider thread.

## State authority

The ordinary state machine is:

```text
prepared -> canceled | dispatch_authorized
dispatch_authorized -> succeeded | failed_definitive | not_submitted | unknown
unknown -> succeeded | failed_definitive | not_submitted
```

`canceled`, `succeeded`, `failed_definitive`, and `not_submitted` are terminal. Every
terminal transition after dispatch authorization requires positive evidence. `unknown`
has no automatic retry transition.

Restore has one fail-closed projection. It changes every present `prepared` or
`dispatch_authorized` row to `unknown` with reason `restore_projection`. A restored
prepared row can be older than authorization that happened after the restored point.
The projection therefore grants no dispatch and makes no non-submission claim.

Preparation and dispatch authorization share the restore advisory gate. Restore
projection takes the exclusive gate. Consumer-scoped serialization prevents a new intent
from racing an unknown transition and bypassing duplicate-risk acknowledgement.

## Positive-only reconciliation

The closed positive evidence sources are:

- exact provider terminal receipt;
- positive idempotency lookup;
- exact Turn readback;
- exact thread and Turn readback; or
- positive non-submission receipt.

Evidence binds the original attempt, pre-transition revision, request identity, one
retained provider key, outcome, positive identities, and SHA-256 witness digest. Exact
thread readback must match the accepted RuntimeSession thread. Missing thread identity is
inconclusive.

Timeout, absence, negative search, exhausted list, missing event, process death, kqueue,
boot change, EOF, restart, lease expiry, and row absence are not evidence kinds. They
cannot produce `not_submitted` or another terminal state.

A late positive result updates the original attempt after generation death. A replacement
service can read and reconcile that row. It cannot recreate the fresh prepared capability
or one-time dispatch fence and therefore cannot replay the request.

Runtime performs restore projection and one bounded reconciliation pass during startup.
The server lifecycle then owns bounded background passes until shutdown. Each pass pages
each reconcilable state and advances a persistent state-specific cursor so an unresolved
prefix cannot starve later attempts. One evidence-source error affects only that item.

## Candidate-5 initial thread

Candidate-5 preparation and dispatch authorization add one exact `initial_thread`
binding. The transaction must lock and require:

- the selected initial Routing Decision and its complete zero-source lineage;
- the inert first-session Continuation Plan;
- selected account and copied RoleProfile snapshots;
- the ready ProcessGeneration revision and live execution epoch;
- the exact completed RuntimeSession thread fence and bind receipts;
- the exact post-bind `active` RuntimeSession revision; and
- the selected Turn under that Conversation/session, still `active` at revision 1.

The thread-binding protocol and idempotency-key identities are immutable attempt fields.
Every non-initial plan keeps those fields absent and retains its existing same-thread,
Context Pack, ManagedRun, predecessor, and positive-lineage predicates.

The dispatch-authorization command must perform the same exact Turn lock itself. A
deferred trigger cannot replace the pre-write lock. An absent, terminal,
changed-revision, wrong, or cross-linked Turn fails before provider dispatch.

Lost results at prepare or authorize remain bound to the original Turn and attempt.
They cannot create a duplicate attempt, terminalize the Turn, or authorize replay.

## Authority and observability

Runtime has no relation DML on ProviderAttempt relations. It can execute only the closed
prepare, authorize, cancel, mark-unknown, positive-evidence, restore-projection, and
bounded-read functions. PUBLIC has no ProviderAttempt authority. The trigger-only Turn
helper is not directly executable.

Diagnostics expose consumer identity, plan, route decision, RuntimeSession/revision,
selected account, ProcessGeneration/revision/epoch, request identity, key-presence flags,
state, reason, evidence identity, revision, and update time. Provider keys and request
digests are not representable in the diagnostic type. Debug projections redact exact
provider keys.

The fresh dispatch fence contains no provider transport, credentials, request bytes,
retry operation, or public consumer. No protocol, CLI, scheduler, UI, remote-auth, or
client path can obtain it before a separately accepted dispatch composition exists.

## Consumer isolation

Conversation and ManagedRun consume the same attempt authority. ManagedRun does not copy
provider-effect state into a submitted-turn receipt, effect barrier, or second ledger.
ProviderAttempt does not create routing decisions, Continuation Plans, RuntimeSessions,
Turns, or ManagedRun lifecycle transitions.

Quick Task remains an ordinary Conversation. A ManagedRun remains the owner of execution
assignment, wait state, review, acceptance, and completion. Provider outcome alone cannot
synthesize ManagedRun or WorkItem acceptance.

## Acceptance

After source freeze, validation must cover:

- fresh former server store 18 latest-schema bootstrap and second-bootstrap refusal;
- exact ProviderAttempt catalog, function, trigger, ownership, ACL, dependency, and
  negative PUBLIC/runtime authority;
- both consumer shapes and every partial, stale, cross-linked, or ambiguous binding;
- atomic prepare/authorize behavior, rollback, lost response, exact replay,
  changed-intent conflict, and concurrency;
- every legal state edge and rejection of illegal transitions, identity rewrites,
  deletes, truncation, and history rewrites;
- every positive evidence source, evidence replay, late result, positive
  non-submission, and malformed witness;
- rejection of every negative observation as terminal evidence;
- restore projection, replacement reconciliation without replay, duplicate-risk
  acknowledgement, and bounded background progress;
- Candidate-5 exact route/plan/process/thread/Turn binding; and
- reverse scans proving no second ledger, credential path, provider replay, remote path,
  or unauthorized production dispatch root.

No historical upgrade, numbered-schema, schema-ledger, or migration proof is part of
acceptance.
