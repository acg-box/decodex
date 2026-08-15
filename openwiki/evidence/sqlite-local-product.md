---
type: "Evidence"
title: "SQLite Local-Product Evidence"
description: "Accepted automated and live evidence for the bundled SQLite product, app-server freshness boundary, Quick Task controls, and process retirement."
tags: [local-product, sqlite, evidence, quick-task]
openwiki:
  roles: [testing, architecture, workflow]
  change_kinds: [lifecycle, public-api, validation]
  source_paths: [crates/decodex-runtime/src/quick_task.rs, crates/decodex-runtime/src/account_launch/process.rs, crates/decodex-protocol/src/quick_task.rs, apps/decodex-gpui/src/quick_tasks.rs]
  symbols: [control_thread, reconcile_archive, QuickTaskExecutionSettings]
  test_paths: [database/tests/quick_task_restart.rs]
  invariants: [Lossy external thread turns are not imported during lifecycle refresh.; Archive commits only after positive post-readback.; RestoreProcessReadiness is pre-effect.]
  validation_commands: [cargo make check]
---

# SQLite Local-Product Evidence

Status: accepted implementation and signed live-cutover evidence.

Date: 2026-08-14.

This page contains no credential value, email address, provider-account identifier, or
credential fingerprint.

## Implemented evidence

- `database/` owns one bundled SQLite connection, immutable V1 and V2 migrations, digest ledger,
  exact schema inventory verification, WAL, full synchronous mode, foreign keys,
  integrity checks, no-follow open, and owner-private file creation.
- Unit tests cover initialization, reopen, migration tamper refusal, exact credential
  compare-and-swap, foreign-key refusal, file mode, symlink refusal, account lifecycle,
  routing, quota uncertainty, and command replay.
- `database/tests/quick_task_restart.rs` uses only public APIs. It persists initial account
  routing, RuntimeSession and Codex-thread binding, a ready ProcessGeneration, one
  authorized ProviderAttempt, assistant history, positive provider evidence, and atomic
  terminalization. It then reopens SQLite, reserves a later Turn, and proves a SameThread
  plan on the original account and Codex thread. It admits one fresh rehydrated
  ProcessGeneration only from the active, acknowledged source RuntimeSession and exact prior
  terminal evidence. It also proves that an exact completed process-admission receipt plus an
  absent target generation is durable pre-effect evidence. Replaying the old attempt leaves
  exactly one dispatch intent.
- `database/transfer` tests a real redb fixture, exact import, exact replay, SQLite
  readback, owner-private mode, and byte-for-byte source retention.
- Installer tests cover fresh install, direct LaunchAgent composition, transfer ordering,
  bounded subprocesses, signature checks, account-count readback, and source retention.
- Runtime and daemon focused checks compile without former server store. Daemon signal tests cover
  SIGINT, SIGTERM, exact socket cleanup, and stale-socket recovery after SIGKILL. In the full
  nextest run, these real cold-start tests reserve all global test threads while retaining their
  20-second startup bound.

## Workbench and account-affinity correction

The 2026-08-14 correction restores the accepted transparent GPUI Workbench from its exact
committed source. It includes the conversation-first surface, mounted left and right sidebars,
`Command-B` and `Command-Shift-B` panel controls, symmetric 240 ms motion, the context inspector,
and the floating composer. GPUI and its platform crate now come only from the official Zed
repository at one pinned revision. A deterministic native visual capture completed successfully.

The restart integration test now imports two accounts. It starts one conversation on account A,
reopens SQLite, changes the global route to account B, and starts an independent conversation on
B. It then resumes the first conversation on its original account and Codex thread. The replay
still contains one provider dispatch intent. This proves that global route changes apply to new
conversations and do not break the cache affinity of an existing conversation.

The obsolete server-store crate and its compatibility configuration were deleted. A repository
reverse scan finds no remaining source, configuration, documentation, or dependency reference to
that removed implementation. The current local-database gate passed with schema version 3, WAL,
all three migration digests, and the exact 28-table inventory. `cargo test --workspace --all-targets`
also passed on stable Rust with the Xcode beta Metal toolchain.

## Signed live acceptance

On 2026-08-14, the daemon, CLI, and transfer binaries built from the final source were signed by
one Apple Development Team and installed atomically. Strict signature verification passed for
the three fixed executable identifiers. The installer then reported:

- SQLite active as the local database;
- six accounts available after the one-shot transfer;
- the transfer was not repeated during final-source reinstall;
- the retired former server store directory and redb vault retained; and
- the direct `decodexd serve` LaunchAgent running.

A fresh real Quick Task conversation against that final installed set completed one Codex
app-server turn, restarted `decodexd`, and completed a later turn that recalled a nonce from the
first turn. The probe reported one preserved RuntimeSession and one Codex thread. Independent
read-only SQLite queries found:

- four completed Turns: two user and two assistant;
- 48 ordered history items;
- one InitialThread and one SameThread continuation plan, each with a distinct operation and
  idempotency key;
- two distinct succeeded ProviderAttempts, each with exact terminal evidence; and
- two distinct ProcessGenerations: the pre-restart generation dead and the current generation
  ready.

The exact Codex thread value and all credential-bearing values were suppressed. The failed
pre-dispatch probe and the later acceptance-unknown probe remain durable diagnostic evidence and
were not automatically retried.

An additional new conversation was started while the selected account still owned the prior
conversation's ready ProcessGeneration. Before the repair, this case returned
`AcceptanceUnknown` despite having no target generation or ProviderAttempt. After the repair, the
same live condition returned the explicit `RestoreProcessReadiness` rejection. Read-only SQLite
evidence showed one failed user Turn, no active Turn, no Codex thread, no target
ProcessGeneration, and no ProviderAttempt. The one-live-generation-per-account policy remains;
cross-conversation process multiplexing is deferred.

## Live GPUI workbench acceptance

The current GPUI Workbench uses one retained same-UID protocol session for Conversation,
Accounts, and Health. The Accounts destination shows bounded account lifecycle, quota,
and routing controls. It does not read SQLite or expose credentials. The title bar is 42
pixels high. Deterministic native captures cover the live Accounts destination and the
transparent Workbench with the thinner title bar.

The client does not activate the deferred WorkItem and Project query surface. Protocol
V2.1 is strict, and an older daemon can close a retained session when it receives an
unknown `ListProjects` query. The WorkItem controller stays dormant until a later
capability-negotiation contract exists. This prevents the deferred factory surface from
making Conversation history stay in loading state or making the whole Workbench appear
offline.

One ignored live integration test connected to the installed signed daemon and completed
two new real Conversations in sequence. It then returned to the first Conversation,
submitted a later Turn, and proved that its RuntimeSession revision and complete
cursor-paged history advanced. The test read only bounded presentation state and did not
emit an account, Conversation, Codex thread, or credential identifier.

The runtime now retires the exact app-server process after positive provider terminal
evidence and durable Turn terminalization. It records positive ProcessGeneration death
before it publishes `Ready`. The later Turn rehydrates the original account and Codex
thread in a fresh ProcessGeneration. After the three-turn live test, read-only SQLite
inspection found zero non-dead ProcessGenerations and no app-server child owned by the
daemon.

A terminal event now refreshes history when its Conversation is open. If another
Conversation is open, the pager records a bounded invalidation. The next open bypasses
the cached head and waits for a fresh server page. A deterministic test proves that the
old cached page cannot become visible after this invalidation.

## App-server freshness and execution controls

The selected-thread refresh boundary treats Codex Desktop and Decodex as app-server
clients. It does not synchronize the two UIs. An exact account-bound `thread/read`
re-observes the selected thread lifecycle. External archive readback atomically archives
the local Conversation and ends its RuntimeSession. A Decodex archive uses same-process
pre-read, `thread/archive`, and post-read before the local transition. The active SQLite
list no longer retains rows absent from a complete current local page.

The generated schema from the installed Codex app-server confirms `thread/read`,
`thread/archive`, `model/list`, and request-scoped `model`, `effort`, and `serviceTier`
fields. Fast maps to `serviceTier = "priority"`; Fast off sends an explicit null. The
GPUI carries its selected model, reasoning effort, and Fast value on each new or later
Turn without changing global Codex configuration. External visible turns remain outside
this lifecycle-refresh milestone because `thread/read(includeTurns=true)` is explicitly
lossy and cannot prove complete history or tool effects.

Protocol V2.1 carries the new controls and archive event. SQLite schema version 3 adds
the original Quick Task execution settings. The local database gate passed with all
three migration digests, WAL, the exact 28-table inventory, and the `model`,
`reasoning_effort`, and `fast` request columns.

Focused implementation evidence is split across `crates/decodex-runtime/src/quick_task.rs`
(exact control sequencing and process retirement),
`crates/decodex-runtime/src/account_launch/process.rs` (thread read/archive readback),
`crates/decodex-protocol/src/quick_task.rs` (wire settings and recovery action), and
`apps/decodex-gpui/src/quick_tasks.rs` (bounded list, settings, and archive command
presentation). The narrow checks are the Quick Task, process reconciliation, protocol,
and database restart suites; use the workspace gate only when a change crosses package
or generated-schema boundaries.

Final milestone validation passed `cargo test --workspace --all-targets --all-features`,
strict workspace Clippy, the schema-3 local database gate, and the vNext architecture
tests. The installed Codex executable also passed the ignored live read-only probe,
which negotiates schema/version and read-only RPCs without dispatching a Turn. The
deterministic native Workbench capture includes the refresh, Archive, model, Fast, and
reasoning controls without changing the existing shell layout.

## Desktop account surfaces and atomic packaging

GPUI and the Swift menu-bar companion remain separate same-UID processes. Both use the
daemon-owned account model through the current typed Rust protocol. Swift reaches that
protocol only through its embedded `decodex-app-client-ffi` library; neither UI reads the
SQLite database or credential bytes.

A live failure probe found that the separately installed menu-bar bundle was one protocol
minor behind the running daemon. Its embedded FFI returned `protocol_minor_mismatch` for
`list_accounts`, while the current source bridge returned all six account rows. The GPUI
staging path now builds the Swift companion from the same checkout and embeds it at
`Contents/Library/LoginItems/DecodexMenuBar.app`. The companion has the fixed bundle identity
`box.acg.decodex.menubar`, which is the identity controlled by GPUI Settings. This makes the
two desktop surfaces one release artifact without combining their process or authority
boundaries.

Quota presentation follows one rule on both surfaces: render a window only when the provider
has supplied a current value. An absent, unknown, unsupported, or failed five-hour observation
does not create a synthetic `5 HOUR` row. It remains a typed protocol fact for routing and
diagnostics.

The staged nested bundle passed strict deep code-signature verification. A direct call through
its embedded FFI negotiated the running daemon, returned `available`, and read six accounts.
The older installed FFI remained the negative control and continued to return the typed minor
version mismatch.

Final acceptance from the task checkout included 983 Rust tests with five intentional skips,
215 Swift tests, 30 desktop architecture and contract tests, the two real CLI diagnostics tests,
the SQLite local-database gate, and a stable-toolchain workspace check. Every applicable test and
gate passed. The staged application also passed strict deep code-signature verification before
the live FFI readback.

## Final repository gates

One complete `cargo make check` run finished successfully on the final source. It included:

- exact npm 11.17.0 installation, lock provenance and signature checks, zero high-level npm
  vulnerabilities, site build, and Astro diagnostics with zero errors, warnings, or hints;
- all-feature, all-target workspace compilation, Rust formatting, Taplo formatting, and strict
  Clippy checks for all 12 active Rust packages;
- the schema-V3 local database gate with all three immutable migration digests, WAL, foreign keys,
  integrity checks, owner-private mode, and the exact 28-table inventory;
- 833 passed nextest tests with three declared skips, including the globally isolated
  full-daemon signal tests; and
- architecture-contract and real CLI/daemon diagnostic tests.

The final Rust advisory scan reports zero vulnerabilities. Its information-only result remains
the pre-existing baseline of four unmaintained and two unsound transitive packages. Dependency
inspection confirms that normal `decodexd` composition contains neither former server store nor redb;
redb is present only in the separate one-shot transfer executable.
