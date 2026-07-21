# Decodex vNext Authority Decision

Status: accepted repository authority for vNext.

Tracking issue: [XY-1260](https://linear.app/hack-ink/issue/XY-1260/promote-the-vnext-authority-contract-and-supersede-lane-authority-v2)

Source planning record: [Decodex vNext Design Baseline](https://linear.app/hack-ink/document/decodex-vnext-design-baseline-681f7d8ad284), accepted 2026-07-12 against repository snapshot `5546dcb3b2eb0a8aecf7d6a3b117d2605ea315b8`.

Authority amendment: Manager decision accepted 2026-07-13. The decision accepts the
conceptual XY-1262 foundation/live-enablement split proposed by merged PR #1098 at
`687605583817eca32cbdfb1107f3ee18d3106cea`, but only through this independently
reviewed normative amendment. The merged proposal was evidence, not authority by
itself. Repository authority remains normative; Linear is planning metadata.

XY-1271 storage-boundary amendment accepted 2026-07-14: retain the migration and
daemon-private runtime identities, and treat `decodexd`, that identity, and BlobStore access as one
trusted service boundary. PostgreSQL owns committed metadata/domain/receipt/activity/outbox state;
local CAS owns large bytes, which PostgreSQL alone cannot attest. Blob-backed commands use a
receipt-first, fenced, exact-response saga and dedicated session-level hash plus per-shard
coordination around create-only synchronized CAS publication. Arbitrary/manual use of the private
runtime credential is unsupported and equivalent to daemon compromise.

Readiness authority correction accepted 2026-07-16: Codex 0.144.2 and 0.144.4 expose
no stable passive complete account-owned plugin, skill, and MCP readiness receipt. Passive
inventory is therefore withdrawn from the accepted XY-1262 foundation evidence. Host files,
manifests, configuration, remote catalogs, process binding, and user declarations do not become
observed account readiness. The first-release doctor result remains typed `unknown` for plugins;
`plugin_unready` is inert reserved state. XY-1336 tracks the upstream receipt gap outside the
vNext critical path and neither closes nor blocks XY-1275.

XY-1274 quota-authority amendment accepted 2026-07-16: quota storage accepts only exact
product-valid Unix microseconds, and no layer may round or truncate an ingress timestamp.
Canonical quota mutation identities move to their `/2` typed-integer documents. The V8
migration is an atomic zero-state boundary that locks every quota writer surface, rejects all
structurally classified pre-V8 quota evidence, and alters the proven-empty `quota_windows`
table in place. Populated V7 conversion and table drop/recreation are not compatibility
features. Recovery is whole disposable pre-release database recreation; XY-1302 retains
whole-ledger production-baseline and cutover authority. XY-1357 must retain and characterize the
natural raw upstream timestamp before live quota ingestion or routing, and a value that cannot be
converted exactly to UTC Unix microseconds keeps production routing disabled and reopens only the
ingress authority. The normative
details and proof gates live in the authority contract and gate manifest; the three rejected
old-boundary candidates remain historical provenance in the runtime proof.

XY-1272 PostgreSQL-authority reset accepted 2026-07-16: XY-1272 owns only configured-principal
and ACL manifest/readiness closure against landed V8. It owns no migration or Codex creation,
mapping, or reconciliation surface. Under the later XY-1345 reset, XY-1346 owns expected V9 and
re-bounded XY-1337 owns expected V10; XY-1358/V15 owns causal experiment creation
and positive-only observation authority; XY-1276 owns production Quick Task creation. Lossy or
paginated Codex evidence cannot authorize negative `Present`, `Complete`, or context-free `Absent`
authority. A future configured PostgreSQL role must atomically extend configuration, bootstrap,
manifest/readiness, and negative tests.

XY-1345 exact-command authority amendment accepted 2026-07-16: pure database commands use
operation-specific, command-complete `SECURITY DEFINER` entrypoints. PostgreSQL constructs and
consumes the complete typed request envelope; runtime supplies only the protocol-scoped
idempotency key and typed operation inputs. A separate `decodex.exact_command_receipts` relation is
keyed by `(protocol_version, idempotency_key)`, while operation identity remains inside the
envelope so cross-operation reuse conflicts. Runtime has no exact-receipt table privilege and no
private-helper or canonical activity/outbox mutation authority. An executing row is transaction
internal: a `DEFERRABLE INITIALLY DEFERRED` commit-time invariant rejects every incomplete commit,
and completed response bytes and effects are immutable and undeletable. Stable domain rejection,
idempotency conflict, and retryable infrastructure failure are distinct outcomes.

Normal execution is one exact command per top-level `READ COMMITTED` transaction. The command uses
a later read/lock statement after `ON CONFLICT DO NOTHING`; classified `40001` and `40P01` failures
retry the whole identical transaction. Request JSONB uses equality, includes every optional key
with explicit JSON null, receives enums/numerics through typed inputs, and preserves exact
PostgreSQL text/code-point semantics. Derived revisions, selected rows, generated identities,
timestamps, digests, snapshots, activity/outbox identities, and responses are effects. Effect and
response evidence comes from actual `RETURNING` rows and canonical audit identities.

Candidate 3 is superseded as implementation and remains hostile-test/design provenance only.
[XY-1345 evidence](../evidence/xy-1345-exact-command-authority.md) records the passing isolated
PostgreSQL 18 proof. The serial vertical order is XY-1345 authority/prototype, then XY-1346 exact
receipts plus immutable global RoleProfile bootstrap/update in V9, then re-bounded XY-1337
RuntimeSession V10. Legacy
`command_receipts` semantics remain unchanged for unrelated external or long-running sagas.

XY-1337 RuntimeSession amendment, implemented as expected V10: creation and transition are separate
command-complete `SECURITY DEFINER` operations using the unchanged V9 exact receipt. PostgreSQL
constructs creation identity from RuntimeSession ID, Conversation ID, role, complete non-secret
account snapshot identity/facts, nullable Codex thread ID, and initial state, then resolves exactly
one current immutable RoleProfile revision server-side. Transition identity contains only session
ID, expected revision, and target state. Both commands atomically store domain state, canonical
activity/outbox effects, and immutable response bytes; stable rejection completes in the same
transaction. V10 is a zero-state forward cutover, preserves the existing table identities, removes
runtime snapshot/session DML, fences RuntimeSession audit namespaces, and does not authorize Codex
creation, reconciliation, routing, scheduling, WorkItems, ManagedRuns, UI, or plugin readiness.
Forward-only V21 repairs that fence without changing the command or trigger boundary: a scalar
RuntimeSession/profile/account snapshot identity is provenance owned by the enclosing domain event,
not a RuntimeSession ownership claim. Aggregate/event/kind markers, complete RuntimeSession or
profile/account snapshot objects under any wrapper, and outbox links to activity carrying those
shapes remain reserved.

XY-1284 managed-repository authority reset was finalized by the accepted XY-1348
stage-two amendment on 2026-07-17. PostgreSQL is the durable authority for the current
repository projection, monotonic generation/tip, globally immutable operation
assignment, append-only authority and operation evidence, compare-and-swap, atomic
command completeness, and restart loads. Pure deciders and facts in `decodex-core` are
mechanism-neutral inputs to that authority; no standalone object, snapshot, caller
projection, or operation view is authoritative or can grant execution.

Every repository operation ID has one complete canonical descriptor across all
repositories and operation kinds. Exact descriptor equality resolves to
`ExistingExact(OperationView, NoDispatch)`; any difference is permanent
`OperationIdConflict`. A fresh affine execution receipt exists only after successful
COMMIT acknowledgement on the same adapter control path that prepared the new assignment.
Persistence, readback, exact repeat, restart, terminal state, and an unknown COMMIT
outcome never mint or reconstruct it. An unknown COMMIT outcome therefore authorizes no
external execution.

Allocate is PostgreSQL-only and uses strictly read-only repository evidence. `Register`,
`WorktreeReady`, and `Commit` are distinct durably fenced `PossiblyEffected` operations with
readback-only restart: no retry, replay, adoption, repair, or import is authorized.
`Register` requires exact reciprocal registration, `WorktreeReady` preserves the exact
head, and `Commit` advances exactly once from its authorized predecessor head to its exact
successor. Accepted XY-1354 descriptor-assisted, symlink-free persisted absolute-path
reacquisition and pinned Git 2.54 remain unchanged. Rejected candidate trees
`6e20e9b3cf1415cce9b399da173b0410cc4c80dc`,
`6979e3831da772fca3fe0f0e0b4699df642d3a65`, and
`e42212add13af3f702e0ec8966ce3d6a7b682d12` are superseded evidence only, not current
authority. The accepted V11 commit `33159d0cb2da7f86748f1a380def0927970a409a`
and V12 commit `a6bfb0aefc72f2a65d14fc3755b556f959ec2d4e` remain unchanged.

V13 is accepted on `main` as the sole XY-1349 managed-repository PostgreSQL authority
migration. The serial routing migration owners are fixed: XY-1356 solely owns V14 durable
routing-policy and complete candidate-set authority; XY-1358 solely owns V15 causal Codex
experiment authority; XY-1359 solely owns V16 atomic routing decisions; XY-1360 owns V17
continuation authority after source inspection proved durable atomic fallback state was required;
and XY-1362 owns V18's ledger-first `waiting_usage` wake authority. Migration allocation, the
embedded ledger, migration authority, schema/digest inventory, and aggregate migration evidence
remain one non-commutative singleton serial-writer domain.

XY-1355 live-routing authority reset accepted 2026-07-18 after three materially rejected
XY-1304 candidates: PostgreSQL owns one revisioned complete routing-authority snapshot.
Runtime and callers cannot supply authoritative policy order, candidate membership, sticky
identity, eligibility facts, or exclusions. The database-produced snapshot binds every
account-inventory member to an explicit disposition; canonical user-owned order and accepted
Policy revision; sticky affinity and the exact RuntimeSession revision from which it was
derived; account, RoleProfile, and Codex-build compatibility; exact account and evidence
revisions; and the required capabilities plus applicability for each member. An omitted or
unknown inventory member is a blocker, never silent absence or an eligible candidate.
The existing bounded inert `PolicySnapshot` remains accepted Policy-revision content only; it is
not a complete routing snapshot and cannot prove candidate completeness or provenance.

`decodex-core` remains a pure decision kernel over that complete database-produced value.
PostgreSQL persists the atomic decision and evidence linkage; runtime sequences only effects;
Codex supplies positive capability evidence and never routing authority. One app-server process
remains immutably bound to one account, credentials never switch in a live process, and separate
account quotas are never represented as a merged pool.

Account-owned readiness is evaluated only for capabilities explicitly required by the accepted
routing policy. Unknown never satisfies a required capability. When the accepted required-
capability set is empty, unknown plugin inventory is non-applicable rather than positive readiness
evidence. XY-1336 remains future passive-receipt tracking outside this routing chain. Host-owned
before/after receipts establish only no-mutation integrity.

Ingress retains the exact raw provider timestamp value. UTC Unix-microsecond construction must be
exact and rejects every rounding or truncation path. V14 through V16 remain precision-agnostic and
fail closed, allowing XY-1357 natural provider evidence to remain in the unified post-freeze gate.

After V16, XY-1360 owns same-thread continuation and one atomic Context-Pack fallback;
XY-1361 owns runtime orchestration while production dispatch remains structurally disabled;
XY-1362 owns `waiting_usage` wake lifecycle under scheduler authority; and XY-1363 owns
retained-title Codex Desktop discovery evidence. Ambiguous-turn replay and repository,
worktree, Git, and artifact reconciliation remain in the accepted ManagedRun and repository-
effect authorities, not routing. XY-1304 is only the final aggregate live gate and the owner of
a separate reviewed enablement amendment.

The rejected dirty combined XY-1304/V14 candidate, its partial fourth repair, its caller-
authoritative request shape, Rust authorization wrapper used as provenance, global
`SupportedPositive` plugin requirement, combined experiment/routing schema, and sequential
exclusion -> RuntimeSession -> decision composition are superseded evidence only. They must not
be repaired, revived, or wholesale transplanted.

XY-1372 private-artifact capability evidence accepted 2026-07-21 freezes one
`decodex-core` Unix authority. It uses descriptor-pinned bounded capture and one semantic
publication, retirement, and collection state machine with private macOS
`renameatx_np(RENAME_EXCL)` and Linux `renameat2(RENAME_NOREPLACE)` shims. A
`PrivateArtifactDirectory` can capture and publish create-new artifacts. Only a
Decodex-created, operation-unique `OwnedEphemeralArtifactRoot` can grant retirement authority.
Retirement moves the whole owned root into a controlled, unique, same-device quarantine before
`QuarantinedPrivateArtifact` can grant collection authority. A path, retained descriptor, or
ordinary Decodex child never gains retirement authority by observation.

The operation token and expected digest are durable before the first namespace effect. Staging
occurs in the retained target parent. Every no-replace effect requires exact post-effect
verification and synchronization before it grants a success capability. Ambiguous or unexpected
results preserve the stage, target, active root, or quarantine for targeted reconciliation. No
publication, retirement, error, or rollback path may unlink one of those objects. Producer stop
means tracked leader exit plus tracked process-group absence. Exclusive maintenance means
cooperative Decodex quiescence; it is not containment of hostile same-UID code. Collection is
separate maintenance and cannot determine whether capture, publication, or attestation succeeded.

The exact supported and excluded environments, proof lineage, typed stops, and no-overclaim
boundary are recorded in the [XY-1372 capability evidence](../evidence/xy-1372-private-artifact-capabilities.md).
Unsupported or unproven semantics require a future enablement gate. XY-1373 must receive fresh
exact-candidate review before XY-1371 implementation resumes. The accepted XY-1371 implementation
must exist before XY-1369 or XY-1370 resumes, and both accepted preflight receipts remain required
before a new XY-1363 live-effect decision.

The V1 trust boundary is one trusted single-host service. `decodexd` remains the sole
repository-effect owner. Its in-process repository executor is a correctness,
determinism, and admitted-authority-continuity boundary, not isolation from malicious
same-UID code. Project validation may supervise lifecycle, bound output and time, and
detect mutation, but it is not hostile-code confinement. A hostile same-UID project or
multi-tenant requirement would require a separate UID/sandbox authority plus a new
feasibility gate; it cannot be inferred from this design.

Authorized whole-cluster restore is inside the trusted PostgreSQL-administrator boundary
and may redefine current authority; V1 has no automatic full-cluster rollback detection.
The trusted single-daemon/same-UID boundary remains unchanged. XY-1349 solely owns V13
persistence, XY-1350 may proceed in parallel only against this accepted contract, and
XY-1351 owns the first shared saga path.

## Decision

Decodex vNext is a rebuild of the agent workspace, not an incremental extension of the
current Linear-lane and SQLite runtime. The normative product and runtime contract is
[the vNext authority contract](../specs/vnext-authority.md); its ordered proof and
implementation boundaries are in [the vNext gate manifest](../specs/vnext-gates.md).

The accepted planning baseline is provenance for this decision. Where the baseline and
these repository documents agree, these repository documents own implementation. A
concrete contradiction must stop the affected milestone for explicit resolution; later
workers must not silently reinterpret either source.

## Decision hierarchy

Within vNext work, authority descends in this order:

1. explicit user direction and checked-in project policy;
2. this decision, the vNext authority contract, and the vNext gate manifest;
3. accepted project policies and versioned domain/protocol contracts created under
   those documents;
4. source, tests, migrations, and operational runbooks implementing an accepted gate;
5. OpenWiki navigation and current-runtime descriptions;
6. Linear plans and research as provenance or candidate changes.

Current source remains authority for current v0.2 behavior but cannot override the
accepted vNext target. Conversely, target documents do not claim that vNext behavior is
already implemented. Git/filesystem and GitHub remain the authorities for repository
content and provider readback respectively; PostgreSQL becomes product-state authority
only when its bootstrap/cutover gate is accepted.

## Supersession

Lane Authority v2 and its C1-C7 program are superseded and frozen. Its decision,
contract, effect registry, gate manifest, checkpoint ledger, fixtures, scripts, review
findings, and incident scenarios remain useful historical evidence. They must not be
continued, repaired, activated, or treated as vNext acceptance gates. In particular,
vNext rejects Lane Authority v2's SQLite authority, Linear issue/lane identity, local
Unix authority ledger, and compatibility migration target.

PR #1092 is likewise historical/incident evidence: freeze or close it, do not merge it,
and do not wholesale cherry-pick its implementation. Reuse a behavior only after the
vNext owner and a focused gate/test make that behavior authoritative.

## Accepted shape

- PostgreSQL owns Decodex product state; a transactional outbox and leases support
  commands and recovery. It is not event sourced.
- A shared normal `~/.codex` owns persistent Codex rollout/thread continuity and Codex
  UI visibility. Decodex maps only threads that it created.
- `decodexd` alone owns scheduling, app-server children, product mutations, repository
  side effects, and adapters. GPUI, the menubar app, CLI, and MCP use the same versioned
  application protocol and are not parallel authority paths.
- Decodex owns Project, stable Advisor/Lead, execution-scoped Task/Reviewer, Program,
  Objective, WorkItem, Conversation, RuntimeSession, ManagedRun, Automation,
  ContextRevision, AgentMessage, Artifact, RoleProfile, and their policies.
- One Project has exactly one stable Lead in V1. The Lead serializes decisions while
  Task and Reviewer execution may be parallel. Advisor is global and advisory only.
- A managed implementation Task owns its implement/review/repair/revalidate loop by
  spawning an independent read-only reviewer subagent. Lead/Manager owns dispatch,
  final acceptance, and merge; Quick Tasks are exempt, and missing/failed review is a
  typed ManagedRun wait and optional WorkItem block rather than reviewed success.
- A Project policy grants bounded unattended authority once; repository, tool, path,
  merge, budget, approval, parallelism, and quiet-period limits remain hard stops.
- Work lands through focused task branches/PRs directly to `main`; there is no long-lived
  vNext branch.

## XY-1262 gate split

The XY-1262 foundation gate is accepted only for the evidence scope enumerated in the
[gate manifest](../specs/vnext-gates.md): shared-home and one-account-per-process
boundaries, creation-receipt ownership, negotiated app-server contracts, supported
exact-ID/list/read/archive operations, lossy-read/divergence policy, native run-local
collaboration normalization, process-scoped authentication/redaction, read-only
integrity evidence, and pure duration-typed quota policy. This acceptance does not
claim global Codex title search, live quota-driven routing, automatic continuation, or
release readiness.

The separate [XY-1262 live account-routing enablement gate](https://linear.app/hack-ink/issue/XY-1304)
remains failed and fail-closed. The bounded foundation work named by the manifest may
proceed after its own dependencies, but no live-routing, managed production execution,
dogfood, cutover, or release path may use the disabled capabilities until that later gate
passes through another explicit repository authority amendment. Unknown or stale quota
facts never establish eligibility.

## Rejected alternatives and falsifiers

Rejected V1 alternatives are enumerated normatively in the contract. The decision may
change only for evidence that cross-account continuation cannot preserve useful
continuity, GPUI cannot meet pinned build/package/accessibility/test gates, PostgreSQL
cannot be operated reliably on the intended host, app-server cannot provide required
ownership/read/list/collaboration behavior, or real dogfood proves one Lead cannot keep
decision latency within policy. Such evidence blocks or revises the owning gate; it does
not authorize a compatibility facade or silent fallback.

For the XY-1284 managed-repository boundary, the prioritized falsifiers in the
[gate manifest](../specs/vnext-gates.md#xy-1284-managed-repository-authority-gate) are
incorporated into this decision as decision-changing evidence. In fixed order, they are:
an architecture that cannot preserve admitted authority or separate its distinct state
owners; unrecoverable or ambiguous restart/effect states; a security/authority path that
does not fail closed or cannot disable repository-controlled execution; evidence that
cannot distinguish completion from stale, duplicate, rollback, lost, or ambiguous
outcomes; integrity that permits unreserved, drifted, undetected, or unreconciled effects;
and performance that cannot meet an explicit later host budget without weakening an
earlier guarantee. An earlier class cannot be traded for success in a later class. Any
such contradiction stops the affected replacement gate and returns to an explicit
architecture decision; it never authorizes a fourth patch under the rejected combined
boundary, a mechanism outside the accepted stage-two contract, or a silent fallback.
