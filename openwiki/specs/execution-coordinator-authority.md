# Stateless Execution Coordination

Status: normative current owner composition plus the accepted Candidate-5 Quick Task
target. Candidate-5 implementation and integrated acceptance remain pending.

The coordinator is not a schema owner. Its domain records are not schema migration
records. The one latest schema contains only final current relations and omits the retired
ManagedRun-local submitted-turn/effect-barrier machinery.

## Owner composition

`ExecutionCoordinator` is a zero-sized, stateless sequencer. It does not persist state or
own a lifecycle, retry machine, receipt, effect barrier, ambiguity ledger, account
decision, RuntimeSession decision, process state, or ProviderAttempt state.

| Owner | Sole authority |
| --- | --- |
| Conversation authority | Ordinary Conversation and Turn lifecycle, including Candidate-5 atomic first Turn/history admission and legal Turn finalization. |
| ManagedRun authority | ManagedRun lifecycle, execution assignment, wait state, review, acceptance, and completion. |
| Routing Snapshot | Complete account universe, policy order, account/evidence revisions, capability/quota facts, and blocker completeness. |
| Routing Decision | Sole account selection, exact cause projection, and immutable route result. |
| Continuation Plan | First-session planning, exact same-thread reuse, atomic Context Pack plus fallback RuntimeSession, and PostgreSQL-only explicit-successor evidence. |
| RuntimeSession Thread Establishment | Exact thread request fence, start binding, activation, and acknowledgement. |
| ProcessSupervisor | ProcessGeneration intent, live fence, positive-only death evidence, and account-local quarantine. |
| ProviderAttemptService | Atomic attempt binding, dispatch authorization, provider-effect state, positive evidence, restore projection, and reconciliation. |
| Reviewer | Execution-scoped read-only review; missing or ambiguous output grants no approval or completion. |
| ExecutionCoordinator | One in-memory sequence across accepted owners; no independent authority. |

The coordinator consumes typed results in owner order and returns an inert projection
unless a separately accepted production dispatch root consumes the complete fence chain.
It cannot reconstruct a fresh fence from durable readback.

## Closed execution consumer

Routing, continuation, and ProviderAttempt use one closed consumer union:

- `conversation_turn` binds Conversation identity/revision, optional source
  RuntimeSession identity/revision, and one prospective or existing Turn identity; and
- `managed_run_execution` binds ManagedRun identity/revision and one distinct managed
  execution identity.

The Conversation variant contains no ManagedRun identity. Quick Task remains an ordinary
multi-turn Conversation. Conversation authority materializes its reserved Turn only after
the accepted first-session plan or for an existing-session intent. ProviderAttempt cannot
create or rewrite a Turn or RuntimeSession.

The ManagedRun variant does not move ManagedRun lifecycle authority. Its read model can
consume ProviderAttempt projections, but it cannot copy provider-effect authority into a
second ledger or synthesize acceptance.

## Final latest-schema cut

The latest schema includes the final closed route and wait vocabulary and the final
consumer-generic routing, continuation, and ProviderAttempt definitions directly. It does
not contain the retired ManagedRun-local submitted-turn receipts, safety inputs, effects,
or barriers. It contains no compatibility or fallback writer for those shapes.

There is no data conversion or historical backfill. Current local data is disposable. If
old local state cannot satisfy the latest schema, the operator replaces the local
database or directly transforms that one development database under the reviewed local
action. Product runtime never interprets an old shape.

The latest schema retains sole writers:

- Routing Decision for account choice and route causes;
- Continuation Plan for RuntimeSession continuation/creation;
- ProcessSupervisor for ProcessGeneration;
- ProviderAttemptService for provider effects; and
- Conversation or ManagedRun for their own lifecycle.

## Route projection

Routing Decision loads and locks the complete Routing Snapshot. The caller supplies no
account list, policy order, eligibility, exclusion, or account choice.

Selection and pure-wait classification use only independently eligible included members.
Policy-excluded members never become eligible and do not participate in quota or
reconciliation wait classification. A `no_route` projection uses the complete persisted
policy-member universe. Every excluded member keeps `excluded_by_policy` plus every other
blocker. Every included member keeps its exact blockers. An all-excluded universe is a
cause-complete `no_route`; cause-free `no_route` is invalid.

The 300-minute and 10,080-minute quota facts remain independent. Each retains duration,
source identity, observation revision, exact raw timestamp, exact microsecond value,
confidence, and reset instant. Routing classifies both again at its database-authored
decision instant. A fact that ages after snapshot creation keeps the exact stale or
reset-elapsed cause.

Sticky affinity applies only after independent account, capability, quota, process, and
attempt eligibility. Route results are:

- `selected`: one independently eligible account;
- `waiting_usage`: every otherwise eligible account is blocked only by current positive
  quota depletion; includes complete depletion causes and earliest ready instant;
- `waiting_reconciliation`: every otherwise eligible path is blocked only by unresolved
  ProcessGeneration or ProviderAttempt state; and
- `no_route`: complete mixed or non-wake causes; it grants no wake and does not fail the
  task by itself.

Authentication, plugin, disabled account, unknown capability, dependency, approval,
user, external, and Reviewer causes do not become quota or reconciliation causes. Missing
quota provenance becomes `usage_unproven`. Reconciliation without an exact unresolved
process or attempt becomes `reconciliation_unproven`.

An unresolved process plus positive depletion is mixed `no_route`, not a quota wake.
Separate account quotas are never pooled, merged, summed, averaged, or caller-ranked.

## Candidate-5 initial operation

Candidate 5 adds no coordinator state or new owner. The exact order is:

1. Conversation authority creates the Conversation.
2. The routing request supplies one prospective Turn UUID as intent only. No Turn row or
   Turn foreign key exists yet.
3. Routing Snapshot proves the exact initial zero-source lineage and complete locked
   account universe.
4. Routing Decision selects one account exactly once.
5. Continuation Plan atomically creates selected account/profile snapshots, the first
   revision-1 unfenced `starting` RuntimeSession, inert `initial_thread` plan, exact
   receipt, activity, and outbox.
6. Conversation authority atomically admits the exact active revision-1 sequence-1 user
   Turn and exactly one ordinal-0 completed Message.
7. Account Service fences only the selected account immediately before spawn.
8. ProcessSupervisor can spawn only from a fresh ProcessGeneration outcome.
9. RuntimeSession Thread Establishment fences, starts, binds, and activates the exact
   thread.
10. ProviderAttemptService prepares and authorizes the exact attempt.

There is no second route decision, alternate account, initial fallback, wake, or account
re-selection. Initial planning has no source RuntimeSession and no sticky member. It is
first-session creation, not same-thread reuse, explicit successor, or Context Pack
fallback.

### Initial lineage

`L0` has all six RuntimeSession, account-snapshot, and profile-snapshot
identity/revision fields absent. `L6` has all six present with positive revisions.
Conversation work permits `L0` or `L6`; ManagedRun work permits only `L6`.

The final schema keeps all foreign keys and one exact all-null/all-present check. `L0`
requires an open exact-revision Conversation, no RuntimeSession or Turn, zero sticky
members, and exact complete locked membership/order/account/two-window quota/eight
capability/blocker facts. Partial lineage, source fields, sticky `L0`, existing session or
Turn, closed/stale Conversation, source-less ManagedRun, and missing, extra, duplicate,
cross-linked, or reordered evidence reject.

One Conversation lock permits one cross-key initial decision to be fresh. Exact-key replay
is read-only. Continuation planning accepts only the selected `L0` result and rolls back
the complete first-session authority cluster on any failure.

### Initial admission

Conversation authority creates the first Turn and history item in one one-winner
transaction. Required Turn values are the prospective UUID, sequence 1, `user`,
`possible_side_effects=unknown`, `active`, revision 1, and the exact Conversation/new
RuntimeSession cross-link. The only first history item is ordinal 0, Message, `completed`,
revision 1.

The fresh/replay/refusal result, Turn, history, receipt, activity, and outbox are one
atomic owner result. A competing key commits none of those effects. Wrong identity,
sequence, role, status, revision, side-effect state, history ordinal/kind/status, second
item, or cross-link rejects the complete transaction.

### Exact fences and replay

Every process, thread, and provider-effect owner locks and rechecks the exact selected
Turn as active revision 1. ProcessGeneration and thread establishment through bind require
the applicable `starting` RuntimeSession revision. ProviderAttempt preparation and
authorization require the exact post-bind `active` revision plus exact thread fence/bind
receipts.

Only a fresh ProcessGeneration outcome can spawn. Replay, rejection, and uncertainty use
durable ProcessGeneration, RuntimeSession, ProviderAttempt, and Conversation readback.
They cannot spawn, replace, adopt, create a successor, duplicate an attempt, or
terminalize the Turn.

Conversation authority can move the Turn to failed revision 2 under a starting session
only after positive proof of definite pre-effect refusal. The proof excludes every process
state that may have created a child, thread fence/start/bind, and prepared, authorized, or
unknown ProviderAttempt. Ambiguous work remains active and returns `Unknown`.

Explicit successor remains PostgreSQL-only non-dispatch evidence. Before any write it
locks the Turn named by the selected route and requires the same Conversation/source
RuntimeSession, `failed`, revision 2. It has no protocol field, command, runtime grant,
facade, fallback, or wake path.

## RuntimeSession continuity

For an existing ordinary Conversation session, same-thread reuse requires positive exact
thread evidence from the original ProviderAttempt. Evidence must bind the same
Conversation, source RuntimeSession, account, and Codex thread.

For ManagedRun, same-thread reuse requires the accepted causal experiment and positive
exact-thread attestation bound to the source RuntimeSession.

When accepted existing-session same-thread evidence is absent, stale, negative,
incomplete, ambiguous, or cross-linked, Continuation Plan uses its atomic Context Pack
path. It creates the Context Pack, account snapshot, starting fallback RuntimeSession,
plan, activity, outbox, and exact receipt as one authority cluster.

Candidate-5 initial planning cannot enter either existing-session branch. Continuation
Plan never writes ProviderAttempt state, and ProviderAttempt never writes RuntimeSession
state.

## Effect ambiguity and isolation

An authorized ProviderAttempt remains `unknown` until positive evidence proves a terminal
result. Process death, timeout, missing events, EOF, restart, negative search, and absent
rows do not prove non-submission and do not authorize replay.

Late positive evidence remains bound to the original attempt. A replacement process can
reconcile that attempt but cannot replay it.

An attempt blocks only its exact Conversation Turn or managed execution. Process
uncertainty blocks only the affected account path. Other consumers, accounts,
dependencies, and routes remain independently eligible.

## Transport and production isolation

The exact-current protocol may expose one read-only immutable execution-decision query.
It returns the closed consumer, route kind, complete exact causes, and independent quota
exclusions. It cannot create a route, RuntimeSession, process, ProviderAttempt, wake,
retry, receipt, or dispatch fence.

The service uses the owner-only same-UID Unix transport. It has no TCP or loopback
fallback. Remote and cross-UID authority remains separately gated.

The coordinator method remains crate-private and accepts only crate-private owner
capabilities. No protocol, CLI, scheduler, UI, credential owner, Codex adapter, or client
can construct the sequence or mint dispatch authority. Product dispatch remains disabled
until the Candidate-5 gate accepts the complete composition.

## Acceptance

After source freeze, validation must cover:

- fresh PostgreSQL 18 latest-schema bootstrap, second-bootstrap refusal, and runtime-only
  daemon startup with zero DDL and no schema-owner credential;
- exact current catalog/authority for all routing, continuation, Conversation,
  ProcessGeneration, ProviderAttempt, and read-only projection objects;
- both consumer shapes and rejection of every partial or cross-linked identity;
- complete route universe, all-excluded and mixed policies, cause-free rejection,
  independent quota facts, aging, sticky eligibility, and exact pure-wait rules;
- Candidate-5 `L0`/`L6`, sole account selection, atomic first-session cluster, atomic
  first Turn/history admission, exact effect fences, fresh/replay/reject/unknown behavior,
  and definite pre-effect refusal;
- existing-session same-thread and atomic Context Pack paths;
- positive-only process/attempt reconciliation, late evidence, smallest-scope isolation,
  and no replay after lost supervision;
- ManagedRun and Reviewer authority isolation;
- stateless coordinator layout and restart behavior;
- same-UID transport and complete read-only protocol projection; and
- reverse scans proving no old barrier writer, second ledger, alternate selector,
  unauthorized dispatch root, or schema upgrade machinery remains.

No historical upgrade, populated cutover, schema-ledger, migration, or restore-digest
proof is part of acceptance.

## Architecture falsifiers

Stop and return to the authority owner if:

- ordinary Conversation execution requires a ManagedRun;
- ProviderAttempt must create or rewrite RuntimeSession state;
- Continuation Plan must write ProviderAttempt state;
- ExecutionCoordinator must persist authority;
- the retired ManagedRun effect path requires a second live or compatibility writer;
- mixed route causes must collapse into a pure wait;
- lost supervision must authorize replay without positive evidence; or
- Candidate-5 requires a second account selector, pre-admission Turn row, generic
  recovery framework, or alternate dispatch owner.
