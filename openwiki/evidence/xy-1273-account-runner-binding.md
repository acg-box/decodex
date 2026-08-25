---
type: "Reference"
title: "XY-1273 account and runner boundary evidence"
openwiki_generated: true
---

# XY-1273 account and runner boundary evidence

Status: implementation candidate, intentionally unstaged and uncommitted.

## Implemented authority

- `decodex-core` owns canonical lowercase UUID account identities and the closed non-secret account
  observations `unavailable`, `unknown`, `available`, `depleted`, `auth_failed`, `plugin_unready`,
  and `disabled`. These types expose no selection operation.
- former server store remains the only product-state authority. Forward-only V4 adds `unavailable` to the
  existing enum and adds no table, column, credential reference, selector, or callable routing
  function. Account reads require an exact account ID. Existing recursive Rust and former server store
  predicates protect account metadata, activity, outbox, and command receipts from normalized,
  case-folded, nested, serialized, assignment-shaped, authorization-shaped, and known token-shaped
  credential material. Account mutation/readback `Debug` output omits all caller-controlled labels
  and metadata.
- `CredentialVault` is a host port whose material can move only into a single-use process projection.
  The default implementation is unavailable. The production ambient-account probe is absent. The
  projection is serialized from borrowed material directly into fixed-size wipe-on-drop blocks;
  no growing ordinary allocation owns outbound credential bytes. Growth changes only a pointer
  vector, while serialization error, oversize rejection, write failure, and success wipe all blocks.
  Inbound read
  storage and frame storage use fixed zeroizing blocks, queued values use the same owner, and the
  exact-sized parse copy is zeroizing; overflow, disconnect, partial EOF, read error, parse error,
  queued teardown, and ordinary success therefore wipe every raw transport allocation. Each
  completed typed inbound string field is independently zeroizing even when a later field fails.
  This does not claim control over opaque parser scratch or a string that itself fails to decode.
  Unexpected projection-response fields fail closed,
  and errors and debug surfaces are closed enums or redacted structures.
- Every runnable child receives an immutable exact account ID, shared normal Codex home, exact
  protected executable snapshot/build, and OS process ID. Before snapshot authority, runtime accepts
  only native 64-bit thin or universal Mach-O images on macOS and native ELF images on Linux; shell,
  Python, Node, `env`, and other shebang/interpreter files fail before vault projection. The platform
  loader validates the accepted image and its architecture/interpreter; dynamic-loader state remains
  inside the host OS trust boundary. The opened source descriptor is copied under a fixed bound and
  fsynced. macOS uses a private mode-0500 `UF_IMMUTABLE` file. Linux requires an executable mode-0500
  memfd with irreversible write/grow/shrink/exec/seal seals and verifies that its procfs descriptor
  path resolves to the same open object. The protected object's digest defines BuildId, and every
  preflight and final exec uses that exact object; original-path replacement fails closed before the
  boundary and cannot change the image receiving credentials after it. Projection precedes the first account read; every subsequent
  authority read re-attests the same exact zeroizing in-memory identity. A requested/observed mismatch kills the
  process group, and uncertain cleanup transfers ownership to the bounded quarantine and returns no
  runner.
- The child argv is fixed. Parent environment is cleared and rebuilt with only `HOME` and a fixed
  `PATH`; no `CODEX_HOME` is set. The pre-exec boundary marks every non-stdio descriptor
  close-on-exec, stderr is discarded, protocol frames are bounded, and caller-side process-group
  shutdown and quarantine transfer are bounded. Background quarantine may retain capacity
  indefinitely when death cannot be confirmed. A manual fixture proves shared authentication,
  account-pool, and plugin-state files are
  byte-identical before and after bound execution.
- The dormant runtime composition observes the exact manually selected former server store account
  ID/revision as `available` before mechanics, explicitly releases the result row and pool checkout,
  and repeats the exact observation only after process cleanup or quarantine transfer. It returns
  only a non-live post-cleanup observation. A concurrent disabling mutation progresses while a
  synchronous vault is blocked and makes the final observation fail closed. No database transaction,
  row lock, client checkout, or callback spans vault/process work. The blocked vault can retain its
  local task and mechanical capacity indefinitely. Runtime is the sole workspace sibling-adapter
  composition owner. Its private
  `RunnerCapacity` is fixed at 64 and exposes no constructor or reservation. Codex exposes no child
  mechanics. The private concrete permit reserves a cleanup slot before spawn, enters each preflight
  and final `ProcessGroupOwner`, returns between sequential groups only after confirmed absence,
  child reaping, and pump join, and moves into its fixed slot on uncertainty. Installation is
  lock-free and bounded; round-robin cleanup prevents a stuck slot from starving another. The
  capacity-lifecycle janitor must start before the capacity owner is constructed, so repeated start failure
  admits neither a permit nor a process and cleanup never depends on a later launch. Per-iteration
  unwind plus the in-flight owner recover panic/poison paths autonomously. A weak registry reuses the
  one live daemon authority without retaining it forever; a finite coordinator stops and joins the
  janitor after the last capacity, permit, and cleanup job is released. A 65th attempt is rejected
  before a process exists.
  A fresh process cannot resurrect a binding, and another observation requires fresh exact
  former server store pre- and post-observations.
  Uncatchable daemon/host death can orphan an OS group; no cross-crash cleanup guarantee is claimed.
  XY-1304's live
  dispatch gate remains failed.

## Reviewer repair disposition

The first independent review returned two valid P2 findings. Both were reproduced before repair.

1. Caller-asserted readiness and separable capacity were removed. That repair initially used a
   closure-scoped former server store launch capability; the fourth review correctly rejected its unbounded
   lock duration, and the later repair described below replaced it with exact pre/post observations
   that release the pool before mechanics. The runner permit remains private. Process spawn,
   post-spawn pipe failures, success, protocol error, timeout, mismatch, explicit shutdown, Drop,
   and cleanup quarantine now share process-group ownership of that permit.
2. Ordinary `BufReader<Vec<u8>>`, `Receiver<Vec<u8>>`, and growing line vectors were removed.
   Secret-bearing buffers are fixed 8 KiB zeroizing allocations collected in pointer-only chunk
   vectors, so their contents never pass through a reallocating secret-bearing allocation. Queue
   elements are `InboundFrame`; all `TrySendError`, receiver teardown, unread, partial, and error
   branches drop the same zeroizing owner. Flattening allocates exact capacity once under
   `Zeroizing<Vec<u8>>`.

The bounded scout found that a public readiness trait, caller metadata, or separately returned
reservation remained forgeable. The fresh skeptic additionally identified the stale-revision window
in a passive read-then-launch grant. The initial row-lock solution was later rejected because a
caller-controlled synchronous vault could retain the lock indefinitely. The final composition keeps
the launcher/result private and requires exact pre- and post-mechanics former server store observations; it
does not claim readiness stayed true during mechanics and exposes no live runner.

## Second reviewer repair disposition

The next independent review returned three valid P2 findings.

The delegated read-only scout was stopped at the bounded deadline without a usable report and made
no worktree change. The implementation owner completed the scout inline against the checked-in
process/runtime owners and the locally resolved `zeroize` 1.9 Serde implementation. A separate
fresh read-only skeptic then challenged the concrete design and required successful child wait
before permit recovery plus explicit limitation of the architecture claim to repository production
call sites.

1. Every executable/version/schema preflight now receives the same linear capacity permit as the
   final child. A successful preflight returns it only after confirmed process-group absence and a
   successful child reaping. Any post-spawn uncertainty transfers the permit into cleanup ownership and ends
   the attempt. Deterministic first- and second-preflight fixtures delay reaping, prove capacity one
   remains active, and prove repeated reservations cannot create another group.
2. Each completed inbound DTO string now deserializes directly into the existing `zeroize` crate's
   Serde-enabled `Zeroizing<String>` owner behind a redacted wrapper. Late missing/wrong fields,
   escaped strings, nested accounts, nested threads, later vector elements/cursors, and the real
   queued typed-parser path count every completed wrapper drop independently of raw-block wipes.
   Opaque Serde scratch and a string allocation that itself fails are not claimed as zeroized.
3. The rejected Codex-to-former server store sibling dependency was removed. Runtime now owns the explicit
   manual launch composition and the exact dependency graph is restored. The later third repair
   replaces the source-spelling call-site guard and public capacity-shaped Codex primitive described
   here with private runtime authority plus manifest and compile-time evidence.

## Third reviewer repair disposition

The third independent review returned two valid P2 findings. A bounded read-only scout reproduced
the synchronous infinite fallback, serial-worker starvation, row-lock consequence, public forgeable
capacity seam, and alias-sensitive architecture test. A fresh bounded read-only skeptic accepted a
fair quarantine and runtime-private authority only if job ownership preceded startup, cleanup rotated
one bounded step per job, worker/panic/poison paths retained guards, resource bounds were hard, and
the Rust visibility claim stayed repository-scoped. The subagent interface did not expose an
independently attestable model selector; both passes were prompt-constrained to the requested profile
and made no repository change.

1. `transfer_to_reaper` no longer invokes unbounded cleanup synchronously. Capacity reservation also
   reserves one of 64 fixed cleanup slots before spawn. Atomic FREE/RESERVED/READY/WORKING states
   make transfer lock-free and keep every job discoverable; the 65th permit is rejected before a
   process exists. The lifecycle-owned janitor performs one bounded cleanup step per slot in round-robin
   order. An in-flight RAII owner restores the same slot on panic. Completed jobs release permits
   only after child reaping, group absence, and stdout-pump join. Worker-start failure resets to
   retryable idle, poisoned wake state is recovered, and later reservation/maintenance retries.
2. `RunnerCapacity`, its account/revision permit, reservation, errors, process supervisor, vault,
   and sensitive wire DTOs moved out of Codex into the private runtime account-launch owner. The
   permit is constructed only after an exact former server store
   revision/readiness observation and matching `ReadOnlyProbe::account_id`; runtime validates
   the returned mechanical observation and exact former server store predicate again and is the only constructor of
   `ManualAccountLaunchResult`. Codex exposes no child-launch API. Cargo metadata proves runtime is the only workspace package with a
   normal dependency on Codex and also owns former server store composition. Compile-fail doctests prove the
   former Codex command/probe/vault/capacity surfaces and runtime capacity constructor are
   unavailable. This does not claim that future source changes or wrappers are impossible.

## Fourth reviewer repair disposition

The fourth independent review returned three valid P2 findings. The bounded scout and skeptic agreed
that database authority and process mechanics must be temporally separated, worker liveness must not
depend on a contended queue mutex, and metadata evidence must be limited to dependency reachability.

1. The arbitrary `FnOnce` under `FOR UPDATE` was removed. `account_is_ready_at_revision` performs one
   exact ID/revision/`available` predicate, extracts the boolean, and explicitly drops the result row
   and pooled client before returning. Runtime performs this check before capacity reservation and
   repeats it only after mechanics have returned or transferred uncertain cleanup ownership. A
   pool-size-one former server store 18 fixture runs the complete private composition on a dedicated blocking
   executor thread, blocks the synchronous vault, and proves a concurrent disabling mutation
   completes. Releasing the vault produces final revision rejection, no live result, and capacity
   recovery. No arbitrary one-second database query timeout was invented; ordinary configured
   former server store connectivity bounds still apply. The one-second value in the fixture bounds only the
   mutation-progress assertion.
2. Quarantine startup now uses atomic idle/starting/running transitions independent of the queue
   mutex. Start failure restores idle without taking that mutex. A worker-liveness guard restores idle
   on unwind, and an in-flight-job guard reinstalls the complete child/process-group/permit owner
   before the worker restarts. Queue poison is recovered, condition-variable waits use the queue
   predicate, and bounded admission retains ownership fail closed. Production-transition tests cover
   contended start reset, panic after pop, poison plus waiting-worker wakeup, independent-job progress,
   permit retention, and eventual recovery.
3. The runtime launcher, request, result, capacity, and permit are private in a non-reexported dormant
   module. Workspace metadata tests enumerate every library/binary target, prove only runtime directly
   owns both sibling adapters, prove `decodexd` reaches them only through runtime, and reject synthetic
   fixture features on normal production edges. Compile-fail doctests prove the launcher and capacity
   names are absent from public crate APIs. This evidence does not claim provenance, alias detection,
   absence of future wrappers, or universal downstream friend visibility. The former server store 18 fixture
   exercises the complete supported private path for non-ready, stale, success, revision race,
   capacity exhaustion, and account mismatch.

## Fifth reviewer repair disposition

The fifth review identified four additional P2 gaps. A bounded read-only scout and a separate
read-only skeptic agreed that Rust friend visibility could not repair the public Codex seam and that
the process mechanics had to move behind runtime authority. Their interface did not attest the
requested model profile; neither pass edited or executed the repository.

1. App-server command, binding, vault, probe, supervision, wire DTOs, and fixture constructors now
   live under private `decodex-runtime::account_launch` modules. `decodex-codex` retains only pure
   capability/schema/build evidence and has no process module or launch reexport. The process path
   accepts the concrete private `RunnerPermit`, never a caller-selected generic guard.
2. Each permit reserves a fixed cleanup slot before any spawn. Transfer writes directly into that
   exclusively reserved slot without a contended registry lock or full queue. Atomic slot states,
   round-robin scanning, in-flight RAII, worker-liveness recovery, timed wake rechecks, and hard
   64-slot capacity keep jobs and permits discoverable and recoverable across contention, start
   failure, panic, poison, and uncertainty.
3. Before either header or typed decoding, a zero-allocation lexical gate rejects every escaped JSON
   string. Locked serde_json 1.0.150 therefore takes its borrowed slice fast path for inbound strings
   and cannot copy credential-bearing string bytes into its ordinary escape scratch. Escaped success,
   error, Unicode, newline, quote, backslash, nested, late-failure, queue, and teardown cases fail
   closed while raw blocks and completed typed strings remain zeroizing. This does not claim opaque
   structural/number scratch is zeroizing.
4. The stdout pump is nonblocking and owned by `ProcessGroupOwner`/`ReapJob`; cancellation, finished
   polling, join, partial-frame wiping, child reaping, and group absence are all required before
   successful cleanup. A descendant retaining stdout cannot make join unbounded, and uncertain pump
   cleanup retains the same permit in its fixed slot.
5. The prior unsalted low-entropy account digest was removed. Exact account identity exists only in
   zeroizing, redacted process memory for comparison and is absent from the returned observation.
   No authority-defined observation-age TTL exists, so evidence proves stale-revision rejection, not
   age freshness. A live shared-home vault fixture remains blocked on operator-supplied disposable
   credentials, and stronger descriptor-backed descendant identity than macOS process groups remains
   an explicit platform limitation.

## Fifth-candidate review and sixth-candidate repair disposition

The fresh review of the fifth candidate returned three valid P1 findings. The sixth candidate
repaired them. The owner performed a bounded source
study of locked serde_json 1.0.150 serialization and Darwin execution primitives, followed by a
fresh skeptic pass over allocator, janitor, and check-to-exec failure paths. `serde_json::to_writer`
writes escaped fragments directly to an `io::Write`; Darwin provides neither a usable `fexecve` nor
executable `/dev/fd` path for this contract. The accepted design therefore uses zeroizing fixed
blocks, prerequisite persistent cleanup ownership, and a protected private executable snapshot.

1. `serde_json::to_vec` was removed from the credential projection path. Serialization now targets
   fixed 8 KiB boxed blocks whose full allocations wipe in `Drop`; only a credential-free pointer
   vector grows. The one-mebibyte bound is enforced before copying each fragment. Tests count wipes
   after multi-block growth, a late custom `Serialize` error, oversize rejection, synthetic transport
   failure, and the real `ChatgptAuthParams` projection path. This controls all outbound allocations
   owned by the runtime serializer; it does not claim allocator or kernel transport buffers.
2. Capacity construction now succeeds only after its cleanup janitor thread has started.
   Startup failure returns `CleanupUnavailable` before any capacity, permit, credential projection,
   or process can exist. The janitor owns autonomous timed scans for the capacity lifetime, catches each
   iteration independently, and relies on the existing in-flight RAII owner to restore a popped job
   before unwind. Repeated start failure and panic recovery tests require no later launch or external
   maintenance signal. Uncatchable daemon/host termination remains outside this in-memory guarantee.
3. The source executable is opened and copied into a private bounded snapshot before authority is
   issued. After fsync and closure of all write handles, macOS mode 0500 plus `UF_IMMUTABLE` prevents
   replacement between the final check and exec. Version, schema, and final app-server all execute
   that exact snapshot, whose digest defines the build identity. Deterministic hooks replace the
   original path at the first preflight and final-spawn boundaries and prove the unverified image
   never runs; a separate hook proves direct snapshot write and rename attempts fail while the same
   snapshot still executes. A compromised daemon process or same-uid principal able to subvert the
   daemon itself is not claimed as an isolation boundary.

## Sixth-candidate review and seventh-candidate repair disposition

The fresh review of the sixth candidate (`019f645f-d8d4-7462-899e-104288a6a5f1`) returned one
valid native-image authority finding and one valid cleanup-lifecycle finding. The seventh candidate
repaired them. The owner checked the current Darwin Mach-O loader headers and formats,
then challenged both fixes against interpreter indirection, malformed images, worker self-join,
queued-job reference cycles, startup failure, and bounded `Drop` behavior.

1. Executable admission now reads the already-open descriptor and accepts only current native
   64-bit thin Mach-O magic or native 32/64-bit universal-container magic on macOS. Shebang scripts,
   including `/usr/bin/env` indirection, fail during command construction, before a probe or vault
   projection exists. The accepted descriptor is copied, protected, hashed, and used for every
   preflight and final launch as before. The gate excludes interpreter activation; it does not replace
   the macOS loader's full image/slice validation or claim a hash closure over platform dynamic
   libraries. Direct shell/Python/Node negatives, thin/universal classifier cases, and the native host
   Python image cover the boundary.
2. The janitor no longer owns a strong reference to the quarantine facade. A worker owns only the
   fixed slot state, while a finite coordinator owns and joins its handle. The daemon registry stores
   a weak reference, so exactly one live authority is reused without making test or graceful-shutdown
   resources immortal. The last facade signals shutdown; ordinary drops wait only for a bounded join
   notice, while a last release on the worker delegates the join to the coordinator and cannot
   self-deadlock. Jobs retain the facade and permit until cleanup, so shutdown cannot precede queued
   process/pump/credential/permit cleanup. Tests cover worker-start and coordinator-start failure,
   repeated construction/drop, queued cleanup before teardown, worker panic, poison recovery, and
   autonomous progress.

## Seventh-candidate review and eighth-candidate repair disposition

The fresh independent visible review of the seventh candidate
(`019f6487-a888-7ba3-a3f9-882cbf56692c`) confirmed the macOS native-image and
janitor-lifecycle repairs, then found one Linux exact-image gap and the review-provenance error
corrected immediately above. This eighth candidate addresses those findings; it has not yet received
the Manager's next independent review.

1. Account-bound launch now has an explicit per-platform protected-object contract. The source file
   is opened, native-format checked, copied under the executable-size bound, protected, and hashed
   before any vault call. macOS retains the private `UF_IMMUTABLE` file. Linux requires a kernel
   supporting executable memfds and executable sealing, applies and reads back irreversible
   write/grow/shrink/exec/seal seals, and verifies procfs resolves the inherited close-on-exec
   descriptor to that exact memfd. BuildId, both preflights, and final app-server execution all use
   this one protected object. The Linux kernel resolves the native ELF from the open descriptor
   before close-on-exec closes it; scripts are rejected, so the documented interpreter-script
   `ENOENT` caveat does not apply. Missing memfd, seal, procfs, loader, or architecture support fails
   closed before projection or at spawn.
2. Descriptor ownership is linear: the command's `Arc<ExecutableSnapshot>` owns the only runtime
   memfd; each fork temporarily inherits that descriptor; successful exec closes the inherited copy;
   spawn failure leaves parent ownership intact; and command drop closes the final parent copy.
   The source path is never the Linux exec target. A deterministic Linux hook runs after the last
   verification but before `spawn`, proves writes through a duplicate descriptor fail with `EPERM`,
   replaces the source path with `/bin/false`, and still observes the admitted fixture complete. If
   the replacement were executed, the probe would fail. Cross-compilation plus the same test in the
   exact Linux environment recorded below exercises the platform branch.
3. Failure handling is deliberately conservative: unsupported native format, copy overflow, sync,
   chmod, memfd creation, seal application/readback, procfs identity, digest, loader, or spawn errors
   return only closed executable/preflight errors. No child or credential exists for admission
   failures; preflight/final spawn failures retain or return the lifetime permit according to the
   existing bounded cleanup paths. The digest does not claim the ELF interpreter or shared-library
   closure; those remain host-kernel/loader authority, as on macOS.

The Linux design follows the kernel's executable-memfd contract and Linux man-pages rather than a
path recheck. The kernel documents `MFD_EXEC` as explicit executable creation and its namespaced
no-exec policy; `memfd_create(2)` documents fork inheritance and `MFD_CLOEXEC`; the seal API
documents inode-wide irreversible seals and `EPERM` for prohibited writes; and exec documentation
defines descriptor-selected images, close-on-exec behavior, and loader/architecture failures. The
alternative was to disable account-bound launch on every Linux host. That broader denial is not
needed because the admitted bytes, digest, and executed open object now coincide; however, hosts
without all checked primitives still take that fail-closed alternative locally.

The same audit corrected authority drift: process supervision is private runtime composition;
`decodex-codex` retains typed schema/capability/event contracts and no child-launch surface.

## Bounded skeptic pass

| Counterexample | Disposition |
| --- | --- |
| An ambient shared-home account could differ from the requested account. | Production execution requires the vault projection and exact readback receipt; the old ambient probe is test-only. |
| A vault could switch credentials twice under one child. | The projection sink is single-use and the second projection returns a closed error. |
| A vault could claim one account while the child reports another. | The expected receipt is installed before `account/read`; mismatch terminates the process group before exposure. |
| A crash/restart could recover a different account from persisted pool state. | Capacity is deliberately non-persistent and restart reconstructs no assignment; bound restart is not public and another observation requires fresh exact pre/post former server store checks. Uncatchable daemon/host death can orphan an OS group, so cross-crash cleanup is not claimed. |
| Unknown, stale, authentication-failed, plugin-unready, disabled, or unavailable metadata could be treated as ready. | The exact former server store revision/state predicate rejects every value except current `available` both before and after mechanics; callers never supply `AccountState` to capacity. |
| A permit could be dropped while a preflight, final process, or descendant remains alive. | Every sequential process group owns the same permit immediately after spawn. It returns only after confirmed group absence and child reaping; uncertainty moves it into the quarantine or intentional fail-closed retention. |
| One uncertain group or failed cleanup worker could block former server store or every later cleanup. | No database checkout spans cleanup. Caller and Drop paths perform only bounded shutdown/transfer. Jobs rotate one nonblocking cleanup step per round; atomic start/panic recovery retains jobs for retry, and admission failure retains capacity without blocking. |
| A repository caller could forge account/revision values through the Codex capacity API. | Codex exports no product capacity or revision-shaped seam. Runtime privately constructs the permit after exact precheck; its nonreexported result requires exact final recheck. Manifest reachability and compile-fail tests enforce the stated current repository/API boundary only. |
| Queue overflow, disconnect, partial input, or teardown could free raw child bytes ordinarily. | Reader blocks, chunked frames, queued values, and the contiguous parse copy are all zeroizing owners; instrumented branch tests count wipes after each adversarial path. |
| Parent secrets could leak through environment, argv, handles, logs, errors, or debug output. | Environment is allowlisted, argv fixed, descriptors close-on-exec, stderr discarded, raw transport and completed typed string owners are zeroizing, and public errors/debug output omit controlled material. |
| Forward enum insertion could make logical restore inventory differ. | V4 appends the value, so fresh migration and logical restore produce the same former server store 18 inventory digest. |

## Tune recovery audit

The rejected `cargo vstyle tune --language rust --workspace --all-features --strict` run was audited
against its adjacent rollout records and the complete candidate diff. It reported five net changed
files: `decodex-core/src/account.rs`, `decodex-server-store/src/types.rs`,
`decodex-server-store/src/accounts.rs`, `decodex-runtime/src/account_launch/process.rs`, and
the then-current Codex runner source. Its transformations were import/declaration regrouping, impl and
private-helper adjacency, crate-absolute test imports, and whitespace normalization. No branch,
atomic ordering, lifetime, cleanup, shutdown, redaction, account identity, descriptor operation, or
assertion changed. Its interim Codex files did not compile. Imports, generic bounds, and former server store
test-module placement were therefore reconstructed manually; mutation-based vstyle was not reused.

After the manual audit, a fresh target directory was created and the affected sources were compiled
without prior artifacts:

- `CARGO_TARGET_DIR=/tmp/xy1273-clean-source-20260714-2003 cargo check -p decodex-core -p
  decodex-server-store -p decodex-codex --all-features --all-targets` — passed.
- The focused commands below were then run with that same clean `CARGO_TARGET_DIR` and passed.

## Pre-freeze focused verification history

These results describe development-time checks. They are not a self-referential candidate receipt:
embedding a complete candidate fingerprint in this file would change that fingerprint. The visible
owner handoff therefore freezes the repository first, records the complete fingerprint twice, runs
the final commands into external `/tmp` logs without further repository mutation, hashes those logs,
and records matching post-validation fingerprints in the task transcript.

- `cargo nextest run -p decodex-core -p decodex-server-store -p decodex-codex -p decodex-runtime
  --all-targets --all-features` — 200 passed and 17 owning-gate fixtures skipped. The passing suite
  includes quarantine start/reset/panic/poison/wakeup/fairness, preflight/final permit transfer,
  lifecycle teardown/join, account mismatch, inbound/outbound zeroization, native-image rejection,
  original-path and protected-snapshot replacement, credential-negative storage, and private runtime
  capacity tests.
- `cargo make test-vnext-server-store-store` — passed a fresh former server store 18 migration, account authority,
  credential-vector, tamper, restart, Turkish collation, hostile search path, logical restore, and
  migration-ledger suite. Its pool-size-one runtime fixture passed exact non-ready/stale/success,
  blocked-vault mutation progress plus final revision rejection, capacity exhaustion, and account
  mismatch through the complete private former server store-to-Codex composition.
- `cargo make test-vnext-architecture` — 11 passed, including exact sibling dependencies,
  enumeration of every workspace library/binary target, current production dependency reachability,
  and absence of synthetic fixture features on normal edges. This metadata evidence does not claim
  call provenance or wrapper detection.
- `cargo check --target x86_64-unknown-linux-gnu -p decodex-runtime --lib` passed the Linux cfg and
  API branch. The runtime suite uses the immutable image reference
  `rust@sha256:7c4ae649a84014c467d79319bbf17ce2632ae8b8be123ac2fb2ea5be46823f31`
  (local image ID `sha256:346182e770acabf336789808931b7f73316cc1e69f4ce6fc47e6843a9e24381a`,
  OCI OS/architecture `linux/arm64`). Its userspace readback is Debian GNU/Linux 12 (bookworm).
  Docker 29.4.0 uses the `orbstack` context/VM, whose readback is
  `Linux 7.0.11-orbstack-00360-gc9bc4d96ac70 #1 SMP PREEMPT Thu Jun 4 16:40:25 UTC 2026
  aarch64 GNU/Linux`; `uname -m` is `aarch64`. The container does not provide that kernel: it uses
  the OrbStack Docker host/VM kernel exposed to it.

  The exact process-suite invocation is:

  ```sh
  docker run --rm --init \
    -v "$PWD:/workspace:ro" \
    -v xy1273-linux-target:/target \
    -v xy1273-linux-cargo:/usr/local/cargo/registry \
    -w /workspace \
    -e CARGO_TARGET_DIR=/target \
    rust@sha256:7c4ae649a84014c467d79319bbf17ce2632ae8b8be123ac2fb2ea5be46823f31 \
    sh -c '. /etc/os-release; printf "userspace=%s\n" "$PRETTY_NAME"; uname -a; uname -m; \
      printf "pid1="; tr "\0" " " </proc/1/cmdline; printf "\n"; \
      cargo test -p decodex-runtime --lib account_launch::process::tests -- --test-threads=1'
  ```

  Docker `--init` installs `/sbin/docker-init` as PID 1. The suite includes
  `sealed_snapshot_is_the_image_executed_after_final_verification`; the final transcript receipt
  records its complete result, timestamp, external log hash, and candidate fingerprint binding.
- `cargo vstyle curate --language rust --workspace --all-features --strict` — 213 files checked
  read-only after manual item-order, import, and spacing repairs; no mutation-based vstyle command
  was used.
- `cargo clippy -p decodex-core -p decodex-server-store -p decodex-codex -p decodex-runtime
  --all-targets --all-features -- -D clippy::all -D clippy::too_many_lines -D
  clippy::unwrap_used -D clippy::use_self -D clippy::wildcard_imports -D missing-docs -D
  unused-crate-dependencies -D warnings` — passed.
- `cargo test -p decodex-core -p decodex-server-store -p decodex-codex -p decodex-runtime --doc
  --all-features` — Codex 1/1 and runtime 4/4 compile-fail doctests passed; core and former server store have
  no doctests. The contracts prove that product capacity and the dormant runtime launcher are absent
  from public crate APIs.
- `cargo +nightly fmt --all -- --check`, `taplo format --check`, and `git diff --check` are the
  read-only formatting/diff gates used for the final candidate.

One process-test invocation was started concurrently with four separate Cargo test processes and
hit the pre-existing two-second fixture PID deadline before the fake child wrote its marker. The
exact test passed immediately in isolation, and the complete process suite then passed when
rerun without competing Cargo builds. The final canonical gate runs the suite sequentially through
the repository task authority.

The first broad Linux container invocation omitted an init reaper and ran tests in parallel. Nine
descendant-cleanup fixtures retained zombie process groups under the non-reaping Cargo PID 1 and
failed their bounded availability assertions; the sealed-image test and 52 other process tests
passed. Repeating the complete suite with Docker `--init` and one test thread passed. This is an
honest availability boundary: a containerized daemon requires a functioning init/subreaper to
recover capacity from orphaned descendants. Without one, process ownership remains quarantined and
capacity fails closed rather than being undercounted.
The production prerequisite and the 64-slot fail-closed exhaustion consequence are owned by
`openwiki/operations/operator-runbooks.md`, not only by this issue evidence.

No live host-vault fixture was run because no operator vault implementation or disposable credential
was supplied. Using the ambient shared account would bypass the boundary being proved and could
mutate global authentication state. The credential-negative synthetic process fixture is therefore
the only enabled binding fixture; the production default remains honestly unavailable.

`Cargo.toml` and `Cargo.lock` are mechanically shared with XY-1269. This candidate uses the
already-locked `zeroize` 1.9 crate's maintained Serde feature; it adds no package version or new
third-party package. Final acceptance still requires the
Manager's owner-controlled rebase and dependency regeneration after XY-1269 lands if that issue is
still active.

The authoritative final freeze gate remains:

`SCCACHE_DISABLE=1 DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer cargo make check`
