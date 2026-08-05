# Stateless Execution Coordination

Status: normative current owner composition plus the accepted Candidate-5 Quick Task
target. Candidate-5 implementation and integrated acceptance remain pending.

The coordinator is not a schema owner. Its domain records are not schema migration
records. The one latest schema contains only final current relations and omits the retired
ManagedRun-local submitted-turn/effect-barrier machinery.

## Owner composition

Execution coordination is stateless. Runtime `routing_orchestration.rs` owns only the
owner-order sequences from routing Conversation successor to initial route, route to
initial plan, `resume_establishment`, and continuation binding to plan. Runtime
`quick_task.rs` owns the remaining Quick Task lifecycle and process, thread, and provider
execution. Neither module persists routing authority or owns a receipt, retry machine,
effect barrier, ambiguity ledger, account decision, RuntimeSession decision, process
state, or ProviderAttempt state.

| Owner | Sole authority |
| --- | --- |
| PostgreSQL Conversation authority | Ordinary Conversation and Turn lifecycle, one-winner routing Conversation successor creation/archive/readback, Candidate-5 atomic first Turn/history admission, legal Turn finalization, and Conversation/Turn/history readback. |
| ManagedRun authority | ManagedRun lifecycle, execution assignment, wait state, review, acceptance, and completion. |
| PostgreSQL `quick_task_routing` | One atomic initial Quick Task snapshot/decision operation with exact receipt, codecs/readback, and one immutable non-selecting continuation binding; no independently committable half-command. |
| ManagedRun routing | Closed `managed_run_project_policy` `L6` facts and the sole ManagedRun selector; unchanged by the Quick Task reset. |
| `decodex-core` routing | Bounded I/O-free pure selection kernel only; no lock, persistence, provenance, receipt, completeness, retry, or lifecycle authority. |
| Continuation Plan | First-session planning from an already selected decision plus ordinary Quick Task same-thread or Context Pack planning from an already persisted continuation binding; PostgreSQL-only Explicit successor evidence. |
| RuntimeSession Thread Establishment | Exact thread request fence, start binding, activation, and acknowledgement. |
| ProcessSupervisor | ProcessGeneration intent, live fence, positive-only death evidence, and account-local quarantine. |
| ProviderAttemptService | Atomic attempt binding, dispatch authorization, provider-effect state, positive evidence, restore projection, and reconciliation. |
| Reviewer | Execution-scoped read-only review; missing or ambiguous output grants no approval or completion. |
| Runtime `routing_orchestration.rs` | Owner-order routing and planning sequences; no independent authority. |
| Runtime `quick_task.rs` | Quick Task lifecycle plus process/thread/provider execution; no routing persistence. |
| PostgreSQL `exact_commands.rs` | Exact receipt and error primitives only; no generic transaction or workflow framework. |

The runtime modules consume typed results in owner order and return an inert projection
unless a separately accepted production dispatch root consumes the complete fence chain.
They cannot reconstruct a fresh fence from durable readback.

The deletion test is normative. Deleting PostgreSQL `quick_task_routing` must spread its
atomic route transaction, codecs/readback, and continuation-binding invariants into
callers. Deleting runtime `routing_orchestration.rs` must spread its owner-order recovery
sequencing into `quick_task.rs` or other callers. A file whose deletion requires only
forwarding changes is wrapper plumbing, not an accepted owner.

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
consumer-specific routing, continuation, and ProviderAttempt definitions directly. It does
not contain the retired ManagedRun-local submitted-turn receipts, safety inputs, effects,
or barriers. It contains no compatibility or fallback writer for those shapes.

There is no data conversion or historical backfill. Current local data is disposable. If
old local state cannot satisfy the latest schema, the operator replaces the local
database or directly transforms that one development database under the reviewed local
action. Product runtime never interprets an old shape.

The target retains sole writers:

- PostgreSQL `quick_task_routing` for the indivisible initial Quick Task snapshot and
  decision, exact route readback, and non-selecting Quick Task continuation binding;
- ManagedRun routing for its retained Project-policy selection;
- PostgreSQL Conversation authority for the routing Conversation successor, archive, and
  redirect/list readback;
- Continuation Plan for RuntimeSession continuation/creation from committed routing
  authority;
- ProcessSupervisor for ProcessGeneration;
- ProviderAttemptService for provider effects; and
- Conversation or ManagedRun for their own lifecycle.

## Route projection

Routing Snapshot and Routing Decision expose closed consumer-specific authority shapes.
For ordinary Quick Task, PostgreSQL `quick_task_routing` locks current authority,
materializes the complete snapshot, runs selection, and persists the snapshot and decision
in one transaction. ManagedRun keeps its separate selection owner. The caller supplies no
account list, order, eligibility, exclusion, evidence, or account choice.

### ManagedRun Project-policy `L6`

Only `managed_run_project_policy` uses the complete policy-member universe, policy-excluded
members, sticky affinity, capability facts, Project evidence, and Project-era quota provenance.
Selection and pure-wait classification use independently eligible included members. A `no_route`
projection retains `excluded_by_policy` and every other exact blocker for each excluded member and
all exact blockers for included members. An all-excluded universe is cause-complete; cause-free
`no_route` is invalid.

For this shape only, each independent 300-minute and 10080-minute quota window retains source
identity, observation revision, raw provider timestamp, exact microsecond value, confidence, reset
instant, and other accepted Project-era provenance. Routing reclassifies both at its
database-authored decision instant and retains exact stale or reset-elapsed causes.

Sticky affinity applies only after independent account, capability, quota, process, and attempt
eligibility. ManagedRun route results are `selected`, `waiting_usage`,
`waiting_reconciliation`, or `no_route`. `waiting_usage` requires pure positive depletion and an
earliest-ready instant. `waiting_reconciliation` requires exact unresolved ProcessGeneration or
ProviderAttempt state. Mixed causes are `no_route`. Missing quota provenance is `usage_unproven`;
reconciliation without an exact unresolved process or attempt is `reconciliation_unproven`.

### Initial Quick Task Account Registry `L0`

`conversation_account_registry` binds complete non-tombstoned Account Registry membership,
canonical mode/fixed target/order and routing revision, exact account revisions and blockers, and
the current Task RoleProfile revision. Every member has exactly two duration-keyed quota slots:

- `missing`: `used_percent`, `resets_at`, `error_code`, and `observed_at` are null;
- `current`: `used_percent`, `resets_at`, and `observed_at` are present and `error_code` is null;
  or
- `observation_error`: typed `error_code` and `observed_at` are present while `used_percent` and
  `resets_at` are null.

The slot durations are exactly `300` and `10080`. This shape has no policy member/exclusion,
sticky, capability, Project evidence, quota source identity, observation revision, remaining,
confidence, or provenance fields. Fixed mode accepts only its exact eligible target. Balanced mode
uses canonical Account Registry order. The independent slots and separate accounts are never
pooled. The immutable result persists only `selected`, `waiting`, or `no_route` plus exact replay
exclusions. `waiting` and `no_route` grant no wake or same-Conversation re-selection.
Explicit retry uses the separate Conversation-owner routing Conversation successor
command.

### Later Quick Task continuation `L6`

`conversation_continuation` is an immutable non-selecting routing binding bound to the
current RuntimeSession and its original initial decision, selected account snapshot, and
copied Task RoleProfile snapshot. It has no candidate projection and never invokes Routing
Snapshot resolution or the selector. Ordinary Quick Task same-thread and Context Pack
planning retain that exact RuntimeSession, account, and profile lineage.

## Candidate-5 initial operation

Candidate 5 adds no coordinator state. The exact order is:

1. PostgreSQL Conversation authority creates the revision-1 Conversation and retains its
   durable initial request coordinates. Before a decision commits, readback is
   `routing_pending`.
2. Runtime `routing_orchestration.rs` invokes the one command-complete
   `quick_task_routing` operation. Runtime supplies typed inputs and a protocol-scoped
   idempotency key, but no generated identity.
3. The operation locks the Conversation, Account Routing Control, all complete
   non-tombstoned accounts in UUID order, current Task RoleProfile, complete routing order,
   and exact quota facts in one top-level `READ COMMITTED` transaction.
4. With those locks retained, it materializes the complete
   `conversation_account_registry` `L0` snapshot, runs the bounded I/O-free core kernel,
   persists and validates the snapshot, one initial decision, references, exact receipt,
   activity, and outbox, and commits them together.
5. If the result is `selected`, Continuation Plan atomically creates selected
   account/profile snapshots, the first revision-1 unfenced `starting` RuntimeSession,
   inert `initial_thread` plan, exact receipt, activity, and outbox.
6. Conversation authority atomically admits the database-generated prospective Turn as
   the exact active revision-1 sequence-1 user Turn and creates exactly one ordinal-0
   completed Message.
7. Account Service fences only the selected account immediately before spawn.
   ProcessSupervisor can spawn only from a fresh ProcessGeneration outcome. RuntimeSession
   Thread Establishment and ProviderAttemptService retain their exact thread and effect
   authority.

Any authority drift, kernel error, insert error, reference error, or completeness failure
rolls back the complete initial route transaction. No snapshot, decision, reference,
receipt, activity, or outbox subset can commit. PostgreSQL allocates generated route and
prospective-Turn identities as transaction effects. Exact identity equality is required
only when a committed exact receipt replays its stored response. If the transaction rolled
back, no generated identity remains authoritative and a resumed operation may allocate
replacement identities; runtime never has to reproduce an uncommitted random value.
Different keys serialize on the Conversation and have one winner.

There is no second initial selection on the same Conversation, alternate account, wake,
or account re-selection. Initial planning has no source RuntimeSession and no sticky
member. Later Turns use only `conversation_continuation`; selected-account drift,
exhaustion, or readiness failure returns typed manual recovery without invoking the
selector or switching accounts.

### Initial lineage

`L0` has all six RuntimeSession, account-snapshot, and profile-snapshot identity/revision fields
absent. `L6` has all six present with positive revisions. Initial Quick Task selection is
`conversation_account_registry` `L0`; later Quick Task binding is non-selecting
`conversation_continuation` `L6`; ManagedRun selection is `managed_run_project_policy` `L6`.

The final schema keeps all foreign keys and the exact all-null/all-present check. Initial `L0`
requires an open exact-revision Conversation, no RuntimeSession or Turn, zero sticky members, and
complete Account Registry membership/order/routing/account/blocker/Task RoleProfile facts with
exactly the two tri-state quota slots above. Its policy/capability/Project evidence and Project-era
quota fields are null. ManagedRun `L6` retains the complete Project-policy representation. Reverse
constraints reject partial lineage, mixed shapes, wrong consumers, missing or duplicate members or
slots, cross-links, and reordered facts.

One Conversation lock permits one cross-key initial operation to be fresh. A committed
exact-key replay is read-only and returns its stored identities and response bytes. Initial
planning accepts only selected `conversation_account_registry` `L0` and rolls back the
complete first-session authority cluster on failure. Later planning accepts only the
non-selecting binding and preserves the original RuntimeSession, account, and profile
lineage.

### Routing recovery and readback

`routing_pending` means no initial decision or downstream RuntimeSession, Turn, Message,
plan, process, thread, or provider effect exists. `resume_routing` invokes the same
Conversation's one initial route operation from its durable request coordinates. It does
not create an attempt ordinal, attempt head, second snapshot, second decision, or routing
Conversation successor.

A committed `waiting` or `no_route` decision remains immutable. Explicit retry invokes a
separate PostgreSQL Conversation-owner exact command with the expected source revision.
Under one lock, that command requires no selected result or downstream authority, creates
exactly one fresh revision-1 Conversation, records the exact one-to-one relation, archives
the source, writes one receipt/activity/outbox result, and commits. This new Conversation
is the **routing Conversation successor**. Same-key replay returns the committed stored
response. A competing key returns the existing successor or a typed stale result and
cannot create another.

Routing Conversation successor creation and initial routing are separate owner commands.
A crash between them leaves the successor open and `routing_pending`; restart may resume
its one initial route.
Routing never creates or archives a Conversation.

Exact ordinary readback follows the one-to-one relation. Get-by-Conversation-ID for an
archived source returns a typed `routing_successor` redirect containing its direct
successor Conversation identity and revision, not an archived-source summary. Ordinary
lists apply the archived-source filter before ordering, cursoring, and limiting: archived
sources are absent, and each open routing Conversation successor appears exactly once.

`establishment_pending` means one `selected` decision committed without a RuntimeSession
or initial plan. `resume_establishment` consumes only that selected decision and may replay
or complete initial planning; it cannot invoke selection. After `selected`, or once any
RuntimeSession, Turn, Message, plan, process, thread, or provider effect exists, routing
Conversation successor creation, route retry, wake, account re-selection, account switch,
and any other automatic cross-account fallback are rejected.

### Initial admission

Conversation authority creates the first Turn and history item in one one-winner
transaction. Required Turn values are the database-generated prospective UUID returned by
the committed selected route, sequence 1, `user`,
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
They cannot spawn, replace, adopt, create a routing Conversation successor, duplicate an
attempt, or terminalize the Turn.

Conversation authority can move the Turn to failed revision 2 under a starting session
only after positive proof of definite pre-effect refusal. The proof excludes every process
state that may have created a child, thread fence/start/bind, and prepared, authorized, or
unknown ProviderAttempt. Ambiguous work remains active and returns `Unknown`.

Explicit successor remains PostgreSQL-only non-dispatch evidence. Before any write it
locks the Turn named by the selected route and requires the same Conversation/source
RuntimeSession, `failed`, revision 2. It has no protocol field, command, runtime grant,
facade, fallback, or wake path.

This failed-Turn Explicit successor is distinct from the routing Conversation successor
used only after a committed initial `waiting` or `no_route` decision.

## RuntimeSession continuity

For an existing ordinary Conversation session, same-thread reuse requires positive exact
thread evidence from the original ProviderAttempt. Evidence must bind the same
Conversation, source RuntimeSession, account, and Codex thread.

For ManagedRun, same-thread reuse requires the accepted causal experiment and positive
exact-thread attestation bound to the source RuntimeSession.

When accepted existing-session same-thread evidence is absent, stale, negative,
incomplete, ambiguous, or cross-linked, ordinary Quick Task Continuation Plan uses its
atomic Context Pack path. It creates the Context Pack, account snapshot, new starting
RuntimeSession, plan, activity, outbox, and exact receipt as one authority cluster while
preserving the immutable source RuntimeSession and original account/profile lineage.

XY-1304 disables automatic cross-account same-thread substitution and all-depleted wake.
It does not disable this ordinary same-account Quick Task Context Pack path.

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
It returns the closed consumer and authority shape. A ManagedRun result includes complete
Project-policy causes and quota exclusions; an initial Quick Task result includes only its exact
Account Registry tri-state causes; a later Quick Task result includes only the non-selecting source
binding. The query cannot create a route, RuntimeSession, process, ProviderAttempt, wake, retry,
receipt, or dispatch fence.

The service uses the owner-only same-UID Unix transport. It has no TCP or loopback
fallback. Remote and cross-UID authority remains separately gated.

The `routing_orchestration.rs` methods remain crate-private and accept only typed owner
results. `quick_task.rs` consumes them without persisting routing. No protocol, CLI,
scheduler, UI, credential owner, Codex adapter, or client can construct the sequence or
mint dispatch authority. Product dispatch remains disabled until the Candidate-5 gate
accepts the complete composition.

## Acceptance

After source freeze, validation must cover:

- fresh PostgreSQL 18 latest-schema bootstrap, second-bootstrap refusal, and runtime-only
  daemon startup with zero DDL and no schema-owner credential;
- exact current catalog/authority for all routing, continuation, Conversation,
  ProcessGeneration, ProviderAttempt, and read-only projection objects;
- exactly one `conversation_account_registry` `L0` snapshot and initial decision per
  ordinary Conversation, retained `managed_run_project_policy` `L6`, and a distinct
  non-selecting `conversation_continuation` `L6` binding, including reverse-shape and
  cross-link rejection;
- exact Account Registry missing/current/observation-error slots, fixed/balanced selection, and no
  Project-era fields in initial Quick Task;
- ManagedRun policy-member/exclusion, capability, sticky, provenance/confidence, aging, mixed-cause,
  and pure-wait behavior without leaking those fields into Conversation routing;
- one atomic Quick Task route transaction with rollback on authority, kernel, insert,
  reference, or completeness failure; no orphan snapshot/decision; committed-receipt
  identity replay without runtime replay of rolled-back generated IDs; and one-winner
  different-key concurrency;
- no-decision `routing_pending` restart and `resume_routing`; committed waiting/no-route
  routing Conversation successor concurrency, typed archived-source redirect, list
  filtering, and crash-before-route recovery; and selected-without-session
  `establishment_pending`/`resume_establishment` without selection;
- atomic first-session cluster and Turn/history admission, exact effect fences,
  fresh/replay/reject/unknown behavior, and definite pre-effect refusal while preserving
  the distinct failed-Turn Explicit successor evidence;
- existing-session ordinary Quick Task same-thread and atomic Context Pack paths bound to
  the original RuntimeSession/account/profile lineage without selector invocation;
- positive-only process/attempt reconciliation, late evidence, smallest-scope isolation,
  and no replay after lost supervision;
- ManagedRun and Reviewer authority isolation;
- module reverse scans and the deletion test: one substantive PostgreSQL
  `quick_task_routing` owner, runtime `routing_orchestration.rs` sequencing, runtime
  `quick_task.rs` lifecycle/effects, pure-core kernel only, and primitive-only
  `exact_commands.rs`;
- stateless runtime sequencing and restart behavior;
- same-UID transport and complete read-only protocol projection; and
- reverse scans proving no same-Conversation `RetryRouting`/`RetryQuickTaskRouting`, split
  snapshot/decision command, route-attempt ledger, old barrier writer, second ledger,
  alternate selector, or unauthorized dispatch root remains.

No historical upgrade, populated cutover, schema-ledger, migration, or restore-digest
proof is part of acceptance.

## Architecture falsifiers

Stop and return to the authority owner if:

- ordinary Conversation execution requires a ManagedRun;
- ProviderAttempt must create or rewrite RuntimeSession state;
- Continuation Plan must write ProviderAttempt state;
- runtime routing sequencing must persist authority;
- the initial Quick Task snapshot and decision cannot share one transaction and receipt;
- a committed initial decision requires same-Conversation re-selection;
- selected-without-session recovery must invoke the selector;
- ordinary Quick Task Context Pack planning cannot preserve the original
  RuntimeSession/account/profile lineage;
- the retired ManagedRun effect path requires a second live or compatibility writer;
- mixed route causes must collapse into a pure wait;
- lost supervision must authorize replay without positive evidence; or
- Candidate-5 requires a route-attempt ledger, second account selector, pre-admission Turn
  row, generic recovery or transaction framework, wrapper-only routing module, or
  alternate dispatch owner.
