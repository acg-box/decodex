---
type: "Reference"
title: "Private-artifact foundations (retired design)"
openwiki_generated: true
---

# Private-artifact foundations (retired design)

Status: frozen historical, non-executable design evidence.

At and after the [repository effective point](decision.md#repository-effective-point),
every rule marker, owner, capability, invariant, state, dependency, and modal verb in
this file describes the retired private-artifact design only. Nothing in this file is
a current rule, runtime input, implementation instruction, or future vNext obligation.
Before that point, the fail-closed conditions in the retirement decision apply and no
private-artifact work can start.

## Frozen historical foundations

### Authority and trust boundary

<a id="rule-PA-FND-0001"></a>
**[rule:PA-FND-0001]** `decodex-core` owns one total, pure private-artifact
reducer and the canonical value model. former server store is the only durable authority
for plans, cluster rosters, heads, receipts, records, events, attempts,
observations, dependencies, debts, epochs, producer admission, GC work, status,
pruning, and replay prevention. Local content-addressed storage (CAS) owns large
bytes. former server store does not independently attest those bytes.

The concrete former server store adapter owns transaction order. There is no production
private-artifact store trait and no alternate runtime store. The daemon owns one
private-artifact execution lane. GPUI, SwiftUI, CLI, MCP, protocol clients, and
downstream consumers do not read former server store, CAS, private paths, or descriptors.
V1 is single-host and has no worker registry or distributed effect mesh.

The runtime proposes deterministic next ordinals. former server store locks the current
authority and requires the exact next value. Only an acknowledged transaction
commit can mint an affine filesystem-effect permit. An unknown commit cannot mint
or reconstruct a permit. A permit is private-constructible, nonserializable, and
lane-local. It cannot be queued for later use.

### Capability separation

<a id="rule-PA-FND-0002"></a>
**[rule:PA-FND-0002]** Keep these capabilities distinct:

- `PrivateArtifactDirectory` can capture bounded artifacts and publish create-new
  artifacts for an admitted absolute operator-owned private directory or an
  admitted relative DecodexRoot child.
- `OwnedEphemeralArtifactRoot` grants retirement authority only for an
  operation-unique, non-reused root that Decodex created below a retained
  controlled immediate parent.
- `QuarantinedPrivateArtifact` grants collection authority only after verified
  whole-root retirement.
- `ProducerStopped` means that the tracked producer leader exited and every
  tracked process group is absent.
- `ExclusiveMaintenancePermit` means cooperative scheduler and namespace
  quiescence after `ProducerStopped`. It does not contain an escaped or hostile
  same-UID process.

Opening, retaining, or discovering a path does not grant owned-root authority. A
caller assertion, ordinary Decodex child, matching bytes, matching marker, or
retained descriptor cannot upgrade one capability to another. Publication success
is immutable. Later stage collection, retirement, quarantine collection, or a
collection residual cannot create, revoke, or replace publication authority.

### Cluster and consumer boundary

<a id="rule-PA-FND-0003"></a>
**[rule:PA-FND-0003]** One private-artifact cluster contains exactly one plan of
each kind: `CaptureFile`, `PublishFile`, `CaptureTree`, `PublishTree`, and
`OwnedRootLifecycle`. Every plan contains the same immutable `ClusterRoster`.
Identifiers, preparation receipts, publication receipts, links, project,
revision, server, boot, and environment fields must be symmetric. A partial
cluster is not consumption authority.

The first cluster action is the `OwnedRootLifecycle` Tx A reservation. The exact
admission order is:

1. Reserve and create the owned-root operation.
2. Add `CaptureTree` only after `OwnedRootDurable` and bounded capture.
3. Add `PublishTree` only after tree Tx B registers its manifest and evidence.
4. Add `CaptureFile` after its independent bounded capture.
5. Add `PublishFile` only after file Tx B registers its content.
6. Enter `Declared` only when all five plans and every enforceable symmetry rule
   pass.

The derived cluster states are `Unreserved`, `Reserved`, `Expanding`, `Declared`,
`Active`, `Retained`, `Blocked`, and `Pruned`. Rows are append-only except for
their forward receipt and operation transitions.

After `CaptureFile` reaches `ContentRegistered`, XY-1369 receives only a bounded
`VerifiedCapturedFileV1`: capture receipt ID, semantic content digest, length, and
at most 16,777,216 bytes. After `CaptureTree` reaches `ContentRegistered`, XY-1370
receives only a bounded `VerifiedCapturedTreeV1`: receipt ID, semantic manifest
digest, codec 1, canonical directories and files, per-file bytes, and at most
33,554,432 aggregate bytes. Neither value contains a path, raw name, descriptor,
CAS reference, object identity, producer fact, dependency handle, maintenance
authority, marker, or capture-evidence bundle.

Only `PublishedDurable` creates `PublishedArtifactReceiptV1`. The receipt binds
receipt ID, operation ID, artifact kind, semantic digest, and canonical revision.
Downstream values cannot retire, collect, repair, prune, complete a dependency, or
mint a receipt.

### Frozen safety invariants

<a id="rule-PA-FND-0004"></a>
**[rule:PA-FND-0004]** Apply all of these invariants:

1. Producer admission is daemon-wide. V1 has no proven per-project or
   per-namespace confinement boundary.
2. Every daemon-owned child that can reach an admitted namespace gets a durable
   launch reservation before `spawn`.
3. Same-boot dirty, reserved, active, closing, or ambiguous producer state makes
   that execution scope `MaintenanceUnavailable`. There is no clear, adopt,
   timeout, or override command.
4. Every namespace effect requires the live kernel guard, current epoch, exact
   host and boot scope, closed-and-quiescent admission, current revision, current
   step, and current maintenance generation.
5. Every possible namespace effect gets an immediate post-effect observation. A
   pathname is not success evidence.
6. Filesystem effects are retryable but are not exactly once. A retry needs a
   fresh complete observation and immediate identical revalidation without an
   intervening await.
7. Every changed file and directory enters the canonical durable sync-debt set.
   Success requires an empty set.
8. Captured regular files, payload files, markers, and CAS objects have link count
   one. All distinct admitted names have distinct device/inode identities.
9. Symlinks, FIFOs, sockets, devices, unknown types, duplicate directory
   identities, mount transitions, and unsupported filesystems grant no authority.
10. Missing or corrupt referenced CAS bytes remain liveness roots and enter
    attention. They are not regenerated from an uncontrolled namespace.
11. Pending receipts and prepared-blob declarations are roots before an operation
    head exists.
12. Callers cannot select limits, retries, retention, dependencies, or effect
    policy.
13. Status and doctor are read-only.

The Unix restriction applies only to this private-artifact subsystem. It does not
change unrelated accepted core modules.

### Unsupported behavior and no-fallback rule

<a id="rule-PA-FND-0005"></a>
**[rule:PA-FND-0005]** V1 does not support:

- recovery of a pending effect across host boot, VM restart, Docker-daemon
  restart, or container namespace replacement;
- same-boot recovery from dirty or ambiguous producer authority;
- adoption of an orphan process, stage, target, root, or quarantine;
- hostile or escaped same-UID containment;
- distributed workers or multiple effect-enabled Decodex roots;
- remote filesystems, unaccepted mounts, Windows, or non-Unix publication;
- symlink, special-file, or hard-linked regular-file capture;
- overwrite, direct-final-write, plain-rename fallback, `linkat`, rollback,
  move-back, or destination adoption;
- exactly-once filesystem effects or automatic repair of missing/corrupt
  referenced CAS bytes;
- secure byte erasure, artifact export, artifact-specific backup receipts, legal
  holds, administrator holds, or mutating operator commands.

An unsupported, unproven, ambiguous, malformed, or unavailable condition returns a
typed stop or attention state. It does not authorize a fallback, override,
compatibility layer, alternate store, or second daemon.

### Redaction boundary

<a id="rule-PA-FND-0006"></a>
**[rule:PA-FND-0006]** Status, doctor, CLI, protocol errors, logs, panic text,
tracing, telemetry, and metrics must not expose paths, raw Unix names, raw hashes,
semantic digests, canonical record bytes, captured or published bytes, device or
inode facts, owner IDs, process identities, descriptors, guard or boot identities,
namespace facts, raw idempotency keys, SQL text, former server store diagnostics,
constraint names, errno text, or free-form error text.

Metrics contain aggregate counts only by closed lifecycle, effect class, and
reason. They contain no operation ID or status-row ID. Trusted in-process XY-1369
and XY-1370 values can contain semantic digests and bounded bytes; they are not
operator output.
