---
type: "Reference"
title: "Lane Authority V2 Checkpoints"
openwiki_generated: true
---

# Lane Authority V2 Checkpoints

Status: frozen historical ledger. [XY-1260](../decisions/vnext-authority.md) superseded
Lane Authority v2; C1-C7 must not advance. Preserve this file as evidence and provenance.

Tracking issue: [XY-1251](https://linear.app/hack-ink/issue/XY-1251/lane-authority-v2-unify-decodex-project-lane-lifecycle-and-effect-authority)

This was the durable anti-drift ledger for the Lane Authority v2 program. Its entries are
frozen snapshots of facts, inferences, exact source/PR identities, validation results,
migration state, unresolved objections, scope changes, and planned next checkpoints. Do
not update them as active checkpoint state.

## C0 Baseline And Architecture Freeze

Historical status at supersession: in progress.

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
- New reviewed surfaces included the retired automation checkout resolver,
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

### Architecture decisions recorded at C0 (superseded)

- At C0, [Lane Authority v2](../decisions/lane-authority-v2.md) was the accepted target;
  XY-1260 later superseded it.
- The historical [Lane Authority v2 target contract](../specs/lane-authority-v2.md) defines records,
  transitions, migration, telemetry, and the scenario matrix.
- PR #1073 is not the final implementation path. Its requirements and tests remain
  evidence until replacement capability is proven.
- Migration is an offline one-shot cutover, not a long-lived dual-authority rollout.

### Objections unresolved when the program was frozen

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

| Checkpoint | Status | Required completion evidence |
| --- | --- | --- |
| C0 baseline and architecture freeze | Ready to land | PR #1084 exact-head confirmation, required checks, Decodex land, merge/readback cleanup |
| C1 project/lane identity and migration | Pending | ProjectBinding/LaneId, brokered sole transition writer, hash-chain telemetry core, schema cutover/restore, quarantine/rebind, effect core, v12 path fencing, PONR, OutputBoundary |
| C2 intake and dispatch authority | Pending | Host workspace credential directory, unbound issue resolution, Typed IntakeAuthority, binding attestations, issue create/archive effects, PUB-1711 rejection replay |
| C3 transition and effects | Pending | Complete mutation registry, receipts, crash replay, per-invocation revalidation, publication handoff, provider capabilities |
| C4 supersession and conflicts | Pending | Typed edge, deterministic closeout crash/replay, conflict release, obsolete scan, PUB-1704/PUB-1705 fixture/recovery |
| C5 telemetry and operator audit | Pending | Signed chain audit/recovery, diagnose/timeline/audit, metrics, full bounded projections while preserving C1 privacy boundary |
| C6 adjacent defects | Pending | Already-satisfied/bounded-retry/attention outcomes and parser-level manual `--related` rejection |
| C7 final validation and cleanup | Pending | Attested activation binary, full gates, review, exact-head landing, issue/PR/worktree/authority audit |
