---
type: "Reference"
title: "Lane Authority V2"
openwiki_generated: true
---

# Lane Authority V2

Status: superseded and frozen by [XY-1260](vnext-authority.md). Historical architecture
and incident provenance only; do not implement or advance C1-C7.

Tracking issue: [XY-1251](https://linear.app/hack-ink/issue/XY-1251/lane-authority-v2-unify-decodex-project-lane-lifecycle-and-effect-authority)

## Decision

Decodex will replace its duplicated project, issue-lane, review, and recovery authority
with one project-qualified lane aggregate and one transition/effect protocol. The cutover
is intentionally architectural: the completed runtime must not keep legacy ownership
readers or writers as a compatibility path.

This decision addresses three connected failures:

- Program Intake can persist a caller-selected project without proving that the issue is
  in that repository's tracker scope.
- leases and worktrees carry `project_id` but are keyed globally by `issue_id`, so a bad
  admission can overwrite or retain the wrong project mapping.
- review, closeout, supersession, cleanup, and conflict occupancy are reconstructed from
  multiple projections and then mutated through an imperative cross-system sequence.

XY-1249's no-effective-delta retry policy and the manual landing `--related` contract
are adjacent defects. They will consume the new lane contract but will not control the
authority migration rollout.

Their final behavior is not optional: one unexpected no-effective-delta result schedules
one deterministic diagnostic retry, a second identical result terminalizes the operation
as reason-coded attention, and independently proven already-satisfied work may complete
only through a distinct validator decision. Manual-authority commit and land commands do
not accept `--related`; issue relationships belong to typed Lane authority, never the
commit record.

## Authority Model

### Project binding

Each registered project has an immutable `ProjectBinding` containing:

- a generated UUID `project_key`, stored in the registered project contract and runtime;
- the immutable GitHub repository database id; current `owner/repository` is a separate
  mutable provider-read locator projection;
- the Linear workspace id and an executable routing predicate over immutable team and
  label ids;
- a monotonically increasing binding revision and its predicate schema version;
- a path-independent semantic policy/configuration fingerprint.

The local checkout path and caller-selected service id are not repository authority.
Registration, intake, and dispatch must prove the binding independently.
`service_id` remains a separate display/operator alias, repository locator is a mutable
projection, and host checkout attestation is a separate local resource; none participates
in immutable binding/Lane fingerprints.
One active ProjectBinding owns one
GitHub repository database id. Predicate evaluation across active bindings must return
exactly one match: zero matches rejects admission, and overlapping matches quarantine
the request. Multi-repository work uses project-scoped child issues rather than one
executable issue with overlapping repository predicates.

Changing repository or tracker identity creates a new project. Changing an eligibility
predicate creates a new binding revision under the same project key. Existing lanes
remain bound to their admitted revision and stop on drift until an explicit rebind or
transfer transition accepts the new revision.

Binding revisions are `current` or `historical`. Exactly one current revision exists per
ProjectKey, and only current revisions participate in routing. Publishing a new current
revision and retiring the previous one is atomic after a global overlap check and
RoutingCatalog epoch CAS. Registration, revision, pause/resume/retire, and project
quarantine adjudication all increment that global catalog in the same transaction.

Project availability is separate revisioned authority with an epoch and
`active|paused|retired` state. Only active projects route. Pause/resume/retire are kernel
transitions with fresh global routing and dependency checks; retirement preserves
history and project deletion may not cascade through lanes, Programs, or evidence.

### Lane identity

The tracker identity is
`TrackerIssueKey(provider, workspace_id, immutable_issue_id)`; team, identifier, and
title are mutable attributes. The canonical lane identity is
`LaneId(project_key, TrackerIssueKey)`. One lane aggregate owns current lifecycle and
ownership state. Attempts, worktrees, control channels, Programs, review state,
evidence, conflict leases, and effects reference the lane.

Historical or quarantined records for one tracker issue may exist under more than one
project. Only one non-terminal tracker-issue claim may exist. Cross-project movement is
an explicit transfer/release transition, never an overwrite.

Quarantine creates a durable reservation for the TrackerIssueKey. Admission, dispatch,
and transfer stay blocked until a typed adjudication transition resolves it; removing
the executable claim alone never makes the issue admissible.

### Intake authority

`IntakeAuthority` is a typed union:

- `decision_contract`, carrying the accepted contract id and fingerprint; or
- `issue_batch`, carrying the accepted intake id, actor/source, timestamp, and
  fingerprint; or
- `transfer`, carrying immutable TransferAuthority, source IntakeAuthority provenance,
  and fresh destination binding attestation.

A valid issue batch does not fabricate a Decision Contract or non-null
`source_contract_id`. All variants carry project-binding attestation, plan/program
identity, correlation id, and source provenance.

### Transition and effects

The pure lane transition kernel receives fresh normalized facts and returns a
`TransitionPlan`. Applying a plan uses:

1. authoritative external readback and prerequisite fingerprints;
2. a lane epoch compare-and-swap claim;
3. durable ordered effects written before public mutation;
4. deterministic effect ids, receipts, retry state, and reconciliation;
5. post-effect readback; and
6. one terminal runtime transaction that records authority and releases owned
   resources.

SQLite, Linear, and GitHub cannot form one distributed transaction. Decodex therefore
uses a durable saga/outbox and convergence, not an atomicity claim. Public comments are
projections and never lifecycle authority.

Provider capability is part of the contract, not an optimistic assumption. Current
Linear create has no immutable idempotency primitive and current update/archive has no
conditional version/CAS mutation. V2 therefore rejects automated Linear issue creation
and overwriting/destructive issue mutations before invocation; operators supply existing
issues through issue-batch intake. Append-only public comments remain non-authoritative
when their marker/readback semantics pass. A future provider capability can enable the
generic create/update protocol only with immutable idempotency/conditional-CAS tests.
The current capability-degraded workflow uses explicit Program Intake, internal Lane
authority, GitHub landing, and local cleanup end to end; it removes mandatory Linear
label/state/closeout tools and queue-label polling. Optional append-only comments are
detachable projection debt and cannot block Lane completion.

### Supersession

A typed supersession edge records immutable predecessor and successor lane, issue, PR,
head, and successor merge identities. Generic tracker relations, comments, PR titles,
labels, worktree presence, or historical closeout comments are supporting evidence only.
Terminal supersession authority and conflict-lease release commit together.

`SupersessionAuthority` is either a runtime-created `repair_handoff` established when a
successor repair lane is created, or an explicit `operator_attestation` for legacy
recovery. Both forms bind exact predecessor/successor identities and must pass the same
acceptance transition. Supporting evidence cannot create an edge by itself.

Because a successor PR and merge do not exist at repair-lane creation,
`RepairHandoffAuthority` and `SupersessionAcceptance` are separate immutable records.
The latter binds the landed successor identities and a typed disposition for every
unique predecessor patch. Legacy acceptance requires accountable operator and distinct
reviewer identities.

PR #1073 is designated superseded/do-not-merge. Its production implementation and
imperative mutation sequence must not be merged or cherry-picked. Its incident facts,
review findings, and behavioral scenarios may be re-expressed as v2 fixtures. It is
closed only after the replacement supersession capability is landed and read back.

## One-Shot Migration

The schema cutover runs offline under an exclusive runtime lock:

1. dry-run and emit a sanitized classification report;
2. create a runtime backup;
3. migrate only rows whose project and lane identity are uniquely proven;
4. quarantine every surviving row in an ambiguous lane's connected component and
   record tombstones for predecessor records known to have been overwritten but no
   longer recoverable;
5. atomically install the v2 schema and readers/writers; and
6. make older binaries refuse the new schema.

There is no long-lived dual-read or dual-write mode. Cutover takes a SQLite exclusive
transaction on the legacy database, checkpoints WAL, and holds the transaction until the
entire v12 state directory is atomically detached to the journaled migration-input path.
It atomically installs a non-database tombstone directory at the old database pathname
before releasing the transaction. A baseline binary that opened the old inode before the
rename cannot write through the exclusive lock and, after release, can affect only the
detached immutable legacy generation; one that opens afterward fails on the tombstone.
Migration prepares and fsyncs a generation-specific v2 state directory that no v12 binary
knows, creates and verifies an immutable WAL-safe SQLite backup, and publishes the
runtime-format manifest last by atomic rename plus directory fsync. V2 startup never
falls back to the v12 path. Journal, detached-input digest, tombstone, manifest, and v2
database generation disagreement freezes all mutation.

Before schema cutover, a guard-only prerequisite release moves every supported CLI,
daemon, app, MCP, automation, and shim launch through one version-pinned supervisor that
acquires the global runtime-generation lock before opening state. Migration takes the
exclusive supervisor lock and the independent SQLite exclusive transaction before path
detachment. The supervisor lock prevents supported launches; the SQLite/path protocol
prevents even an accidentally invoked unmanaged baseline binary from racing or reopening
the authoritative generation. Deliberate same-account tampering with journaled paths or
credentials is outside the non-malicious local-operator threat model and is detected by
signed generation and audit-chain verification.

Before the first v2 mutation outside the database restore unit, including Git,
filesystem, process, hook, Linear, or GitHub effects, Decodex writes and fsyncs a
point-of-no-return fence outside that unit. Rollback tooling permits backup restoration
only when that fence is absent. A crash after an effect attempt therefore always
requires readback and roll-forward reconciliation, even when no receipt exists.

V2 authority-mutating Linear and GitHub effects use in-process provider clients rather
than `gh` subprocesses. Local subprocess effects run in supervised process groups and
must be terminated and reaped before their process-generation lock is released.

### Checkpoint staging and activation

C1-C6 may land implementation incrementally, but v2 runtime mutation stays dormant until
the C7 cutover. The runtime-format selector admits exactly one authority generation:
before cutover, supported launches use the v12 runtime and every v2 mutation entrypoint
fails closed; after cutover, the final binary accepts only v2 and legacy runtime modules
are absent except for the read-only offline migration decoder. Fixture migration/apply
tests do not select a host runtime generation.

This is expand-then-one-shot-cutover, not dual authority. C1-C6 PRs must keep main
operational under the sole v12 authority while building and testing unreachable v2
components. C7 lands the removal/activation release, stops the supervisor, runs the
offline migration, publishes the v2 manifest, and restarts only the exact accepted
binary. No supported binary can operate normal v12 and v2 mutation paths concurrently.

C1 itself has three ordered, non-skippable subgates. C1I first lands only the
multi-language AST/syntax/call-graph inventory verifier and exact baseline
classifications against C0 source-node digests; it cannot change runtime behavior. C1A
then lands and deploys only the
guard/supervisor release, then records exact binary, shim, app, daemon, MCP, and
automation launcher identities plus lock-acquisition readback. Host migration apply is
disabled before and after C1A. C1B may land dormant v2 foundations and migration tooling
only after that deployment evidence is accepted; live apply remains disabled until the
exact C7 activation release. This prevents migration code from depending on an
undeployed launch guard in the same release.

## Telemetry Decision

Authority telemetry is a private tamper-evident append-only ledger, separate from
ordinary logs. Each event carries a global generation-local monotonic sequence,
previous-event hash, canonical event hash, event, transition, correlation, and causation
ids; project binding and LaneId; actor/source; observed-fact fingerprints; decision and
reason codes; effect ids and receipts; runtime version; and timestamps. AuthorityTransaction
appends state, event, sequence, and chain hash atomically. After commit the supervisor
advances a HostAuthorityKey-signed protected chain head in KeyProtector. Startup and
offline audit detect deletion, rewrite, truncation, reordering, fork, and protected-head
mismatch; wall-clock time is evidence only and never event ordering authority.

Admission telemetry is written before Program persistence and records invocation origin,
accountable principal/job/thread or automation, requested selector, config-resolution
source, all candidate binding fingerprints and predicate results, resolver version,
selected binding and reason, and propagated correlation id. This is the minimum evidence
needed to attribute a PUB-1711-style wrong-project selection.

Ordinary tracing records reference the authority event id. Operator diagnose, timeline,
evidence, and audit surfaces read the structured ledger. Public tracker or GitHub text
contains only privacy-classified summaries.

## Rejected Alternatives

- Requiring every Program to have `source_contract_id`: valid issue-batch authority is
  different from Decision Contract authority.
- Adding repository checks only at intake: persisted Programs and tracker facts can
  drift before dispatch.
- Making `(project_id, issue_id)` a composite worktree key without a global active
  tracker claim: this prevents overwrite but still permits two projects to execute the
  same issue concurrently.
- Continuing to extend PR #1073's validator: it reconstructs authority from projections
  and does not provide a general effect protocol.
- Long-term compatibility readers or dual authority: they preserve the failure class
  the migration is intended to remove.

## Stop Conditions

Stop and re-plan rather than weakening acceptance when a change would:

- add another ownership projection;
- infer ambiguous legacy ownership;
- let Program dispatch bypass project binding;
- treat public comments or generic tracker relations as authority;
- release conflict ownership before terminal authority commits;
- perform an irreversible external mutation without a durable effect and fresh
  prerequisite readback; or
- expose private runtime evidence on a public surface.
