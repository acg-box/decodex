---
type: "Evidence"
title: "SQLite Local-Product Evidence"
description: "Accepted automated and live evidence for the bundled SQLite product, app-server freshness boundary, Quick Task controls, and process retirement."
tags: [local-product, sqlite, evidence, quick-task]
openwiki:
  roles: [testing, architecture, workflow]
  change_kinds: [lifecycle, public-api, validation]
  source_paths: [crates/decodex-runtime/src/quick_task.rs, crates/decodex-runtime/src/application.rs, crates/decodex-runtime/src/account_launch/process.rs, crates/decodex-codex/src/quick_task.rs, database/src/conversations.rs, database/src/continuations.rs, database/src/program_cycles.rs, apps/decodex-gpui/src/programs.rs, apps/decodex-gpui/src/quick_tasks.rs, apps/decodex-gpui/src/factory_surface.rs, apps/decodex-gpui/src/shell.rs]
  symbols: [control_thread, ExactSubmittedTurnReadback, recover_unknown_quick_task_turn, plan_continuation, QuickTaskExecutionSettings, TranscriptRow]
  test_paths: [database/tests/quick_task_restart.rs, database/src/conversations.rs, crates/decodex-runtime/src/account_launch/process.rs, apps/decodex-gpui/src/quick_tasks.rs, apps/decodex-gpui/src/shell.rs]
  invariants: [Lossy external thread turns are not imported during lifecycle refresh.; Stable client Turn identity permits positive correlation but never replay.; Inconclusive recovery requires positive exact process death.; A successor Context Pack excludes its own Turn.; Durable positive evidence survives an interrupted local terminalization.; Archive commits only after positive post-readback.; RestoreProcessReadiness is pre-effect.]
  validation_commands: [cargo test -p decodex-core -p decodex-codex -p decodex-database -p decodex-runtime -p decodex-gpui --all-targets, cargo clippy -p decodex-core -p decodex-codex -p decodex-database -p decodex-runtime -p decodex-gpui --all-targets -- -D warnings, python3 scripts/vnext/local_database_gate.py, python3 -m unittest tests/scripts/test_vnext_architecture.py]
---

# SQLite Local-Product Evidence

Status: accepted implementation and signed live-cutover evidence.

Date: 2026-08-15.

This page contains no credential value, email address, provider-account identifier, or
credential fingerprint.

## Implemented evidence

- `database/` owns one bundled SQLite connection, six immutable migrations, digest ledger,
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

## Silent Recovery V1 evidence

Every `turn/start` now serializes the existing Decodex Turn ID as
`clientUserMessageId`. A focused request test verifies the exact wire field. The bounded
thread-read projection accepts one matching user-message `clientId`, concatenates only
that Turn's assistant messages, retains its terminal status and provider Turn ID, and
computes a SHA-256 witness. Its focused test proves the positive match, a safe absent
observation, and duplicate-match refusal.

The SQLite recovery test starts with one active user Turn, one `unknown`
ProviderAttempt, and a non-dead exact ProcessGeneration. Recovery is rejected. After the
test adds exact process-death evidence, the same command succeeds and replays exactly:
the user Turn becomes failed, one interrupted activity is appended, the ProviderAttempt
remains `unknown`, and the Conversation projection no longer exposes an active unknown
attempt. This is a product-state recovery, not a fabricated provider outcome.

A separate crash-window test records exact positive thread-read evidence while the user
Turn is still active. The pending-terminalization readback reconstructs the exact
Conversation, RuntimeSession, Turn, ProviderAttempt, evidence, provider thread, and
provider Turn coordinates. The normal terminalization command then completes once and
the pending projection disappears. This proves that a process stop between the evidence
transaction and the local terminalization transaction does not require another provider
read or dispatch.

The fallback test then admits a distinct successor user Turn. Planning selects
`ContextPackFallback`, preserves the exact unknown predecessor identity, ends the source
RuntimeSession, creates one starting RuntimeSession on the same account, moves the new
Turn to that session, and stores the compiled Context Pack through migration 0004.
Reopening SQLite and replaying the plan reconstructs the same Context Pack digest. The
pre-existing public restart integration still selects `SameThread` after exact terminal
evidence, so Silent Recovery does not weaken the normal cache-affine path.

The fallback history read excludes the exact successor Turn and its history item. The
persisted binary Context Pack verifies before `render_model_input` removes its framing
and returns represented UTF-8 sources. Core tests cover deterministic rendering and
reject a binary-framing forgery as model text. The current user request therefore enters
Codex once, after the bounded historical evidence, instead of appearing in both inputs.

GPUI now includes `OutcomeUnknown` in selected and batch readback candidates. The
recovery action is `Retry sync`, not `Start new`, and the transient presentation says
that Decodex is checking durable state. The composer remains disabled until that
readback resolves, so the UI does not turn uncertainty into a duplicate send.

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
that removed implementation. The accepted pre-Silent-Recovery gate passed with schema version 3,
WAL, all three migration digests, and the exact 28-table inventory. `cargo test --workspace --all-targets`
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

The 2026-08-15 transcript correction requests and displays daemon-owned local history
before it queues selected-thread provider reconciliation on that retained connection.
It projects each queued prompt immediately and removes the projection only after the
matching durable user history item is visible. The transcript groups adjacent assistant
fragments by Turn, keeps tool and system activity as separate low-weight rows, and omits
redundant actor labels in the single-Codex view. Focused tests cover all three state
transitions. Strict Clippy passed with warnings denied, the architecture suite passed 10
tests, and a new deterministic native Workbench capture completed with the Xcode beta
Metal toolchain.

A real send smoke showed the queued prompt in the first 100 ms capture and rendered the
terminal assistant text as one response. It also exposed an existing multi-page reload
defect: a fresh head with an already retained valid successor was treated as a cursor
cycle. The repair validates the fresh page before it atomically replaces the retained
window, then rebuilds the successor from the fresh cursor. Its focused regression and
the pre-existing malformed-cycle tests pass together. After this correction, the
complete GPUI package passed 120 tests with two live tests ignored.

The client does not activate the deferred general WorkItem and Project query surface.
Protocol V2.5 is exact-current. The Adaptive Program controller uses only its bounded
Program and built-in Domain Pack commands and queries. The unrelated WorkItem board
controller stays dormant. This keeps deferred Factory surfaces from affecting
Conversation history or the Workbench connection.

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

The thread-refresh boundary treats Codex Desktop and Decodex as app-server clients. It
does not synchronize the two UIs. An exact account-bound `thread/read` re-observes one
thread lifecycle. Opening a Conversation uses that path for the selected thread. The
sidebar sync executes the same command sequentially for every local provider-backed
Conversation and performs a final SQLite list readback. External archive readback
atomically archives the local Conversation and ends its RuntimeSession. Definite busy
or recovery refusals are counted as skipped; ambiguous transport outcomes stop the
batch. A Decodex archive uses same-process pre-read, `thread/archive`, and post-read
before the local transition. The active SQLite list no longer retains rows absent from
a complete current local page.

The installed `codex-cli 0.148.0-alpha.9` omits `archived` from the current Thread JSON
shape. The runtime now treats that field as optional and derives lifecycle state only
from exact membership in bounded `thread/list` scans for the current and archived
filters. The fixture suite covers the current schema, conflicting list facts, missing
results, wrong correlation, malformed pages, pagination bounds, and archive readback.

The 2026-08-15 live check created two Conversations in an empty owner-private project,
completed both, returned to the first Conversation, completed a later Turn on the same
RuntimeSession, and read a larger fresh server history. It then archived the second
Codex thread through an independent app-server client. Provider readback proved that the
exact thread was absent from the current list, present in the archived list, and still
readable by exact ID. Decodex sync changed the active list from four rows to three,
reported one archive and zero skipped rows, and atomically ended the matching local
RuntimeSession.

Read-only SQLite inspection during the earlier lifecycle-refresh milestone found three active Conversations. Two had
acknowledged successful Turns. The third retained the sole ProviderAttempt with state
`unknown` and reason `dispatch_outcome_unavailable`. Decodex did not terminalize or
delete that record because lossy provider history cannot correlate the missing dispatch
outcome. That observation is the migration fixture for Silent Recovery V1: older Turns
without `clientUserMessageId` cannot produce a positive match, so after exact process
death they converge to the durable interrupted presentation while retaining the unknown
attempt.

Focused database tests prove two guarded local repairs. A stranded active user Turn can
be failed only with exact inactive-owner coordinates and no unresolved ProviderAttempt,
non-dead ProcessGeneration, or streaming history. A provider-less starting Conversation
can be archived only when no thread-start or external-effect evidence exists. Both
operations replay through exact durable receipts. The sidebar batch includes only states
that can use these guards; pre-session establishment recovery remains an explicit
`Recover` action.

The composer now has a confirmation fence. It retains text while a submission is queued
or awaiting a result, after archived-thread or recovery-required rejection, and after a
selection disappears. It clears only when the exact submitted content receives a newer
accepted terminal result and the user has not edited it. Recovery has a separate button,
so typed text cannot become a recovery command.

The generated schema from the installed Codex app-server confirms `thread/read`,
`thread/archive`, `model/list`, and request-scoped `model`, `effort`, and `serviceTier`
fields. It also exposes optional `TurnStartParams.clientUserMessageId` and user-message
`clientId` readback. Fast maps to `serviceTier = "priority"`; Fast off sends an explicit null. The
GPUI carries its selected model, reasoning effort, and Fast value on each new or later
Turn without changing global Codex configuration. Arbitrary external visible turns
remain outside this lifecycle-refresh milestone because
`thread/read(includeTurns=true)` is explicitly lossy and cannot prove complete history
or tool effects. Only one positively client-ID-correlated Decodex Turn may repair its own
terminal state and assistant suffix.

Protocol V2.5 carries the controls, archive event, bounded Program aggregate, built-in
Domain Pack projection, and the optional exact recovery-operation identity for official
device-login takeover. It carries no credential value. SQLite
schema version 3 adds the original Quick Task execution settings. Schema version 4 adds
the migration-owned `context_packs` table. Schema version 5 adds the Program, Signal,
Claim, Proposal, Objective, WorkItem binding, Evidence, Review, and semantic identity
tables. Schema version 6 adds exact predecessor Review lineage for continued Signals,
root and successor uniqueness, and same-Program lineage guards. Schema version 7 adds
one immutable Program-to-Pack binding table. Schema version 8 adds two account-operation
takeover links and replacement indexes. It does not add another table or rewrite an
ambiguity as cancellation. It retains the exact 40-table inventory. The schema-8 local
database gate passed with WAL, `quick_check`, foreign-key verification, all eight exact
migration digests, and the 40-table inventory. The exact
two-domain result is in the
[Built-in Domain Pack Pressure Test V1 evidence](builtin-domain-pack-pressure-test-v1.md).

Focused implementation evidence is split across `crates/decodex-runtime/src/quick_task.rs`
(exact control sequencing and process retirement),
`crates/decodex-runtime/src/account_launch/process.rs` (thread read/archive readback),
`crates/decodex-protocol/src/quick_task.rs` (wire settings and recovery action), and
`apps/decodex-gpui/src/quick_tasks.rs` (bounded list, settings, and archive command
presentation). The narrow checks are the Quick Task, process reconciliation, protocol,
and database restart suites; use the workspace gate only when a change crosses package
or generated-schema boundaries.

The Silent Recovery affected-package gate passed 560 tests with five intentional live
or CLI skips. Strict Clippy passed with warnings denied for `decodex-core`,
`decodex-codex`, `decodex-database`, `decodex-runtime`, and `decodex-gpui`. The vNext
architecture suite passed 10 tests. The installed Codex executable also passed the
ignored live read-only probe,
which negotiates schema/version and read-only RPCs without dispatching a Turn. The
deterministic native Workbench capture includes the refresh, Archive, model, Fast, and
reasoning controls without changing the existing shell layout.

The Health destination separates required core services, deferred app-server probes,
and optional capabilities. The summary reports core readiness only. `NotProbed`,
`Disabled`, and unconfigured plugin inventory render as `Not checked`, `Disabled`, and
`Not configured`; they do not appear as generic failures. A deterministic native Health
capture and focused GPUI tests cover this presentation contract.

The final live doctor report had all eight required core components in `Ready` state.
Managed repository was explicitly disabled. Blob integrity and eight app-server
capabilities were `NotProbed`, and plugin readiness was unconfigured. The Health UI
therefore reports `Core ready` and preserves these deferred states as `Not checked`,
`Disabled`, or `Not configured`; it does not turn schema evidence into a live readiness
claim.

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

## Dual-method account enrollment acceptance

The account enrollment repair keeps one native login manager and one daemon enrollment
command. The menu-bar App offers automatic browser redirect by default and a manual device
code alternative. The manual alternative displays the official URL and one-time code and
does not open a browser until the user selects Open. Both alternatives use the same private
temporary Codex home, operation journal, command receipt, credential-store authority, and
terminal cleanup.

The command choices match official Codex source at `openai/codex` commits
`9392c3fa5bcda342b5b96a1a04d67b2f781617c2` and
`5ee6baee2fcc0b6ffd413d9611f5538dad40d0f2`. The default command uses `codex login`; the
manual command adds `--device-auth`. Repository history at
`419535e159edadaa978e527e8f88b838b7e4ca66` supplied the earlier bounded device-code
presentation and cleanup evidence. The repair does not restore its separate state machine.

A later installed-App regression showed that current Codex did not flush its device prompt
while stdout was a pipe. The native manager now attaches only the device-code child's stdout
to an owner-only pseudo-terminal and continues to drain stderr through a pipe. A regression
child proves that its prompt is observable before exit only when stdout is a terminal; separate
tests retain the browser pipe and exact argv. The current terminal prompt wraps the code in
ANSI SGR, so the bounded parser removes only CSI SGR before applying its existing URL, code
shape, and token-boundary checks. An installed-FFI probe against Codex 0.148.0-alpha.9 moved
from `requesting_code` to `waiting_for_browser` with a prompt in 707 milliseconds. Typed cancel
then removed the child process and private login home without exposing the code.

Focused acceptance passed 38 native FFI tests and 80 Swift login-flow tests. The signed
installed App matched the tested candidate. Its FFI negotiated protocol V2.5 and artifact
cohort 1 with the running daemon and read seven account rows. The new account was ready and
available, its redacted profile was current, and the enrollment operation and receipt were
committed and succeeded with no unsettled enrollment operation. No login child or private
temporary login home remained. Enrollment did not issue a shared Codex projection command.
A later, separate user route action selected the new account through the ordinary typed
projection and fixed-routing commands.

## Earlier repository gates

One complete `cargo make check` run finished successfully on the pre-Silent-Recovery source. It included:

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
