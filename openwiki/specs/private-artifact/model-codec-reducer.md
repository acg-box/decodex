---
type: "Reference"
title: "Private-artifact model, codec, and reducer (retired design)"
openwiki_generated: true
---

# Private-artifact model, codec, and reducer (retired design)

Status: frozen historical, non-executable design evidence.

At and after the [repository effective point](decision.md#repository-effective-point),
every rule marker, model, codec, reducer transition, invariant, numeric value, and
modal verb in this file describes the retired private-artifact design only. Nothing
in this file is a current rule, schema contract, runtime input, implementation
instruction, or future vNext obligation. Before that point, the fail-closed
conditions in the retirement decision apply and no private-artifact work can start.

## Frozen historical model

### Canonical frame and primitives

<a id="rule-PA-MODEL-0001"></a>
**[rule:PA-MODEL-0001]** Every independently persisted semantic value, exact
response, tombstone, canonical former server store `bytea` argument, and canonical
former server store `bytea` result is one framed root record:

```text
offset  size  value
0       6     ASCII DECXPA
6       2     codec version, u16be, exactly 1
8       1     root record tag
9       4     payload length, u32be
13      N     exact payload
```

Only roots have the frame. Nested values use their inline bodies and have no
frame, tag, codec field, implicit length, or padding. Field order is written
order. The primitive aliases are:

```text
Id       = 16-byte RFC-variant UUIDv4
BootId   = 16-byte RFC-variant UUID
D        = 32-byte digest
U        = u64be in 0..=9_223_372_036_854_775_807
Time     = U in 0..=253_402_300_799_999_999
Bytes[n] = u32be length in 0..=n || length raw bytes
O<T>     = u8 0, or u8 1 || inline T
S<T,n>   = u32be count in 0..=n || count inline T values
Reason   = domain:u8 || code:u8
```

Signed integers are fixed-width big-endian two's-complement values. A Boolean is
one byte, exactly 0 or 1. `StatTime` is `i64 seconds || u32 nanoseconds`, with
nanoseconds below 1,000,000,000. Every canonical `u64`, including revision,
epoch, generation, length, device, inode, link count, and process tick, follows
`U`. former server store represents it as nonnegative `bigint`, never `numeric`.

The path codec and internal names are exact:

```text
RawUnixNameV1 = u16be(byte_length) || byte_length raw bytes
  byte_length is 1..=255
  NUL, '/', '.', and '..' are invalid
  UTF-8 is neither required nor implied

AbsoluteUnixPathV1 = u8(component_count) || component RawUnixNameV1 values
  component_count is 1..=64
  the leading '/' is implicit and is not encoded
  materialized length, including separators, is at most 4,096 bytes
  maximum canonical encoding is 4,161 bytes
  a controlled parent is at most 63 components and 4,036 materialized bytes
  maximum controlled-parent encoding is 4,100 bytes

RelativeUnixPathV1 = u8(component_count) || component RawUnixNameV1 values
  component_count is 0..=8
  only the manifest root has zero components
  maximum canonical encoding is 2,057 bytes
```

Path order compares each component as an unsigned byte sequence. A shorter equal
prefix sorts first. Duplicate paths are invalid. Each nonroot manifest entry has
an earlier directory parent. The exact internal names are:

| Object | Exact name | Raw name bytes |
| --- | --- | ---: |
| Stage root | `.decodex-pa-stage-` plus the operation ID as a canonical lowercase UUID | 54 |
| Owned root | `.decodex-pa-owned-` plus the operation ID as a canonical lowercase UUID | 54 |
| Quarantine root | `.decodex-pa-quarantine-` plus the operation ID as a canonical lowercase UUID | 59 |
| Ownership marker | `.decodex-pa-owner-v1` | 20 |
| Payload | `payload` | 7 |

The three root names are derived from the operation ID. A caller cannot supply or
change them. No map, float, omitted field, duplicate field, alternate width,
textual enum, padding, or trailing byte is valid.

### Closed numeric vocabulary

<a id="rule-PA-MODEL-0002"></a>
**[rule:PA-MODEL-0002]** Every value below is one `u8`. An unlisted value is
invalid.

| Type | Exact values |
| --- | --- |
| `OperationKind` | `1 CaptureFile`, `2 CaptureTree`, `3 PublishFile`, `4 PublishTree`, `5 OwnedRootLifecycle` |
| `ConsumerProfile` | `1 XY1369File`, `2 XY1370Tree` |
| `ScopeKind`; `EntityKind` | `1 Project`; `1 PrivateArtifactCluster` |
| `Lifecycle` | `1 Running`, `2 Waiting`, `3 Attention`, `4 Terminal` |
| `Milestone` | `1 IntentDurable`, `2 ContentRegistered`, `3 StageOwned`, `4 PayloadDurable`, `5 PublishedDurable`, `6 OwnedRootDurable`, `7 QuarantinedDurable`, `8 CollectedDurable` |
| `ResultClass` | `0 None`, `1 Captured`, `2 Published`, `3 StageRetainedTargetExists`, `4 StageRetainedCrossDevice`, `5 StageRetainedUnsupported`, `6 StageNeedsAttention`, `7 OwnedRootReady`, `8 Quarantined`, `9 QuarantineRetainedTargetExists`, `10 QuarantineRetainedCrossDevice`, `11 QuarantineNeedsAttention`, `12 Collected`, `13 CollectionIncomplete`, `14 NoEffectUnsupported` |
| `Obligation` | `0 None`, `1 LinkedPublication`, `2 StageCollection`, `3 Retirement`, `4 QuarantineCollection`, `5 RetainedResidual`, `6 Complete` |
| `EffectClass` | `0 None`, `1 Create`, `2 WriteExact`, `3 NamespaceNoReplace`, `4 DurabilitySync`, `5 Collection` |
| `StepKind` | `1 CreateStageRoot`, `2 CreateOwnershipMarker`, `3 CreatePayloadDirectory`, `4 WritePayloadFile`, `5 SyncObject`, `6 PublishNoReplace`, `7 CreateOwnedRoot`, `8 RetireNoReplace`, `9 CollectStageEntry`, `10 CollectQuarantineEntry`, `11 CompleteDependency` |
| `StepState` | `1 Pending`, `2 Authorized`, `3 Attention`, `4 Complete` |
| `ReducerInputKind` | `1 Initialize`, `2 Authorize`, `3 RecordEffect`, `4 CompleteDependency`, `5 Reconcile`, `6 StopBound`, `7 RecordBlockedObservation` |
| `ReducerDecisionKind` | `1 Accept`, `2 Reject` |
| `EventKind` | `1 Initialized`, `2 StepAuthorized`, `3 StepSucceeded`, `4 StepRetryable`, `5 DependencySatisfied`, `6 AttentionEntered`, `7 Recovered`, `8 TerminalEntered`, `9 BoundReached` |
| `ObservationPhase`; `Presence` | `1 PreEffect`, `2 PostEffect`, `3 Reconcile`; `1 Absent`, `2 PresentSafe`, `3 PresentUnsafe`, `4 InspectionFailed` |
| `ObjectKind` | `1 Regular`, `2 Directory`, `3 Symlink`, `4 Fifo`, `5 Socket`, `6 BlockDevice`, `7 CharacterDevice`, `8 Unknown` |
| `Platform`; `FilesystemKind` | `1 MacOS`, `2 Linux`; `1 ApfsData`, `2 OrbStackOverlayfs`, `3 OrbStackVirtiofs`, `4 Unsupported` |
| `PreparedBlobKind`; `ReferenceKind` | `1 Content`, `2 Manifest`, `3 CaptureEvidence` |
| `DependencyKind`; `DependencyState` | `1 LinkedPublicationReceipt`, `2 Xy1363ConsumptionDecision`; `1 Pending`, `2 Complete` |
| `ReceiptState` | `1 PendingDeclared`, `2 AttentionMissingBytes`, `3 AttentionCorruptBytes`, `4 AttentionReconcileExhausted`, `5 Completed`, `6 TerminalRejected` |
| `AdmissionState` | `1 OpenClean`, `2 OpenDirty`, `3 Closing`, `4 ClosedQuiescent`, `5 Unavailable` |
| `LaunchState` | `1 Reserved`, `2 Spawned`, `3 LeaderExitedGroupPresent`, `4 GroupAbsent`, `5 SpawnFailedAbsenceProven` |
| `LaunchRoute` | `1 AccountAppServer`, `2 ManagedRepository`, `3 SupervisedValidation`, `4 RetainedTitleExperiment` |
| `GcState`; `GcAction` | `1 ObservedOnce`, `2 UnlinkSyncPending`, `3 Complete`, `4 Residual`; `0 None`, `1 Unlink`, `2 ShardSync` |
| `MarkerRole`; `PayloadKind`; `ArtifactKind` | `1 PublishStage`, `2 OwnedRoot`; `1 File`, `2 Tree`, `3 OwnedRoot` |
| `SyncKind`; `SyncPhase` | `1 DurableFile`, `2 DurableDirectory`; `1 StagePreparation`, `2 PostPublication`, `3 OwnedRootPreparation`, `4 PostRetirement`, `5 StageCollection`, `6 QuarantineCollection` |
| `OperatorRowKind` | `1 MaintenanceUnavailable`, `2 Incompatible`, `3 AttemptExhausted`, `4 Attention`, `5 PendingPreparation`, `6 ReconciliationResidual`, `7 GcOrphanResidual` |
| `IncompatibilityReason` | `1 UnknownVersion`, `2 UnknownTag`, `3 Noncanonical`, `4 DerivedColumnMismatch`, `5 StoredDigestMismatch`, `6 HostBootMismatch`, `7 EnvironmentMismatch` |
| `ExecutorHeadState`; `ResponseKind` | `1 Active`, `2 Maintenance`, `3 Unavailable`; `1 Preparation` |
| `Disposition`; `TombstoneTerminalResult` | `1 Pending`, `2 Accepted`, `3 TerminalRejected`, `4 PrunedReplay`; `1 Completed`, `2 TerminalRejected` |
| `ReturnClass` | `0 NotCalled`, `1 Success`, `2 NoEffectError`, `3 EffectUnknownError`, `4 EndOfStream`, `5 Absent` |
| `Role` | `1 ControlledParent`, `2 StageRoot`, `3 StageMarker`, `4 PayloadObject`, `5 ActiveRoot`, `6 ActiveMarker`, `7 QuarantineRoot`, `8 PublishedTarget`, `9 CurrentObject`, `10 ContainingDirectory` |
| `SyscallKind` | `0 None`, `1 Inspect`, `2 CreateDirectory`, `3 CreateAndWriteExactFile`, `4 DurableFileSync`, `5 DurableDirectorySync`, `6 PublishNoReplace`, `7 RetireNoReplace`, `8 UnlinkFile`, `9 RemoveDirectory` |
| `ResourceKind` | `1 Operation`, `2 PreparationReceipt`, `3 Executor`, `4 ProducerAdmission`, `5 ProducerLaunch`, `6 GcCandidate`, `7 BlobObject` |
| `BoundKind` | `1 Files`, `2 Directories`, `3 AggregateBytes`, `4 Name`, `5 Depth`, `6 Manifest`, `7 Envelope`, `8 CasReferences`, `9 Steps`, `10 Attempts`, `11 Events`, `12 Observations`, `13 SyncDebts`, `14 EvidenceBytes`, `15 PrivateArtifactHandles`, `16 StartupRows`, `17 ReconciliationRows`, `18 StatusRows`, `19 SqlDeadline`, `20 FilesystemController`, `21 GcRows`, `22 QueueOccupancy` |

<a id="rule-PA-MODEL-0003"></a>
**[rule:PA-MODEL-0003]** `authority/inventories.json#/reason_codes` owns the
complete `Reason(domain,code)` vocabulary. `Gc/LivenessMetadataMismatch` is only
`Reason(8,6)`. GC codes 1 through 5 keep their earlier meanings. Role observations
sort by `(role, subject ordinal with None first)` and are unique.

### Inline records and root payloads

<a id="rule-PA-MODEL-0004"></a>
**[rule:PA-MODEL-0004]** The canonical inline records are:

```text
CasReference =
  algorithm:u8=1; raw_sha256:D; length:U

DeclaredBlob =
  blob_kind:PreparedBlobKind; cas_reference:CasReference

ClusterRoster =
  capture_file_operation_id:Id; capture_file_prepare_receipt_id:Id;
  publish_file_operation_id:Id; publish_file_prepare_receipt_id:Id;
  publish_file_published_receipt_id:Id;
  capture_tree_operation_id:Id; capture_tree_prepare_receipt_id:Id;
  publish_tree_operation_id:Id; publish_tree_prepare_receipt_id:Id;
  publish_tree_published_receipt_id:Id;
  owned_root_operation_id:Id; owned_root_prepare_receipt_id:Id

RoleObservation =
  role:Role; subject_ordinal:O<u32>; presence:Presence;
  object_identity:O<ObjectIdentity body>; semantic_digest:O<D>

EffectIntent =
  effect_class:EffectClass; step_kind:StepKind; role:Role;
  subject_ordinal:O<u32>; expected_object_digest:O<D>;
  expected_cas:O<CasReference>; semantic_digest:O<D>;
  syscall_kind:SyscallKind

ManifestEntry =
  entry_kind:u8; path:RelativeUnixPathV1; uid:u32; gid:u32; mode:u16;
  if entry_kind=1 Directory: no further fields;
  if entry_kind=2 Regular:
    length:U; cas_reference:CasReference; semantic_content_digest:D

CaptureEntryEvidence =
  pass:u8 in {1,2}; manifest_ordinal:u16;
  object_identity:ObjectIdentity body; content_digest:O<D>; exact_eof_length:U

ProcessIdentity =
  platform:Platform; pid:u32; start_variant:u8;
  if platform=MacOS and start_variant=1:
    seconds:i64; microseconds:u32;
  if platform=Linux and start_variant=2:
    start_ticks:U

ResourceRef =
  resource_kind:ResourceKind;
  if Operation, PreparationReceipt, ProducerLaunch, or GcCandidate: value:Id;
  if Executor, ProducerAdmission, or BlobObject: value:D

LogicalObjectKey =
  phase:SyncPhase; role:Role; subject_ordinal:O<u32>; sync_kind:SyncKind

StableObjectIdentity =
  platform:Platform; filesystem_kind:FilesystemKind; object_kind:ObjectKind;
  device:U; inode:U; uid:u32; gid:u32; mode:u16

SyncDebtAction =
  action:u8;
  if action=1 Ensure: debt:SyncDebt body;
  if action=2 Consume: debt_ordinal:u16; expected_record_digest:D

GcSubject =
  variant:u8;
  if variant=1 Canonical: cas_reference:CasReference;
  if variant=2 NoncanonicalShardEntry: shard:u8; entry_fingerprint:D

GcObservation =
  presence:Presence; identity:O<ObjectIdentity body>;
  actual_digest:O<D>; observed_at:Time
```

`ClusterRoster` is exactly 192 bytes. `ProcessIdentity` platform and start
discriminants must agree, and macOS microseconds are below 1,000,000.

`BootScope` is the following explicit sum. Its platform, host-boot, and
execution-scope discriminants must agree. Every namespace device, namespace
inode, and process tick uses `U`. macOS boot microseconds are below 1,000,000.

```text
BootScope =
  platform:Platform;
  host_boot_variant:u8;
    if platform=MacOS and host_boot_variant=1:
      bootsession_uuid:BootId; boottime_seconds:i64; boottime_microseconds:u32;
    if platform=Linux and host_boot_variant=2:
      boot_id:BootId;
  execution_scope_variant:u8;
    if platform=MacOS and execution_scope_variant=1 MacOSHost:
      no payload;
    if platform=Linux and execution_scope_variant=2 LinuxNamespaces:
      pid_namespace_device:U; pid_namespace_inode:U;
      mount_namespace_device:U; mount_namespace_inode:U;
      pid1_start_ticks:U;
  environment_receipt_digest:D
```

Dependency payloads are discriminated by `dependency_kind` and are exact:

```text
LinkedPublicationReceipt =
  source_operation_id:Id; required_receipt_id:Id
Xy1363ConsumptionDecision =
  subject_receipt_id:Id; peer_receipt_id:Id
```

Plan variants are discriminated only by `operation_kind` and have this exact field
order:

```text
CaptureFile =
  source_path:AbsoluteUnixPathV1; required_uid:u32; required_gid:u32;
  linked_publish_operation_id:Id; linked_publish_receipt_id:Id

CaptureTree =
  source_path:AbsoluteUnixPathV1; required_uid:u32; required_gid:u32;
  linked_owned_root_operation_id:Id;
  linked_publish_operation_id:Id; linked_publish_receipt_id:Id

PublishFile =
  target_path:AbsoluteUnixPathV1; content_ref:CasReference;
  semantic_content_digest:D; required_uid:u32; required_gid:u32; mode:u16;
  linked_capture_operation_id:Id; published_receipt_id:Id;
  paired_tree_receipt_id:Id

PublishTree =
  target_path:AbsoluteUnixPathV1; manifest_ref:CasReference;
  semantic_manifest_digest:D; linked_capture_operation_id:Id;
  linked_owned_root_operation_id:Id; published_receipt_id:Id;
  paired_file_receipt_id:Id

OwnedRootLifecycle =
  controlled_parent:AbsoluteUnixPathV1;
  required_uid:u32; required_gid:u32; mode:u16;
  linked_capture_operation_id:Id; linked_publish_operation_id:Id;
  linked_publish_receipt_id:Id
```

The 34 root tags and exact payload order are:

| Tag | Root | Exact payload |
| ---: | --- | --- |
| 1 | `PlanV1` | `operation_id:Id; cluster_id:Id; operation_kind:OperationKind; consumer_profile:ConsumerProfile; scope_kind:ScopeKind=1; project_id:Id; entity_kind:EntityKind=1; entity_id:Id=cluster_id; expected_scope_revision:U>=1; server_identity_digest:D; boot_scope_digest:D; environment_receipt_digest:D; cluster_roster:ClusterRoster; variant:PlanVariant(operation_kind)` |
| 2 | `OperationHeadV1` | `operation_id:Id; plan_digest:D; revision:U; lifecycle:LifecycleMilestone body; last_step_ordinal:u32; last_event_ordinal:u32; total_attempts:u32; total_observations:u32; evidence_bytes:U; pending_step_id:O<D>; sync_debt_count:u16; sync_debt_set_digest:D; published_receipt_id:O<Id>; last_executor_epoch:U; attention_digest:O<D>; created_at:Time; updated_at:Time` |
| 3 | `ReducerInputV1` | `operation_id:Id; input_kind:ReducerInputKind; expected_revision:U; proposed_event_ordinal:u32; proposed_new_step_ordinal:O<u32>; step_id:O<D>; proposed_attempt_ordinal:O<u8>; executor_epoch:U; maintenance_generation:U; database_time:Time; current_head_digest:D; plan_digest:D; observation:O<Observation body>; dependency_receipt_digest:O<D>; bound_kind:O<BoundKind>` |
| 4 | `ReducerDecisionV1` | `operation_id:Id; decision_kind:ReducerDecisionKind; reason:Reason; current_head_digest:D; next_head:O<OperationHead body>; event:O<Event body>; current_step_after:O<Step body>; created_step:O<Step body>; attempt:O<Attempt body>; effect_intent:O<EffectIntent>; sync_debt_actions:S<SyncDebtAction,2>` |
| 5 | `LifecycleMilestoneV1` | `lifecycle:Lifecycle; milestone:Milestone; result:ResultClass; obligation:Obligation` |
| 6 | `StepV1` | `operation_id:Id; step_ordinal:u32; step_id:D; step_kind:StepKind; effect_class:EffectClass; subject_ordinal:O<u32>; created_revision:U; completed_revision:O<U>; attempts_used:u8; step_state:StepState; active_attempt_ordinal:O<u8>; outcome_probes_used:u8` |
| 7 | `AttemptV1` | `operation_id:Id; step_id:D; attempt_ordinal:u8; authorization_event_ordinal:u32; authorized_revision:U; executor_epoch:U; maintenance_generation:U; pre_observation_digest:D; effect_intent_digest:D; committed_at:Time` |
| 8 | `EventV1` | `operation_id:Id; event_ordinal:u32; event_kind:EventKind; revision_before:U; revision_after:U; step_id:O<D>; attempt_ordinal:O<u8>; input_digest:D; head_before_digest:D; head_after_digest:D; reason:Reason; occurred_at:Time` |
| 9 | `ObservationV1` | `operation_id:Id; step_id:O<D>; attempt_ordinal:O<u8>; phase:ObservationPhase; observed_at:Time; environment_receipt_digest:D; syscall_kind:SyscallKind; return_class:ReturnClass; errno:O<i32>; outcome_reason:Reason; evidence_bundle_ref:O<CasReference>; role_observations:S<RoleObservation,4>` |
| 10 | `PreparationReceiptV1` | `receipt_id:Id; protocol_major:u16=1; protocol_minor:u16=3; idempotency_digest:D; operation_id:Id; envelope:Envelope body; request_digest:D; state:ReceiptState; reconciliation_attempts:u8; attention_reason:O<Reason>; exact_response:O<ExactResponse body>; created_at:Time; updated_at:Time; next_reconcile_at:O<Time>; completed_at:O<Time>` |
| 11 | `PreparedBlobV1` | `receipt_id:Id; operation_id:Id; ordinal:u16; blob_kind:PreparedBlobKind; cas_reference:CasReference` |
| 12 | `BlobReferenceV1` | `operation_id:Id; ordinal:u16; reference_kind:ReferenceKind; cas_reference:CasReference; semantic_digest:O<D>; created_revision:U` |
| 13 | `DependencyV1` | `operation_id:Id; dependency_kind:DependencyKind; state:DependencyState; kind_payload:DependencyPayload(dependency_kind); completion_receipt_id:O<Id>; completion_receipt_digest:O<D>; completed_at:O<Time>` |
| 14 | `ExecutorEpochV1` | `epoch:U; server_identity_digest:D; boot_scope_digest:D; environment_receipt_digest:D; guard_identity_digest:D; daemon_instance_id:Id; maintenance_generation:U; registered_at:Time` |
| 15 | `ExecutorHeadV1` | `current_epoch:U; epoch_digest:D; state:ExecutorHeadState; server_identity_digest:D; boot_scope_digest:D; environment_receipt_digest:D; updated_at:Time` |
| 16 | `TombstoneV1` | `protocol_major:u16=1; protocol_minor:u16=3; idempotency_digest:D; receipt_id:Id; operation_id:Id; request_digest:D; plan_digest:D; declared_set_digest:D; terminal_result:TombstoneTerminalResult; dependency_count:u8; completed_at:Time; pruned_at:Time` |
| 17 | `ManifestV1` | `operation_id:Id; entry_count:u16; directory_count:u16; file_count:u16; aggregate_file_bytes:U; entries:S<ManifestEntry,1024>` |
| 18 | `EnvelopeV1` | `protocol_major:u16=1; protocol_minor:u16=3; idempotency_digest:D; receipt_id:Id; plan:Plan body; declared_set_digest:D; declared_blobs:S<DeclaredBlob,514>; capture_evidence_ref:O<CasReference>; capture_evidence_digest:O<D>` |
| 19 | `CasReferenceV1` | `CasReference` |
| 20 | `SyncDebtV1` | `operation_id:Id; debt_ordinal:u16; logical_key:LogicalObjectKey; depth:u8; authority_root_digest:D; stable_identity:StableObjectIdentity; latest_expected_identity:ObjectIdentity body; created_revision:U; updated_revision:U` |
| 21 | `AttentionV1` | `status_row_id:Id; resource:ResourceRef; operation_id:O<Id>; reason:Reason; first_detected_at:Time; last_detected_at:Time; attempts:u8; pre_attention_head_digest:O<D>` |
| 22 | `IncompatibilityV1` | `status_row_id:Id; resource:ResourceRef; operation_id:O<Id>; reason:IncompatibilityReason; record_tag:O<u8>; observed_codec:O<u16>; stored_digest:D; detected_at:Time` |
| 23 | `OperatorProjectionV1` | `row_kind:OperatorRowKind; status_row_id:Id; operation_id:O<Id>; operation_kind:O<OperationKind>; lifecycle:O<Lifecycle>; milestone:O<Milestone>; effect_class:O<EffectClass>; attempts_used:u8; reason:Reason; obligation:O<Obligation>` |
| 24 | `OwnershipMarkerV1` | `operation_id:Id; cluster_id:Id; plan_digest:D; marker_role:MarkerRole; payload_kind:PayloadKind; expected_cas:O<CasReference>; semantic_digest:O<D>` |
| 25 | `ProducerAdmissionHeadV1` | `server_identity_digest:D; host_boot_digest:D; boot_scope_digest:D; generation:U; state:AdmissionState; next_launch_ordinal:U; active_count:u16; daemon_instance_id:Id; updated_at:Time` |
| 26 | `ProducerLaunchV1` | `server_identity_digest:D; generation:U; launch_ordinal:U; launch_token:Id; route:LaunchRoute; state:LaunchState; reserved_at:Time; leader_pid:O<u32>; process_group:O<u32>; process_identity:O<ProcessIdentity>; updated_at:Time` |
| 27 | `GcCandidateV1` | `candidate_id:Id; status_row_id:Id; subject:GcSubject; state:GcState; observation_generation:u32; first_observation:GcObservation; second_observation:O<GcObservation>; pending_started_at:O<Time>; unlink_attempts:u8; sync_attempts:u8; active_action:GcAction; active_attempt_ordinal:O<u8>; active_authorization_digest:O<D>; unlinked_at:O<Time>; shard_synced_at:O<Time>; last_attempt_reason:O<Reason>; last_attempt_at:O<Time>; next_attempt_at:O<Time>; residual_reason:O<Reason>; completed_at:O<Time>` |
| 28 | `TransitionProposalV1` | `operation_id:Id; expected_revision:U; proposed_event_ordinal:u32; proposed_new_step_ordinal:O<u32>; step_id:O<D>; proposed_attempt_ordinal:O<u8>; executor_epoch:U; maintenance_generation:U; input_kind:ReducerInputKind; observation_digest:O<D>` |
| 29 | `ExactResponseV1` | `response_kind:ResponseKind; receipt_id:Id; operation_id:Id; disposition:Disposition; canonical_revision:U; result:ResultClass; artifact_kind:O<ArtifactKind>; semantic_digest:O<D>` |
| 30 | `BootScopeV1` | `BootScope` |
| 31 | `GuardIdentityV1` | `server_identity_digest:D; parent_identity:ObjectIdentity body; lock_identity:ObjectIdentity body` |
| 32 | `ObjectIdentityV1` | `platform:Platform; filesystem_kind:FilesystemKind; object_kind:ObjectKind; device:U; inode:U; uid:u32; gid:u32; mode:u16; link_count:U; length:U; mtime_seconds:i64; mtime_nanoseconds:u32; ctime_seconds:i64; ctime_nanoseconds:u32` |
| 33 | `PublishedArtifactReceiptV1` | `receipt_id:Id; operation_id:Id; cluster_id:Id; artifact_kind:ArtifactKind; semantic_digest:D; canonical_revision:U; published_at:Time` |
| 34 | `CaptureEvidenceBundleV1` | `operation_id:Id; plan_digest:D; manifest_digest:O<D>; aggregate_data_bytes:U; entry_count:u16; entries:S<CaptureEntryEvidence,2048>` |

Tags 0 and 35 through 255 are invalid. Exact framed maxima are:

| Tags | Bytes |
| --- | --- |
| Plan; OperationHead; ReducerInput; ReducerDecision | 4,675; 238; 793; 1,253 |
| LifecycleMilestone; Step; Attempt; Event; Observation | 17; 94; 162; 191; 600 |
| PreparationReceipt; PreparedBlob; BlobReference; Dependency | 26,644; 89; 114; 122 |
| ExecutorEpoch; ExecutorHead; Tombstone; Manifest; Envelope | 181; 158; 195; 2,157,095; 26,426 |
| CasReference; SyncDebt; Attention; Incompatibility; OperatorProjection | 54; 186; 131; 125; 60 |
| OwnershipMarker; ProducerAdmissionHead; ProducerLaunch; GcCandidate | 154; 152; 124; 379 |
| TransitionProposal; ExactResponse; BootScope; GuardIdentity; ObjectIdentity | 131; 91; 104; 183; 82 |
| PublishedArtifactReceipt; CaptureEvidenceBundle | 110; 198,764 |

### Canonical validity, digests, and projection

<a id="rule-PA-MODEL-0005"></a>
**[rule:PA-MODEL-0005]** The decoder checks the root ceiling before allocation,
then magic, version, tag, exact payload length, primitive validity, enum and option
validity, sums, cardinality, order, uniqueness, and cross-field rules. It requires
cursor exhaustion, byte-identical re-encoding, the exact record digest, and the
exact relational projection in that order. New invalid input rolls back. A bad
stored record enters the same-transaction incompatibility path.

Cross-field validity is closed. A combination not permitted below or by the exact
GC and observation tables is noncanonical.

Plan and cluster validity is exact:

- `scope_kind` is Project, `entity_kind` is PrivateArtifactCluster, and
  `entity_id` equals `cluster_id`.
- `XY1369File` is valid only for `CaptureFile` and `PublishFile`.
  `XY1370Tree` is valid only for `CaptureTree`, `PublishTree`, and
  `OwnedRootLifecycle`.
- A cluster has at most one receipt and one operation for each of the five
  operation kinds. Every plan contains the same byte-identical `ClusterRoster`.
  Each plan ID, receipt ID, published-receipt ID, link, and paired-receipt ID
  equals its roster slot.
- The OwnedRoot receipt is the reservation anchor. A cluster can be `Reserved`
  with only that receipt or `Expanding` with the anchor and one through three
  other exact plan receipts. Every insert enforces unique operation kind and
  roster symmetry. `Declared` and every consumption require all five kinds, all
  shared cluster, project, revision, server, boot, and environment values, all
  identifier symmetry, and all available content and manifest symmetry. A
  partial cluster is not consumption authority.

Manifest and capture validity is exact:

- A manifest contains at most 512 directories, including the root, and at most
  512 regular files. `entry_count` equals the sequence count,
  `directory_count + file_count` equals `entry_count`, and
  `aggregate_file_bytes` equals the checked sum of regular-file lengths.
- Ordinal 0 is a Directory with the zero-component relative path. No other entry
  has that path. All later entries use the unsigned-byte path order defined in
  `PA-MODEL-0001`; each parent directory precedes its children; paths and
  `(platform,device,inode,object_kind)` identities are unique.
- A regular file has link count one in both capture passes. A directory link
  count can differ from one but is equal in both passes. UID, GID, and low
  `0o7777` mode bits are portable policy. Device, inode, link count, timestamps,
  process facts, and handles are evidence only. Ownership markers are not
  manifest entries.
- A file capture has no `manifest_digest`; a tree capture has
  `manifest_digest=Some(manifest)`, where `manifest` is the digest defined below
  over canonical `ManifestV1`. Capture evidence has
  exactly two entries for each captured object, ordered by pass and then manifest
  ordinal. Its `entry_count` equals that sequence count. Both passes have the
  same object identity for an ordinal. A directory entry has no content digest
  and has `exact_eof_length=0`. A regular entry has a content digest and an EOF
  length equal to its manifest length. The bundle operation and plan digest equal
  the enclosing capture plan.

Envelope, receipt, dependency, head, step, and response validity is exact:

- Declared blobs sort by `(raw_sha256,blob_kind,length)`. Raw digests are unique;
  one raw digest cannot have two kinds or lengths. The declared-set digest covers
  that complete order.
- Receipt and envelope protocol fields, receipt ID, operation ID, plan,
  declared-set digest, capture-evidence reference, and capture-evidence digest
  are equal. Their request, plan, envelope, and record digests use the exact
  formulas below.
- `Completed` requires an `Accepted` exact response and `completed_at`.
  `PendingDeclared` and the three preparation Attention states forbid both.
  `TerminalRejected` requires a nonzero `attention_reason`, has no operation
  head, and cannot claim accepted operation authority.
- `DependencyState::Pending` requires `completion_receipt_id`,
  `completion_receipt_digest`, and `completed_at` all absent. `Complete` requires
  all three present. Its kind payload must match `dependency_kind`.
- `StepState::Pending` has no `completed_revision`, no active attempt, and zero
  outcome probes. `Authorized` has no `completed_revision` and requires an active
  attempt in `1..=3`. `Complete` requires `completed_revision` and has no active
  attempt. `Attention` can retain an active attempt only for a reconcilable or
  bound-latched unknown outcome while probes are below 3; a sticky or exhausted
  Attention has no active attempt. `attempts_used` is in `0..=3`. A present
  active ordinal is in `1..=3` and equals the retained attempt.
- `OperationHeadV1.attention_digest` is present if and only if lifecycle is
  `Attention`. A terminal head has no pending step and has zero sync debts. Head
  counts, last ordinals, pending step digest, debt count, and debt-set digest
  equal their canonical child records.
- Role observations are strictly ordered by `(role,subject_ordinal)` with None
  first and are unique. `PresentSafe` requires identity. `Absent` forbids identity
  and semantic digest. `ProcessIdentity` and `BootScope` obey their platform and
  discriminant rules above.
- An exact response with `Pending` disposition has revision 0, result `None`, and
  no artifact kind or semantic digest. `Accepted` has revision 1 and its result,
  artifact kind, and semantic digest agree with the envelope operation kind and
  its canonical initialization result. `TerminalRejected` and `PrunedReplay`
  have revision 0 and cannot claim an accepted artifact. The response receipt and
  operation IDs equal the receipt that stores it.
- Every mode contains only low `0o7777` bits.

`ReducerDecisionV1` option coexistence is exact:

| Accepted decision | Required fields | Forbidden fields |
| --- | --- | --- |
| Initialize | next head, event, created step | current-step update, attempt, effect intent, debt actions |
| Authorize | next head, event, current step, attempt, effect intent | created step, debt actions |
| Successful effect or reconcile | next head, event, completed current step, and a next step unless terminal; zero through two exact debt actions | attempt, effect intent |
| Retryable result | next head, event, current Pending step | created step, attempt, effect intent, debt actions |
| Attention result | next head, event, current Attention step | created step, attempt, effect intent; debt actions unless the exact successful outcome already produced them |
| CompleteDependency | next head, event, completed current step, and a next step unless terminal | attempt, effect intent, debt actions |
| StopBound | next head, `BoundReached` event, current Attention step | created step, new attempt, effect intent, debt actions |
| Reject | nonzero reason only | every mutation field and all debt actions |

The exact observation/errno matrix is `PA-MODEL-0009`. The exact GC observation
and candidate field-presence matrices are `PA-GC-0002`. Malformed stored-record
options are exact: `observed_codec` is present only when at least eight bytes allow
bytes 6 through 7 to be decoded, and `record_tag` is present only when byte 8
exists. Bad or short magic is `Noncanonical`; there is no sentinel value.

Use `H(label,payload) = SHA256(ASCII(label) || 0x00 || payload)`. The required
formulas include:

```text
raw CAS hash = SHA256(raw object bytes)
content = H("decodex/private-artifact/v1/content",
            u64be(length) || raw object bytes)
manifest = H("decodex/private-artifact/v1/manifest", canonical ManifestV1)
plan = H("decodex/private-artifact/v1/plan", canonical PlanV1)
envelope = H("decodex/private-artifact/v1/envelope", canonical EnvelopeV1)
request = H("decodex/private-artifact/v1/request", canonical EnvelopeV1)
observation = H("decodex/private-artifact/v1/observation",
                canonical ObservationV1)
capture evidence = H("decodex/private-artifact/v1/capture-evidence",
                     canonical CaptureEvidenceBundleV1)
record = H("decodex/private-artifact/v1/record", framed record)
declared set = H("decodex/private-artifact/v1/declared-set",
                 u32be(count) || ordered inline DeclaredBlob values)
step = H("decodex/private-artifact/v1/step",
         operation_id || u32be(step_ordinal) || step_kind || effect_class ||
         O<u32>(subject_ordinal) || plan_digest)
idempotency = H("decodex/private-artifact/v1/idempotency",
                u16be(1) || u16be(3) || idempotency_uuid)
boot scope = H("decodex/private-artifact/v1/boot-scope", canonical BootScopeV1)
guard = H("decodex/private-artifact/v1/guard", canonical GuardIdentityV1)
marker = H("decodex/private-artifact/v1/marker", canonical OwnershipMarkerV1)
debt set = H("decodex/private-artifact/v1/sync-debt-set",
             u16be(count) || ordered SyncDebtV1 records)
cluster roster = H("decodex/private-artifact/v1/cluster-roster",
                   ClusterRoster body)
effect intent = H("decodex/private-artifact/v1/effect-intent",
                  EffectIntent body)
attempt = H("decodex/private-artifact/v1/attempt", framed AttemptV1)
noncanonical entry = H("decodex/private-artifact/v1/noncanonical-shard-entry",
                       shard || u16be(raw_name_length) || raw_name_bytes)
GC authorization = H("decodex/private-artifact/v1/gc-authorization",
                     current_candidate_record_digest || action ||
                     attempt_ordinal || database_time)
expected object = H("decodex/private-artifact/v1/expected-object",
                    u32be(count) || ordered inline RoleObservation values)
authority root = H("decodex/private-artifact/v1/sync-authority-root",
                   operation_id || root_role || O<u32>(root_subject) ||
                   inline StableObjectIdentity)
```

Sequence projection uses
`H("decodex/private-artifact/v1/projection-sequence", canonical Seq body)`.
Manifest-entry identity uses
`H("decodex/private-artifact/v1/manifest-entry", ManifestEntry body)`. No digest
formula is recursive. Lowercase hexadecimal exists only for the CAS pathname and
advisory-lock input; it is not canonical record representation.

`Projection(tag)` is the tag byte followed by the complete canonical payload body.
It contains every field, option marker, discriminant, sequence count, and nested
inline body. `projection_record` equals the validator output byte-for-byte.
Commands derive scalar columns only from that output and compare them with `IS NOT
DISTINCT FROM`. Sequence columns store exact count and projection-sequence digest.
Locked mutation repeats every comparison.

former server store uses exactly one private validator with this signature:

```sql
decodex.private_artifact_validate_record_v1(
    p_record bytea,
    p_expected_tag smallint,
    p_expected_record_digest bytea
) RETURNS bytea
```

The validator returns the canonical projection record. Runtime roles cannot call
it directly. Commands call it only inside their authority transaction and use its
output for every relational projection comparison.

### Subjects and fixed plans

<a id="rule-PA-MODEL-0006"></a>
**[rule:PA-MODEL-0006]** Subject ordinals are: controlled parent 0, stage root 1,
stage marker 2, tree payload root 3, active root 4, active marker 5, quarantine
root 6, published target 7, file payload 8, reserved 9 through 15, and manifest
entry `m` as `16+m`.

There is no capture-source subject. Ordinals 9 through 15 are invalid for every
current step, role observation, effect intent, and debt key.

Manifest ordinal 0 maps to subject 3 before tree publication, subject 4 while
active, subject 6 after retirement, and subject 7 after publication. A nonroot
manifest entry always retains `16+m`. Step ordinal is its one-based append position.

Capture operations have one `CompleteDependency` step. `PublishFile` has exactly
14 steps: create stage, marker, payload, four preparation syncs, publish, two
post-publication syncs, marker collection and sync, root collection and sync.
`PublishTree` creates the stage, marker, payload root, ordered directories and
files; syncs files, marker, directories, stage, and parent; publishes; syncs stage
and parent; and collects marker and stage. Its exact size is `2F + 2D + 12`.
`OwnedRootLifecycle` creates and syncs the root and marker, consumes the linked
publication dependency, retires and syncs the parent, collects files in reverse
raw-path order, directories by descending depth then raw path, then marker and
root with their parent syncs. Its exact size is `2F + 2D + 10`.

Exact plan construction is:

```text
CaptureFile or CaptureTree:
  1 CompleteDependency(None)

PublishFile:
  1 CreateStageRoot(1)
  2 CreateOwnershipMarker(2)
  3 WritePayloadFile(8)
  4 SyncObject(8)
  5 SyncObject(2)
  6 SyncObject(1)
  7 SyncObject(0)
  8 PublishNoReplace(7)
  9 SyncObject(1)
 10 SyncObject(0)
 11 CollectStageEntry(2)
 12 SyncObject(1)
 13 CollectStageEntry(1)
 14 SyncObject(0)

PublishTree:
  append CreateStageRoot(1)
  append CreateOwnershipMarker(2)
  append CreatePayloadDirectory(3)
  for each nonroot directory m in manifest order:
    append CreatePayloadDirectory(16+m)
  for each file m in manifest order:
    append WritePayloadFile(16+m)
  for each file m in manifest order:
    append SyncObject(16+m)
  append SyncObject(2)
  for each directory m in (depth descending, raw path ascending), including root:
    append SyncObject(root ? 3 : 16+m)
  append SyncObject(1), SyncObject(0), PublishNoReplace(7)
  append SyncObject(1), SyncObject(0)
  append CollectStageEntry(2), SyncObject(1)
  append CollectStageEntry(1), SyncObject(0)

OwnedRootLifecycle:
  1 CreateOwnedRoot(4)
  2 CreateOwnershipMarker(5)
  3 SyncObject(5)
  4 SyncObject(4)
  5 SyncObject(0)
  6 CompleteDependency(None)
  7 RetireNoReplace(6)
  8 SyncObject(0)
  for each file m in reverse raw-path order:
    append CollectQuarantineEntry(16+m), SyncObject(QP(m))
  for each nonroot directory m in (depth descending, raw path ascending):
    append CollectQuarantineEntry(16+m), SyncObject(QP(m))
  append CollectQuarantineEntry(5), SyncObject(6)
  append CollectQuarantineEntry(6), SyncObject(0)
```

The immutable manifest is loaded and verified before a transition transaction.
For `PublishTree`, the locked plan owns the manifest reference and semantic digest.
For `OwnedRootLifecycle`, the dependency-completion transaction owns the immutable
Manifest `BlobReferenceV1` at ordinal 0; the lock readback returns that complete
record. Inside the lock, the adapter compares the verified manifest with the
applicable plan fields or returned bound-reference record. The reducer borrows the
verified value. No filesystem or CAS I/O occurs in the transaction.

### Total reducer and lifecycle

<a id="rule-PA-MODEL-0007"></a>
**[rule:PA-MODEL-0007]** `reduce_v1` is total and pure. It accepts `Initialize`
from genesis; `Authorize` for a pending effect; `RecordEffect` or `Reconcile` for
an authorized step; `Reconcile` for reconcilable attention with a retained active
attempt; `CompleteDependency` for a waiting dependency step whose locked
dependency is complete; `RecordBlockedObservation` for an exact negative
pre-effect result; and `StopBound` for a reached hard bound. Terminal state accepts
no input. Every other combination is a rejection with no consumed authority.

Its complete logical signature is:

```text
reduce_v1(
  plan: &PlanV1,
  state: Genesis | ReducerStateV1,
  verified_manifest: Option<&ManifestV1>,
  input: &ReducerInputV1
) -> ReducerDecisionV1
```

`ReducerStateV1` is only the in-memory view of the locked head, current step,
complete active attempt, canonically ordered dependencies, canonically ordered
sync debts, and immutable bound-manifest reference. It is not a persisted root.

One accepted input increments revision and event ordinal by one. A new step uses
the next ordinal. `Authorize` requires exact revision, event, attempt, current
epoch/generation, `ClosedQuiescent`, ordered positive preconditions, and every
step-specific identity and debt predicate. It writes one attempt and returns one
effect intent. Only acknowledged commit creates a permit.

Each attempt has one pre-effect observation and at most three shared post-effect or
reconciliation probes: immediate, at least one second later, then at least 30
seconds later. There is no fourth probe. Exact success completes the step.
Transient proven no-effect retries below attempt 3; attempt 3 enters
`Transition/AttemptExhausted`. An unknown effect retains the attempt for remaining
probes. Reconciliation that proves no effect returns pending if another attempt is
available, otherwise terminal attempt exhaustion. Reconciliation that proves the
effect completes the step without a new permit. Probe 3 ambiguity enters sticky
`Reconciliation/AttemptsExhausted`.

`RecordBlockedObservation` creates no attempt or permit. It persists the exact
negative observation, advances revision and event once, enters typed Attention,
and preserves objects and debts. `StopBound` retains an active attempt until
reconciliation resolves it. It never reconstructs a permit or erases observation
history.

Capture starts `Waiting/ContentRegistered/Captured/LinkedPublication` and becomes
terminal after its dependency. Publication progresses through `StageOwned`,
`PayloadDurable`, and `PublishedDurable`; the published receipt is inserted only
after both post-publication syncs. Owned-root lifecycle progresses through
`IntentDurable`, `OwnedRootDurable`, `QuarantinedDurable`, and
`CollectedDurable`. A retained or uncertain object enters Attention with
`RetainedResidual`.

| Operation point | Lifecycle | Milestone | Result | Obligation |
| --- | --- | --- | --- | --- |
| Capture initialized | Waiting | ContentRegistered | Captured | LinkedPublication |
| Capture dependency consumed | Terminal | ContentRegistered | Captured | Complete |
| Publish initialized | Running | ContentRegistered | None | None |
| Stage-root complete | Running | StageOwned | None | StageCollection |
| Final pre-publication parent sync | Running | PayloadDurable | None | StageCollection |
| Both post-publication syncs | Running | PublishedDurable | Published | StageCollection |
| Final stage-parent sync | Terminal | PublishedDurable | Published | Complete |
| Owned-root initialized | Running | IntentDurable | None | Retirement |
| Owned-root step 5 | Waiting | OwnedRootDurable | OwnedRootReady | Retirement |
| Owned-root dependency consumed | Running | OwnedRootDurable | OwnedRootReady | Retirement |
| Owned-root step 8 | Running | QuarantinedDurable | Quarantined | QuarantineCollection |
| Final quarantine-parent sync | Terminal | CollectedDurable | Collected | Complete |

### Step safety and sync debt

<a id="rule-PA-MODEL-0008"></a>
**[rule:PA-MODEL-0008]** Every step has a fixed class, role, subject, syscall,
ordered role set, intent fields, result mapping, debt actions, and milestone
effect. Creation uses `mkdirat` or create-new exact-file operations. Write steps
use verified CAS bytes, exact length and EOF, owner, mode, identity, and digest.
Publication and retirement use only the platform no-replace syscall after immediate
whole-source revalidation. Collection is descriptor-relative and removes only the
exact planned object. `ENOENT` is success only after immediate exact absence and
stable-parent proof.

The debt key is `(operation_id, phase, role, subject_ordinal, sync_kind)`. Depth is
derived and is not identity. A new key gets the next ordinal. An existing key
requires the same authority-root digest and stable identity, preserves its first
ordinal and revision, and atomically replaces the complete latest identity. A
stable identity or authority-root change enters `Filesystem/WrongIdentity`.

Canonical debt-set order is phase ascending, depth descending, role ascending,
subject None before Some and then numeric, sync kind ascending, and debt ordinal
ascending. Object identity is checked content and is not a key or ordering field.

A consume reopens through the same pinned root and logical subject, checks stable
and latest identity, performs the complete sync primitive, rechecks stable
identity, and deletes only that debt. A crash can repeat synchronization. Maximum
simultaneous PublishTree debt is `F + D + 3 = 1,027`.

The exact Ensure and SyncObject mappings follow the fixed plan position, phase,
role, subject, sync kind, and authority root. Stage work uses controlled parent or
stage root. Owned-root work uses controlled parent or active root. Quarantine
collection uses quarantine root internally and controlled parent externally.
Missing, duplicate, wrongly rooted, wrongly phased, or wrongly roled debt is
`Transition/SyncDebtMismatch`.

The step-safety matrix is exact:

`PS(r,s)` is the canonical `PresentSafe` role observation with exact identity.
`A(r,s)` is exact absence. `EO` is the ordered pre-effect observation digest
defined below. `MI(m)` is the manifest-entry digest for ordinal `m`. For stage
collection, `parent(2)=1` and `parent(1)=0`. For quarantine collection,
`parent(16+m)=QP(m)`, `parent(5)=6`, and `parent(6)=0`. Authorize emits
`StepAuthorized`; effect success emits `StepSucceeded`; recovered success emits
`Recovered`.

| Step | Class; primary role; syscall | Ordered observations, action, and postcondition | Exact intent fields | Result and debt | Milestone effect |
| --- | --- | --- | --- | --- | --- |
| `CreateStageRoot(1)` | Create; StageRoot; CreateDirectory | `PS(ControlledParent,0), A(StageRoot,1)`; require supported parent, device, and mode; use `mkdirat`; require the exact new directory and the same parent identity except permitted mutable fields | CAS absent; semantic `plan_digest`; object `EO` | Common create result; Ensure StageRoot 1 and ControlledParent 0 as specified below | `StageOwned` |
| `CreateOwnershipMarker(2 or 5)` | WriteExact; StageMarker or ActiveMarker; CreateAndWriteExactFile | Stage form: `PS(StageRoot,1), A(StageMarker,2)`; active form: `PS(ActiveRoot,4), A(ActiveMarker,5)`; create-new and write the complete marker; verify identity, mode, length, and marker digest | CAS absent; semantic marker digest; object `EO` | Common create result; Ensure marker and containing root | No direct milestone |
| `CreatePayloadDirectory(3 or 16+m)` | Create; PayloadObject; CreateDirectory | Root form: `PS(StageRoot,1), A(PayloadObject,3)`; nonroot form: `PS(PayloadObject,SP(m)), A(PayloadObject,16+m)`; require parent-before-child order; use `mkdirat`; verify manifest owner, mode, and type | CAS absent; semantic `MI(m)`; object `EO` | Common create result; Ensure child and canonical parent | No direct milestone |
| `WritePayloadFile(8 or 16+m)` | WriteExact; PayloadObject; CreateAndWriteExactFile | File form: `PS(StageRoot,1), A(PayloadObject,8)`; tree form: `PS(PayloadObject,SP(m)), A(PayloadObject,16+m)`; require verified CAS bytes; create-new, write exact length, require exact EOF, and verify digest, owner, mode, identity, and length | Exact content `CasReference`; semantic content digest; object `EO` | Common create result; Ensure file and canonical parent | No direct milestone |
| `SyncObject(s)` | DurabilitySync; the canonical role for `s`; DurableFileSync or DurableDirectorySync | Require exactly one plan-position key below and the complete positive root and target role set; require stable and latest identity; perform Linux `fsync`, macOS `fsync` then successful `F_FULLFSYNC`, or the accepted directory primitive; require stable post-identity | CAS absent; semantic absent; object `EO` | Success consumes only the exact debt; uncertainty retains the debt and active attempt | Exact plan position advances `PayloadDurable`, `PublishedDurable`, `OwnedRootDurable`, `QuarantinedDurable`, or terminal state |
| `PublishNoReplace(7)` | NamespaceNoReplace; PublishedTarget; PublishNoReplace | `PS(ControlledParent,0), PS(StageRoot,1), PS(PayloadObject,8 or 3), A(PublishedTarget,7)`; require empty debt and immediate source revalidation; rename only `payload` with the platform no-replace call; require exact target, absent payload source, and retained stage root | File content or tree manifest `CasReference`; matching semantic digest; object `EO` | `EEXIST` gives `StageRetainedTargetExists`; `EXDEV` gives `StageRetainedCrossDevice`; exact unsupported gives `StageRetainedUnsupported`; ambiguity gives `StageNeedsAttention`; success Ensures StageRoot 1 and ControlledParent 0 | Publication is not durable until both following syncs complete |
| `CreateOwnedRoot(4)` | Create; ActiveRoot; CreateDirectory | `PS(ControlledParent,0), A(ActiveRoot,4)`; use the operation-derived name; create the exact root; verify identity and policy | CAS absent; semantic `plan_digest`; object `EO` | Common create result; Ensure ActiveRoot 4 and ControlledParent 0 | No direct milestone |
| `RetireNoReplace(6)` | NamespaceNoReplace; QuarantineRoot; RetireNoReplace | `PS(ControlledParent,0), PS(ActiveRoot,4), PS(ActiveMarker,5), A(QuarantineRoot,6)`; require empty debt, complete dependency, and immutable manifest binding; immediately revalidate the whole root; use the no-replace rename; require absent active name and exact quarantine tree and marker | Bound manifest `CasReference`; semantic manifest digest; object `EO` | `EEXIST` gives `QuarantineRetainedTargetExists`; `EXDEV` gives `QuarantineRetainedCrossDevice`; ambiguity gives `QuarantineNeedsAttention`; success Ensures ControlledParent 0 | Quarantine is not durable until the following parent sync |
| `CollectStageEntry(2 or 1)` | Collection; CurrentObject; UnlinkFile or RemoveDirectory | `PS(CurrentObject,s), PS(ContainingDirectory,parent(s))`; require the exact planned marker or empty stage root; remove descriptor-relatively; require exact absence and stable parent | CAS absent; marker digest for 2 or plan digest for 1; object `EO` | Exact absence succeeds; wrong object or nonempty root gives `CollectionIncomplete`; Ensure canonical parent | Final parent sync enters terminal Published |
| `CollectQuarantineEntry(16+m,5,6)` | Collection; CurrentObject; UnlinkFile or RemoveDirectory | `PS(CurrentObject,s), PS(ContainingDirectory,parent(s))`; require the fixed reverse-file, depth-descending-directory, marker, and root order; remove descriptor-relatively; require exact absence and stable parent | File CAS for a file; `MI(m)` for a directory; marker digest for 5; manifest digest for 6; object `EO` | Exact absence succeeds; mismatch, nonempty, or unexpected state gives `CollectionIncomplete`; Ensure canonical parent | Final parent sync enters `CollectedDurable/Terminal` |
| `CompleteDependency(None)` | None; no role; None | Locked dependency is Complete. Capture verifies its linked publication. OwnedRoot also requires its immutable manifest binding. No observation, attempt, effect, or syscall exists. | CAS, semantic digest, and object digest absent | Complete the current step; no debt action | `DependencySatisfied`, or `TerminalEntered` for capture |

The expected-object bytes and digest are exact:

```text
ordered_role_observations =
  u32be(n) ||
  canonical_inline(RoleObservation[0]) || ... ||
  canonical_inline(RoleObservation[n-1])

EO = H("decodex/private-artifact/v1/expected-object",
       ordered_role_observations)
```

The sequence is the complete `ObservationV1.role_observations` from the accepted
positive `PreEffect` observation in canonical `(role,subject with None first)`
order. It has one count and no root frame, observation field, or padding. Every
authorized effect sets `EffectIntent.expected_object_digest=Some(EO)`.
`CompleteDependency` and `RecordBlockedObservation` create no effect intent.
former server store recomputes EO from the locked canonical input and rejects a difference.

For a `PresentSafe` authority-root observation `r`, define:

```text
stable(r) =
  platform || filesystem_kind || object_kind ||
  device || inode || uid || gid || mode

authority_root_digest(operation_id, root_role, root_subject, r) =
  H("decodex/private-artifact/v1/sync-authority-root",
    operation_id:Id || root_role:u8 ||
    0x01 || u32be(root_subject) ||
    canonical_inline(stable(r)))
```

The `0x01` is the canonical present marker for the root subject. The source role
observation must have the exact root role and subject, `PresentSafe`, and
`Some(ObjectIdentity body)`. Root aliases are:

| Alias | Exact root role and subject |
| --- | --- |
| `CP` | `ControlledParent(1)`, subject 0 |
| `SR` | `StageRoot(2)`, subject 1 |
| `AR` | `ActiveRoot(5)`, subject 4 |
| `QR` | `QuarantineRoot(7)`, subject 6 |

On Ensure, the target and designated root come from the same qualifying
successful outcome defined by `PA-MODEL-0009`. The target supplies
`latest_expected_identity`; the root supplies `authority_root_digest`. On Consume,
the positive `PreEffect` observation must recompute the stored root digest. A
missing, unsafe, or unequal root enters `Attention(Filesystem/WrongIdentity)`.

Authority-root selection is exact:

| Sync phase | Exact authority-root selection |
| --- | --- |
| `StagePreparation(1)` | `CP` for ControlledParent subject 0; `SR` for every stage marker, payload, or stage-root debt |
| `PostPublication(2)` | `CP` for ControlledParent subject 0; `SR` for StageRoot subject 1 |
| `OwnedRootPreparation(3)` | `CP` for ControlledParent subject 0; `AR` for ActiveRoot or ActiveMarker |
| `PostRetirement(4)` | `CP` only |
| `StageCollection(5)` | `SR` for containing StageRoot subject 1; `CP` for containing parent subject 0 |
| `QuarantineCollection(6)` | `QR` for containing directories inside quarantine; `CP` for outer containing parent subject 0 |

For manifest entry `m`, where `p` is its manifest parent ordinal, define:

```text
SP(m) = 3      if p is 0
        16 + p otherwise

QP(m) = 6      if p is 0
        16 + p otherwise
```

Debt depth is deterministic: `CP`, `SR`, `AR`, `QR`, and their self-sync debts
have depth 0; stage marker, active marker, file payload 8, and tree payload root 3
have depth 1; staged manifest entry `16+m` has
`1 + path_component_count(m)`; quarantine containing subject 6 has depth 0; and a
quarantine containing subject `16+p` has `path_component_count(p)`.

Let `K(phase,role,subject,sync_kind,root)` denote the one `SyncDebtV1` with that
logical key, its designated authority-root digest, stable identity from its first
successful observation, and complete latest identity from the current successful
observation. The Ensure derivation is exhaustive:

| Successful effect | Exact Ensure set |
| --- | --- |
| `CreateStageRoot(1)` | `K(StagePreparation,StageRoot,1,DurableDirectory,SR)`; `K(StagePreparation,ControlledParent,0,DurableDirectory,CP)` |
| Stage `CreateOwnershipMarker(2)` | `K(StagePreparation,StageMarker,2,DurableFile,SR)`; `K(StagePreparation,StageRoot,1,DurableDirectory,SR)` |
| `CreatePayloadDirectory(3)` | `K(StagePreparation,PayloadObject,3,DurableDirectory,SR)`; `K(StagePreparation,StageRoot,1,DurableDirectory,SR)` |
| Nonroot `CreatePayloadDirectory(16+m)` | `K(StagePreparation,PayloadObject,16+m,DurableDirectory,SR)`; `K(StagePreparation,PayloadObject,SP(m),DurableDirectory,SR)` |
| `WritePayloadFile(8)` | `K(StagePreparation,PayloadObject,8,DurableFile,SR)`; `K(StagePreparation,StageRoot,1,DurableDirectory,SR)` |
| Tree `WritePayloadFile(16+m)` | `K(StagePreparation,PayloadObject,16+m,DurableFile,SR)`; `K(StagePreparation,PayloadObject,SP(m),DurableDirectory,SR)` |
| `PublishNoReplace(7)` | `K(PostPublication,StageRoot,1,DurableDirectory,SR)`; `K(PostPublication,ControlledParent,0,DurableDirectory,CP)` |
| `CreateOwnedRoot(4)` | `K(OwnedRootPreparation,ActiveRoot,4,DurableDirectory,AR)`; `K(OwnedRootPreparation,ControlledParent,0,DurableDirectory,CP)` |
| Active `CreateOwnershipMarker(5)` | `K(OwnedRootPreparation,ActiveMarker,5,DurableFile,AR)`; `K(OwnedRootPreparation,ActiveRoot,4,DurableDirectory,AR)` |
| `RetireNoReplace(6)` | `K(PostRetirement,ControlledParent,0,DurableDirectory,CP)` |
| `CollectStageEntry(2)` | `K(StageCollection,ContainingDirectory,1,DurableDirectory,SR)` |
| `CollectStageEntry(1)` | `K(StageCollection,ContainingDirectory,0,DurableDirectory,CP)` |
| `CollectQuarantineEntry(16+m)` | `K(QuarantineCollection,ContainingDirectory,QP(m),DurableDirectory,QR)` |
| `CollectQuarantineEntry(5)` | `K(QuarantineCollection,ContainingDirectory,6,DurableDirectory,QR)` |
| `CollectQuarantineEntry(6)` | `K(QuarantineCollection,ContainingDirectory,0,DurableDirectory,CP)` |
| `SyncObject` or `CompleteDependency` | No Ensure |

A repeated Ensure for one key keeps its earliest debt ordinal and
`created_revision`, requires the same root digest and stable identity, and
atomically replaces the complete latest identity and `updated_revision`.

Each `SyncObject` plan position consumes exactly the following key. A subject does
not imply a phase, role, kind, or root.

| Plan position | Exact consumed debt |
| --- | --- |
| PublishFile step 4 | `K(StagePreparation,PayloadObject,8,DurableFile,SR)` |
| PublishFile step 5 | `K(StagePreparation,StageMarker,2,DurableFile,SR)` |
| PublishFile step 6 | `K(StagePreparation,StageRoot,1,DurableDirectory,SR)` |
| PublishFile step 7 | `K(StagePreparation,ControlledParent,0,DurableDirectory,CP)` |
| PublishFile step 9 | `K(PostPublication,StageRoot,1,DurableDirectory,SR)` |
| PublishFile step 10 | `K(PostPublication,ControlledParent,0,DurableDirectory,CP)` |
| PublishFile step 12 | `K(StageCollection,ContainingDirectory,1,DurableDirectory,SR)` |
| PublishFile step 14 | `K(StageCollection,ContainingDirectory,0,DurableDirectory,CP)` |
| PublishTree file-sync loop, manifest order | For file `m`: `K(StagePreparation,PayloadObject,16+m,DurableFile,SR)` |
| PublishTree marker sync | `K(StagePreparation,StageMarker,2,DurableFile,SR)` |
| PublishTree directory-sync loop, depth descending then raw path | Root uses subject 3; nonroot entry `m` uses subject `16+m`; each consumes `K(StagePreparation,PayloadObject,subject,DurableDirectory,SR)` |
| PublishTree fixed prepublication syncs | StageRoot then ControlledParent, using the PublishFile step-6 and step-7 keys |
| PublishTree fixed postpublication syncs | StageRoot then ControlledParent, using the PublishFile step-9 and step-10 keys |
| PublishTree stage-collection syncs | ContainingDirectory subject 1 under `SR`, then subject 0 under `CP` |
| OwnedRoot step 3 | `K(OwnedRootPreparation,ActiveMarker,5,DurableFile,AR)` |
| OwnedRoot step 4 | `K(OwnedRootPreparation,ActiveRoot,4,DurableDirectory,AR)` |
| OwnedRoot step 5 | `K(OwnedRootPreparation,ControlledParent,0,DurableDirectory,CP)` |
| OwnedRoot step 8 | `K(PostRetirement,ControlledParent,0,DurableDirectory,CP)` |
| Sync after collecting manifest entry `m` | `K(QuarantineCollection,ContainingDirectory,QP(m),DurableDirectory,QR)` |
| Sync after collecting marker 5 | `K(QuarantineCollection,ContainingDirectory,6,DurableDirectory,QR)` |
| Final sync after collecting root 6 | `K(QuarantineCollection,ContainingDirectory,0,DurableDirectory,CP)` |

The reducer finds exactly one matching record and consumes it by debt ordinal and
record digest. Missing, duplicate, differently rooted, differently phased, or
differently roled debt is `Transition/SyncDebtMismatch`.

### V4.3 observation amendments

<a id="rule-PA-MODEL-0009"></a>
**[rule:PA-MODEL-0009]** A debt-producing successful outcome uses exactly one
`ObservationV1`. Its phase is `PostEffect` or `Reconcile`; step ID is present and
equals the current step; attempt ordinal is present and equals the retained active
attempt; return is `Success`; errno is absent; reason is `Reason(0,0)`; syscall is
the authorized effect for `PostEffect` or `Inspect` for `Reconcile`; and roles are
the complete canonical successful-postcondition set. Every Ensure target and its
designated root occur in that same sequence as `PresentSafe` with
`Some(ObjectIdentity body)`. The target supplies the latest identity and the root
supplies the authority-root digest. If their role/subject tuples differ, both
entries are present. Earlier, pre-effect, cross-observation, other-probe, or
mutable-state identities are invalid.

A `Reconcile` observation qualifies only when it proves the exact effect occurred.
It produces the unchanged Ensure and milestone effects without a new permit. A
probe that proves no effect or remains ambiguous produces no debt. Rust and
former server store apply the same predicate. Missing, unsafe, duplicate, differently
phased, or cross-observation identity input is
`Transition/ObservationMismatch`.

The canonical errno/evidence combinations are:

| Outcome | Phase | Errno | Reason | Evidence |
| --- | --- | --- | --- | --- |
| Positive `NotCalled` | `PreEffect` | absent | `Reason(0,0)` | exact positive precondition role set |
| Blocked `NotCalled` | `PreEffect` | absent | exact nonzero typed reason | exact negative precondition role set |
| `Success` | `PostEffect` or `Reconcile` | absent | `Reason(0,0)` | exact successful state |
| `EndOfStream` or `Absent` | permitted outcome phase | absent | `Reason(0,0)` | exact end/absence state |
| `NoEffectError` | `PostEffect` | `Some(e)`, actual `e != 0` | exact nonzero typed reason | exact unchanged pre-state |
| `EffectUnknownError` after syscall error | `PostEffect` or `Reconcile` | `Some(e)`, actual `e != 0` | exact nonzero typed reason | neither exact state proved |
| `EffectUnknownError` after syscall success | `PostEffect` or `Reconcile` | absent | exact nonzero typed reason | complete ordered roles prove neither state |

The errno-absent unknown case requires a successful syscall, 1 through 4 canonical
roles, no `InspectionFailed`, at least one failed postcondition comparison, and a
failed unchanged-pre-state comparison. The failed postcondition is a presence,
identity, policy, or semantic-digest comparison. Canonical role observations are
the required evidence. A step-required `evidence_bundle_ref` keeps its separate
presence rule and cannot replace them. `Some(0)`, errno-absent no-effect,
errno-absent real syscall error, an errno-bearing success/end/absence, a
zero-reason error, or an errno-absent unknown result without the required mismatch
evidence is noncanonical. Every other combination is rejected.
