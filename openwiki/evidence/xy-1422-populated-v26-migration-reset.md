# XY-1422 Populated V26 Migration Reset

Status: rejected-candidate provenance and architecture-reset rationale. This note is
not implementation or acceptance evidence.

## Rejected boundary

The exact rejected candidate-3 index tree is
`ef66d028ff447c4507b6d37de0835166a6fd7f26`.

Source inspection found this deterministic transition:

1. V26 can contain an Account UUID, label, and revision without an administrative
   enabled field or credential projection.
2. V27 adds `enabled` with the fail-closed default `false`.
3. Legacy normalization maps an absent or false `disabled` flag to desired
   `enabled=true`.
4. Candidate 3 passed this desired `true` to the existing-row credential import.
5. The V27 existing-row operation requires the requested revision, label, and enabled
   value to equal the current PostgreSQL tuple.
6. Current `false` and requested `true` did not match. PostgreSQL returned
   `stale_account` before it inserted an operation or changed Keychain or PostgreSQL
   credential state.
7. Same-digest restart repeated the same refusal.

The same overloaded enabled value existed in all three rejected XY-1422 candidates.
Candidate 3 therefore completed material rejection cycle 3.

## First authority checkpoint rejection

The first authority checkpoint produced exact tree
`fa633f8761bdded4ac376ea9db01a6d9e3c39e25`. Independent review rejected that
checkpoint for two material gaps:

1. It assigned continuous namespace-lock ownership to a generic migration process.
   The actual cutover spans installer-owned configuration and retirement effects plus
   separate migration, finalizer, and completed-verifier children. The checkpoint did
   not define one executable descriptor lineage across those boundaries.
2. It classified current account state before it read a persisted manifest operation.
   After an absent initialization had prepared its PostgreSQL row, restart could
   reclassify it as an existing hydration. Generic startup reconciliation could also
   cancel the prepared manifest-bound import before exact Keychain creation.

Tree `fa633f8761bdded4ac376ea9db01a6d9e3c39e25` is rejected authority evidence. It is
not accepted provenance and does not authorize candidate 4.

## Second authority checkpoint rejection

The second authority checkpoint produced exact tree
`7e4b9296040acee90a23533b185aa0b21539b713`. Independent review accepted its
operation-first replay, phase and revision model, and current-state hydration. It
rejected only the physical lock proof:

1. The checkpoint required a child to prove that its FD shared the installer's open
   file description and held the flock. The child cannot distinguish a `dup`-derived
   FD from an independently opened same-identity FD or prove another process's lock
   ownership from its local metadata checks.
2. The checkpoint said that process death releases all duplicates. The flock instead
   survives until the final descriptor for the locked open file description closes.

Tree `7e4b9296040acee90a23533b185aa0b21539b713` is rejected authority evidence. It is
not accepted provenance and does not authorize candidate 4.

## Candidate 4 validation reset

Candidate 4 retained approved index tree
`3c96dec51569baa24150287b669c33703e47ec92` and froze worktree tree
`adb9cbb784d2a8afb3dc0c25648571ff43f76511`.

Two canonical invocations stopped in shared setup:

1. Run 1 returned `legacy account source parent is unsafe`.
2. Run 2 used a synthetic passwd record with `pw_dir` but no `pw_name`. PostgreSQL
   setup refused that incomplete record.

No semantic migration case ran in either invocation. Neither result tests
`ExistingHydrate`, `AbsentInitialize`, operation-first replay, a revision sequence,
lock continuity, receipt ordering, or final destination verification. No Candidate 4
test passed.

Both independent read-only reviews found the next deterministic refusal. The installer
rejects an effective-UID-owned source ancestor when `mode & 077 != 0`. The Rust child
applies the same rule to its real filesystem path. The actual host has these modes:

```text
/Users/x                 0750
/Users/x/.codex          0755
/Users/x/.codex/decodex  0700
legacy source files      0600
```

The first two directories are not group or other writable. The direct
secret-bearing parent and source files remain private. The old ancestor rule therefore
blocks the real host without identifying another UID or group that can mutate the
source.

The accepted correction keeps the source below the real `pwd.getpwuid(euid).pw_dir`,
where `euid` is the process effective UID. It keeps no-follow traversal, effective-UID
ownership, regular type, one link, and exact mode 0600 for every source and generated
credential file. It keeps no-follow traversal, effective-UID ownership, directory
type, and exact mode 0700 for every direct source or secret-bearing parent and the
generated credential directory. Above that private boundary, each ancestor must be a
no-follow directory owned by the effective UID or root, with `mode & 022 == 0`. A
foreign non-root owner is rejected. Harmless read and execute bits are permitted.

The installer and Rust child must enforce the same predicate. They must not change the
real home mode, trust ambient `HOME` or a synthetic passwd record, or create a fallback.
These POSIX checks do not prove that arbitrary ACL entries are absent. ACL semantics
remain an explicit residual and non-regression boundary. This amendment makes no claim
that modes 0700 and 0600 establish every ACL property.

A one-line passwd patch is rejected. The canonical harness repair must also:

- schedule a bounded stage graph in which every case reports `passed`, `failed`, or a
  typed `blocked` result with its dependency;
- keep independent FD, no-follow path, namespace-lock, and live-daemon refusal cases
  runnable when identity, path, tool, or migration setup blocks only other cases;
- use one coherent PostgreSQL 18 toolchain and a collision-safe endpoint;
- use fresh, run-unique, gate-owned Keychain Account UUIDs, prove each selected
  identity absent, and refuse before mutation when any item exists;
- exercise the production protected-credential API and exact metadata contract without
  putting credential bytes in process arguments;
- record each Keychain item that the gate creates and delete only those items during
  unconditional cleanup, with cleanup failure reported as gate failure;
- use no value-only or long-lived Keychain backup or rollback; conflict and drift cases
  use separate gate-owned identities or exact metadata-preserving operations and never
  delete and recreate a positive item;
- exercise installer-owned operator orchestration, the real migration, prepared
  verifier, finalizer, completed verifier, final launch decision, live-daemon
  exclusion, and attempted selection, Reset Card, and spawn admission at required
  intermediate states without duplicating product authority; and
- place every process, worker, lock, temporary path, PostgreSQL instance, and Keychain
  item under bounded unconditional cleanup with exact process identity.

After that repair is frozen, one independent exact-tree review must approve it before
the Manager authorizes exactly one replacement
`cargo make test-vnext-account-migration-transition` invocation. A focused copy and an
automatic retry are not authorized.

This is only a path-policy and validation-harness amendment. It does not reopen
`ExistingHydrate`, `AbsentInitialize`, operation-first replay, the exact revision
sequences, continuous installer lock lineage, child capability limits, receipt
ordering, or the no-new-ledger and no-new-public-API decisions.

## Retained reset decision

The selected design is current-state hydration followed by desired administration.
For an existing account, credential import uses the complete current PostgreSQL tuple
`(revision, display_label, enabled)`. After the exact credential binding is durable,
the existing administration operation applies the manifest label and enabled value.
An absent account can use manifest administration during initialization.

The migration must also freeze the exact credential target before its first
destination or credential mutation. The macOS installer must retain one exact
`decodex.lock` open file description across every child and installer effect through
the final receipt and launch decision. Child checks prove only descriptor and
filesystem identity. The external contention gate proves continuous lock ownership.
Persisted manifest operations must replay by exact descriptor and phase before any
current-state classification. Completed state must match the exact destination
receipt.

This reset keeps the existing Account Service, account-operation journal, V27
fail-closed default, administration operation, routing operation, and receipt. It adds
no public protocol, SQL operation kind, migration ledger, fallback, or fourth
authority owner.

## Rejected alternatives

- Administration first was rejected because a crash can expose
  `enabled=true` before exact credentials are bound.
- Relaxed existing-row SQL checks were rejected because the operation descriptor
  would stop representing an exact current-state precondition.
- A V27 backfill change was rejected because it weakens fail-closed migration.
- A migration-specific durable operation was rejected because the accepted offline
  contract does not require another state machine.

The selected design is proportionate only while migration has continuous offline
namespace-lock ownership. Online migration requires a new proportionality review.

## Consumed transition gate and daemon-wrapper reset

The canonical transition gate ran once from signed commit
`7a703c0f0f8492601f93f2ebaa959ce03e0f1cf1`, tree
`5b9a7b864294150a5e03d82cc8cc7bba0ee18368`, with run identifier
`fcb7d020c85db4bf`. It returned 18 passed stages, 4 failed stages, and 60 stages
blocked by those failures. The private 20,548-byte run log has SHA-256
`9b84c9a5d2620092ad51502097f7d8189792827e45a151ed2d2aafec1c98523b`.
The gate was not retried.

The three descriptor and lock failures form one causal chain. The installer borrowed
a duplicate lock descriptor and tried to make it inheritable before the gate's cleanup
scope. macOS returned `EPERM`. The original installer guard then remained open in the
gate process. The later independent contention and path-drift stages failed when they
tried to acquire that leaked lock, before either core assertion ran. The repair keeps
the parent duplicate non-inheritable, transfers it only through the exact
`Popen(pass_fds=...)` call, and starts conditional cleanup after the first acquired
resource in both the production child runner and gate.

The protected-store stage failed only as `protected-store prove_conflict failed`.
The harness discarded the exact Security.framework status, so
`errSecMissingEntitlement (-34018)` is a high-confidence explanation, not an observed
fact. Independent signature readback established the material product defect: the raw
debug daemon, the raw daemon helper in the current outer app, the installed
`~/.local/bin/decodexd`, and the outer app had no usable Data Protection Keychain
entitlement and embedded-profile context. Apple requires an app-like wrapper for a
daemon that claims the restricted application identifier and Keychain group. The
production repair is therefore required even if a future typed gate result identifies
another immediate Security.framework status.

Cleanup for the consumed run passed. Final absence was verified for all five run-owned
Keychain identities, the run recorded no owned item, global cleanup recorded no
deletion, the fixture and owned processes were removed, and the live default
credential source was not accessed. The failed self-contained conflict probe did not
retain enough phase evidence to prove whether it made and removed a transient item.
No manual Keychain action followed.

The replacement source must use one fixed app-like `decodexd` wrapper, bind its full
non-secret identity through the existing migration manifest and receipt, use its exact
application identifier as the only explicit Keychain access group, and provide closed
non-secret protected-store phase/category evidence. A development-profile wrapper is
only local dogfood evidence. Real `gui/<uid>` LaunchAgent protected-store acceptance
remains required before `MacDogfoodReady`. File-based Keychain, weaker accessibility,
plaintext or environment fallback, gate-only signing, a shared group, arbitrary CRUD,
generic packaging, identity/profile fallback, a new ledger, and notarization expansion
remain rejected.

Candidate 4 remains unauthorized for a replacement gate run until this authority and
the bounded source repair are frozen and receive independent exact-tree approval. The
canonical implementation gate is
`cargo make test-vnext-account-migration-transition`.

## Evidence limit

The original reset used exact-tree source inspection and an independent skeptic review.
This amendment also uses the frozen Candidate 4 tree, the existing failed canonical run
records including the consumed log digest above, and independent root-cause and skeptic
verdicts. This documentation checkpoint ran no formatter, compile, test, gate,
PostgreSQL, Keychain, installer, or runtime command.
