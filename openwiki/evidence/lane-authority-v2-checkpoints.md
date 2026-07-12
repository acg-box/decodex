# Lane Authority V2 Checkpoints

Tracking issue: [XY-1251](https://linear.app/hack-ink/issue/XY-1251/lane-authority-v2-unify-decodex-project-lane-lifecycle-and-effect-authority)

This is the durable anti-drift ledger for the Lane Authority v2 program. Update it at
every checkpoint with facts, inferences, exact source/PR identities, validation results,
migration state, unresolved objections, scope changes, and the next checkpoint.

## Runtime Implementation Reset

Status: active on draft PR #1092.

Recorded at: 2026-07-12 (America/New_York).

The earlier C0/C1I proof branch and draft PR #1090 are frozen. They are incident and
inventory evidence only, are not a runtime prerequisite, and must not be landed as the
Lane Authority implementation. In particular, generated whole-repository source graphs,
generic AST/call/dataflow analysis, and their large audit artifacts are outside the
runtime cutover path. Reusable incident fixtures, mutation inventories, target contracts,
and scenario ids may be retained only when a runtime gate consumes them directly.

The runtime implementation is isolated in `.worktrees/lane-authority-v2-runtime` on
`xv/lane-authority-v2-runtime`, with draft PR #1092. The following exact commits are
implemented and pushed:

- `8bfbce85`: immutable `ProjectBinding`, required GitHub owner/repository and tracker
  team configuration, project registry persistence, and drift rejection.
- `f2081b1d`: pure typed lane transition kernel.
- `806059f2`: project-qualified lane aggregate, binding/epoch CAS, and transactional
  SQLite persistence.
- `ec82217e`: fail-closed tracker-team filtering before queue mutations or dispatch.
- `890b425a`: registered project/GitHub/tracker binding attestation before recovery,
  baseline, lease, or worktree side effects.
- `449b5197`: normal dispatch lease acquisition/release projected from canonical lane
  claim commands, including retry and cross-project collision tests.
- `541a79e6`: normal dispatch worktree attachment through the canonical lane command.
- `fcced4aa`: immutable typed Intake Authority union and deterministic tamper-evident
  fingerprint.
- `b7c4a56a`: issue-batch Intake Authority persisted atomically with its Execution
  Program; reapply preserves the original accepted authority and restart validates it.
- `6ca0f718`: accepted Decision Contract actor/source/time/fingerprint and project
  binding persisted as typed Intake Authority; Program dispatch rejects missing,
  wrong-kind, or stale-binding authority outside test-only legacy fixtures.
- `0f78ef5c`: canonical lane aggregate persists immutable Intake Authority identity.
- `53570a7c`: normal queue intake creates deterministic typed issue-batch authority and
  seals it into the lane before lease/worktree effects.
- `1f9f01da`: Program dispatch admits the lane only through its persisted Program
  authority.
- `f66d6b65`: production lane claims reject unless immutable typed admission already
  succeeded; targeted Program selection attests binding before recovery side effects.
- `af9fc6cd`: normal runtime attempt writes require project-qualified admitted LaneId and
  refuse movement of a run id between lanes.
- `e4f9d7fb`: run-control publication resolves ownership from the project-qualified
  attempt and active canonical lane claim instead of the legacy lease projection.

Fresh focused evidence on this runtime branch:

- lane authority tests: 7 passed;
- state tests: 114 passed;
- lease tests: 17 passed;
- intake tests: 93 passed;
- issue-batch Program Intake tests: 7 passed;
- goal Program Intake tests: 13 passed;
- Program reconciler tests: 18 passed;
- `cargo check -p decodex --all-targets`: passed;
- `git diff --check`: passed before each runtime commit.

Current limitations are explicit. Legacy `leases` and `worktrees` remain temporary
projections and many recovery/read surfaces still use issue-only keys. Typed
`IntakeAuthority`, effect journal/outbox, migration/quarantine, supersession, telemetry,
and adjacent XY-1249 fixes are not complete. No C1 or C2 completion claim is valid until
those old authority writers are removed or converted, PUB-1711 is replayed end to end,
and the one-shot migration gate proves no executable ambiguity.

PUB-1711 now has an end-to-end runtime regression: after registering one repository
binding, rewriting the same service config to another GitHub repository makes normal
intake fail at project binding attestation. The fixture verifies that no lease, worktree,
or run attempt is persisted. This proves the pre-side-effect rejection path that now
feeds the completed new-admission behavior below.

C2 new-admission behavior is now complete: normal queue and Program Intake both persist
typed authority, exact authority identity is sealed into the canonical lane, and the
kernel refuses claims without admission. C2 does not adjudicate pre-v2 rows; legacy or
ambiguous runtime rows remain blocked on the C1 one-shot migration/quarantine work.

The current sole-writer cutover has started but is not complete. Normal runtime attempt
and run-control writes are lane-qualified. Recovery review-handoff adoption and runtime
activity recovery still contain issue-only attempt/lease writers; retry-budget and thread
archive reads are still issue-only; legacy lease/worktree projections and tables still
exist. None of those paths may be treated as v2 authority or used to claim C1 complete.

Next implementation checkpoint: carry binding/intake authority into the lane aggregate,
convert attempts/control/review/Program references and recovery worktree adoption to
project-qualified lane commands, then remove issue-only lease/worktree authority APIs.

## C0 Baseline And Architecture Freeze

Status: in progress.

Recorded at: 2026-07-09 (America/New_York).

### Verified facts

- Decodex `main` and `origin/main` both resolved to
  `92e181707c26b36642466badb2c37f571e36bd01` before the C0 worktree was created.
- The root worktree was clean on `main`.
- PR [#1073](https://github.com/hack-ink/decodex/pull/1073) was open and clean at
  `2377fb44a3c89b7ef8aa46714472d972191bd522`; its CodeQL checks were successful.
- The #1073 branch was clean and changed 32 files with 4,169 insertions and 60
  deletions relative to `main`.
- XY-1248 was `In Review`, XY-1249 was `Backlog`, and XY-1250 was `Todo`. XY-1250
  blocked XY-1248 when read back. XY-1251 is the umbrella issue.
- Eleven projects were registered and enabled. Both `pubfi` and `pubfi-mono` existed
  as independent registered services.
- The checked-in schema version was 12. `leases.issue_id` and
  `worktrees.issue_id` were global primary keys even though both tables carried
  `project_id`.
- `execution_programs`, Program Intake mappings, review lifecycle, private execution
  events, and several control tables already used project-qualified keys. The authority
  model was therefore inconsistent rather than uniformly global.
- The Decodex operator snapshot reported no current Decodex worktree mappings but did
  report a `control_plane_tick_context_failed` warning. Private warning details are not
  recorded here.
- Git reported 45 linked worktrees before implementation. The count includes existing
  unrelated worktrees plus the clean XY-1248 worktree and the new C0 worktree. No
  unrelated worktree was modified or removed.

### Verified source causes

- Issue-batch intake resolves supplied issue identifiers under the caller-selected
  service and persists the resulting Program under that service.
- `repo:*` labels currently create conflict domains; they do not prove project
  admission.
- Program dispatch records program, node, issue, run, source-contract, and queue intent
  but does not record the selected repository binding, routing attestation, actor, or
  source invocation.
- PR #1073 sequences cross-system superseded-closeout mutations without a general
  durable effect/outbox model.

### C0 baseline refresh

Refreshed at: 2026-07-10 (America/New_York).

- While C0 skeptic review was in progress, `origin/main` advanced from the original
  `92e181707c26b36642466badb2c37f571e36bd01` baseline through automation authority/
  scorecard/handoff changes and then Publisher terminal-SLA commit `4626aaa5`.
- The isolated C0 branch was rebased without conflict and now matches current
  `main`/`origin/main` at `24022558732b57b0bf96fac5bc4d16cd9613c3ad` before its
  uncommitted OpenWiki changes. The root worktree was clean at the same commit.
- New reviewed surfaces include `automations/decodex/scripts/config/automation_checkout.py`,
  primary-main-checkout resolution, automation sync/live-config writes, evaluator/
  manager/daily/weekly jobs, effectiveness scorecards, handoff reconciliation, and the
  Publisher terminal-outcome SLA.
- C1A launcher inventory now explicitly covers these automation launchers and primary
  checkout rules. The mutation inventory is multi-language and classifies live automation
  config, account/auth, usage-history, automation-artifact, Python subprocess/path writes,
  and every v12 legacy callsite with a replacement owner and C7 removal checkpoint.
- The original baseline remains historical incident evidence only. C0 readiness and all
  later source scans use `24022558732b57b0bf96fac5bc4d16cd9613c3ad` or a newer explicitly
  recorded refresh, never the stale SHA.

### C0 second baseline refresh

Refreshed at: 2026-07-10 (America/New_York).

- `origin/main` advanced through merge commit
  `d57553bc1bcdceebe1d0c7ec5ad5dc492b695348`, containing product-coupling removals
  `4d85d45d` and `5e351cc9`. The isolated branch was rebased without conflict and its
  uncommitted C0 documents restored exactly; the current implementation baseline is now
  `d57553bc1bcdceebe1d0c7ec5ad5dc492b695348`.
- Decodex removed its `openwiki` CLI command, OpenWiki MCP resources/templates,
  OpenWiki terminal-checkpoint fields, and automation knowledge-path coupling. OpenWiki
  remains repository knowledge for maintainers; it is not a Decodex product command,
  runtime input, dynamic-tool capability, or authority precondition.
- C0 and C7 therefore no longer invoke `decodex openwiki check`. The repository-owned
  broad readiness gate is `cargo make check`; document whitespace/current-main checks
  and fresh skeptic review remain independent C0 evidence.
- Reviewed runtime changes include strict schema/version matching for private execution
  and authority events, `decodex.authority_boundary_check/2`, revised blocking-evidence
  recovery, tracker completion/progress/review surfaces, private-event persistence, CLI
  root-surface closure, and removal of knowledge-derived lifecycle fields.
- C1 must regenerate both launcher and mutation inventories from this exact baseline.
  Removed OpenWiki surfaces must not survive as launchers, effects, telemetry fields, or
  compatibility adapters. Existing tracker/provider writes, private event writes,
  authority-event writes, and recovery readbacks remain classified mutation or authority
  surfaces even when this upstream change only tightened their payload contract.
- The earlier SHAs remain historical review points only. Readiness and all later scans
  use `d57553bc1bcdceebe1d0c7ec5ad5dc492b695348` or a newer explicitly recorded refresh.

### Inferences

- The persisted `issue-batch-pubfi-*` Program proves that PUB-1711 was admitted under
  `pubfi`; current evidence does not prove which caller or automation selected that
  project. Lane Authority telemetry must make that attribution available in future.
- Existing overwritten global-key rows cannot be reconstructed safely from the winning
  row alone. Migration must use independent evidence or quarantine them.

### Architecture decisions

- [Lane Authority v2](../decisions/lane-authority-v2.md) is the accepted target.
- [Lane Authority v2 target contract](../specs/lane-authority-v2.md) defines records,
  transitions, migration, telemetry, and the scenario matrix.
- PR #1073 is not the final implementation path. Its requirements and tests remain
  evidence until replacement capability is proven.
- Migration is an offline one-shot cutover, not a long-lived dual-authority rollout.

### Unresolved objections

- Provider capabilities for conditional PR close/reopen, exact-head merge, and comment
  reconciliation must be verified before C3 implementation. Unsupported semantics are
  an automation stop, not a reason to weaken the effect contract.
- The 45-worktree inventory must be classified before C7; unrelated user-owned
  worktrees cannot be destructively removed as part of this program.

### First skeptic review

Verdict: C0 not ready.

The review found five architecture blockers and five high/medium gaps. C0 was revised as
follows:

- froze UUID ProjectKey, immutable RepositoryKey, TrackerIssueKey, binding revisions,
  exactly-one routing, and uniqueness behavior;
- added an authority-disposition table, single transition writer, transactional
  invariants, and a reverse-scan gate for old readers/writers;
- separated surviving quarantined rows from unrecoverable overwritten-row tombstones
  and made quarantine cover the full TrackerIssueKey connected component;
- added an fsynced runtime-format manifest, non-SQLite legacy sentinel, external
  point-of-no-return fence, and operator-verifiable rollback rule;
- added claimant fencing, effect states, ordinal barriers, unknown-outcome
  reconciliation, and per-effect compensation/stop rules;
- defined typed SupersessionAuthority and marked PR #1073 superseded/do-not-merge;
- expanded pre-persistence admission attribution and the projection privacy allowlist;
  and
- assigned stable scenario ids and falsifiable entry/exit gates to C0-C7.

The first review does not satisfy the C0 skeptic gate because it rejected readiness. A
second fresh review must validate the revised contract with no unresolved blocker/high
authority objections.

### Second skeptic review

Verdict: C0 not ready.

The second independent review found eight remaining high gaps and two medium gaps. C0
was revised again:

- binding revisions are now current/historical with exactly one current revision and an
  atomic publication/overlap check;
- quarantine now holds a persistent TrackerIssueKey reservation until typed,
  dual-accountable adjudication;
- the authority disposition inventory now includes protocol/activity summaries, Linear
  projection events, review checkpoints, evidence artifacts, loop guardrails, connector
  backoff, autonomy records, and filesystem markers;
- cutover now has a durable journal and strict prepare/move/sentinel/manifest ordering,
  directory fsyncs, generation/hash agreement, and a crash scenario after every
  filesystem operation;
- effect execution now requires an OS process-generation lock through I/O, forbids
  reassignment of a live `invoking` worker, and defines legal forward, unknown,
  compensation, and blocked transitions;
- supersession is staged into immutable RepairHandoffAuthority and later
  SupersessionAcceptance with typed predecessor patch dispositions;
- admission decision and Program persistence now commit together through a non-null
  event reference;
- `terminal_cleanup_pending` is non-executable, releases tracker/conflict claims, and
  retains only fenced cleanup ownership;
- projection privacy now uses named, deny-by-default field allowlists including metrics
  and error/crash output; and
- the normative gate manifest fixes scenario ids, fixture paths, commands, assertions,
  reverse scans, and evidence requirements.

The second review also does not satisfy the C0 gate. A third fresh review must return
`READY` with no unresolved blocker/high authority objection.

### Third skeptic review

Verdict: C0 not ready.

The third independent review found ten remaining high gaps and one provenance correction.
C0 was revised again:

- added a guard-only prerequisite release and version-pinned supervisor so every
  supported v12 launch honors the runtime-generation lock before schema cutover;
- specified WAL-safe SQLite Online Backup, integrity/logical-equivalence verification,
  complete restore-unit behavior, and restore crash scenarios;
- prohibited `gh` subprocesses for v2 provider mutations, required in-process clients,
  and required supervised/reaped process groups for local subprocess effects;
- separated `forward_applied` from `saga_finalized`, added reverse-order compensation,
  operation abort states, and irreversible-final-ordinal behavior;
- added a normative exhaustive effect registry and direct-mutation enforcement gate;
- added ProjectBinding/repository/routing revalidation before every effect;
- defined the canonical predecessor PatchSet, path-unit digests, exceptional commit
  classes, and invalidation on force-push/base/head drift;
- added a machine scenario manifest verifier that fails on missing, duplicate, skipped,
  unexpected, or zero-match tests;
- added a machine legacy authority inventory covering issue-only lease/worktree APIs and
  direct SQL writers, with dominance through `LaneStore::commit_transition`; and
- required centralized typed terminal/tracing/panic/provider sinks, secret-injection
  tests, and a direct-output reverse scan.

The recorded checkpoint date was corrected to the local America/New_York date. The third
review does not satisfy the gate.

### Fourth skeptic review

Verdict: C0 not ready.

The fourth completed independent review found eleven high gaps and two medium gaps. C0
was revised again:

- moved the foundational AuthorityOperation/effect store, claimant fencing,
  reconciliation, uniqueness, PONR protocol, and core effect scenarios into C1; C2 issue
  creation/archive consumes that protocol, while C3 expands the remaining registry;
- moved the central typed OutputBoundary, panic/error/provider sinks, direct-output
  verifier, privacy corpus, and TEL-04 into C1 before any v2 mutation can ship;
- widened the mutation registry to every project/lane-relevant mutation regardless of
  entrypoint and added fenced Linear archive/unarchive hygiene;
- moved the PONR boundary from first network call to the first Git, filesystem, process,
  hook, Linear, or GitHub mutation outside the restore unit;
- defined a separately fsynced, idempotent rollback journal with crash recovery at every
  restore stage while preserving the immutable backup;
- classified effects as compensable, durable publication, or irreversible terminal;
  split new-ref and update-ref push semantics and added push-then-PR-failure roll-forward
  scenarios;
- required ProjectBinding/Lane/object/epoch revalidation before every forward, retry,
  reconciliation read/write, and compensation invocation;
- made the machine v12 inventory define graph edges for every table, file, marker,
  event, checkpoint, and receipt, with every row classified into one component or an
  unattached diagnostic/tombstone;
- explicitly migrated the legacy manual-authority closeout receipt to proven scoped
  authority or a non-authoritative diagnostic/tombstone and required removal of its old
  reader/writer;
- prohibited `runtime.intake_commit` from writing Lane/claim state except through the
  same private `LaneStore::commit_transition` primitive;
- specified typed `rebind` and `adjudicate_quarantine` transitions with fresh routing,
  immutable identities, no active operation, epoch CAS, atomic reservation/claim
  updates, and dual accountability;
- replaced textual Git diff canonicalization with versioned raw-object
  `decodex.patch_set/1`; and
- added effect primary/ordinal/target-scoped idempotency uniqueness constraints and a
  rejecting scenario.
- froze checkpoint activation so C1-C6 land dormant v2 components while v12 remains the
  sole active generation; C7 removes legacy runtime modules and performs the one-shot
  host cutover, avoiding an unusable intermediate release or dual authority.

The fourth review does not satisfy the gate. A fifth fresh review must return `READY`
with no unresolved blocker/high authority objection.

### Fifth skeptic review

Verdict: C0 not ready.

The fifth independent review found one blocker, eight high gaps, and two medium gaps.
C0 was revised again:

- replaced the untyped TrackerIssueKey connected component with typed migration
  partitions and edge kinds: project roots migrate once, lane-affinity edges spread
  quarantine, multi-issue Programs use explicit ExecutionGroups, and shared project or
  connector references cannot quarantine unrelated lanes;
- added epoch-fenced ProjectAvailability with active/paused/retired transitions,
  deterministic resume overlap checks, dependency-gated terminal retirement, immutable
  history, and removal of cascading project deletion;
- split C1 into C1A guard deployment and C1B dormant foundation/migration tooling, with
  separate PR/deployment evidence and host apply disabled until the exact C7 release;
- added every registered project contract to the immutable migration backup bundle,
  cutover journal, per-contract fsync ordering, rollback stages, and DB/contract
  agreement checks;
- introduced a single AuthorityTransaction that atomically owns lane/availability CAS,
  operation/effect creation, active-operation pointer, authority event, sole Lane writer,
  and Intake/Program/ExecutionGroup rows, with project-to-lane handoff after issue create;
- corrected effect semantics: PR creation is durable publication; spawn is separated
  from irreversible interrupt/terminate; destructive cleanup orders remote ref,
  worktree, then final local ref and never claims false compensation;
- required every EffectStore transition to CAS effect state, claimant epoch, lane epoch,
  and project availability epoch before exposing a sealed invocation capability;
- froze TransferAuthority, source/destination epochs and bindings, exactly-one target,
  complete resource/conflict disposition, atomic claim movement, and immutable source
  Program history;
- expanded the mutation registry from the actual call graph to fetch, ref, default-branch
  index/worktree, cleanup, process, and canonical-object boundaries, enforced by AST/
  call-graph analysis and sealed capabilities rather than grep;
- froze routing predicate v1 as normalized finite team/required-label/forbidden-label
  clauses with deterministic intersection and fail-closed schema upgrades; and
- defined the exact PatchUnit disposition universe as endpoint path deltas, empty
  commits, and merge topology records, with evidence-only objects/ordinary commits
  excluded from one-to-one disposition counting.

The fifth review does not satisfy the gate. A sixth fresh review must return `READY` with
no unresolved blocker/high authority objection.

### Sixth skeptic review

Verdict: C0 not ready.

The sixth independent review found one blocker, eight high gaps, and one medium gap. C0
was revised again:

- introduced durable IntakeIntentId, provider marker, PublishedTrackerIssueReservation,
  unified TrackerIssueOccupancy, and a receipt-bound deterministic child-operation
  protocol so issue-create effects remain immutable yet admission/cleanup can be planned
  after the immutable issue id exists;
- required fully paginated, double-pass stable RoutingIssueSnapshot facts and fail-closed
  behavior for provider caps, incomplete labels, forbidden labels beyond page one, or
  torn team/label/version reads;
- added journaled pending-contract-to-current ProjectPublication for normal registration
  and revisions, canonical UUID bytes, and one immutable MigrationPlan that allocates
  ProjectKeys once for plan/dry-run/apply;
- froze the complete operation FSM and one finalize_operation AuthorityTransaction that
  atomically finalizes effects, Lane/resources/event, active-operation pointer, and
  operation state with statement-level failure tests;
- constrained supersession to one active handoff per predecessor epoch and one terminal
  edge per predecessor Lane, with typed replacement/cancellation and CAS losers retained
  as rejected-stale history;
- added Git remote-config mutation plus accepted-before-act soft interrupt/steer request
  and response-receipt protocols with request-id deduplication;
- added deny-by-default `authority_agent_evidence_private/1` and required the evidence
  adapter to serialize only typed ids/enums/digests, never raw paths/cwd/provider/protocol
  content;
- added sealed transport/supervisor-derived InvocationIdentity and made all caller-sent
  identity fields untrusted metadata unable to populate accountability;
- upgraded migration inventory to a closed-world source-node manifest covering schema,
  every reader/writer/path discoverer, owned artifacts, partition/quarantine rules, and
  unknown-artifact refusal; and
- specified first-parent/root semantics for canonical empty-commit PatchUnits and
  byte-level merge fixtures.

The sixth review does not satisfy the gate. A seventh fresh review must return `READY`
with no unresolved blocker/high authority objection.

### Seventh skeptic review

Verdict: C0 not ready.

The seventh independent review found one blocker, eight high gaps, two medium gaps, and
one low prose defect. C0 was revised again:

- made provider capability explicit: current Linear lacks immutable create idempotency
  and conditional update/archive CAS, so automated create/destructive/overwriting issue
  effects reject before invocation; only a capable-provider fixture may exercise the
  receipt-bound generic protocol, and occupancy collisions quarantine without adoption
  or cleanup;
- replaced the unconditional availability epoch with a subject-tagged expected authority
  version for registration, existing project, project quarantine, Lane, and migration;
- defined non-self-referential project-contract hashing with a semantic content
  fingerprint and separate final-file digest;
- froze all non-contract legacy filesystem artifacts byte/mode/path-identical before
  PONR and required rollback to verify the full MigrationPlan hash inventory; registered
  cleanup may retire them only after PONR;
- forced fetch into isolated operation refs with `--no-write-fetch-head`, removed shared
  ref/FETCH_HEAD mutation, and added exact credential-helper publish/retire effects;
- made the supervisor the sole StateStore writer and defined one-use, FD-delivered,
  binary/process/session-bound invocation credentials plus AccountabilityRoot uniqueness
  for independent operator/reviewer authority;
- expanded C7 into explicit binary pin, drain/stop, one-use cutover receipt, plan,
  dry-run, apply, exact-binary activate/restart, status, and generation readback commands;
- defined unique best merge-base requirement and deterministic parents-first Kahn/raw-OID
  commit ordering for PatchSet, rejecting criss-cross/missing/shallow ambiguity;
- clarified retirement preserves terminal Lane history; and
- fixed the untracked-file whitespace gate to require both expected no-index exit and
  empty diagnostics, and removed duplicated normative prose.

The seventh review does not satisfy the gate. An eighth fresh review must return `READY`
with no unresolved blocker/high authority objection.

### Eighth skeptic review

Verdict: C0 not ready.

The eighth independent review found one blocker, seven high gaps, and one medium gap.
C0 was revised again:

- added singleton RoutingCatalog epoch/digest and made registration, revision,
  pause/resume/retire, and project-quarantine adjudication re-evaluate the whole active
  catalog and CAS/increment it atomically, preventing concurrent overlap publication;
- added the `transfer` IntakeAuthority variant with immutable TransferAuthority,
  original source authority/provenance, and fresh destination attestation;
- gave ExecutionGroup an epoch, explicit planned/active/draining/terminal/quarantined
  lifecycle, and kernel terminal prerequisites so historical mappings and retirement are
  unambiguous;
- registered lane-attempt worker spawn/interrupt/terminate/reap and replaced legacy
  issue-claim/dispatch-lock authority with occupancy/operation/supervisor CAS, requiring
  old readers/writers/artifacts to disappear;
- unified remote-ref creation as durable publication, required server-enforced Git lease
  for deletion, and marked current unconditional GitHub update/delete unsupported;
- moved alias and host checkout attestation out of immutable ProjectBinding and defined a
  path-independent semantic configuration fingerprint;
- made first registration atomically create paused availability epoch 1 and made
  migration publish all binding/availability/catalog records in one transaction;
- inserted explicit PONR before supervisor restart and required all post-activation
  status/audit/readback commands to use the exact pinned binary; and
- added encrypted LegacyEvidenceVault migration with Keychain-sealed key, journaled raw
  removal/rollback restore, sanitized v2 indexing, no normal plaintext discovery, and
  dual-accountable offline forensic export/retention.

The eighth review does not satisfy the gate. A ninth fresh review must return `READY`
with no unresolved blocker/high authority objection.

### Ninth skeptic review

Verdict: C0 not ready.

The ninth independent review found one blocker, nine high gaps, and two medium gaps. C0
was revised again:

- separated RoutingCatalog publication serialization from immutable semantic
  RoutingAttestation, adding CAS re-attestation for unrelated catalog changes and a
  readback-first drift recovery path for unknown effects;
- made C1 explicitly allow only machine-inventoried `v12_legacy` callsites while v12
  remains operational, expanded registry/verifiers across Rust/Python/Swift/shell plus
  account/auth/config/usage/automation writes, and kept C7 as the zero-legacy gate;
- added unbound RoutingRequest authority for zero/multiple matches, including atomic
  overlap quarantine occupancy/candidate evidence without fabricated project/Lane/
  Program records;
- bracketed each fully paginated provider snapshot with start/end metadata versions,
  required two consecutive identical valid passes, and made label predicates unsupported
  when provider version coverage cannot be proven;
- routed project-quarantine resolve/split through pending ProjectPublication contract
  effects and one batch catalog/quarantine finalization, so partial projects never appear;
- froze transfer/quarantine ExecutionGroup behavior: source nodes become non-schedulable,
  source group epochs advance/terminalize when complete, and destination group/mapping/
  transfer IntakeAuthority commits atomically;
- made every effect CAS, receipt, and typed telemetry event one AuthorityTransaction with
  transition-sequence uniqueness and no state-only writer;
- defined the current-Linear degraded end-to-end workflow: explicit existing issue,
  internal Lane authority, GitHub landing, local cleanup, no mandatory unsupported
  tracker tools/label polling, and detachable optional comment projection debt;
- replaced the ambiguous encryption design with age-v1 encrypted bundle format,
  in-memory SQLite Online Backup/serialization/zeroization, macOS Keychain or Linux
  Secret Service KeyProtector capability, no plaintext-key fallback, and crash tests for
  named intermediates/rollback temps;
- rebased C0 as main advanced, including the later product-coupling removal baseline
  `d57553bc1bcdceebe1d0c7ec5ad5dc492b695348`, and classified the new automation
  checkout/sync/manager/scorecard/handoff/Publisher SLA launch and mutation surfaces;
- removed the obsolete `decodex openwiki check` gate after upstream eliminated that
  product/runtime surface, while retaining repository-wide validation and independent
  knowledge review;
- added deterministic net-zero path-history PatchUnits for non-empty histories that
  return to base; and
- added exact-binary normal-v2 cutover preflight before PONR with external/process
  capabilities disabled and a rollbackable brokered writer probe.

The ninth review does not satisfy the gate. A tenth fresh review must return `READY` with
no unresolved blocker/high authority objection.

### Tenth skeptic review

Verdict: C0 not ready.

The tenth independent review found zero blockers, seven high gaps, and five medium gaps.
C0 was revised again:

- added provider-level TrackerWorkspaceDirectory and unbound IssueResolutionRequest;
  authority-bearing intake now requires workspace-qualified immutable ids/identifiers and
  cannot let caller-selected project/config/token choose lookup scope;
- made pause atomically rebase non-invoking operations to the new availability epoch,
  increment claimant fencing, and restrict them to reconciliation/compensation/cleanup;
- registered supervised account-login process, private 0700 temp workspace, auth import,
  exact cleanup and removal of secret-workspace preservation;
- replaced loopback preflight with the real kernel IPC/OS peer/anonymous-FD credential
  boundary using journaled exact-binary probe children, negative replay/hash/peer tests,
  and guaranteed crash recovery/reaping before PONR;
- defined Ed25519 HostAuthorityKey in KeyProtector plus deterministic-CBOR cutover receipt,
  plan/binary/host/main/head bindings, signed stage nonce/hash chain, fsynced anti-replay
  journal, and key rotation/revocation rules;
- made rollback verify digest, apply recorded uid/gid and `fchmod` exact source mode,
  fsync, then rename, with 0600/0644 crash fixtures;
- bound C7 immediately to live remote main, local origin/main/HEAD, reviewed PR head,
  landed source commit, tested head and binary SHA, with PONR recheck;
- clarified paused predicates may overlap and only resume/active migration participates
  in active overlap checks;
- added HostCheckoutAttestation resource/epoch to every local effect plan/revalidation;
- replaced ExecutionGroup mapping removal with append-only versioned membership
  dispositions/current pointers;
- defined absent-preimage project-contract compensation as exact unactivated CAS delete,
  otherwise orphan-contract-blocked with quarantine retained; and
- added machine verification that CLI/MCP/dynamic agent tools/scheduler/closeout expose no
  unsupported Linear mutation or queue-label polling surfaces.

The tenth review does not satisfy the gate. An eleventh fresh review must return `READY`
with no unresolved blocker/high authority objection.

### Pre-eleventh-review validation

Validated at: 2026-07-10 (America/New_York).

- Exact source baseline: `d57553bc1bcdceebe1d0c7ec5ad5dc492b695348`; the branch contained
  all of `origin/main` and had only the six intended C0 OpenWiki paths changed or
  untracked.
- The first `cargo make check` invocation stopped before repository validation because
  the isolated worktree had no `site/node_modules` and `astro` was unavailable. This is
  recorded as an environment prerequisite failure, not a passing or product-failure
  result.
- `npm ci` under `site/` installed the lockfile-defined dependencies with zero audit
  vulnerabilities. The unchanged `cargo make check` command then passed site build and
  check, Rust workspace check, Rust/TOML formatting, clippy, vstyle over 3,175 files, and
  all 1,657 executed nextest tests; one repository-declared test remained skipped.
- `git diff --check`, the closed set of five new-file no-index whitespace checks, and
  the deliberate trailing-whitespace negative control all passed.
- The scenario-table definition scan found no duplicate ID, MIG, QUA, ADM, EFX, SUP,
  TEL, or ADJ row identifiers.

These results establish mechanical readiness only. They do not satisfy C0 without the
fresh skeptic verdict, exact-head commit/push, and XY-1251 linkage.

### Eleventh skeptic review

Verdict: C0 not ready.

The first attempted eleventh reviewer produced no result because its independent model
quota was exhausted; it is not counted as a review. A second fresh read-only reviewer
completed and found one blocker, eight high gaps, three medium gaps, and one low gap:

- an unmanaged baseline binary could open v12 state between process drain and sentinel
  publication;
- C7 source/tested/binary provenance was supplied through operator environment values;
- the sole-writer IPC request, acknowledgement, dedupe and crash-resume protocol was not
  frozen;
- append-only telemetry was not tamper-evident;
- superseded closeout lacked a stage-by-stage deterministic crash/retry operation;
- no-effective-delta and manual-authority `--related` still admitted contradictory final
  behaviors;
- launcher/mutation/legacy source inventories were deferred until implementation;
- project-independent resolution did not define host credential/workspace bootstrap or
  removal of config-first tracker construction;
- provider version capabilities did not say which token covered each mutable field;
- selector/principal/locator/identifier projection fields were insufficiently bounded;
- C0 scenarios were not yet bound to exact future tests; and
- RoutingCatalog/event digest and clock encoding were incomplete.

C0 was revised without weakening the gate:

- cutover now holds a SQLite exclusive transaction while atomically detaching the entire
  v12 state directory, installs a tombstone at the canonical DB path before releasing the
  lock, and publishes v2 only in a generation-specific path selected by signed manifest;
- C7 uses required-check readback plus OIDC-signed GitHub artifact provenance from a
  pinned trusted-builder workflow and binary-embedded source metadata; operator values
  cannot assert source/tested SHA or artifact digest;
- `decodex.authority-broker/1` now freezes Unix packet framing, method/subject capability,
  request sequence, durable dedupe, fsync-before-ack and exact crash resume;
- authority events use deterministic CBOR, generation-local monotonic sequence/hash
  chain, HostAuthorityKey signatures and a KeyProtector protected head, with explicit
  tamper and legitimate DB-ahead crash behavior;
- `SupersededCloseoutOperation` has one deterministic id and ordered acceptance,
  terminal/conflict release, PR reconciliation, resource cleanup and detachable
  projection stages, with SUP-14..19 crash/replay cases;
- no-effective-delta now has exactly three outcomes: independently proven
  `already_satisfied`, one deterministic diagnostic retry, or reason-coded attention on
  repetition; manual-authority `--related` is rejected by Clap and removed from requests;
- host-level TrackerCredentialCatalog/workspace introspection precedes issue resolution;
  issue-batch config/project selectors reject and the config-first tracker boundary is
  removed in v2;
- ProviderSnapshotCapability explicitly declares token coverage per field and disables
  Linear label/team routing when coverage cannot be proven;
- event/projection identifiers are bounded opaque/canonical types; raw selector and
  principal text are forbidden;
- RoutingCatalog and event canonical bytes, domain separation, ordering and clock policy
  are explicit; and
- C0 now contains a baseline verifier plus machine launcher, legacy-source, mutation and
  scenario manifests over 3,332 Rust/Python/Swift/shell/config source files. It recorded
  1,039 launcher files, 2,939 legacy authority candidate files, 2,837 mutation candidate
  files and 129 exact scenario/test bindings. Current fixture SHA-256 values are
  `501cc1ce...99c1d`, `fda29716...a0e4`, `f20eec30...cebb`, and
  `844ef5c3...d235`, respectively; the full values are command evidence and machine
  files.

The eleventh review does not satisfy the gate. A twelfth fresh reviewer must inspect the
revised records and machine inventories and return `READY` with zero blocker/high gap.

### Post-eleventh-review validation

Validated at: 2026-07-10 (America/New_York).

- `scripts/verify_lane_authority_v2_baseline.sh` reproduced all four manifests from the
  exact `d57553bc1bcdceebe1d0c7ec5ad5dc492b695348` source baseline and current scenario
  table; JSON parsing, Python compilation, and shell syntax checks passed.
- No-index whitespace checks passed for all five new OpenWiki records, both verifier
  scripts, and all four machine manifests; the deliberate trailing-whitespace negative
  control failed as required. `git diff --check` also passed.
- Structured Markdown parsing found no missing local link in the six touched OpenWiki
  pages. The branch contained all of current `origin/main` after a fresh authenticated
  fetch.
- `cargo make check` passed site build/check, Rust workspace check, Rust/TOML formatting,
  clippy, vstyle over 3,175 files, and all 1,657 executed nextest tests; one
  repository-declared test remained skipped. Build duration was 51.37 seconds on the
  warm dependency cache.

Mechanical validation is green, but C0 still requires the twelfth skeptic verdict and
then exact-head commit/push/PR/XY-1251 readback.

### Twelfth skeptic review

Verdict: `READY` for C0 architecture, with zero blockers, zero high gaps, two medium
precision findings, and one low wording finding.

One attempted reviewer was terminated by model-capacity limits and produced no verdict;
it is not counted. A different fresh read-only reviewer completed the full rubric,
re-ran the baseline verifier, confirmed all four manifest counts/digests, and accepted the
authority, migration, telemetry, closeout, adjacent-defect and anti-drift architecture.
Its residual findings were:

- no-effective-delta prose named nonexistent persisted operation state
  `retry_scheduled` instead of the frozen FSM;
- admin receipt projection allowed an undefined generic `provider_object_id`; and
- C0 gate prose omitted TOML/YAML although the machine baseline included them.

All three were corrected without changing architecture or scenario ids: no-delta now
uses legal `applying|blocked -> reconciling` plus one typed continuation, admin projection
uses bounded HMAC-backed ProviderObjectRefToken and forbids raw provider ids, and C0/C1I
language coverage explicitly includes TOML/YAML semantic parsing. Because these edits
touch the normative contract after the READY read, one final fresh exact-tree skeptic
confirmation remains required before commit.

### Thirteenth skeptic review

Verdict: C0 not ready.

The fresh exact-tree reviewer confirmed all three twelfth-review precision fixes and
found no regression in the named authority/migration risks, but identified one high
machine-contract mismatch: the effect registry promised per-effect reconciliation,
compensation and provider-capability fields while `mutation_registry.json` contained only
coarse v12 source-candidate classifications.

The generator and contract were corrected rather than narrowing the promise. The machine
registry now contains two explicit sections: 2,837 frozen source candidate files and 104
concrete v2 `effect_kinds` expanded directly from every normative table row. Each kind
records adapter/replacement owner and kind, desired-state readback, reconciliation policy,
compensation class and stop rule, provider capability requirement, runtime generation,
semantic digest and removal checkpoint. Duplicate/malformed rows reject generation and
the baseline verifier reproduces the exact artifact. Its new SHA-256 is
`b83ae4e1...b0ca7`; the other three fixture digests are unchanged.

The thirteenth review does not satisfy the gate. A fourteenth fresh exact-tree reviewer
must return zero blocker/high findings before C0 commit.

### Fourteenth skeptic review

Verdict: C0 not ready.

One reviewer session was interrupted before returning a verdict and is not counted. The
replacement fresh reviewer found two high precision failures in the new per-effect
manifest: all 104 target v2 kinds emitted a null removal checkpoint, and a two-kind alias
row copied the combined `durable_publication / irreversible_terminal` class into both
concrete kinds.

The generator now emits explicit `retained_v2` removal disposition for every target kind.
It also requires class cardinality to be either one shared class or exactly one class per
expanded alias kind; mismatch aborts generation. The automation artifact alias now emits
`write=durable_publication` and `retire=irreversible_terminal`, all 104 removal fields are
non-null, and the baseline verifier reproduces the result. The mutation-registry SHA-256
is now `d7dd40fb...be4c4`.

The fourteenth review does not satisfy the gate. A fresh final exact-tree reviewer must
confirm zero blocker/high findings before C0 commit.

### Fifteenth skeptic review

Verdict: C0 not ready.

One fresh reviewer returned `READY` after checking the prior effect-registry fixes. A
second independent reviewer found two high anti-drift gaps that the first review missed:
ignored untracked Rust/Python/Swift/shell/TOML/YAML paths under the frozen source roots
could escape the source inventory, and the generated scenario manifest proved only
self-consistency with the current Markdown table rather than stability of the frozen C0
scenario set.

The gate was strengthened rather than narrowing its claims. Scope validation now unions
ordinary and ignored source/config discovery for the frozen roots and rejects every
ignored untracked candidate. Scenario generation now compares the parsed id/checkpoint/test
tuples with an independent domain-separated count/digest constant; changing that anchor is
an explicit reviewed scope change. The baseline verifier runs negative controls in a
disposable Git repository for ignored source discovery and in memory for scenario
checkpoint drift. A further fresh exact-tree skeptic must inspect these corrections before
C0 can commit.

Post-correction validation reproduced all four manifests, passed both negative controls,
Python compilation, shell syntax, structured JSON assertions, and `git diff --check`. The
independent scenario-set anchor is
`43b5a08e8c196d8253af341db92858717610c67f77c2718d2bdd973b342ac127`; the resulting
scenario-manifest SHA-256 is
`160e42bf465251e4fa95d5b5d8d80527202d5b0f2a81deba446c1e402c142fa3`.

### Sixteenth skeptic review

Verdict: `READY` for C0, with zero blockers, zero high, zero medium, and zero low
findings.

The fresh exact-tree reviewer independently reproduced both previously failing cases. An
ordinary untracked source path failed scope validation, an ignored source path failed the
ignored-source guard, and a mutated scenario checkpoint failed the independent freeze
before manifest acceptance. It also reconfirmed 104 unique effect kinds, complete semantic
fields, `retained_v2` removal disposition, deterministic alias classes, and separation from
the 2,837 frozen source candidates. The reviewer reran the baseline verifier,
`git diff --check`, current-main equality, and `cargo make check`; all passed. The exact
main-thread gate also passed site build/check, Rust check/fmt/clippy, vstyle over 3,175
files, and all 1,657 executed tests with one repository-declared skip.

### Seventeenth skeptic review

Verdict: PR #1084 requested changes with two high and two medium findings.

The post-commit PR reviewer found that the prior source inventory froze only eight selected
paths while the contract claimed every tracked repository source/config file; the effect
table still lacked an independent semantic freeze; the scenario freeze omitted scenario
and required-result text; and the C7 example passed source/tested/artifact identities as
operator CLI flags despite forbidding that authority source.

The repair expands the baseline to every tracked repository Rust/Python/Swift/shell/TOML/
YAML file, including root and package build manifests. Ignored dependency/build outputs are
explicitly excluded from untracked-source discovery, but tracked files in those locations
remain frozen. The resulting exact baseline contains 3,363 files, 1,052 launcher entries,
2,967 legacy authority candidate files, and 2,865 mutation candidate files. It now includes
`Cargo.toml`, `Makefile.toml`, every package `Cargo.toml`, `Package.swift`, and
`rust-toolchain.toml`. A separate `--post-c0` mode relaxes only the initial changed-path
allowlist so later checkpoints can rerun the frozen manifests without weakening their
baseline, effect, or scenario anchors.

The 104 effect kinds now have an independent full-semantic freeze
`5aef53544036bc289eab1c7edd9e84b197ea667c20633b679894c87d7875311d`.
The 129 scenario rows freeze id, checkpoint, test name, scenario text, and required result
under `41c2860b5d9d887d52e2003f70eeec2af4a122ed9b79f782472f61b554bd29e0`.
Negative controls mutate both semantic sets and must fail. Current artifact SHA-256 values
are launcher `d0dda5f96f95b6fc9b5501fb411587d5c818ce9738e335a18e97f8b80ef3be0a`,
legacy `c2153a3391209ca1bf433b2f36af99fafef1ffd1dfa7373c2131aa90486233fe`,
mutation `32c272bf96f2ecc1da297ba4cee15340ad9bbd751a7cf76f5bb8ba3595aaa569`,
and scenario `c87acaa1373c4a4bc45833e116c9d78208be932d8690fb78108c83d5ffabb914`.

The C7 command no longer accepts source commit, tested PR head, artifact digest, or live-main
identity as cutover authority flags. `cutover-prepare` derives them from verified
attestation, binary build-info, and fresh GitHub readback; later stages consume the signed
receipt and independently revalidate live facts. A fresh exact-head review is required
after the repair commit.

### Eighteenth skeptic review

Verdict: PR #1084 requested changes with one high finding.

The reviewer independently passed the full-repository source inventory, effect semantic
freeze, scenario semantic freeze, post-C0 mode, and all three prior C7 identity removals.
It found one remaining split identity in the C7 command block: early checks resolved and
validated `DECODEX_C7_PR`, while provenance and cutover consumed a separate undefined
`pr_url`. That could abort under `set -u` or select a different PR.

The command block now has exactly one PR locator. Provenance verification and
`cutover-prepare` both consume the same already validated `DECODEX_C7_PR`; no derived or
operator-supplied second locator exists. This repair requires another fresh exact-head
review before landing.

### Nineteenth skeptic review

Verdict: PR #1084 requested changes with one medium finding.

The reviewer confirmed the undefined second locator was removed, but found that a bare
numeric `DECODEX_C7_PR` could still make `gh pr view` consume ambient checkout or `GH_REPO`
repository identity while later Decodex `--pr` parsing expects a full URL.

The C7 contract now accepts only the exact canonical
`https://github.com/hack-ink/decodex/pull/<digits>` shape, rejects suffixes and nonnumeric
identifiers, explicitly pins `--repo hack-ink/decodex` on both GitHub readbacks, and passes
that same URL to provenance and cutover. There is no ambient repository or alternate PR
locator. Another fresh exact-head review remains required.

### Twentieth skeptic review

Verdict: PR #1084 requested changes with two medium findings.

The canonical PR URL repair held, but the reviewer found two remaining ambient repository
inputs: live main was read from the checkout's `origin`, and the required-check helper took
only commit/phase so it could infer a repository from `GH_REPO` or the checkout.

The C7 contract now pins `GH_HOST=github.com`, pins `github.com/hack-ink/decodex` on both PR
readbacks, resolves live main through the explicit GitHub API repository path, and removes
`origin` from authority readback. Both required-check phases receive the exact canonical
repository URL, which the helper must validate rather than infer. Another fresh exact-head
review remains required.

### Twenty-first skeptic review

Verdict: PR #1084 requested changes with two medium findings.

All prior C7 repository-authority repairs passed. The reviewer found that the C0 regex
inventories still overmatched bare `Decodex`, `Lane`, lowercase `.update(...)`, and YAML
`pull_request:` text, while one gate label called the result a supported-launcher inventory.
That could make later work treat broad discovery hits as already classified authority.

The inventory contract now separates closed-world source coverage from candidate
classification. Every launcher/legacy/mutation source hit is explicitly
`unclassified_pending_c1i`; C1I remains the first point where AST/syntax/call-graph evidence
may classify it. Regexes were tightened and durable precision/recall controls cover the
four false-positive examples plus structural process, SQL, LaneId, and provider-readback
positives. The exact source set remains 3,363 files; launcher candidates are now 127,
legacy candidates 2,712, and mutation candidates 2,675. Current artifact SHA-256 values
are launcher `f7d104ba81a793073654082abb6fbda5695ad916b2c5082dd00a67c15d9ad8c9`,
legacy `7443fb30ccbeefe9240d36074de7ec51a29c9b4cd3a378933628762012434917`,
mutation `d0cbd97dfe32376d8a1d41a905a7b85bb5b4eee5c77b1cd2c13a219902fdfee8`,
and scenario `c87acaa1373c4a4bc45833e116c9d78208be932d8690fb78108c83d5ffabb914`.
Another fresh exact-head review remains required.

### Twenty-second skeptic review

Verdict: `APPROVE` with no blocker, high, or medium findings on exact head
`17f50311af30331061a5355ac81bab4e30c0c68f`.

The fresh reviewer independently reran the baseline self-test/verifier, `git diff --check`,
and `cargo make check`; it confirmed exact source closure, explicit
`unclassified_pending_c1i` candidate status, all precision/recall controls, effect/scenario
semantic anchors, and the complete C7 repository/provenance lineage. The only residual is
the intentional C0/C1I boundary: exact AST/syntax/call-graph classification begins in C1I
and is not silently claimed by C0. CodeQL JavaScript/TypeScript, Rust, and aggregate checks
also passed on that head.

This evidence-only ledger update changes no contract, generator, manifest, or runtime
behavior. It requires a final exact-head confirmation and required-check readback before
Decodex landing.

### C0 exit criteria

- XY-1251, ADR, target contract, scenario matrix, and checkpoint ledger exist.
- `scripts/verify_lane_authority_v2_baseline.sh` proves exact-main source and scenario
  coverage with no unexpected C0 path.
- `cargo make check` succeeds on the exact current-main-based C0 tree.
- `git diff --check` and new-file whitespace checks succeed.
- Project/issue identity, current authority disposition, migration quarantine,
  rollback point of no return, effect fencing/reconciliation, supersession acceptance,
  telemetry attribution, and projection privacy are frozen in the contract.
- A fresh final skeptic review reports no unresolved blocker, high, or medium correctness
  objection.
- The exact C0 branch is committed and pushed through Decodex-owned paths and linked to
  XY-1251 before C1 begins.

### C0 evidence commands

```sh
codex-identity
git status --short --branch
git rev-parse HEAD
git rev-parse origin/main
git worktree list --porcelain
decodex project list
decodex status --json --limit 5
gh pr view 1073 --repo hack-ink/decodex --json ...
rg 'CREATE TABLE|PRIMARY KEY' apps/decodex/src/state/sqlite_store/schema*
```

### Next checkpoint

Complete C0 validation and skeptic review, then begin C1 from the accepted records and
migration invariants. C1 must not preserve global issue-keyed ownership behind a facade.

## Program Checkpoint Table

### Runtime checkpoint: project-qualified attempt authority and recovery adoption

PR #1092 now project-qualifies production attempt sequencing, retry accounting, latest
attempt reads, closeout identity reuse, thread archive history, daemon retry, recovery,
and operator projections. The old issue-only attempt APIs are test-only; the remaining
production legacy writer in review-handoff adoption has been replaced by an explicit
`RecoveryAdoption` Intake Authority, binding attestation, canonical claim, lane worktree
attachment, lane-qualified attempt, and `Claimed -> Running -> WaitingReview` transitions.

Recovery adoption external failures no longer erase accepted local authority. Known
post-publication local failures mark the attempt failed and transition the lane to
`NeedsAttention`; unknown comment outcomes remain `WaitingReview` with a durable
`ReconciliationRequired` effect instead of being retried. Pre-commit local failures detach
the exact adopted worktree under CAS and release the claim. `DetachWorktree` rejects stale
branch/path or phase evidence. Focused evidence: 11 lane-authority tests, 17 lease
tests, 128 recovery tests, the recovery-adoption authority/lane fixture, and
`cargo check -p decodex --all-targets` all pass. Migration/quarantine, effect journal,
typed supersession, telemetry, and adjacent XY-1249 fixes remain incomplete, so C1/C2 are
not complete.

The first C3 slice adds the canonical effect transition kernel and persistent SQLite
journal with immutable `(operation_id, ordinal)`, lane/binding/claim/epoch prerequisites,
journal CAS, request/facts/desired-state digests, typed compensation class, unknown-outcome
reconciliation, and immutable receipts. Runtime schema 13 rejects a future schema before
bootstrap mutation. Recovery-adoption audit comments now plan and begin a durable-publication
effect before provider invocation, reconcile an existing idempotency marker after unknown
outcome, and record the receipt before the local Linear projection. Restart and stale-lane
fixtures prove journal reload and fail-closed revalidation. This is not C3 completion: all
remaining external adapters, provider capability gates, pagination, ordinal enforcement,
authority-event transactions, and PONR fencing remain.

The XY-1250 pagination prerequisite now has an all-page GitHub issue-comment readback
adapter using `gh api --paginate --slurp`, a deterministic marker match, and a page-two
process-level fixture proving that retry returns the existing comment instead of posting.
It is intentionally not yet wired to a superseded-closeout writer because C4 typed
supersession does not exist; the adapter remains staged and cannot be used as authority.

The manual commit/land CLI no longer advertises or accepts `--related`. The commit/2
record is intentionally commit-local, so manual requests and commit-message builders no
longer carry a field that the runtime must reject later. A Clap parser fixture proves
that `--related` fails before repository or provider access. Focused evidence: 10
commit-message tests, 14 core CLI tests, 41 manual-command tests, and
`cargo check -p decodex --all-targets` pass. This satisfies only the parser half of C6;
the no-effective-delta outcome FSM and its bounded retry evidence remain incomplete.

The first no-effective-delta v2 slice adds a dormant pure decision kernel over complete
digested diagnostics. It separates initial observation from retry-result observation:
the first creates ordinal-1 recovery, exact command replay returns the same recovery,
fact or lane drift fails closed, and the retry result converges to
`no_effective_delta_unresolved`. Explicit blockers bypass recovery and
`already_satisfied` requires a non-empty independent-validator receipt. Four focused
tests and `cargo check -p decodex --all-targets` pass. Persistence, continuation attempt
wiring, independent validator execution, and runtime cutover remain before C6 is
complete.

No-effective-delta ordinal-1 recovery is now durable in the canonical SQLite store.
Schema 14 adds an immutable operation-keyed record with a canonical-lane foreign key;
the StateStore validates the lane before insertion, exact replay survives restart, and a
future schema is rejected before bootstrap mutation. Five no-effective-delta tests, the
future-schema test, and `cargo check -p decodex --all-targets` pass. The adapter is
deliberately dormant until the continuation-attempt path consumes it; no v12 scheduler
behavior changed.

The first C4 slice adds immutable typed `RepairHandoffAuthority`, exact successor
acceptance, complete one-per-patch disposition validation, and deterministic
`SupersessionEdge` creation. Acceptance rejects stale predecessor epochs, active
predecessor operations, duplicate terminal edges, wrong successor lanes, foreign
repository PRs, unlanded/unreachable successors, duplicate dispositions, and missing or
extra patch units. Six supersession-focused tests and
`cargo check -p decodex --all-targets` pass. Persistence, active-handoff CAS,
terminal-authority/conflict-release transaction, canonical PatchSet construction, and
closeout effects remain before C4 is complete.

C4 now persists active handoffs and terminal edges and commits the first irreversible
local boundary atomically. One SQLite transaction CASes the active handoff and frozen
predecessor epoch, inserts the immutable edge, moves the predecessor Lane to
non-executable `terminal_cleanup_pending`, clears its claimant, and releases only the
exact matching project/run conflict lease. Restart readback preserves the edge and
terminal-cleanup phase. Schema 15 rejects newer state before bootstrap. Seven
supersession tests, the persistent atomic-boundary fixture, and
`cargo check -p decodex --all-targets` pass. External effects still do not run; closeout
operation stages, cleanup receipts, PatchSet canonicalization, handoff replacement, and
crash-point coverage remain.

The superseded-closeout operation is now a durable stage-CAS saga instead of a
reconstructed retry. Its deterministic id binds the immutable edge and predecessor
epoch; prerequisite PR-version, successor-reachability, and resource-plan digests are
persisted at `acceptance_attested` before terminal authority. The terminal-authority
transaction consumes that exact operation and advances it atomically. Subsequent
`predecessor_pr_reconciled -> resources_reconciled -> terminal` transitions are ordered,
epoch-fenced, restart-safe, and idempotent; the final transition moves the Lane from
`terminal_cleanup_pending` to `terminal` in the same transaction. Nine supersession
tests, three schema-migration tests, and all-target checking pass. Provider effect
receipts and per-resource cleanup ordinals are still required before these stages can be
wired to external execution.

The canonical effect journal now has one typed authority-owner union rather than
assuming every effect belongs to an active run claim. Existing dispatch/recovery effects
use `LaneClaim(run_id, lane_epoch)`; superseded closeout effects use
`TerminalOperation(operation_id, stage_epoch)`. StateStore revalidates the corresponding
Lane claim or durable closeout stage before every journal mutation. GitHub PR close and
ordered control/worktree/remote-ref/local-ref cleanup kinds are registered with explicit
compensation classes. Four effect-kernel tests, the persistent effect restart test, and
all-target checking pass. This removes the architectural pressure to retain a fake Lane
claim after terminal conflict release; actual closeout effect planning and stage receipt
gates are next.

Superseded closeout now freezes the complete typed effect plan at attestation. The plan
must begin with exact predecessor PR close, then follows registry-ordered control,
worktree, remote-ref, and local-ref cleanup kinds; its canonical JSON digest is validated
on reload. The terminal-authority transaction creates every effect journal row with
fixed operation ordinal and the stage epoch at which it may invoke. PR reconciliation
cannot advance without the PR receipt, resource reconciliation cannot advance without
all required cleanup receipts, and terminal cannot bypass either gate. The persistent
fixture proves pre-receipt rejection, ordered receipt progression, restart, and final
Lane terminalization. Nine supersession tests, four effect tests, and all-target checking
pass. Provider adapters/readback and individual resource ownership receipts remain.

The first closeout provider adapter performs exact GitHub PR readback without claiming
unsupported mutation semantics. Exact-head/base already-closed state yields an immutable
effect receipt; head/base drift yields typed prerequisite drift; an exact open PR yields
`conditional_mutation_unsupported` and no PATCH/close command. The process-level fixture
records the invoked `gh api --method GET` arguments and proves no unconditional mutation
is attempted. Two adapter tests and all-target checking pass. This deliberately leaves
the predecessor internally terminal with typed close debt when GitHub cannot prove CAS,
rather than reviving executable authority or guessing from stale snapshots.

Repair-handoff replacement now uses immutable history plus a single active-row CAS.
Replacement must preserve the exact predecessor Lane and frozen epoch; one SQLite
transaction marks the current row `replaced` and inserts the new `active` handoff.
Stale/second replacement attempts fail without changing either row or releasing any
conflict. State reload preserves active/replaced/accepted/cancelled/rejected-stale typed
states. Ten supersession tests, including a persistent competing-replacement fixture,
and all-target checking pass. This closes the XY-1250 successor-lineage race at the local
authority boundary; provider effects still require fresh readback independently.

The first local resource executor now handles worktree cleanup with exact ownership
fencing. Before invocation it revalidates terminal-operation stage, canonical Lane,
project/issue mapping, branch, path, and a length-delimited facts digest. Filesystem
failure records unknown outcome and preserves the ownership mapping. Success commits the
effect receipt and deletes the exact durable worktree mapping in one SQLite transaction;
only then may the resource stage advance. The persistent supersession fixture now creates
and removes a real temporary worktree directory, proves pre-receipt blocking, mapping
removal, restart, and terminalization. Ten supersession tests, the effect restart test,
and all-target checking pass. Control and ref executors remain.

Ref cleanup now distinguishes provider capability from local CAS. GitHub remote-ref
readback returns an absence receipt, prerequisite drift on OID/type/ref mismatch, or
`conditional_mutation_unsupported` for an exact existing ref; it never issues
unconditional DELETE. Local refs use Git's native `update-ref -d <ref> <expected-oid>`
compare-and-delete, verify absence afterward, and return idempotent absent/deleted
receipts while preserving a changed ref. Four process-level tests cover exact delete,
replay, drift, remote absence, and the no-DELETE command boundary; all-target checking
passes. These adapters are not yet wired into the closeout executor, and remote cleanup
remains detachable capability debt when GitHub cannot fence deletion.

The named PUB-1704/#826 -> PUB-1705/#827 incident fixture now records fresh public
GitHub readback: predecessor #826 is open at
`f5e89318b8539088121a58ebbb2731bb085f0ae9`; successor #827 is merged from
`798eb431ec04a78b44e9da889b36a455cf51c202` as
`4c3fdc38bb86e8ce683a117fc720b656ba3674ff`. A focused test feeds those exact identities
through `RepairHandoffAuthority` and `SupersessionAcceptance` to create the typed edge,
without reading labels/comments/worktrees or using PR #1073 code. Eleven supersession
tests and all-target checking pass. The fixture's PatchSet disposition remains synthetic
until the canonical raw-object PatchSet builder lands, so this is lineage proof rather
than full C4 fixture completion.

Canonical PatchSet construction has started behind a separate authority module using
`gix` with default features disabled and only revision plus SHA-1/SHA-256 object support.
The first vertical slice opens the repository in process, rejects non-unique best merge
bases, recursively reads raw tree entries, sorts raw path bytes, represents renames as
delete plus add, and length-delimits endpoint path-delta evidence and unit digests. Its
fixture proves stable binary OIDs, mode-only changes, delete/add behavior, and a path
containing a newline. Commit DAG ordering, commit evidence, empty/merge units, net-zero
history, and handoff integration remain mandatory before C4 can be called complete.

The second PatchSet slice now computes the exact head-minus-merge-base commit set from
raw objects, orders it parents-first with Kahn/raw-OID priority, and freezes ordered
parent/tree evidence. First-parent transitions emit exactly endpoint path deltas or
endpoint-net-zero histories; tree-equal commits emit empty units and multi-parent commits
emit ordered merge-topology units. A combined branch fixture proves repeated edits back
to base, an empty commit, sibling topological ordering, ordered merge parents, and the
full deterministic digest; the endpoint fixture additionally proves a gitlink/submodule
entry. Production `RepairHandoffAuthority::new` now accepts only this canonical PatchSet
and freezes target base ref/OID, selected merge base, ordered commits, PatchSet digest,
and its complete unit digest set. Arbitrary digest/unit construction is test-only. Eleven
supersession tests, two focused PatchSet fixtures, and all-target checking pass. Explicit
multiple-best-base, shallow/missing-object, byte-level root/octopus/non-first-parent
fixtures and the named PUB incident's real object replay remain before C4 completion.

PatchSet rejection and topology fixtures now cover criss-cross history with two best
merge bases, octopus parent preservation, raw-OID sibling priority, a merge tree equal
only to a non-first parent, a missing loose tree object, and an explicitly shallow clone.
Criss-cross and incomplete histories reject without emitting partial authority; the
octopus merge retains all parents in object order and is not misclassified as empty.
Five focused PatchSet fixtures pass. The named PUB incident still needs real object replay
and force-push/base-change invalidation evidence before the C4 gate is complete.

The named PUB-1704/#826 predecessor now freezes its real GitHub base
`aa057af13bca1bd69572eb640d125229d7e748fd`, canonical PatchSet digest
`9a872a36ed72f00ce4dd65a6730e762e3ca284afc265cdd2155d7a9c6061f760`, one ordered
commit, and twelve unit digests. An explicit external-replay test rejects the existing
shallow checkout, then passes against a temporary complete public clone: it rebuilds the
same raw-object PatchSet and takes it through production `RepairHandoffAuthority` plus
complete `SupersessionAcceptance`. The fixture embeds only compact identities/digests,
not a base-tree bundle. Force-push/base-change runtime revalidation remains.

Supersession acceptance now requires a fresh typed predecessor PatchSet readback. Target
base ref/OID, selected merge base, predecessor head, and canonical PatchSet digest must
all equal the immutable handoff; any force-push, base advance, merge-base shift, or object
change returns `PredecessorPatchChanged` before disposition or terminal authority can be
accepted. Twelve supersession tests pass (plus the explicit real-object replay test,
ignored unless its complete external checkout is supplied).

Closeout effect plans now freeze a typed target instead of only opaque digests. The
target carries exact PR repository/number/head/base, control resource/owner, worktree
project/issue/branch/absolute path, or remote/local ref repository/branch/expected OID;
effect kind is derived from that target and mismatches fail validation. The durable
operation plan digest therefore covers every executor argument across restart, and the
operation exposes target lookup by immutable ordinal. Existing supersession and
persistent worktree fixtures pass. Provider executor wiring is next.

Terminal-operation ref executors are now wired through StateStore. Both load the frozen
target by effect ordinal, revalidate closeout stage authority, durably enter invoking,
and then use exact provider readback. Local refs execute Git's expected-OID
compare-and-delete and turn deleted/already-absent into receipts; remote refs turn
GitHub absence into a receipt while conditional-mutation-unsupported or drift becomes
`NeedsAttention` without DELETE. External errors become reconciliation-required. The
persistent closeout fixture now performs worktree removal, fake-GitHub remote absence,
real local-ref CAS deletion, receipt-gated terminalization, and SQLite reopen. The
process-level adapter tests continue to prove no unconditional GitHub DELETE.

The remaining superseded-closeout executors now consume frozen targets through the same
journal boundary. PR close performs exact repository/number/head/base GET readback and
only an already-closed match produces a receipt; open/unsupported or drift becomes
attention and no PATCH is issued. Control ownership is no longer a generic token: the
plan freezes project, issue, run, attempt, channel path, transport, and terminal status.
Retirement removes the exact local channel path, then atomically CASes the active channel
to retired status together with the effect receipt in SQLite. The persistent fixture now
uses canonical Admit/Claim/Attempt setup and executes PR, control, worktree, remote-ref,
and local-ref effects before receipt-gated terminalization and reopen. Supersession,
PR-close, remote-ref, local-ref, and all-target checks pass.

C3/C4 gate audit found that `scripts/verify_lane_authority_v2_gates.sh` does not yet
exist and no scenario IDs are registered in tests. Broad filtered tests therefore cannot
be treated as full checkpoint evidence; the eventual gate manifest must bind every EFX
and SUP id to an actual fixture. The spec does not authorize reconstructing obsolete
authority from labels/relations, so no projection-based "obsolete scan" was introduced.

C5 authority telemetry now has its canonical storage core. Closed event/decision/reason
enums encode with pinned `minicbor` numeric keys; generation, global sequence, previous
hash, deterministic CBOR draft bytes, and the `decodex.authority-event/1` domain produce
the SHA-256 chain. Schema 17 adds a singleton chain head and append-only event rows.
Generation initialization is immutable, append inserts the event and CAS-advances the
head in one SQLite transaction, and readback cross-checks every indexed column against
canonical CBOR before verifying the full chain/head. Tests detect draft rewrite, row
delete/truncation, indexed-hash rewrite, reorder/fork, and verify exact reopen. The
KeyProtector-signed protected head, legitimate DB-ahead recovery window, transition
transaction integration, typed projections, and output-sink privacy remain.

| Checkpoint | Status | Required completion evidence |
| --- | --- | --- |
| C0 baseline and architecture freeze | Frozen evidence; proof PR #1090 must not land | Runtime PR #1092 consumes only accepted contracts, incident fixtures, scenario ids, and directly useful inventories |
| C1 project/lane identity and migration | In progress on PR #1092 | ProjectBinding/LaneId and transactional lane CAS exist; sole-writer cutover, migration/quarantine, and old issue-only API removal remain |
| C2 intake and dispatch authority | New-admission core complete; full gate pending C3 effects | Typed authority, binding attestations, claim fencing, and PUB-1711 replay exist; issue create/archive outbox receipts and remaining gate evidence remain |
| C3 transition and effects | In progress on PR #1092 | Complete mutation registry, receipts, crash replay, per-invocation revalidation, publication handoff, provider capabilities |
| C4 supersession and conflicts | Pending | Typed edge, deterministic closeout crash/replay, conflict release, obsolete scan, PUB-1704/PUB-1705 fixture/recovery |
| C5 telemetry and operator audit | Pending | Signed chain audit/recovery, diagnose/timeline/audit, metrics, full bounded projections while preserving C1 privacy boundary |
| C6 adjacent defects | Pending | Already-satisfied/bounded-retry/attention outcomes and parser-level manual `--related` rejection |
| C7 final validation and cleanup | Pending | Attested activation binary, full gates, review, exact-head landing, issue/PR/worktree/authority audit |
