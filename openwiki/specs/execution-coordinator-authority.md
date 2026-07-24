# XY-1402 stateless execution coordination

## Status and authority

This page records the source projection for XY-1402. Linear issue XY-1402 is the
complete scope and acceptance contract. The following accepted records supply the
composed authority:

- parent decision XY-1398;
- product authority comment `099fb36d-9a48-407e-abdd-80dd56d13051`;
- skeptic authority comment `02d37443-f3cf-4c11-a462-e3da80c40481`;
- decision memo `134844d5-ad31-484f-9409-4591638c966d`;
- accepted XY-1399 R2 receipt `4afabdfc-a694-4d16-ba61-6cb016f38fe9`;
- accepted XY-1400 R2 receipt `e9509cd9-fb60-43ab-a9ab-d235a8e14e7f`; and
- accepted XY-1401 cycle-2 R2 receipt `8a784c0a-2e2f-4675-adba-d3baaaddb3f1`.

The XY-1396 and XY-1397 candidates are rejection evidence. They do not supply
implementation authority.

This candidate is source-only work before the unified core-freeze gate. No
formatter, build, compiler, lint, static analysis, migration parser, SQL parser,
test, fixture, generator, service, VM, UI check, live Codex experiment, account
operation, or provider effect is part of this candidate evidence.

## Owner composition

`ExecutionCoordinator` is a zero-sized, stateless sequencer. It does not persist
state. It does not own a lifecycle, retry machine, receipt, effect barrier,
ambiguity ledger, account decision, RuntimeSession decision, process state, or
ProviderAttempt state.

| Owner | Sole authority retained |
| --- | --- |
| Conversation owner | Ordinary Conversation and Turn lifecycle. Quick Task stays an ordinary multi-turn Conversation. |
| ManagedRun owner | ManagedRun lifecycle, execution assignment, wait state, and acceptance. |
| V14 and V16 | Complete account universe, eligibility facts, account selection, and immutable route decision. |
| V17 | Exact same-thread RuntimeSession reuse or atomic Context Pack plus fallback RuntimeSession creation. |
| ProcessSupervisor | ProcessGeneration intent, live fence, positive death evidence, and account-local quarantine. |
| ProviderAttemptService | Atomic prepared binding, provider-effect state, positive evidence, restore projection, and reconciliation. |
| Reviewer | Execution-scoped, read-only review. Missing or ambiguous output grants no approval or completion. |
| ExecutionCoordinator | One in-memory sequence across the accepted owners. It grants no dispatch authority. |

The coordinator first consumes one V16 decision. A selected decision can consume
one V17 plan. The coordinator then consumes one ProcessSupervisor-owned live
fence and asks ProviderAttemptService to prepare the exact attempt. The
coordinator consumes the fresh prepared capability and returns only an inert
attempt projection. It cannot authorize dispatch. No production composition
root can call this sequence.

## Closed execution consumer

V14, V16, and V17 use one closed consumer union:

- `conversation_turn` binds the Conversation identity and revision, source
  RuntimeSession identity and revision, and reserved Turn identity; and
- `managed_run_execution` binds the ManagedRun identity and revision and one
  distinct managed execution identity.

The ordinary variant does not contain a ManagedRun identity. The Conversation
owner can materialize its reserved Turn after ProviderAttempt preparation.
ProviderAttempt does not create or rewrite a RuntimeSession or a Turn.

The managed variant does not move ManagedRun lifecycle authority. The ManagedRun
read model consumes all ProviderAttempt result projections. It does not copy
provider-effect authority into a second ledger.

## V25 and V26 forward cutover

`V25__execution_route_enum_expansion.sql` adds only the route and wait vocabulary.
PostgreSQL requires that transaction to commit before a later transaction uses a
new value from an existing enum.

`V26__execution_coordinator_cutover.sql` is the forward-only coordinated cutover.
It takes exclusive locks on the retired V12 relations and their current
integration relations before it changes authority.

The migration stops if any V12 effect barrier, effect, submitted-turn receipt,
safety input, or V12 exact-command receipt remains live. It also stops when
historical routing or continuation data maps one accepted ManagedRun revision to
more than one managed execution identity. V24 could represent an ordinary
ProviderAttempt over its then-ManagedRun-only V17 plan. V26 stops if such a
historical row exists because it cannot convert that lineage into a direct
ordinary consumer without inventing authority. These conditions require an
external cutover decision. The migration does not invent compatibility
authority.

After the checks pass, V26:

- removes the V12 safety command, helper, effect ledger, submitted-turn ledger,
  and barrier relations;
- removes V12 runtime grants;
- makes V14, V16, and V17 consumer-generic;
- backfills one unambiguous historical managed execution identity;
- adds exact Conversation and ManagedRun consumer completeness constraints;
- adds exact route-cause and reconciliation projection;
- keeps V16 as the only account decision writer;
- keeps V17 as the only RuntimeSession continuation writer;
- keeps ProviderAttempt as the only provider-effect writer; and
- adds least-privilege read functions for immutable execution decisions and
  ManagedRun execution projections.

There is no live, latent, compatibility, or fallback V12 writer after V26.
Rollback means restore of a pre-V26 database. It does not mean reverse SQL or a
dual-writer interval.

## Route projection

V16 loads and locks the complete V14 account universe. The caller supplies no
account list and cannot select an account.

Selection and pure-wait classification use only independently eligible included
members. Policy-excluded members never become eligible and do not participate in
quota or reconciliation wait classification. A `no_route` projection instead
uses the complete persisted policy-member universe. Each excluded member retains
`excluded_by_policy` and every other persisted blocker, while each included
member retains its exact blockers. An all-excluded universe therefore produces a
cause-complete `no_route`. A cause-free `no_route` is invalid.

The 300-minute and 10,080-minute quota facts stay independent. Each fact keeps
its duration, source identity, observation revision, exact raw timestamp,
microsecond value, confidence, and reset instant. V16 applies sticky affinity
only after it proves independent eligibility. V16 classifies both duration-typed
facts again at its own database-authored decision instant. A fact that becomes
stale or reset-expired after V14 keeps that exact new cause.

V16 uses these exact projection rules:

- `selected` names one independently eligible account.
- `waiting_usage` is valid only when every otherwise eligible account is blocked
  only by current positive quota depletion. It contains the complete exact
  depletion causes and the earliest account-ready instant.
- `waiting_reconciliation` is valid only when every otherwise eligible path is
  blocked only by an unresolved ProcessGeneration or ProviderAttempt.
- `no_route` keeps every exact cause for mixed or non-wake conditions. It grants
  no quota wake and does not fail the task.

Authentication, plugin, disabled-account, unknown-capability, dependency,
approval, user, external, and Reviewer causes do not become quota or
reconciliation causes. Missing timestamp provenance becomes `usage_unproven`.
ManagedRun reconciliation without an exact unresolved process or attempt becomes
`reconciliation_unproven`.

V16 adds a process cause to a path that is otherwise eligible except for positive
quota depletion. Therefore, an unresolved process and quota depletion produce a
mixed `no_route`, not a quota wake.

## RuntimeSession and thread continuity

For an ordinary Conversation, V17 permits same-thread reuse only when the
original ProviderAttempt has positive exact-thread readback evidence. The
evidence must bind the same Conversation, source RuntimeSession, account, and
Codex thread. The reserved current Turn remains a new Conversation-owned intent.

For a ManagedRun, V17 retains the accepted V15/V22 causal experiment path. An
exact accepted experiment and positive `thread/read` attestation must bind the
same source RuntimeSession thread.

If same-thread evidence is absent, stale, negative, incomplete, ambiguous, or
cross-linked, V17 uses its atomic fallback path. The transaction creates the
Context Pack, account snapshot, starting fallback RuntimeSession, continuation
plan, activity, outbox, and exact receipt as one authority cluster. V17 does not
write ProviderAttempt state. ProviderAttempt does not write RuntimeSession state.

The shared Codex home stays visible. Exact thread identity, supported same-thread
resume, and the accepted Context Pack fallback stay intact.

## Effect ambiguity and isolation

An authorized ProviderAttempt remains `unknown` until positive evidence proves a
terminal result. Process death, timeout, missing events, EOF, restart, negative
search, and absent rows do not prove non-submission. These observations do not
authorize replay.

Late positive evidence stays bound to the original attempt. A replacement
process can reconcile that attempt. It cannot replay the attempt.

An existing exact attempt blocks only its exact Conversation Turn or managed
execution intent. A process cause blocks only the affected account path. Other
consumers, accounts, dependencies, and routes remain eligible.

## Transport and production isolation

The protocol exposes one read-only immutable execution-decision query. The query
returns the closed consumer, route kind, exact causes, and independent positive
quota exclusions. It cannot create a route, RuntimeSession, process,
ProviderAttempt, wake, retry, receipt, or dispatch fence.

The service uses the accepted XY-1399 owner-only same-UID local transport. It
does not restore unauthenticated loopback admission. Remote and cross-UID
authority remains deferred to XY-1299.

Live provider dispatch and production routing stay structurally disabled. The
coordinator method is crate-private. It accepts only a crate-private live
`FencedProcess`. No protocol, CLI, scheduler, application, Codex adapter,
credential owner, UI, or daemon composition root can acquire the coordinator
sequence or a dispatch authorization.

## Deferred acceptance matrix

The later unified core-freeze gate must bind every result to the exact candidate
tree. It must execute the complete matrix below. This source candidate does not
execute any row.

| Boundary | Representative deferred acceptance cases and required evidence | Residual risk before execution |
| --- | --- | --- |
| Rust source quality | Compile all affected crates and run the accepted formatter, lint, static, and documentation checks on the exact tree. | Type, visibility, warning, and formatting defects are not excluded by source inspection. |
| Migration paths | Prove clean V1-to-V26 and populated V24-to-V25-to-V26 migration. Prove that V25 commits enum vocabulary before V26 uses it. Prove transaction rollback for each V26 cutover checkpoint. | PostgreSQL syntax, lock order, and catalog behavior are not executed. |
| Cutover drain | Prove that every live V12 row or V12 exact-command receipt stops V26 without partial change. Prove that a drained cutover removes all V12 writers and grants. | A latent historical writer or unexpected production row can still falsify the cutover. |
| Historical ambiguity | Prove that zero or one accepted managed execution identity backfills exactly. Prove that two identities stop the migration. Prove that every historical ordinary ProviderAttempt over the old ManagedRun-only V17 lineage stops the cutover. | Existing data can contain an unobserved ambiguous or cross-linked lineage. |
| Authority manifests | Capture S0, R1, and R2 manifests. Verify the V26 migration ledger, relation, function, trigger, enum, ACL, dependency, and security-definer inventories. | The schema and configured-authority digests remain intentionally unfrozen until the aggregate gate. |
| ACL closure | Prove runtime can execute only the required exact functions and read only the required projections. Prove PUBLIC and runtime cannot use private helpers or relation DML. | A catalog or default-ACL drift can add an authority path. |
| Ordinary Conversation | Route and plan an ordinary Turn without a ManagedRun, WorkItem, Reviewer, or second effect ledger. Prove Quick Task stays a multi-turn Conversation. | No end-to-end ordinary execution was run. |
| Conversation Turn owner | Prepare one reserved Turn, then materialize it only through the Conversation owner. Reject a cross-Conversation, cross-session, changed, or duplicate Turn. | Conversation-owner integration is source-inspected only. |
| ManagedRun | Route one exact managed execution and consume ProviderAttempt results without changing ManagedRun lifecycle ownership. | ManagedRun lifecycle and acceptance integration are not executed. |
| Account authority | Supply no account list. Prove V16 loads the full persisted V14 universe and rejects policy, member, revision, and evidence drift. | A query or trigger error can weaken full-universe closure. |
| All-excluded policy | Exclude every persisted policy member. Require one `no_route` containing `excluded_by_policy` for every member and every other persisted member blocker. | Database trigger and Rust-kernel cause completeness are not executed. |
| Mixed included/excluded policy | Combine excluded members with included members carrying quota, reconciliation, and non-wake blockers. Require pure waits to use only included members; require every excluded and included cause when the result is `no_route`. | Mixed policy-disposition and blocker schedules are not executed. |
| Cause-free NoRoute | Remove all causes or one required excluded-member cause from a `no_route` projection. Require PostgreSQL integrity, exact-command readback, read-only adapter, runtime projection, and protocol decoding to fail closed. | Cross-layer malformed-projection cases are source-inspected only. |
| Quota separation | Exercise 300-minute and 10,080-minute facts independently and together. Prove duration, source, revision, raw value, precision, and reset identity stay separate. | SQL and Rust ordering parity are not executed. |
| V14-to-V16 quota aging | Place each duration just before, at, and after its freshness and reset boundaries between snapshot and decision. Require the exact stale or reset-elapsed cause and never an empty, selected, or pure-depletion projection. | Cross-transaction clock boundaries are not executed. |
| Sticky affinity | Prove sticky selection only after independent account, capability, quota, process, and attempt eligibility. | A data-dependent ordering defect can select too early. |
| Pure usage wait | Prove `waiting_usage` only when all otherwise eligible accounts have only positive current depletion. Verify the exact earliest account-ready instant. | Boundary timestamps and provenance comparisons are not executed. |
| Pure reconciliation wait | Prove `waiting_reconciliation` only when all otherwise eligible paths have only unresolved ProcessGeneration or ProviderAttempt causes. | Process/attempt race schedules are not executed. |
| Mixed causes | Combine quota with process, authentication, plugin, disabled, unknown-capability, dependency, approval, user, external, and Reviewer causes. Require `no_route`, every exact cause, no wake, and no task failure. | Cross-product cause schedules are not executed. |
| Missing quota provenance | Remove each duration, source, revision, raw timestamp, precision, confidence, or reset field. Require `usage_unproven` or the exact existing typed cause, never a quota wake. | Malformed and boundary timestamp fixtures are deferred. |
| RuntimeSession reuse | For Conversation work, require original positive ProviderAttempt thread evidence. For ManagedRun work, require accepted causal experiment evidence. Reject stale, negative, missing, duplicate, or cross-linked evidence. | Same-thread evidence queries are not executed. |
| Atomic fallback | Inject failures before and after each Context Pack, snapshot, RuntimeSession, plan, activity, outbox, receipt, and commit step. Require zero or one complete authority cluster. | Transaction and blob crash behavior are deferred. |
| Process fence | Accept only a live ready ProcessGeneration with an unretired execution epoch. Reject dead, starting, stopping, death-unknown, cross-account, stale-revision, and retired-epoch generations. | Host and database races are not executed. |
| ProviderAttempt preparation | Prove one atomic binding to the exact consumer, V16 decision, V17 plan, RuntimeSession, account, process generation, epoch, request identity, and provider keys. Prove that the coordinator consumes the fresh prepared capability and returns only an inert projection. | V24 replacement-function behavior and capability consumption are not executed. |
| Exact-intent replay | Prove same-key replay returns stored authority. Prove a new attempt for the same Turn or managed execution cannot change account or replay an effect. | Concurrency and response-loss schedules are deferred. |
| Lost supervision | Inject process death, timeout, EOF, restart, missing events, negative search, and absent result rows after authorization. Require `unknown`, not `not_submitted`. | Provider and host adapters are not exercised. |
| Late positive evidence | Reconcile a late positive result to the original attempt after process loss. Reject attribution to a replacement attempt. | Provider evidence adapters are not exercised. |
| Smallest-scope isolation | Block one attempt, dependency, account, and route in turn. Prove unrelated work remains eligible. | Large concurrent account and consumer schedules are deferred. |
| ManagedRun projection | Read every exact ProviderAttempt state, positive terminal evidence identity, and unknown reason through the ManagedRun view. Prove it cannot mutate ProviderAttempt or synthesize acceptance. | Read-model completeness is not executed. |
| Reviewer | Exercise unavailable, failed, missing, ambiguous, and positive output. Require missing or ambiguous output to grant neither approval nor completion. | Reviewer integration remains read-only and unexecuted. |
| Coordinator statelessness | Inspect object layout and run repeated, concurrent, crash, and restart calls. Prove no coordinator relation, receipt, task, retry state, or durable projection exists. | Runtime memory and concurrency behavior are not executed. |
| Protocol projection | Query selected, usage-wait, reconciliation-wait, mixed, missing, malformed, and incompatible decisions. Require complete causes without truncation or mutation authority. | V1.2/V1.1 wire compatibility and response-size limits are not executed. |
| Same-UID transport | Prove fixed owner-only Unix endpoint, client and server peer-UID checks, and no TCP or loopback fallback. | XY-1399 host matrix remains deferred. |
| Shared Codex home | Prove the exact configured home, account visibility, thread identity, supported resume, and Context Pack fallback across restart. | No live Codex experiment is part of this candidate. |
| Production isolation | Reverse-scan every composition root. Prove no routing, coordinator, process spawn, provider dispatch, credential, remote transport, UI, packaging, or release path is enabled. | A dependency or feature-flag change can invalidate source isolation. |
| Replay and concurrency | Run same-key, cross-key, response-loss, rollback, serialization, deadlock, restore, and concurrent-consumer schedules for V16, V17, and V24. | Exact-command convergence is not executed on the integrated tree. |
| Hostile cross-links | Substitute every Conversation, Turn, ManagedRun, managed execution, RuntimeSession, account, process, attempt, plan, evidence, operation, and idempotency identity. Require fail-closed rejection. | The full hostile matrix is deferred. |
| Documentation alignment | Compare the final source, Linear authority, manifests, OpenWiki, and accepted parent receipts on the exact tree. | Later changes can cause documentation or authority drift. |

## Architecture falsifiers

Stop integration and return to the authority owner if any of these conditions is
true:

- ordinary Conversation execution requires a ManagedRun;
- ProviderAttempt must create or rewrite a RuntimeSession;
- V17 must write ProviderAttempt state;
- the coordinator must persist durable authority;
- V12 cannot be retired without a second live, latent, compatibility, or fallback
  writer;
- the accepted parent contracts cannot compose without a change to accepted
  authority;
- route projection must collapse a mixed cause into a quota or reconciliation
  wait; or
- lost supervision must authorize replay without positive evidence.
