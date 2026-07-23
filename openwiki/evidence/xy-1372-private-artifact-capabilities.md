# XY-1372 private-artifact capability evidence

Status: accepted feasibility evidence for the exact frozen matrix. This page records proof
provenance and boundaries. The normative architecture is the
[vNext authority decision](../decisions/vnext-authority.md),
[private-artifact contract](../specs/vnext-authority.md#private-artifact-authority), and
[gate manifest](../specs/vnext-gates.md#xy-1372-private-artifact-capability-and-consumption-gate).

XY-1372 proves platform feasibility. It does not implement the API, enable a production
composition root, or authorize a downstream experiment.

## Evidence identity and integrity

Capability owner: `codex://threads/019f842c-67a5-7292-9830-872955ba345b`

Fresh independent skeptic: `codex://threads/019f84a8-528f-7da3-8217-3ce5e50dede5`

Skeptic verdict: **ACCEPT, no blockers**.

The private durable run root is:

```text
/Users/x/.decodex/evidence/xy-1372/run-20260721TXXXXXX-N7E5Ei6S
```

The raw evidence package remains outside Git. The three external manifest-file identities are:

| Manifest | SHA-256 |
| --- | --- |
| `source-evidence/SOURCE-SHA256SUMS` | `7d86dd12ba005c1047a7a0569d4521e5bc06e23d80a1082908513fd93a71ac3f` |
| `closure-receipt/CLOSURE-SHA256SUMS` | `7076361ab2424f5159d873d2a5ee9f899e5bedea28b0fba13bf0ce449bfcf372` |
| `post-seal-verification/POST-SHA256SUMS` | `bcb70c1cb278e314912a1cd82fc47ad55a8ff7d2a09fffabd617f1ad956c96ea` |

The source authority names nine raw JSONL receipts and rejects the lost earlier package as
authority. The bounded raw verifier passed 2,912 checks. The closure verifier passed 425 checks.
The post-seal readback reports that the source and closure trees were unchanged, cleanup readback
succeeded, and the source repository was clean. The inventories cover regular and nonregular
entries and use explicit exclusions for the self-referential manifest and inventory files.

The independent skeptic also applied bounded negative checks. Missing receipts, an extra or wrong
case, wrong return code or errno, and a weak empty failure array each failed closed. Temporary
review copies were removed.

## Observed platform matrix

| Environment | Recorded identity | Accepted scope |
| --- | --- | --- |
| macOS | macOS 27.0 build 26A5388g, Darwin 27 arm64, APFS Data `/System/Volumes/Data`, `/dev/disk3s1`, device `16777233` | `renameatx_np(RENAME_EXCL)`, file `fsync`, successful `F_FULLFSYNC`, changed-parent synchronization, process exit, and separate-process readback. |
| Linux overlayfs | Docker 29.4.0 under OrbStack kernel `7.0.11-orbstack-00360-gc9bc4d96ac70`, aarch64 overlayfs device `1048589` | `renameat2(RENAME_NOREPLACE)`, file and changed-parent `fsync`, and readback during the second start of the same retained container. Scope is limited to the recorded image, configuration, and lifecycle. |
| Linux virtiofs | The exact OrbStack bind backed by the recorded APFS path, statfs type `65735546`, device `35` | `renameat2(RENAME_NOREPLACE)`, guest file and changed-parent `fsync`, and second-start readback. Guest success does not prove host-storage persistence. |
| APFS image | device `16777240` | Distinct cross-device `EXDEV` fixture only. It is not a supported artifact store. |
| Linux tmpfs | device `1048605` | Excluded from the durable matrix. The first start establishes rename behavior. Absence on the second start establishes expected non-retention. |

## Direct outcomes

The final APFS, overlayfs, and virtiofs receipts cover the required no-replace cases. The tmpfs
receipt covers rename behavior only.

| Case | Observed result |
| --- | --- |
| Absent destination | Success. The retained source descriptor matched the target identity and bytes. |
| Existing regular file, directory, symlink, FIFO, or socket | `EEXIST`; source and destination remained unchanged. |
| Source replacement immediately before rename | The rename succeeded and moved the replacement. The retained original remained preserved. |
| Destination collision immediately before rename | `EEXIST`; both identities and contents remained unchanged. |
| Invalid flag | `EINVAL`; no namespace effect. This is fault evidence, not permission to classify every production `EINVAL` as unsupported. |
| Same-device root retirement | Success. The retained owned-root descriptor matched quarantine. |
| Root replacement before retirement | The replacement moved into quarantine. The original remained preserved. Final verification must detect the mismatch. |
| Cross-device rename | `EXDEV`; source remained and destination was absent. |

The substitution cases show that retained descriptors and immediate revalidation cannot eliminate a
last-moment namespace substitution. The platform result therefore supports the normative need for
post-effect target or quarantine verification, typed attention states, and preservation. It does
not support rollback deletion.

## Durability boundary

- APFS evidence is successful file `fsync`, `F_FULLFSYNC`, changed-parent synchronization,
  process close, and separate-process readback.
- Overlayfs evidence is successful file and changed-parent `fsync` plus readback during a second
  start of the same retained container.
- Virtiofs evidence is successful guest file and changed-parent `fsync` plus second-start readback.
  It is not host-storage persistence evidence.
- tmpfs is expected non-retention evidence and is excluded from durable support.

The run did not test Docker-daemon restart, VM restart, host reboot, kernel-crash recovery, or
power-loss persistence. It does not prove hostile same-UID writer containment, another kernel,
runtime, mount or configuration, another APFS volume, ext4, XFS, Btrfs, a remote filesystem, or
broader overlayfs or virtiofs semantics.

## Decision and residual gates

The accepted evidence supports one Unix semantic state machine with private macOS and Linux
no-replace and synchronization shims. It supports the exact preservation, typed-stop, final-
verification, same-device retirement, and no-rollback requirements in the normative contract.

Every unsupported or unobserved environment or semantic remains a future enablement gate. The
accepted evidence does not approve the exact XY-1373 repository candidate. A fresh visible reviewer
must review that candidate before XY-1371 implementation resumes. XY-1371 implementation and its
own independent source review remain separate work.
