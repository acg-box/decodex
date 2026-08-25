---
type: "Reference"
title: "Lane Authority V2 Effect Registry"
openwiki_generated: true
---

# Lane Authority V2 Effect Registry

Status: superseded and frozen by the [vNext authority decision](../decisions/vnext-authority.md).
Historical design and incident provenance only; this is not a normative vNext registry
and must not be implemented.

Every Decodex mutation of project-, tracker-issue-, lane-, lifecycle-, cleanup-, archive-,
or authority-relevant state is either a local `commit_transition` transaction or one
registered effect kind below, regardless of command, daemon, automation, recovery,
maintenance, or migration entrypoint. Adapters are private to the effect executor.
Adding a mutation kind changes the authority contract and requires a new registry entry,
semantic class, reconciliation/compensation policy, scenario, and skeptic review before
use. Archive hygiene and manual-authority receipt handling are not exceptions.

`transactional` denotes a mutation fully contained in the owning SQLite transaction and
is not an outbox effect. Every outbox effect is exactly one of `compensable`,
`durable_publication`, or `irreversible_terminal` as defined by the target contract.

## Runtime And Tracker Effects

| Kind | Class | Desired-state readback | Compensation/stop rule |
| --- | --- | --- | --- |
| `runtime.project_binding_commit` | `transactional` | ProjectPublication state, canonical ProjectKey bytes, revision/current marker, repository/tracker identity, content fingerprint, final file digest/attestation, RoutingCatalog epoch/digest. | Pending never routes; first finalization atomically creates paused availability epoch 1 and CASes catalog after exact contract readback; cannot rewrite immutable identity. |
| `runtime.project_quarantine_transition` | `transactional` | Source quarantine id/epoch, catalog epoch, candidate immutable identities, complete dependent-node mapping, resolve/split outcome, operator/reviewer, pending publication/contract attestations and created ProjectKeys. | No ProjectKey exists before adjudication; source stays quarantined while contract effects run; one batch finalization maps all nodes, activates paused projects and CASes catalog. |
| `runtime.project_availability_transition` | `transactional` | ProjectKey, prior/new availability epoch/state, expected/new RoutingCatalog epoch/digest, dependent-row counts, and authority event. | Pause/resume/retire CAS catalog atomically; active dependencies reject retire; history is never deleted. |
| `runtime.project_alias_transition` | `transactional` | ProjectKey, prior/new alias epoch/value, global alias uniqueness and event. | Alias is an operator projection only; cannot change binding/catalog/Lane fingerprints or route an issue without independent binding resolution. |
| `runtime.repository_locator_refresh` | `transactional projection` | RepositoryKey, locator epoch, provider-read owner/repository and event. | Mutable locator projection only; immutable repository database id must match and binding/Lane fingerprints cannot change. |
| `runtime.host_checkout_attestation_commit` | `transactional resource` | Host id, ProjectKey, checkout resource id, RepositoryKey readback, host config fingerprint and epoch. | Host-local resource only; relocation creates a new attestation epoch and cannot rewrite binding/Lane identity. |
| `runtime.intake_commit` | `transactional` | Admission event, IntakeAuthority, Program/mapping digest plus Lane transition receipt. | Non-lane rows roll back in the intake transaction; Lane/claim fields are written only through `LaneStore::commit_transition`. |
| `runtime.commit_transition` | `transactional` | Lane epoch/state/event plus transactional claim/resource invariants. | SQLite transaction rollback; no outbox ordinal. |
| `runtime.operation_claim_commit` | `transactional` | Operation/effect id, prior/new claimant epoch, subject authority-version digest, OS generation and expiry. | SQL CAS only; replaces legacy issue-claim/dispatch-lock records, which are removed and never consulted by v2. |
| `runtime.authority_event_append` | `transactional` | Event id/schema, generation/sequence, previous/event hash, causation/correlation, subject, fact fingerprint, and owning transaction id. | Append and chain-head advance only through AuthorityTransaction; generic update/delete is absent; sequence/hash mismatch aborts all state. |
| `runtime.authority_chain_anchor` | `durable_publication protocol` | HostAuthorityKey signature over host/generation/sequence/hash/database digest and matching KeyProtector protected head. | Runs after DB commit. DB-ahead crash verifies/signs the exact suffix; protected-head-ahead, same-sequence mismatch, broken chain, or invalid signature freezes mutation. Never rewinds or invents an event. |
| `runtime.authority_broker_request_commit` | `transactional protocol` | Invocation/channel/request sequence, idempotency/request/result digests, method/subject/capability, transaction/effect/receipt refs. | Ack only after fsync; exact replay returns durable result, conflicting reuse rejects, crash resume starts from last committed sequence and reconciles unknown effects. |
| `runtime.superseded_closeout_commit` | `transactional` | Supersession edge/acceptance, deterministic operation id, predecessor epoch, terminal-cleanup state, ExecutionGroup disposition, exact conflict releases, planned PR/local effects and optional projection debt. | All local authority commits together before external close/cleanup; rollback on SQL failure; later failure leaves only fenced cleanup/reconciliation ownership. |
| `runtime.lane_diagnostic_append` | `transactional diagnostic` | LaneId, run/attempt, protocol/private/Linear event type, payload digest/privacy class, and source generation. | May append diagnostics but has no Lane/claim writer capability; malformed/unbound events reject. |
| `runtime.lane_evidence_index_commit` | `transactional` | LaneId, authority event, artifact id/digest/privacy class/path receipt. | Commits with artifact publication handoff; cannot advance Lane or overwrite an artifact. |
| `runtime.project_planning_commit` | `transactional` | ProjectKey/binding/availability epochs and Decision Contract/autonomy/objective/proposal ids/fingerprints. | Project-scoped planning authority only; cannot admit or claim an issue. |
| `runtime.connector_state_commit` | `transactional diagnostic` | ProjectKey/binding/availability epoch, connector kind, backoff/checkpoint version and digest. | Cannot reference or mutate Lane ownership; stale epoch rejects. |
| `runtime.diagnostic_retention_commit` | `transactional diagnostic` | Retention operation, policy/version, exact diagnostic row ids/digests, and tombstones. | Cannot target authority events, Lane transitions, claims, evidence indexes, receipts, migration journals, or planning authority. |
| `runtime.manual_closeout_receipt_migrate` | `transactional` | Source receipt identity, project/Lane authority event or typed diagnostic/tombstone, and source disposition. | Classifier and migration transaction are atomic; ambiguous receipts never become authority. |
| `linear.issue.create` | `durable_publication` when immutable provider idempotency is proven, otherwise unsupported | Stable workspace/issue id plus provider idempotency-key readback and secondary privacy-safe PublicIntakeMarker. | Private IntakeIntentId is the provider key. Current Linear capability is unsupported, so no create invocation occurs. A capable provider receipt creates reservation/continuation atomically. |
| `linear.issue.archive` / `unarchive` | `compensable` when conditional provider version/CAS is proven, otherwise unsupported | Immutable issue id, archive state, exact version, routing/binding result, and deterministic orphan marker. | Inverse only under provider CAS. Current Linear capability is unsupported; archive hygiene cannot invoke it automatically. |
| `linear.issue.brief_update` | `compensable` when conditional provider version/CAS is proven, otherwise unsupported | Issue id, exact version, normalized public brief digest. | Restore captured previous digest/version only by provider CAS. Current Linear capability is unsupported. |
| `linear.issue.state_set` | `compensable` when conditional provider version/CAS is proven, otherwise unsupported | Immutable issue id, exact state id and version. | Restore captured prior state only by provider CAS. Current Linear capability is unsupported. |
| `linear.issue.label_add` / `label_remove` | `compensable` when conditional provider version/CAS is proven, otherwise unsupported | Immutable issue/label ids and exact version. | Apply inverse only by provider CAS. Current Linear capability is unsupported. |
| `linear.issue.comment_create` | `durable_publication` | All-page deterministic public marker and comment id. | Non-authoritative; do not delete. Unknown outcome reconciles marker before retry. |
| `linear.issue.relation_create` / `relation_remove` | `compensable` when conditional provider version/CAS is proven, otherwise unsupported | Immutable relation type, both issue ids, and exact versions. | Apply inverse only by provider CAS for operation-owned relation. Current Linear capability is unsupported; generic relation never creates supersession authority. |

## GitHub Effects

V2 uses in-process GitHub API clients for authority-mutating effects. Provider repository
database id, object id, expected head/version, and operation id are part of every request
digest and readback.

| Kind | Class | Desired-state readback | Compensation/stop rule |
| --- | --- | --- | --- |
| `github.pr.create` | `durable_publication` | Repository id, PR number, base, head branch/SHA, deterministic marker. | A PR cannot be erased. Preserve it and reconcile/roll forward; close is a later explicit projection effect, never compensation to nonexistence. |
| `github.pr.metadata_update` | `compensable` | Exact PR object, base/head and normalized title/body digest. | Restore captured metadata only when object/head still match. |
| `github.pr.label_add` / `label_remove` | `compensable` | Exact PR object and immutable label id/name readback. | Inverse only for operation-owned label change on unchanged PR. |
| `github.pr.comment_create` | `durable_publication` | All-page deterministic marker and comment database id. | Non-authoritative; no delete; reconcile before retry. |
| `github.pr.close` | `compensable` when provider capability is proven, otherwise unsupported | Exact PR object, planned head, closed state/version. | Reopen only when provider support and exact object/head/prior-open evidence match; otherwise block before invocation. |
| `github.pr.reopen` | `compensable` | Exact PR object/head and open state. | Close only if reopen was operation-owned and no drift occurred. |
| `github.pr.merge` | `irreversible_terminal` | Exact reviewed head, base, merged state and merge commit. | Final external ordinal; provider exact-head precondition required; roll forward only. |
| `github.review.reply` | `durable_publication` | Review thread/comment id and reply marker. | Non-authoritative; no delete; all-page reconciliation before retry. |
| `github.review.resolve` | `compensable` when provider capability is proven, otherwise unsupported | Exact review-thread state/version. | Inverse only with exact provider thread state; otherwise block before invocation. |
| `github.review.request` | `compensable` when provider capability is proven, otherwise unsupported | Exact PR/head, reviewer/app identity and request readback. | Cancel only operation-owned pending request with exact readback. |
| `github.pr.auto_merge_enable` / `auto_merge_disable` | `compensable` | Exact PR/head, method and auto-merge state. | Apply inverse only while PR/head and operation ownership still match. |
| `github.commit_status.publish` | `durable_publication` | Repository/commit/context plus state, target URL and description digest. | Publish corrected state under same context; never infer lane authority from status alone. |
| `github.ref.create` | `durable_publication` when server-enforced absence is proven, otherwise unsupported | RepositoryKey, absent ref precondition, created ref/OID and provider version. | A remote ref is public durable state; never report compensation. Preserve/roll forward. |
| `github.ref.update` | `durable_publication` when server-enforced expected-old-OID CAS is proven, otherwise unsupported | RepositoryKey, ref, expected old OID, new OID and provider version. | Current GitHub adapter lacks expected-OID CAS and is unsupported; never read-then-force-update. |
| `github.ref.delete` | `durable_publication` when server-enforced expected-OID CAS is proven, otherwise unsupported | RepositoryKey, ref, expected OID/version and absence readback. | Current GitHub unconditional DELETE is removed/unsupported; never read-then-delete or claim recreation. |

## Git, Filesystem, Process, And Hook Effects

| Kind | Class | Desired-state readback | Compensation/stop rule |
| --- | --- | --- | --- |
| `git.ref_create_cas` | `compensable` | Repository identity, absent ref, created ref and exact OID. | Delete only the unchanged operation-created ref. |
| `git.remote_config_set` | `compensable` | Git common-dir, remote name, exact prior/new URL or config value, RepositoryKey attestation, and config-file digest. | Restore exact prior value only while config digest and operation ownership match; otherwise block. |
| `git.worktree.create` | `compensable` | Git common-dir, registered worktree path, branch and HEAD. | Remove only exact operation-created clean worktree. |
| `git.fetch_objects` | `durable_publication` | Remote RepositoryKey, exact requested OIDs/refspec, `--no-write-fetch-head`, operation-scoped `refs/decodex/fetch/<operation-id>/...`, and fetched object ids. | Shared remote-tracking refs and `FETCH_HEAD` may not change. Fetched objects are non-authoritative; preserve/reconcile, never infer lane state. Legacy shared-ref fetch callsites are removed. |
| `git.ref_update_cas` | `durable_publication` | Ref name, expected old OID, new OID, and post-update OID. | An existing-ref update never auto-rewinds; reconcile/roll forward or block. |
| `git.index_worktree_update` | `durable_publication` | Git common-dir, worktree identity, prior/new HEAD, index tree, and clean-state fingerprint. | No synthetic reset. This project-scoped operator operation must roll forward to the exact target or block. Lane landing does not require default-checkout mutation. |
| `git.worktree_content_update` | `durable_publication` | Worktree identity, exact preimage/postimage blob+mode map, path bytes, and resulting status fingerprint. | Tracked-file rewrite/cherry-pick/rebase-style transformations roll forward or block; no blind reset. Untracked deletion is unsupported without a separately inventoried artifact owner. |
| `git.index_update` | `durable_publication` | Worktree identity, prior/new index tree, exact path set, and worktree fingerprint. | Preserve and roll forward to commit or block; no synthetic unstaging after later publication. |
| `git.remote_branch_delete` | `durable_publication` | RepositoryKey, ref, expected OID, exact `--force-with-lease=<ref>:<oid>` request, and absence readback. | Server-enforced lease is mandatory; no automatic recreation. Must precede worktree removal in cleanup. |
| `git.worktree.remove` | `durable_publication` | Registry/path absence after before-remove hook and exact clean snapshot. | Destructive cleanup publication; failures roll forward. It follows remote-ref disposition and precedes local checked-out-branch deletion. |
| `git.local_branch_delete` | `irreversible_terminal` | Local ref, expected OID, no attached worktree, and absence readback. | Final Git cleanup ordinal; no synthetic recreation. |
| `git.commit.create` | `durable_publication` | Exact tree, parent(s), signed commit, authority subject. | Immutable local publication; reset/rewrite is not compensation. Must precede remote publication effects. |
| `git.push_new_ref` | `durable_publication` | Exact remote RepositoryKey/ref/OID and server-enforced absent-ref creation result. | Conditional delete is a separate leased remote-branch effect and only under frozen policy; otherwise preserve and roll forward. Never reports compensation. |
| `git.push_update_ref` | `durable_publication` | Exact remote RepositoryKey/ref, prior OID, and published OID. | Never automatically rewind or force update. Downstream failure enters `published_pending_handoff`. |
| `filesystem.project_contract.write` | `compensable` | ProjectPublication id, canonical ProjectKey bytes, contract schema/revision, content fingerprint, prior state `absent|existing(digest,mode)`, new final file digest, rename and directory-fsync receipt. | Before activation, restore exact existing preimage or CAS-delete an exact operation-created file whose preimage was absent, then fsync directory. Drift/deletion failure records orphan_contract_blocked and keeps parent/project quarantine active. |
| `filesystem.runtime_config.write` | `compensable` | Config kind, owning ProjectKey/runtime generation, path identity, prior/new digest, mode, rename/fsync receipt. | Restore exact prior verified revision before dependent launch; otherwise block. Includes global and Codex runtime config, not secrets. |
| `filesystem.account_auth_projection.write` | `compensable` | AccountabilityRoot/account id, auth projection id, prior/new digest, owner/mode, atomic rename/fsync receipt. | Secret-bearing content is adapter-private and absent from telemetry; restore only exact prior projection under account CAS. |
| `filesystem.account_pool.write` | `compensable` | Account-pool generation, canonical account record digests, prior/new file digest, owner/mode and rename/fsync receipt. | Atomic generation CAS; no token material in receipts/output. Covers `accounts.jsonl` replacement. |
| `filesystem.account_usage_history.append` | `durable_publication` | Account id, usage-event id/digest, prior record hash, file generation and fsync receipt. | Append/idempotency only; retention is a separate exact policy effect. |
| `filesystem.account_login_workspace.create` | `compensable` | Login operation id, private workspace id/path resource, owner/mode 0700, empty/preimage proof and process generation. | Secret-bearing temp home is adapter-private, never printed, and removed only by exact owner after process reap. V2 has no preserve-temp option. |
| `process.account_login.spawn` | `compensable` | Login operation id, exact Codex binary SHA-256, PID/start/process group, private workspace id and device-flow receipt class. | Supervised group only; crash/abort terminates and reaps descendants before workspace cleanup. Raw stdout/stderr/auth payload goes only to typed private adapter. |
| `filesystem.account_login_auth_import` | `durable_publication` | Login/account id, source auth digest in private workspace, destination account projection generation/digest and import receipt. | Imports through account-auth projection adapter; no token material in event/output. Later failure rolls forward to pool indexing/cleanup. |
| `filesystem.account_login_workspace.retire` | `irreversible_terminal` | Login operation/workspace id, no-live-process proof, owned file inventory digests and absence readback. | Final login-cleanup ordinal; exact recursive deletion only, no preserved secret workspace. |
| `filesystem.automation_live_config.write` | `compensable` | Automation id, canonical manifest digest, primary-checkout attestation, live config prior/new digest/path id/mode. | Generated projection only; restore exact prior config on failure and never grant repo/lane authority. |
| `filesystem.automation_artifact.write` / `retire` | `durable_publication` / `irreversible_terminal` | Automation/artifact schema+id, source manifest/job identity, prior/new digest or retention tombstone, path id/mode/fsync receipt. | Auxiliary evidence only; cannot create Decodex lane authority. Retirement is a separate policy operation. |
| `filesystem.credential_helper.publish` | `compensable` | Helper id, owning invocation/effect, exact path, mode, content digest, process-generation and expiry. | Create with exclusive permissions; terminate users and remove only the exact operation-created helper. Secret bytes never enter telemetry/readback. |
| `filesystem.credential_helper.retire` | `irreversible_terminal` | Helper id/path/digest, owning invocation/effect, expiry, no-live-user proof, and absence readback. | Final security-cleanup ordinal; cannot delete an unknown/reused helper and never records content. |
| `filesystem.git_hook.write` / `remove` | `compensable` | RepositoryKey, hook name/path, prior/new digest/mode and install generation. | Apply inverse only while exact operation-owned digest/generation remains. |
| `filesystem.evidence_artifact.write` | `durable_publication` | LaneId/authority event, artifact id, content digest, privacy class, path identity, fsync receipt. | Preserve immutable evidence and roll forward its database index; never overwrite by path. |
| `filesystem.evidence_artifact.retire` | `irreversible_terminal` | Terminal Lane, retention policy/version, artifact digest, tombstone and absence readback. | Explicit maintenance operation only; final ordinal, no recreation claim. Authority ledger events are excluded. |
| `filesystem.legacy_evidence.seal` | `transactional migration protocol` | Migration plan/source id, raw path/mode/digest, age-v1 vault object id, KeyProtector handle, ciphertext digest, fsync/verify/raw-removal stage. | Pre-PONR only inside restore unit; encrypted rollback bundle must already contain exact original bytes. No generic reader receives vault capability. |
| `filesystem.legacy_evidence.forensic_export` | `durable_publication` | Vault object id/ciphertext digest, distinct AccountabilityRoots, purpose/retention event, private destination id/mode and export digest. | Offline exclusive-lock operation only; never writes terminal/log/MCP/public output and requires registered cleanup. |
| `filesystem.legacy_evidence.retire` | `irreversible_terminal` | Post-PONR retention authority, vault object/ciphertext digest, KeyProtector key disposition, and absence/tombstone readback. | Separate dual-accountable final retention operation; never part of rollback. |
| `filesystem.diagnostic_prune` | `irreversible_terminal` | Project/runtime scope, diagnostic-only file class, retention policy/version, exact digest/path, absence readback. | Cannot target authority events, evidence, migration bundles, project contracts, receipts, or active control files. |
| `filesystem.migration_backup.retire` | `irreversible_terminal` | Completed C7 generation, retention policy, accountable operator/reviewer, bundle digest and absence readback. | Separate post-cutover operation only; never part of migration/rollback and forbidden while rollback/diagnosis retention is required. |
| `process.app_server.spawn` | `compensable` | PID/start identity, process group, run/attempt and liveness. | Terminate and reap only the exact operation-created process group to restore the prior not-running state. |
| `process.app_server.interrupt` | `irreversible_terminal` | Exact PID/start identity, process group, signal receipt, and liveness readback. | A delivered signal cannot be reversed; final control-operation ordinal, then reconcile process state. |
| `process.app_server.terminate` | `irreversible_terminal` | Exact PID/start identity, process group, termination/reap receipt. | Termination cannot be reversed; final control-operation ordinal. A later spawn requires a new operation. |
| `process.lane_attempt_worker.spawn` | `compensable` | LaneId, run/attempt, worker binary SHA-256, PID/start identity, process group, supervisor generation and handshake receipt. | Terminate/reap only the exact operation-created worker group; no detached child may survive claim loss. |
| `process.lane_attempt_worker.interrupt` | `irreversible_terminal` | Lane/run/attempt, exact PID/start/group, signal receipt and liveness readback. | Delivered signal is final for the control operation; reconcile only. |
| `process.lane_attempt_worker.terminate_reap` | `irreversible_terminal` | Lane/run/attempt, exact PID/start/group, termination and descendant-reap receipt. | Final worker-cleanup ordinal; later restart is a new operation/attempt. |
| `process.thread.archive` | `durable_publication` | App-server thread id plus archive readback event. | No unarchive assumption; failed/unknown result reconciles and rolls forward. |
| `workspace_hook.run_compensable` | `compensable` | Hook contract id/digest, phase, input digest, process group, exit/receipt, and exact inverse contract. | Retry or compensate only under the pinned contract's exact idempotency/inverse semantics. |
| `workspace_hook.run_publication` | `durable_publication` | Hook contract id/digest, phase, input digest, process group, exit/receipt, and publication identity. | Reconcile and roll forward; no inferred inverse. Hooks without one pinned class are unsupported. |
| `run_control.publish` / `retire` | `compensable` | Lane/run/attempt/channel generation and state. | Atomic file plus runtime receipt; stale generation cannot publish or retire current channel. |
| `run_control.interrupt_request.publish` | `durable_publication` | Lane/run/attempt/channel generation, request id, typed interrupt payload digest, request-file or accepted/result receipt. | App server fsyncs a dedupe journal `accepted(request_id)` before acting and later appends result. Reconcile file, accepted, and result states; never redeliver an accepted id. |
| `run_control.steer_request.publish` | `durable_publication` | Lane/run/attempt/channel generation, request id, typed steer payload digest, request-file or accepted/result receipt. | Same accepted-before-act dedupe protocol; replay reads receipt and cannot deliver duplicate steering. |
| `run_control.request_receipt.append` | `durable_publication` | Lane/run/attempt/channel generation, request id, `accepted|applied|rejected` state, payload digest, and previous receipt hash. | App-server protocol publication; append/fsync only; unique `(channel_generation, request_id, state)`; cannot mutate Lane authority. Unknown after accepted is attention/roll-forward, not redelivery. |
| `activity_marker.write` / `remove` | `compensable` | Lane/run/attempt generation and marker digest. | Diagnostic projection only; atomic replace/readback; cannot grant ownership. |
| `terminal_guard.write` / `remove` | `compensable` | Lane/run/reason and marker digest. | Diagnostic projection only; atomic replace/readback; cannot grant terminal authority. |
| `runtime.migration_plan.create` | `transactional migration protocol` | Immutable plan id/digest, source/classifier/contract digests, canonical allocated ProjectKey bytes, path/mode/fsync receipt. | Create-once before dry-run; replacement requires explicit abandon record; not runtime authority and never regenerated by dry-run/apply. |
| `runtime.cutover.install` | `transactional migration protocol` | Supervisor/SQLite exclusive locks, journal, detached v12 directory/inode digest, tombstone, generation-specific v2 DB, backup, manifest generations/hashes. | Holds SQLite exclusion through atomic path detach/tombstone install; ordered fsync state machine; resume or rollback only through migration protocol. |
| `runtime.backup.restore` | `transactional migration protocol` | Verified immutable backup hash, absent external PONR fence, rollback-journal stage, and complete restore unit. | Allowed only before PONR; idempotently resumes every fsynced stage and never consumes the backup or removes a fence. |

The private authority ledger has append-only SQLite mutations through
`AuthorityTransaction` and no generic maintenance delete kind. Retention, if ever
introduced, requires a separate architecture decision, export/verification protocol,
and registry entry. Ephemeral OS generation locks are synchronization primitives owned
by the supervisor, but lock-file create/remove callsites are still capability-bound and
covered by the launcher verifier.

An app-server request receipt is a child protocol publication causally bound to the
parent request effect id and sealed channel-generation capability, not a separately
schedulable ordinal. The runtime accepts it only after digest/id/generation validation
and records its readback on the parent effect.

## Enforcement

- Linear and GitHub mutation clients are private to the effect-adapter modules and use
  in-process provider APIs. `gh` is read-only compatibility tooling outside v2 effects.
- Git, filesystem, hook, and process mutation helpers are private to registered effect
  adapters.
- The executor revalidates the current ProjectBinding, routing result, Lane epoch/claim,
  and effect-specific object version immediately before every forward, retry,
  reconciliation read or write, and compensation invocation. Revalidation is not only
  an irreversible-effect gate.
- EffectStore exposes no state-only writer. Every CAS uses AuthorityTransaction to append
  the corresponding typed authority event and receipt atomically; event/state sequence
  and hash-chain uniqueness are machine-checked. Broker acknowledgement and protected
  chain anchoring follow the frozen crash protocol and cannot turn an uncommitted request
  into success.
- Before the first effect invocation after cutover, the executor durably writes the PONR
  fence for any target, including local Git/filesystem/process/hook mutations.
- Terminal cleanup plans order provider/thread/hook/control/marker disposition first,
  then conditional remote-ref deletion, worktree removal, and final local checked-out-ref
  deletion. Readback and the terminal runtime transaction may follow, but no external
  mutation may follow an `irreversible_terminal` ordinal.
- The machine mutation registry lives at
  `apps/decodex/src/orchestrator/tests/fixtures/lane_authority_v2/mutation_registry.json`
  and has two closed sections. `effect_kinds` is generated directly from every normative
  table row in this file and records each concrete expanded kind, adapter owner,
  desired-state readback, reconciliation policy, compensation class/stop rule, provider
  capability requirement, runtime generation (`v12_legacy|v2`), replacement owner/kind,
  semantic digest, and mandatory removal checkpoint (`retained_v2` for a target v2 kind).
  An independent domain-separated C0 count/digest anchor covers every one of those fields;
  changing the anchor is a reviewed scope change rather than a regeneration step.
  Alias rows must provide either one class shared by all expanded kinds or exactly one
  class per kind; any other cardinality fails generation. `entries` maps frozen source-file
  candidate classifications to their v12 replacement owners/kinds/checkpoints. Missing,
  duplicate, shortened-alias, or semantically changed table rows make baseline
  regeneration differ and fail verification.
- C0 freezes every tracked repository Rust/Python/Swift/shell/TOML/YAML file, including
  root and package build manifests, as a conservative source node with content digest,
  root/tree digest, scope, and grouped high-recall
  launcher/read/write/discovery candidates. Every hit is marked
  `unclassified_pending_c1i`; C0 does not declare regex hits to be supported launchers or
  legacy authority. Precision controls reject known UI/config false-positive shapes while
  preserving structural positive examples. The reviewed registry explicitly includes
  current fetch/default-branch fast-forward/ref/index/worktree paths. C1I, before any
  runtime implementation edit, replaces candidate discovery with the required language
  AST/syntax/call-graph verifier and proves every frozen file/candidate is classified;
  only then may C1A change launchers or C1B move callsites behind sealed capabilities.
  A callsite newly found by AST is not outside the baseline: it must map to its frozen
  file source node, receive an exact classification, and update the machine digest before
  implementation continues. The program never discovers an unbounded baseline after
  runtime implementation has begun.
  Rust mutation APIs require an unforgeable sealed `MutationCapability` available only
  to registered adapters; typed command builders reject mutating Git/filesystem/process
  argv without it. Canonical PatchSet reads use a pinned in-process raw-object reader and
  never spawn Git or mutate refs/index/worktrees.
- `scripts/verify_lane_authority_v2_mutations.sh` uses Rust/Python/Swift/shell AST or
  syntax/call-graph analysis plus
  capability-boundary checks, compares all mutation call sites to the machine registry,
  and fails on unregistered adapters, direct writes, string-built mutating commands, or
  baseline sites not yet classified. The checked roots include SQLite execute/batch,
  provider mutation clients, `std::fs`/file write-remove-rename-permission APIs,
  process/signal spawning, Git command builders, and hook shells. Test-only modules are
  classified separately and cannot be linked into production. A grep-only verifier is
  insufficient.
- At C1, the verifier permits only exact callsites classified `v12_legacy`; it proves the
  ordinary v12 integration cycle still works without any v2 operation/effect row and
  proves every v2 module is capability-bound. No unclassified callsite is allowed. C2-C6
  shrink the legacy set. C7 requires it empty and removes the compatibility adapters.
- Compile visibility and source scans must prove every v2 lane authority write flows
  through `LaneStore::commit_transition` and every v2 external mutation through
  `EffectExecutor`; C7 proves these are the only remaining production paths.

Adding or changing a mutation kind changes this contract, the machine registry digest,
and the checkpoint ledger, and requires fresh skeptic review.
