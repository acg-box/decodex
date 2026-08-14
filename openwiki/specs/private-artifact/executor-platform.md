# Private-artifact executor and platform contract (retired design)

Status: frozen historical, non-executable design evidence.

At and after the [repository effective point](decision.md#repository-effective-point),
every rule marker, executor step, platform claim, synchronization rule, capability,
and modal verb in this file describes the retired private-artifact design only.
Nothing in this file is a current runtime contract, platform requirement,
implementation instruction, or future vNext obligation. Before that point, the
fail-closed conditions in the retirement decision apply and no private-artifact
work can start.

## Frozen historical executor and platform design

### Bounded descriptor-relative capture

<a id="rule-PA-EXEC-0001"></a>
**[rule:PA-EXEC-0001]** Capture is one bounded C-owned turn before Tx A. It uses
retained directory and ancestor descriptors and symlink-free descriptor-relative
traversal. It checks file/directory counts, aggregate and per-file bytes, depth,
names, types, identity, owner, all `0o7777` mode bits, link policy, allocation, and
one monotonic deadline. Admitted names contain only normal lexical components.

The turn performs a complete first pass, builds the deterministic manifest and
returned-byte buffer, then performs a complete second descriptor-relative layout,
metadata, identity, and byte observation. It reopens each file, compares through a
fixed-size buffer, and requires exact EOF. It does not allocate a second
file-sized buffer. Total data reads are at most
`2 * 33,554,432 + 512 = 67,109,376` bytes.

No manifest, bytes, or capture authority exists until the second pass succeeds.
The proof is equality of two bounded observed streams under producer teardown and
cooperative quiescence. It is not an immutable filesystem snapshot and does not
detect a change after the final observation. Capture syscalls are not reducer
steps. Tx B validates the exact `ManifestV1`, content objects, and
`CaptureEvidenceBundleV1`, then initializes the capture operation directly at
`ContentRegistered`.

### Executor-turn discipline

<a id="rule-PA-EXEC-0002"></a>
**[rule:PA-EXEC-0002]** A descriptor remains open for one executor turn only:

```text
acquire roles
-> create or observe
-> commit pre-effect evidence
-> immediate no-await revalidation
-> consume the affine permit for one syscall
-> immediate post-effect observation
-> commit post-effect evidence
-> release every descriptor
```

No descriptor crosses an executor turn or restart. Restart reacquires through the
durable parent, fixed derived name, marker bytes, and exact object identities.
Failure to reacquire every identity enters attention. The executor performs no
filesystem or CAS I/O while a former server store transaction is open.

All distinct admitted names must have distinct device/inode identities. Regular
files and markers require link count one. Directory link count can exceed one but
must remain stable at required observations. Mount transition, duplicate directory
identity, alias, or unsupported type is unsafe.

### Create-new publication and retirement

<a id="rule-PA-EXEC-0003"></a>
**[rule:PA-EXEC-0003]** Publication creates its stage inside the retained target
parent and keeps the stage descriptor for that turn. The durable operation token
and expected digest exist before the first namespace effect. The target parent,
stage, source, device, filesystem, owner, mode, identity, and bytes are revalidated
immediately before the no-replace syscall.

The owned namespace and marker bindings are exact:

| Operation | Durable marker binding | Durable identities |
| --- | --- | --- |
| `PublishFile` | `.decodex-pa-owner-v1` inside `.decodex-pa-stage-<operation UUID>` binds the plan, content `CasReference`, and semantic content digest | target parent, stage root, marker, and fixed `payload` file |
| `PublishTree` | `.decodex-pa-owner-v1` inside `.decodex-pa-stage-<operation UUID>` binds the plan, manifest `CasReference`, and semantic manifest digest | target parent, stage root, marker, fixed `payload` root, and manifest entry identities |
| `OwnedRootLifecycle` | `.decodex-pa-owner-v1` inside `.decodex-pa-owned-<operation UUID>` binds the plan and OwnedRoot role | controlled parent, active root, marker, and later `.decodex-pa-quarantine-<operation UUID>` identity |

Each UUID is canonical lowercase text. The marker is never inside the published
payload. A caller cannot supply an internal name.

macOS uses only `renameatx_np(..., RENAME_EXCL)`. Linux uses only
`renameat2(..., RENAME_NOREPLACE)`. The executor then verifies the target identity,
policy, and semantic bytes and synchronizes every changed object in debt order.
The target path is not receipt authority.

Owned-root retirement requires the Decodex-created operation-unique active root,
controlled same-device parent, exact marker, immutable bound manifest, published
receipt, producer stop, and cooperative maintenance. The quarantine name is unique
and derived from the operation. Retirement uses the same exact no-replace
primitive. After a possible effect, the executor verifies active-name absence,
quarantine identity, complete tree, and marker before parent synchronization.

`EEXIST` retains the source and existing target. `EXDEV` retains the source.
Unsupported semantics before effect return a typed no-effect result. A valid-call
Linux `ENOSYS` means unsupported no-replace semantics; `EINVAL` is unexpected.
Unclassified errors preserve all objects. A possible effect, failed verification,
or failed post-effect sync enters the appropriate attention state. No path uses
plain rename, overwrite, `linkat`, direct-final writing, check-then-rename,
move-back, rollback unlink, or a second publication architecture.

### Synchronization, collection, and exact-owned repair

<a id="rule-PA-EXEC-0004"></a>
**[rule:PA-EXEC-0004]** Linux durable files use `fsync`. macOS durable files use
`fsync` followed by successful `F_FULLFSYNC`. Directories use the accepted platform
directory synchronization primitive. A debt is consumed only after the complete
platform sequence and final stable-identity check.

Creation uses `mkdirat`. Capture, publication, retirement, collection, and repair
are descriptor-relative. Collection accepts only stage-collection authority or
`QuarantinedPrivateArtifact`. It removes exact planned entries in the fixed order
and creates containing-directory debt. Partial failure returns
`CollectionIncomplete` with an observable residual. It does not revoke a
publication receipt and does not promise secure erasure.

Content repair can rewrite only the same committed operation-owned, single-link,
nonaliased inode below the exact committed parent and marker. It needs verified CAS
bytes and immediate identity revalidation. It cannot adopt a namespace object,
change identity, repair a published target, or repair an uncommitted crash object.

### Singleton kernel guard

<a id="rule-PA-EXEC-0005"></a>
**[rule:PA-EXEC-0005]** Production private-artifact effects are enabled only for
the platform-default `~/.decodex`. The guard path is
`~/.decodex/server/private-artifact-v1.lock`.

C resolves the server directory descriptor-relatively; opens the lock with
`O_RDWR | O_CREAT | O_CLOEXEC | O_NOFOLLOW`, mode `0600`; requires a regular
effective-UID-owned, mode-`0600`, single-link file; acquires
`flock(fd,LOCK_EX|LOCK_NB)`; retains the descriptor for the executor lifetime; and
never truncates or unlinks it. Before every permit consumption, C proves that the
directory entry still names the guarded identity.

`EWOULDBLOCK` is `ExecutorBusy`. Unsupported locking, replacement, descriptor loss,
or identity drift is unavailable. There is no PID-file, lock-directory,
former server store-lease, or mutex fallback.

### Boot scope and producer admission

<a id="rule-PA-EXEC-0006"></a>
**[rule:PA-EXEC-0006]** macOS boot authority is exactly
`kern.bootsessionuuid`, `kern.boottime` seconds and microseconds, and the accepted
environment-receipt digest. Linux boot authority is exactly
`/proc/sys/kernel/random/boot_id`, PID and mount namespace device/inode pairs, PID
1 start ticks from `/proc/1/stat` field 22, and the environment-receipt digest.
The host-boot digest excludes Linux namespace identity; boot-scope digest includes
it. Missing, malformed, unreadable, inconsistent, or changed input authorizes
status only.

Before every covered spawn, D holds its global launch/maintenance mutex,
transactionally reserves one launch, changes the admission head to `OpenDirty`,
and requires acknowledged commit. Only then can it call `spawn`. A pre-spawn crash
is a safe dirty false positive. Successful spawn records PID, PGID, and exact
platform process-start identity. The child is a session and process-group leader.

Leader exit is current-process `waitpid` evidence. Group absence is only
`kill(-pgid,0) == ESRCH`. `EPERM`, unreadable data, PID reuse ambiguity, mismatched
start identity, or another inspection failure is not absence. Supported children
remain in their inherited group. Escape through `setsid` or a changed process group
violates the cooperative producer contract. Concurrent covered launches are
capped at 64; launch 65 fails before reservation and spawn.

### Maintenance and restart

<a id="rule-PA-EXEC-0007"></a>
**[rule:PA-EXEC-0007]** Maintenance changes admission to `Closing`, blocks new
launches, terminates and reaps tracked groups, records exact absence, and commits
`ClosedQuiescent` only with zero reserved or active launches. Acknowledged commit
mints one non-clone fence bound to server, boot scope, admission generation, epoch,
and daemon instance. Artifact turns run only while that authority remains current.
End maintenance runs after permit consumption, descriptor release, and lane empty,
then returns to `OpenClean`.

Same-boot `OpenClean` with no launch and `ClosedQuiescent` can register a new epoch.
`OpenDirty`, `Closing`, `Unavailable`, or any reserved, spawned, or ambiguous launch
is `MaintenanceUnavailable`. There is no same-boot adoption, operator clearance,
timeout clearance, PID override, or direct database repair.

A changed host-boot digest is the only V1 scope change that can clear a dirty
producer generation, and only after accepted evidence proves that old processes
cannot survive that exact change. Old operations become incompatible and do not
resume. A same-host namespace-only or environment-only change is not absence proof.

### Frozen platform claim boundary

<a id="rule-PA-EXEC-0008"></a>
**[rule:PA-EXEC-0008]** `authority/inventories.json#/platform_sources` and
`/xy1372_claim_limits` own the exact accepted environments and claim limits.
Support is limited to the recorded macOS 27.0 APFS Data device, recorded Docker
29.4.0 OrbStack overlayfs retained-container lifecycle, and exact OrbStack virtiofs
bind. The APFS image is an `EXDEV` fixture only. tmpfs proves rename behavior and
expected nonretention only.

No evidence proves host reboot, VM restart, Docker-daemon restart, kernel crash,
power loss, hostile same-UID containment, other APFS volumes, other overlayfs or
virtiofs configurations, ext4, XFS, Btrfs, remote filesystems, Windows, guard and
boot integration, PID reuse, process-group integration, or product enablement.
An unproven platform or lifecycle stops before a production experiment.
