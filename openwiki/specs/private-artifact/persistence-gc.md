# Private-artifact persistence and garbage collection (retired design)

Status: frozen historical, non-executable design evidence.

At and after the [repository effective point](decision.md#repository-effective-point),
every rule marker, relation, transaction, receipt, lock, recovery rule, retention
rule, and modal verb in this file describes the retired private-artifact design
only. Nothing in this file is a current schema rule, runtime input, migration
instruction, garbage-collection instruction, or future vNext obligation. Before
that point, the fail-closed conditions in the retirement decision apply and no
private-artifact work can start.

## Frozen historical persistence design

### Receipt-first authority

<a id="rule-PA-PERSIST-0001"></a>
**[rule:PA-PERSIST-0001]** The request owner creates one UUIDv4 idempotency key,
receipt ID, operation ID, immutable plan, and envelope before Tx A. The lookup key
is `(protocol_major=1, protocol_minor=3,idempotency_digest)`. Exact reuse compares
protocol, codec, receipt and operation identity, kind/profile, scope/entity and
revision, plan and request digests, server/boot/environment digests, optional
capture-evidence digest, and the complete ordered deduplicated raw-hash/length
vector.

Equal completed authority returns the exact stored response bytes. Equal pending
authority returns or reconciles its exact state. Equal terminal rejection returns
its stored rejection. An equal tombstone returns `PrunedReplay` without allocation.
Any unequal field is `IdempotencyConflict` with no effect.

A bounded failure before Tx A commits one canonical `TerminalRejected` receipt and
response only. It creates no Tx A declaration, CAS object, operation event, genesis,
or effect permit.

### Tx A, create-only CAS, and Tx B

<a id="rule-PA-PERSIST-0002"></a>
**[rule:PA-PERSIST-0002]** For a new declaration, sort the complete deduplicated
hash set by raw digest. Acquire session hash locks in namespace 1273, then unique
shard locks in namespace 1274. The exact calls are
`pg_advisory_lock(1273,hashtext(lowercase_hex_hash))` in raw-hash order and
`pg_advisory_lock(1274,shard_byte)` in unique ascending shard order. A `hashtext`
collision can only add serialization; it cannot weaken exclusion. Tx A acquires
hierarchy namespace 1271, locks the
receipt/tombstone and candidate rows, applies the candidate-state rule, and inserts
only the pending receipt, envelope, and every prepared-blob declaration. Tx A does
not inspect unpublished bytes, create `blob_objects`, create a reference, create an
operation, or complete a receipt.

Only acknowledged Tx A commit permits create-only CAS publication. Publish in raw
hash order. Existing bytes succeed only after full length and SHA-256 verification.
Unknown Tx A commit permits at most three receipt readbacks and no CAS effect until
the pending declaration is known durable.

For Tx B and every existing verified-reference writer, CAS bytes already exist.
Under the same hash and shard locks, Rust verifies exact length and digest outside a
database transaction. The transaction then acquires 1271, locks candidate,
metadata, liveness-owner, and reference rows, applies the candidate-state rule,
inserts or compares metadata, and creates references. Private Tx B also validates
the structured records, inserts operation genesis, stores the exact accepted
response, and completes the receipt atomically.

The session releases the hash and shard locks only after acknowledged Tx B commit
or session loss.

The candidate rule is exact: no candidate continues; `ObservedOnce` and `Complete`
are deleted in the same transaction that creates the new root; every
`UnlinkSyncPending` substate and `Residual` blocks and rolls back. Candidate deletion
is never a standalone precursor.

### Pending preparation and reconciliation

<a id="rule-PA-PERSIST-0003"></a>
**[rule:PA-PERSIST-0003]** `PendingDeclared` has no operation head and its
declarations are GC roots. Exact available bytes allow Tx B. Missing bytes enter
`AttentionMissingBytes`. Wrong length or digest enters `AttentionCorruptBytes` and
is never overwritten. Three transient probes can enter
`AttentionReconcileExhausted`. Missing, corrupt, GC-blocked, and exhausted states
are sticky and do not consume a transient probe unless the failure is transient.

The transient schedule is immediate, at least one second later, then at least 30
seconds after probe 2. Reconciliation is total. A no-effect third attempt enters
terminal attempt exhaustion and cannot return Pending. All noncompleted receipt
states remain roots indefinitely.

The preparation controller has separate scan and completion lanes. The read-only
scan orders due nonsticky rows by `(next_reconcile_at,receipt_id)`, fetches 65,
examines at most 64, and selects at most the first completion-eligible row. The
in-memory slot is not a lease, durable claim, receipt authority, or permit. It is
lost on crash.

One completion turn revalidates one receipt, acquires at most 514 hash and 256
shard locks, verifies direct and transitive objects, and commits one outcome. An
unknown Tx B commit permits at most three readbacks and no resubmission. Scan time
is 30 seconds; completion time is 600 seconds. The pass has one coalesced wake and
cannot complete more than one receipt.

### Cluster dependencies and immutable receipts

<a id="rule-PA-PERSIST-0004"></a>
**[rule:PA-PERSIST-0004]** Plans, not callers or DTOs, derive dependencies.
CaptureFile and CaptureTree wait for their linked publication receipts. PublishFile
and PublishTree retain paired-receipt consumption dependencies for pruning.
OwnedRoot waits for the linked tree publication before retirement and pruning.
Dependency completion is a former server store-owned immutable fact.

The private completion helper validates the exact linked operation, plan, receipt,
cluster roster, artifact kind, manifest reference, semantic digest, and pair in one
transaction. For OwnedRoot, it also inserts one immutable Manifest
`BlobReferenceV1` at ordinal 0. Equal replay returns false without mutation. An
unequal binding conflicts. Runtime and `PUBLIC` have no execute permission.

```sql
decodex.private_artifact_complete_dependency_from_receipt_v1(
    p_operation_id uuid,
    p_dependency_kind smallint,
    p_completion_receipt_id uuid,
    p_completion_receipt_digest bytea,
    p_subject_receipt_id uuid,
    p_peer_receipt_id uuid
) RETURNS boolean
```

For linked publication, peer receipt is `NULL`. For an XY-1363 decision, both
receipt IDs match the dependency payload and the command inserts its immutable
decision receipt before it calls this helper in the same transaction.

The reducer can consume a lifecycle dependency only after locked `Complete`
authority. A pruning-only completion changes no operation lifecycle or event
ordinal. An immutable successful publication receipt remains successful after any
later collection residual.

### Locked transitions and stored incompatibility

<a id="rule-PA-PERSIST-0005"></a>
**[rule:PA-PERSIST-0005]** One top-level `READ COMMITTED` transition locks and
returns database time, plan, head, current step, complete active attempt, canonical
dependencies and debts, and the immutable bound-manifest reference. Rust decodes
and re-encodes every locked record, runs the reducer synchronously without an
await, and supplies the exact expected revision, event, step, attempt, epoch,
maintenance generation, head digest, active-attempt digest, and bound-manifest
digest to the apply function.

Apply mode 1 requires canonical input and decision data and null incompatibility
fields. Apply mode 2 verifies that malformed bytes and their stored digest still
belong to the locked row; sets `blocked_incompatible`; writes canonical
incompatibility and status records; and updates the exact counter last. Mode 2 does
not run the reducer or change head, revision, step, attempt, or event authority.

Only `commit().await == Ok(())` can return an affine effect permit. Unknown commit
allows bounded authority readback only. An acknowledged ordinary transition uses
the initial authority read plus `BEGIN`, lock, apply, and `COMMIT`: five protocol
statements. Its three-readback unknown-commit maximum is eight.

The exact lock result is:

```sql
decodex.private_artifact_lock_transition_v1(
    p_operation_id uuid,
    p_expected_revision bigint,
    p_executor_epoch bigint,
    p_maintenance_generation bigint
) RETURNS TABLE (
    database_time_micros bigint,
    plan_record bytea,
    plan_record_digest bytea,
    head_record bytea,
    head_record_digest bytea,
    current_step_record bytea,
    active_attempt_record bytea,
    dependency_records bytea[],
    sync_debt_records bytea[],
    bound_manifest_reference_record bytea
)
```

Nullable records are SQL `NULL`; arrays remain in canonical order. Apply is:

```sql
decodex.private_artifact_apply_transition_v1(
    p_mode smallint,
    p_operation_id uuid,
    p_expected_revision bigint,
    p_proposed_event_ordinal bigint,
    p_proposed_new_step_ordinal bigint,
    p_proposed_attempt_ordinal smallint,
    p_expected_executor_epoch bigint,
    p_expected_maintenance_generation bigint,
    p_expected_head_digest bytea,
    p_expected_active_attempt_digest bytea,
    p_expected_bound_manifest_reference_digest bytea,
    p_input_record bytea,
    p_input_digest bytea,
    p_decision_record bytea,
    p_decision_digest bytea,
    p_incompatibility_reason smallint,
    p_bad_record_tag smallint,
    p_observed_codec integer,
    p_bad_record_digest bytea
) RETURNS TABLE (
    outcome smallint,
    canonical_revision bigint,
    canonical_event_ordinal bigint,
    canonical_head_record bytea,
    canonical_head_digest bytea
)
```

### V22 baseline and V23 additions

<a id="rule-PA-PERSIST-0006"></a>
**[rule:PA-PERSIST-0006]** Preserve the bound V22 baseline exactly. The 22 whole
migrations, 78 relations, 161 function contracts, and 54 runtime-executable
functions are fixed by `authority/v22-baseline.tsv`,
`authority/v22-relations.tsv`, and the two verbatim source slices. A count does not
substitute for exact members or bytes.

V23 adds exactly the 21 relations and 24 functions in
`authority/inventories.json#/v23_relations` and `/v23_functions`. Of the 24
functions, 22 are runtime-executable and 2 are private. The final exact totals are
99 relations, 185 function contracts, 76 runtime-executable functions, 67 retained
safety functions, and 142 retained triggers. V23 adds no sequence, trigger
function, caller-facing former server store enum, service, daemon, crate, or dependency.

The configured migration principal owns every V23 object. `PUBLIC` has no object
or function authority. Runtime has schema usage and execute only on the 22 named
runtime functions. Runtime has no table, sequence, DDL, truncate, trigger, owner,
grant-option, role-membership, or private-helper authority. Every runtime function
is migration-owned, `SECURITY DEFINER`, schema-qualified, fixed to
`pg_catalog, decodex`, and uses no dynamic SQL.

### GC liveness and writer exclusion

<a id="rule-PA-GC-0001"></a>
**[rule:PA-GC-0001]** The complete liveness predicate includes every accepted
History, Artifact-revision, and Context-Pack reference; every nonterminal
preparation declaration; every private blob reference; every unpruned terminal
cluster; and every exact receipt or dependency state that retains its cluster.
Declaration, prune, reference creation, and GC use the same predicate.

Every accepted reference creator acquires the full hash set and shard set before
1271 and row locks. It deletes an `ObservedOnce` or `Complete` candidate only in the
same transaction that creates the verified liveness root. It blocks on
`UnlinkSyncPending` or `Residual`.

Immediately before unlink authorization, GC repeats all liveness checks. Exact
matching metadata cancels the candidate. Missing or conflicting metadata enters
`Residual(Reason(8,6))` and issues no permit. There is no unlink path after a
positive, missing, or conflicting liveness result.

A noncanonical shard entry is identified only by shard and redacted fingerprint.
No raw name or reversible path is stored or logged. It always enters sticky
`Cas/NoncanonicalEntry` and never gains automatic unlink authority.

### GC observations, attempts, and recovery

<a id="rule-PA-GC-0002"></a>
**[rule:PA-GC-0002]** GC orders work by
`(work_class,due_at,subject_variant,subject_bytes,candidate_id)`. Canonical subject
bytes are the raw digest. Noncanonical subject bytes are shard then fingerprint.

An exact unreferenced canonical object gets one complete observation. At least
86,400 seconds later, a second observation must match the full first observation.
If any observed field changes, replace the complete first observation, increment
its observation generation, and restart the 86,400-second interval. A changed
observation is not automatically residual.

`UnlinkSyncPending` is committed and metadata is deleted before unlink. Each unlink
or shard-sync effect needs an acknowledged authorization transaction and one
affine permit. The attempt schedule is immediate, at least one second after failure
1, and at least 30 seconds after failure 2. Failure or unresolved outcome on
attempt 3 is `Residual`.

Unlink success requires immediate exact absence. `ENOENT` succeeds only with that
observation. Shard-sync success records both synchronization and completion. An
unknown authorization commit produces no permit. At most three readbacks and
targeted reconciliation can clear a lost authorization, record exact absence, or
enter residual. Blind replay is forbidden.

Metadata without bytes and no root uses the same two observations but needs no
unlink. Referenced missing/corrupt bytes remain roots and enter owner attention.
Unreferenced corrupt canonical paths and noncanonical entries remain residual.
Later valid republish can delete only `ObservedOnce` or `Complete` in the same
reference transaction.

The three GC functions are exact:

```sql
decodex.blob_gc_observe_candidate_v1(
    p_mode smallint,
    p_candidate_id uuid,
    p_expected_candidate_digest bytea,
    p_next_candidate_record bytea,
    p_next_candidate_digest bytea
) RETURNS bytea

decodex.blob_gc_begin_unlink_v1(
    p_candidate_id uuid,
    p_expected_candidate_digest bytea,
    p_next_candidate_record bytea,
    p_next_candidate_digest bytea
) RETURNS bytea

decodex.blob_gc_complete_unlink_v1(
    p_mode smallint,
    p_candidate_id uuid,
    p_expected_candidate_digest bytea,
    p_action smallint,
    p_expected_attempt_ordinal smallint,
    p_authorization_digest bytea,
    p_next_candidate_record bytea,
    p_next_candidate_digest bytea
) RETURNS bytea
```

Observe modes are Read 1 and Observe 2. Completion modes are Authorize 1, Record
2, Reconcile 3, and CancelLiveness 4. Actions are Unlink 1 and ShardSync 2. Every
mutation mode validates and rederives the complete next candidate record.

Candidate field presence is exact:

| State | Required shape |
| --- | --- |
| `ObservedOnce` | Canonical subject; first observation only; no pending, active, unlink, sync, retry, residual, or completion fields |
| Pending unlink | Canonical; matching second observation at least 86,400 seconds old; pending time; no active tuple or result fields |
| Authorized unlink | Pending shape plus complete Unlink action, ordinal, and authorization digest |
| Unlink retry wait | Pending shape, no active tuple, complete reason/time/next-time tuple |
| Pending shard sync | Matching observations and pending time; unlink time; no shard-sync/completion; optional authorized ShardSync tuple or retry tuple |
| Metadata-only Complete | Canonical; two Absent observations; completion time; no pending/unlink/sync/residual fields |
| Byte-object Complete | Canonical; two PresentSafe observations; pending, unlink, shard-sync, and completion times; no active/retry/residual fields |
| Noncanonical residual | Noncanonical subject; only `Cas/NoncanonicalEntry` residual |
| Observation residual | Canonical; coherent observation fields; residual only |
| Unlink residual | Canonical matching observations and pending; failure reason/time without next time; residual; no unlink time |
| Shard-sync residual | Canonical matching observations, pending and unlink time; failure reason/time without next time; residual; no sync/completion time |

Counters are 0 through 3. An active ordinal is 1 through 3 and equals its phase
counter. Active fields are all present or all absent. Authorization and success
clear the earlier retry tuple. Exhaustion requires the matching counter to equal 3.

### Retention, pruning, and tombstones

<a id="rule-PA-GC-0003"></a>
**[rule:PA-GC-0003]** Retain pending/attention receipts, nonterminal or residual
clusters, replay tombstones, current executor/producer heads, and GC residuals
indefinitely. Retain terminal rejected receipts, eligible terminal clusters,
superseded clean epochs, absent launch rows, and completed GC candidates for
7,776,000 seconds. The 90-day cluster clock starts at the latest terminal,
dependency, stage-collection, retirement, or quarantine-collection time.

Pruning requires no pending preparation; all plan-derived dependencies complete;
no open collection obligation; no retained stage, quarantine residual, attention,
or incompatibility; the complete linked cluster terminal; and the full retention
period elapsed. One transaction inserts the tombstone before deleting evidence and
references. CAS GC then starts its independent observation period.

### Backup and restore

<a id="rule-PA-PERSIST-0007"></a>
**[rule:PA-PERSIST-0007]** V1 has no artifact export, backup API, backup receipt,
or cross-medium atomic online snapshot. former server store-only, CAS-only, and
uncoordinated paired copies are not artifact-consistent. Only an external cold copy
after daemon shutdown and writer/GC quiescence can be described as consistent.

Trusted whole-cluster administrator restore remains the accepted authority
boundary and can redefine current authority. After restore, referenced missing
bytes enter missing attention; corrupt bytes enter corrupt attention; extra CAS
bytes enter orphan observation; and restored tombstones remain authoritative.
There is no automatic rollback detector or restore product gate.

### Total lock order

<a id="rule-PA-PERSIST-0008"></a>
**[rule:PA-PERSIST-0008]** Acquire multiple categories in this total order:

1. session hash locks, raw key bytes ascending, namespace 1273;
2. session shard locks, shard ascending, namespace 1274;
3. transaction hierarchy coordinator, namespace 1271;
4. receipts and tombstones by idempotency digest;
5. GC candidates by hash or fingerprint;
6. `blob_objects` by digest;
7. liveness-owner parents by owner kind then UUID;
8. declarations and all reference rows by digest, owner kind, UUID, and ordinal;
9. producer-admission head, then launch rows;
10. executor head and epoch rows;
11. operations by UUID;
12. steps by ordinal;
13. attempts, events, observations, and debts by canonical key;
14. published receipts by UUID;
15. dependencies by kind;
16. attention, incompatibility, and status rows by UUID;
17. status counters by kind.

Implicit unique-index, foreign-key, trigger, deferred-constraint, and conflict
locks use their owner's category. Namespace 1272 never nests with 1273 or 1274.
No database transaction spans filesystem I/O. Session hash/shard locks can remain
held across one bounded filesystem turn.
