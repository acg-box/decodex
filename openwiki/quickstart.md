# OpenWiki Quickstart

Decodex is cutting over to the accepted vNext agent-workspace architecture. The active
Rust workspace currently contains the ownership skeleton and explicitly unavailable
composition roots; it does not provide the old Linear/SQLite runtime. The v0.2 source is
preserved under `apps/decodex/` as frozen provenance and is excluded from the active
workspace. Radar, Publisher, the static site, plugins, and automation source remain
outside the runtime rewrite until their owners adopt them.

OpenWiki is the repo-local project knowledge surface for agents and maintainers. Runtime authority lives in source, project contracts, tests, manifests, and local runtime state; OpenWiki explains where to start and what to watch before editing.

XY-1403 retires the private-artifact lane at the exact repository effective point
defined in the [retirement decision](specs/private-artifact/decision.md#repository-effective-point).
The [private-artifact archive](specs/private-artifact/README.md) is historical
evidence only after that point. It is not a runtime or future-work input.

XY-1276 Candidate 5 is approved architecture, not implemented or ready behavior. Exact
Candidate-4 staged tree `f82b866e21f12742648023a2b468cc057afa52a1` is materially
rejected and superseded source evidence. Current source remains on the V1-V32 ledger with
Quick Task dispatch unavailable. See the
[reset decision](decisions/vnext-authority.md#xy-1276-candidate-5-architecture-reset),
[normative contract](specs/vnext-authority.md#xy-1276-quick-task-thread-establishment),
[acceptance gate](specs/vnext-gates.md#xy-1276-candidate-5-quick-task-acceptance), and
[validation boundary](operations/commands-and-validation.md#xy-1276-candidate-5-validation-boundary).

## Start here

- [Runtime architecture](architecture/runtime-architecture.md): process topology, CLI bootstrap, app-server runs, operator HTTP/MCP, and state ownership.
- [Design rationale](decisions/design-rationale.md): why Decodex keeps loop graphs internal, autonomy authority typed, MCP/skills split, the site static, and Radar/Publisher bounded.
- [vNext authority decision](decisions/vnext-authority.md): the accepted product,
  ownership, state-authority, cutover, delivery, and private-artifact retirement
  decision.
- [vNext authority contract](specs/vnext-authority.md): normative entities, runtime boundaries,
  protocol, account continuity, non-goals, clean-cutover contract, and retained-title
  Git evidence boundary.
- [Account lifecycle authority](specs/account-lifecycle-authority.md): the three-owner
  PostgreSQL/HostCredentialStore/Account Service boundary, MacDogfoodReady, final
  readiness, refresh/recovery, ordinary import, and clean startup.
- [XY-1400 ProcessGeneration authority](specs/process-generation-authority.md): durable
  pre-spawn fencing, opaque exact-build launch attestation, macOS positive-death quarantine,
  exact process identity, ProviderAttempt ambiguity handoff, restore safety, and the deferred
  adversarial acceptance matrix.
- [XY-1401 ProviderAttempt authority](specs/provider-attempt-authority.md): generic
  Conversation/ManagedRun consumer binding, exact V16/V17/ProcessGeneration lineage,
  positive-only reconciliation, restore projection, duplicate-risk fencing, and the deferred
  acceptance matrix.
- [XY-1402 stateless execution coordination](specs/execution-coordinator-authority.md):
  closed Conversation/ManagedRun consumer integration, exact cause projection, V12
  retirement, production isolation, and the deferred unified acceptance matrix.
- [vNext gate manifest](specs/vnext-gates.md): ordered feasibility and implementation gates,
  downstream issue ownership, private-artifact retirement, and decision-changing
  falsifiers.
- [Private-artifact design archive](specs/private-artifact/README.md): frozen historical
  decision, semantic modules, protected authority data, receipts, and retirement
  controls. It contains no executable or future vNext obligation.
- [XY-1262 Codex runtime proof](evidence/vnext-codex-runtime-proof.md): shared-home, ownership, schema, collaboration, cross-account, fallback, crash, and typed-quota evidence for the Codex feasibility gate.
- [XY-1345 exact-command authority proof](evidence/xy-1345-exact-command-authority.md): corrected pure-PostgreSQL command authority, deterministic/concurrency schedules, privilege/catalog closure, restore receipt, and V9/V10 ownership order.
- [XY-1372 private-artifact capability evidence](evidence/xy-1372-private-artifact-capabilities.md): historical accepted APFS, OrbStack overlayfs, and OrbStack virtiofs feasibility provenance; it authorizes no current platform or delivery gate.
- [Lane Authority v2](decisions/lane-authority-v2.md): superseded historical target retained as architecture and incident provenance; C1-C7 are frozen and must not be implemented.
- [Drift audits](evidence/drift-audits.md): public-safe evidence notes, current MCP remote-control watched claims, reverse checks, validation commands, and stop conditions.
- [v0.2 freeze receipt](evidence/v0.2-freeze.md): exact trusted tag, cold-config and automation inventory, frozen legacy work, preserved incident evidence, cleanup ownership, and the unresolved SQLite-backup gap.
- [GPUI feasibility evidence](evidence/gpui-feasibility.md): accepted pinned XY-1263 foundation/toolchain, macOS runtime and package probes, bounded-history measurements, preserved negative provenance, and the independently reviewed normalized current-main 40/40 PID-bound accessibility receipt landed in PR #1109.
- [Runtime operator workflows](workflows/runtime-operator-workflows.md): project registry, run/serve/status, lane control, recovery, intake, commit/land, accounts, and MCP workflows.
- [Contracts and data](specs/contracts-and-data.md): current v0.2 project config, SQLite, Decision Contract, Program Intake, tracker, review, and commit behavior; superseded for vNext target work.
- [Runtime contracts](specs/runtime-contracts.md): current v0.2 state, app-server, tracker, evidence/privacy, and recovery contracts; superseded for vNext target work.
- [Runtime lifecycle](specs/runtime-lifecycle.md): current v0.2 lane, app-server, tracker, review, and autonomy lifecycle; superseded for vNext target work.
- [Lane Authority v2 target contract](specs/lane-authority-v2.md), [effect registry](specs/lane-authority-v2-effects.md), [gate manifest](specs/lane-authority-v2-gates.md), and [checkpoint ledger](evidence/lane-authority-v2-checkpoints.md): superseded provenance only, not active implementation authority.
- [Commands and validation](operations/commands-and-validation.md): task runner, tests, targeted checks, status publishing, app/site/Radar/Publisher validation.
- [Operator runbooks](operations/operator-runbooks.md): lane-control recovery, review handoff recovery, release readiness, GitHub operations, and control-plane workflows.
- [Codex upstream automation](operations/codex-upstream-autopilot.md): agent-led
  upstream research, deterministic PR creation, independent review and landing, and
  portfolio management without the Decodex server.
- [Plugins, automations, and auxiliary tools](integrations/plugins-automations-and-auxiliary-tools.md): installable plugin lifecycle, hook guardrails, automation sync, Radar, Publisher, native App, and site boundaries.
- [Radar, Publisher, and site contracts](integrations/radar-publisher-site.md): Radar artifacts, upstream review, release deltas, social publishing, site contract, and retention.
- [Radar Publisher contracts](integrations/radar-publisher-contracts.md): evidence separation, Publisher hard boundaries, static-site boundary, retention, and stop conditions.

## Repository map

- `crates/decodex-core/` owns domain/application authority contracts, including logical
  Conversation/RuntimeSession/history and deterministic inspectable Context Pack types, plus the XY-1306
  typed `~/.decodex` root, bounded/redacted config profiles, stable server identity,
  content-addressed blobs, and disposable bounded cache foundation.
- `crates/decodex-protocol/` owns the vNext version and the owner-only, same-UID Unix
  transport contract shared with clients.
- `crates/decodex-postgres/` owns the PostgreSQL product-state adapter: explicit
  connection configuration, embedded immutable migrations, optimistic transactions,
  leases, append-only activity, transactional outbox delivery, inert account/window
  metadata, bounded history pagination, immutable snapshots, blob references, Context Pack
  revisions, inert rollover/fallback proposals, exact in-transaction receipts, and immutable
  global RoleProfile revisions. V14 additionally owns revisioned complete routing policies,
  database-timestamped ordinary Codex compatibility evidence, and immutable complete routing
  fact snapshots; it performs no selection or dispatch. V15 adds the uncomposed causal Codex
  experiment persistence protocol. Forward-only V22 repairs its retained-title authority for the
  pinned two-effect protocol. It stores the exact nullable-name `thread/start` request and response.
  It then fences one `thread/name/set`. Only exact-ID `thread/read` can attest the prepared title.
  Positive observations and V17 same-thread authority require that attestation. Forward-only V23
  adds durable ProcessGeneration intent, exact identity, append-only positive death evidence, and
  account-local quarantine. Runtime has function-only ProcessSupervisor authority and no relation
  DML. Forward-only V24 adds one generic ProviderAttempt authority for Conversation Turns and
  ManagedRun executions, append-only transition and positive-evidence histories, restore
  projection, and bounded positive-only reconciliation. Runtime has function-only
  ProviderAttemptService authority and no relation DML. Forward-only V25 adds the
  closed route and wait enum vocabulary in its own transaction. Forward-only V26 removes the
  drained V12 ManagedRun-local submitted-turn and effect-barrier authority. It makes V14,
  V16, and V17 generic over an ordinary Conversation Turn or one ManagedRun execution,
  adds exact route-cause and reconciliation projection, and retains V16, V17,
  ProcessSupervisor, and ProviderAttemptService as the sole writers of their accepted
  decisions. Its zero-sized ExecutionCoordinator stores no state and is not connected to
  a production root. V16 adds inert
  atomic routing decisions over a
  PostgreSQL-authored locked universe, exact evidence references, duration-typed depletion
  exclusions, and pure-kernel readback. No production root reaches either boundary and they enable no
  live execution. XY-1307 wires the typed connection data through runtime composition into the
  existing verification/migration boundary; every bootstrap failure remains fail-closed.
  V18 adds XY-1362's uncomposed ledger-first `waiting_usage` wake authority. Append-only transitions
  own operation results, historical readback, lease/reclaim history, cancellation, supersession,
  and the one fired fresh-routing request; a mutable head is only the exact-tip scheduler index and
  fence. Cross-key operation replay reads the immutable transition result and never reconstructs a
  success from the head. Fired transitions contain no old candidates, quota evidence, eligibility,
  exclusions, or account choice and structurally disable prior-decision reuse and production
  enablement. Forward-only V19 reopens the core freeze only to repair deterministic wake-time
  acceptance authority: the four unchanged public commands still select PostgreSQL time after the
  same locks, while four migration-owner-only internals accept a bounded explicit instant for the
  deferred acceptance gate. Runtime cannot execute those internals, inject time, or reach V18/V19
  through another API. No runtime or application composition root imports the wake adapter.
  Forward-only V20 recreates only nine named CHECK constraints with equivalent explicit lower/upper
  predicates so their exact definitions are stable across restoration. Phase A authority capture
  requires the source S0, first restore R1, and second restore R2 to satisfy both S0=R1 and R1=R2.
  Forward-only V21 repairs the RuntimeSession event classifier without rebinding its triggers:
  scalar RuntimeSession/profile/account snapshot identities are cross-domain provenance, while
  RuntimeSession aggregate/event/kind markers, complete session or snapshot objects, and outbox
  links to activity carrying those ownership shapes remain migration-owner-only.
  Candidate 5 reserves unlanded enum-only V33 and sole integration V34. V16 will be the
  sole initial Quick Task account selector. Initial Conversation routing uses exact L0,
  with all six RuntimeSession/account/profile lineage fields null and zero sticky members;
  unchanged L6 has all six present, positive revisions, and exactly one sticky member.
  V17 will atomically create the first selected snapshots, revision-1 unfenced starting
  RuntimeSession, and inert initial plan before the conversations owner atomically admits
  the first active revision-1 Turn and ordinal-0 completed Message. Account Service will
  fence only the selected account immediately before spawn. V34 will own RuntimeSession
  thread fields and seven exact trigger-function roll-forwards with unchanged bindings and
  ACLs. Candidate evidence must review clean behavioral commit P, run Phase A on P, review
  a reported-digest-only child C, prepare fully staged C once, commit without changing its
  bytes, run Phase B on clean C, and then run the sole six-stage aggregate on unchanged C.
  None of this target is implemented or accepted in current source.
- `crates/decodex-codex/` owns typed app-server contracts, exact-build capability profiles, redacted normalized events, fixed and bounded read-only launch/probe behavior, and immutable one-account process supervision. Current dispatch is disabled. Slice 1 can enable only the fenced initial-selection path; XY-1304 remains later automatic fallback/wake acceptance.
- `crates/decodex-runtime/` owns `decodexd` service assembly and is the only library owner that composes protocol and infrastructure adapters.
- `apps/decodexd/`, `apps/decodex-cli/`, and `apps/decodex-gpui/` are composition
  roots. The client roots depend only on the protocol crate. GPUI opens a real shell and
  window. Health is the only bounded live destination. Every other destination remains a
  placeholder. The Quick Task and WorkItem contracts do not make their shell
  destinations live. GPUI is not generally usable. Remaining Slice 1 UI work is Accounts
  and Conversation. Slice 2 owns Project/Work/Run; Slice 3 owns the Mac package.
- `apps/decodex/` is the frozen v0.2 package. It remains in Git for provenance but is excluded from Cargo workspace membership and must not be used by vNext.
- `apps/radar/` is the Radar auxiliary tool for upstream review queues, release deltas, artifact validation, signal rendering, and bundle generation (`apps/radar/README.md`, `apps/radar/src/lib.rs`).
- `apps/decodex-publisher/` validates and reserves Decodex-owned social artifacts (`apps/decodex-publisher/README.md`, `apps/decodex-publisher/src/lib.rs`).
- `apps/decodex-app/` is the current native macOS account UI. It is a
  credential-negative client of the daemon-owned
  [Account Lifecycle Authority](specs/account-lifecycle-authority.md) and does not
  contain a local account pool, helper/server path, credential authority, or service
  lifecycle owner (`apps/decodex-app/README.md`).
- `crates/decodex-app-client-ffi/` owns the app's credential-negative in-process
  protocol client and private `decodex/app-native-client/1` ABI. Its only child
  process is one user-requested, finite official Codex device-login session in an
  owner-private temporary home. It exposes no credential bytes or file path to
  Swift and does not start the Decodex CLI, helper, app-server, or legacy account
  process. It does not own credentials, account state, or daemon lifecycle.
- `site/` is the static Astro product site; it must not depend on live daemon state (`site/package.json`, `openwiki/integrations/plugins-automations-and-auxiliary-tools.md`).
- `plugins/decodex/` contains the installable Decodex plugin, narrow routing skills, and lifecycle guardrail hooks (`plugins/decodex/.codex-plugin/plugin.json`).
- `automations/upstream/` contains the current standalone Codex App upstream
  adaptation loop. `automations/decodex/` contains the current Content Manager and
  xurl Publisher tasks plus shared config and Publisher assets.
  `automations/radar/` owns reusable Radar assets and has no separate schedule. The
  old multi-task content schedules remain deleted (`automations/upstream/README.md`).
- `scripts/` contains repo maintenance helpers including plugin sync and macOS app staging.
- `tests/scripts/test_vnext_architecture.py` enforces the exact vNext dependency graph, client isolation, and exclusion-with-preservation of the legacy package.

## Runtime in one minute

`apps/decodexd` composes the PostgreSQL and Codex adapter boundaries through
`decodex-runtime` and serves the exact-current typed V2.0 protocol at the fixed
`~/.decodex/server/decodex.sock` endpoint. The active local profile must set
`policy = "same_uid"` and the exact service-owner effective UID. The server directory
has mode 0700. The persistent `decodex.lock`, fixed `decodex.sock.stage`, and published
`decodex.sock` entries have owner-only mode 0600 and exactly one link whenever present.
Publication binds the staging name and uses same-directory descriptor-relative `renameat`
while the one-link lock is held.
It opens a Codex app-server process only for an admitted manual reset-card request;
conversation dispatch remains disabled. It attempts only
the explicitly configured PostgreSQL Unix socket and otherwise retains a typed unavailable
adapter. The protocol accepts only V2.0 and provides typed command receipt/result and
event envelopes, bounded snapshots/queues/wire text, fixed-capacity in-lifetime
idempotency in its one exact-current namespace,
publication-epoch-bound cursor resume,
snapshot fallback, stable
server-identity pinning, bounded doctor/status results, and a bounded typed Conversation-history
query, plus a read-only immutable execution-decision query. The `decodex` and GPUI roots
compile against `decodex-protocol` only. `decodex status` and `decodex doctor` are active
API-only V2.0 diagnostic clients. `decodex reset-card` is the active manual reset-card
client. `decodex account profile` is the independent bounded account-profile client.
GPUI opens its real shell and window. Health is the only bounded live destination. Every
other destination remains a placeholder. The Quick Task and WorkItem contracts do not
make their shell destinations live. GPUI is not generally usable. Remaining Slice 1 UI
work is Accounts and Conversation.

The reset-card service uses only configured vNext account UUIDs. A durable terminal
receipt replays before current account gates. New admission and the pre-effect fence both
require enabled state, AccountLifecycle readiness, no unsettled account operation, and
exact Registry/store revision, credential fingerprint/version, and provider agreement.
Clients select a card by its public grant and expiry timestamps and send the exact account
revision. `decodexd` alone reads the credential
vault, starts and attests the Codex process, resolves the opaque provider credit ID,
persists that exact ID and the logical-command idempotency key before the effect, consumes
the card, and reconciles fresh provider state. Restart recovery reuses the same exact ID
and key; it never selects a replacement card. Both `account/rateLimits/read` and
`account/rateLimitResetCredit/consume` must be present in the generated app-server schema.

Each client reconnect captures the current socket identity and verifies the daemon kernel
peer UID. Each server admission verifies the client kernel peer UID and the current
directory, lock, and socket identities. There is no startup self-connect challenge and no
continuous endpoint watchdog. One lifecycle task owns the listener. One `JoinSet` owns all
session and command tasks with stable spawn IDs and kinds. The same lifecycle directly
owns daemon service futures. Shutdown first closes Reset Card provider-work admission,
then creates one absolute session/command deadline and harvests `join_next_with_id` until
empty. Already registered provider work keeps its own bounded process deadline and must
settle before exact endpoint cleanup and lock release. This boundary serializes legitimate
daemons. It does not claim confinement against hostile code that already has the same UID.

When PostgreSQL is ready, daemon bootstrap projects restored nonterminal
ProcessGenerations to `death_unknown`, performs one positive-only reconciliation pass, and
continues background reconciliation. Same-boot uncertainty remains local to its account. The
runtime exposes an exact diagnostic/reconciliation/owned-termination port, but no protocol,
CLI, routing, or production spawn path.

Daemon bootstrap also projects every present nonterminal ProviderAttempt to `unknown`, performs
one bounded positive-only reconciliation pass, and continues background reconciliation. A
replacement can reconcile the original attempt but cannot replay it. Process death, boot change,
EOF, timeout, restart, missing results, and negative search cannot prove `not_submitted`. The
runtime exposes bounded redacted diagnostics and exact positive-receipt reconciliation, but no
provider gateway or live dispatch path.

The crate-private stateless ExecutionCoordinator can sequence one persisted V16 decision, one V17
plan, one ProcessSupervisor-owned live fence, and one ProviderAttempt preparation. It consumes
the fresh prepared capability and returns only an inert projection. It has no retained service,
lifecycle, retry state, receipt, or durable relation. Its only protocol surface is a read-only
immutable execution-decision query. No protocol command, CLI, scheduler, application service,
Codex adapter, credential owner, UI, or daemon composition root can start the sequence or authorize
dispatch.

When PostgreSQL is ready, daemon bootstrap also opens the accepted pinned repository executor,
retains the single PostgreSQL/executor/saga composition, and completes bounded readback-only
restart reconciliation before serving. No managed-repository or GitHub mutation is exposed by the
current protocol; the GitHub PR/check reconciler remains sealed until the later provider and
PostgreSQL effect-lineage owner connects it.

The PostgreSQL adapter persists its XY-1267 foundation and forward-only XY-1271 history schema when `decodexd` receives one explicit
PostgreSQL 18 Unix-socket endpoint, an operator-pinned expected server UID, and distinct migration
and runtime identities. The socket directory must be owned by that UID and not group/other-writable.
The adapter retains descriptor identities for the directory and socket, rejects replacement, and
verifies the connected kernel peer UID before sending either identity's authentication data. Optional,
separate environment-variable references supply their credentials without entering config,
wire data, logs, or ordinary PostgreSQL rows. The migration identity is used only for forward
migration and migration verification and is closed before the live adapter retains the runtime
pool. The runtime identity must have the exact adapter DML/function/sequence contract. The audit
covers the login role and every NOINHERIT or inherited role reachable with `SET ROLE`, rejecting
membership admin option, ownership of any PostgreSQL 18 object class in the Decodex namespace,
superuser/BYPASSRLS, database/schema/table DDL, TRUNCATE,
grant options, trigger authority, `session_replication_role` SET/ALTER SYSTEM, or any other
retention bypass. The effective login value must be `origin`. Readiness requires a closed inventory
of every runtime-callable Decodex function with exact signatures, overloads, metadata, settings, and
source bodies matching the canonical embedded migrations. The 146 expected safety/state/retention
triggers must also remain enabled, correctly shaped, and bound to their canonical functions; no
additional user trigger, rule, policy, RLS mode, or noncanonical expression dependency may add an
indirect execution path on a runtime relation. The accepted V22 canonical PostgreSQL 18 schema
manifest attests its complete historical relation/column, default, constraint, index, enum-label, and internal
constraint-trigger binding together with each stable catalog dependency identity. It includes
foreign keys whose Decodex relation is either the child or
the referenced parent, so external cascades and internally generated execution paths fail closed.
Extension authority follows `pg_depend` membership,
not extension schema, so a runtime-controlled extension cannot own or drop a Decodex member.
`public.refinery_schema_history` is always schema-qualified and must have table SELECT only. Its
ordered versions, names, and checksums must exactly equal the embedded migration inventory;
missing SELECT is incompatible, while ownership, SET-reachable authority, table/column grant
options, writes, and table DDL privileges are unsafe. All canonical database functions have an exact
function-local `pg_catalog, decodex` search path. Exactly seventy-nine narrowly scoped functions are
security definers: three history cursor/version functions, eleven Project/Agent/Policy/Program/Objective
commands, two command-complete exact RoleProfile entrypoints, two command-complete exact
RuntimeSession entrypoints, four command-complete exact WorkItem entrypoints, one inert future
running/resume guard, twelve inert V14/V15/V22
routing and causal-experiment entrypoints, the inert V16 exact routing-decision entrypoint, V17's inert
exact continuation command plus strict readback, and V18's four exact wake commands plus strict
readback, plus V23's eight ProcessSupervisor fence, transition, projection, evidence, and read
entrypoints, plus V24's seven ProviderAttemptService preparation, transition, positive-evidence,
projection, and read entrypoints, one trigger-only Turn-reservation helper, and V26's immutable
execution-decision and ManagedRun execution-projection read functions, plus V27's sixteen exact
account lifecycle, routing, quota, store, and capability functions, V28's two exact profile
observation/read functions, and V29's PostgreSQL 18 array-zip repair for profile observation.
The helper has a
fixed search path, runs as the migration owner, and grants no direct runtime or PUBLIC execution.
The historical V24 boundary has an exact semantic inventory overlay for its 84-relation,
184-function, 75-safety-function, 154-trigger, 69-runtime-function, and ten-post-V22-enum
shape. V26 has the current 80-relation, 182-function, 74-safety-function, 146-trigger,
70-runtime-function semantic inventory. Full S0/R1/R2 V26 manifest capture and digest refreeze
remain in the deferred unified gate.
A selected V16 decision commits either one positive-evidence-bound same-thread plan or one Context
Pack, fallback RuntimeSession, and plan in the same transaction. Runtime cannot insert
cursor, exact-receipt, RoleProfile, RuntimeSession, RuntimeSession snapshot, WorkItem, ManagedRun,
assignment, routing-decision, decision-member, decision-quota,
decision-capability, decision-blocker, decision-exclusion, continuation-plan, waiting-usage wake
transition, waiting-usage wake head, ProviderAttempt, provider-attempt evidence, or
provider-attempt transition rows or execute trigger/private helpers directly.
The two bound identity sequences require
USAGE only; UPDATE/`setval`, SELECT, ownership, grant options, and SET-reachable surplus authority
are unsafe. Explicit qualification keeps bootstrap correct under a hostile runtime `search_path`.
Missing, malformed, unsafe,
unreachable, authentication-failed, and incompatible inputs remain typed unavailable with no
fallback.
Host repository paths reject symbolic links at any component. PostgreSQL socket paths additionally
use descriptor-pinned component traversal, immutable directory/socket identity checks, explicit
operator UID authority, and kernel peer credentials rather than trusting an observed pathname.
`~/.codex` remains Codex-owned shared continuation state. `decodex-core` owns the typed
`~/.decodex` layout for `config.toml`, logs, SHA-256 blobs, disposable cache, and atomic
server identity.

Doctor/status is a V2.0 read-only query served only by `decodexd`. Queries have client observation
identities but no mutation receipt, deduplication, replay, event, or receipt-capacity effect. Its closed report
covers configuration, database, protocol and version, stable server identity, shared
Codex home, each typed app-server capability, aggregate server-host repository readiness,
blob integrity, credential-vault readiness, and plugin readiness. It carries no repository
path/name, credential text, parser detail, database/socket/user text, or raw app-server
payload. Checks that are not yet safely probed report `unknown`; they never imply ready.
Every doctor read revalidates the pinned socket, a live runtime connection, the closed database
authority contract, and immutable migration history without rerunning migration or repinning the
endpoint. A secure stale listener is database-unreachable; endpoint replacement is unsafe-host-path.
PostgreSQL socket recreation requires restarting `decodexd` so bootstrap can establish a new explicit
operator-authorized pin.
The legacy `~/.codex/decodex` SQLite/config layout is frozen provenance, not a vNext
input or fallback.

The daemon history query is read-only, uses opaque PostgreSQL-issued cursors whose persisted rows bind
the Conversation, high-water snapshot, page size, positive position, exact item, and issued-parent chain, returns
at most eight items per WebSocket result, and verifies referenced blob bytes before returning
metadata. Only the page-level next cursor is exposed; page size is fixed for its chain. Cursor rows
expire after one hour, subsequent issuance prunes expired chains, and serialized hard limits cap
storage at 512 rows per Conversation and 4,096 globally. Never-issued, expired, cross-Conversation,
changed-size, and edited tokens fail closed; capacity refusal is typed. Domain, PostgreSQL, adapter
read/write, and wire boundaries enforce one canonical bounded
`type/subtype` media-type invariant. Wire decode bounds page cardinality, blob length, SHA-256 text,
media type, and a core-owned flat credential-negative metadata projection of at most 32 fields
(64-byte keys; string or boolean values; 256-byte string limit). Credential-bearing key suffixes and
concrete authorization/token/assignment/key patterns are rejected; ordinary text such as `secret
sauce`, `token budget`, and `session summary` remains valid. Inline and offloaded entries both carry
the same media type and projection. Bounded grace-aged orphan reclamation is deterministic and resumable, treats only
history, Artifact revisions, and Context Packs as live references, commits metadata deletion before
a lock-coordinated live-reference recheck and byte removal, and coordinates with writers through
PostgreSQL. Cursor issuance follows the canonical lock order: command receipt for mutations,
statement-level hierarchy coordinator 1271 before executor tuple selection, cursor coordinator
1272, then the Conversation and child rows. Hash-scoped blob coordination uses namespace 1273 and
never nests an outer hierarchy/cursor lock around filesystem I/O. Row-level hierarchy triggers do
not acquire the outer coordinator; their statement-level `BEFORE` guards run before PostgreSQL can
lock an update tuple.
RuntimeSession and Turn insert triggers author their creation/update timestamps, clear terminal
timestamps for legal nonterminal creation, and reject direct terminal creation, so runtime SQL cannot
persist a lifecycle state that typed services cannot represent.
Clients
cannot query PostgreSQL or local blob paths directly. No client mutation or artifact-download route
is exposed by this slice.

Artifact parents are transactionally coherent with one exact immutable current revision through a
deferred composite foreign key and state guard. Revision 1 must exist before an advance, and every
later immutable revision requires its exact legal predecessor, preventing direct-SQL gaps. Context
Pack source manifests are staged under the
writer lock and sealed by the immutable parent with an exact contiguous source count; runtime SQL
cannot append, alter, delete, or commit an incomplete source manifest after persistence.

The API-only diagnostic CLI operations `decodex status` and `decodex doctor` are active.
The `decodex account` lifecycle operations and the `decodex reset-card list`, `use`, and `status`
operations are active clients of the common daemon service. Other unsupported product CLI
operations remain unavailable in current source. Delivery proceeds through exactly three slices:
Accounts/Quick Task/minimal Accounts-Conversation-Health GPUI; then the bounded managed-work
flow and Project-Work-Run GPUI; then the two-account restart E2E and Mac package. A general
PostgreSQL administration plane, authenticated HTTP artifact path, and remote or cross-UID
binding remain later work.
Kernel same-UID credentials are the complete local V2.0 principal. Application PKI and remote
TLS remain outside this boundary and belong to the later remote-security gate.

The macOS source-install path is narrower than a general administration plane.
`scripts/macos/install_decodex_local_service.py` initializes one same-UID PostgreSQL
18 cluster, provisions the exact Decodex roles and grants, and installs one user
LaunchAgent. `decodexd supervise-local` owns the PostgreSQL and daemon process
generations. A PostgreSQL generation change stops the daemon and makes the
supervisor exit; launchd then starts one new coherent generation. An atomic
credential-file replacement restarts only the daemon when its injected credential
projection changes. The LaunchAgent restarts only unsuccessful exits and retains a
60-second final stop timeout. When the installed job has that exact contract, the
installer first signals the loaded supervisor, waits for its bounded daemon and
PostgreSQL drain to leave the job inactive, and only then removes the job. If the loaded
job does not match the exact installed service contract, the installer removes that exact
job directly before replacement. Both paths bind observed processes by PID and full start
time and wait at most 300 seconds before provisioning a replacement. Reset Card
discovery, account binding, provider access, and typed results remain Rust runtime work.
On macOS, the runtime starts the final canonical Codex image suspended and verifies it
against its immutable snapshot before resume so process-aware network extensions can apply
the correct route. Swift remains a client and owns none of these effects.

## First commands

Use these as discovery and validation entrypoints:

```sh
cargo run -p decodexd
cargo run -p decodexd -- --version
cargo run -p decodex-cli -- status
cargo run -p decodex-cli -- doctor --output json
cargo run -p decodex-cli -- account list
cargo run -p decodex-gpui
cargo test -p decodex-core --all-targets --all-features
cargo make test-vnext-architecture
cargo make test-vnext-postgres-store
cargo make check
```

`decodexd` with no arguments, or with `serve`, starts the same-UID Unix WebSocket
service and runs until stopped. `decodexd --version` prints the version and does
not start a service. `decodexd supervise-local --help` describes the bounded
Unix service supervisor used by the macOS source installer. The CLI selects
the configured active profile by default; `--profile NAME` selects an explicit declared
profile and `--root PATH` selects a typed Decodex root. Human output is the default and
diagnostic `--output json` emits `decodex/cli-diagnostics/1`; reset-card JSON emits
`decodex/reset-card-cli/1`. GPUI opens a real shell and window. Health is the only
bounded live destination. Every other destination remains a placeholder. The Quick Task
and WorkItem contracts do not make their shell destinations live. GPUI is not generally
usable. Remaining Slice 1 UI work is Accounts and Conversation.
For a targeted Rust gate,
prefer
`cargo check --all-features --all-targets --workspace` or
`cargo nextest run --workspace --all-targets --all-features` (`Makefile.toml`,
`openwiki/operations/commands-and-validation.md`).

XY-1399 A-prime is the historical source-only ancestor of the integrated same-UID
transport. The current tree must run the commands in this section. It also runs the
focused namespace, WebSocket lifecycle, daemon signal, CLI process, Reset Card
PostgreSQL, Swift, and signed app-staging checks described in
[vNext gates](specs/vnext-gates.md).

## Authority and safety rules

- Do not read `.env` files or live secret-bearing config. `decodex.example.toml` is the
  bounded vNext setup model and stores only a PostgreSQL credential environment-variable
  name, never its value.
- Do not route vNext product state through `apps/decodex`, legacy SQLite, Linear
  lanes, or the legacy operator transport. A legacy macOS account watcher or
  daemon-environment projection is not Mac dogfood authority. Old local credentials can enter vNext only through temporary private files and the
  ordinary public account import command. This finite operator action is not an installed
  migration feature. Startup and packaging must not use a watcher, environment bridge,
  helper/`:8192` service, mapping, or fallback.
- Use `decodex commit` and `decodex land` for Decodex-owned commit/landing authority; the installable plugin hook blocks raw `git commit` and `gh pr merge` inside Decodex scope (`plugins/decodex/scripts/decodex_lifecycle_hook`).
- PostgreSQL is the vNext product-state authority when explicitly configured; unavailable is the only supported service state otherwise, with no fallback authority.
- For project knowledge work, update OpenWiki directly and keep it aligned with source, tests, and manifests.

## Recent development context

XY-1265 established compile-time ownership and composition. XY-1266 established the
historical loopback protocol foundation. XY-1399 A-prime replaces its active production
transport with the same-UID Unix WebSocket authority; XY-1270 implements the bounded Codex adapter foundation
without live dispatch. XY-1267 established PostgreSQL-backed product state and durable
transactions. XY-1306 established the typed `~/.decodex` path/config/blob/cache child of
XY-1268; XY-1307 supplied daemon bootstrap/doctor; XY-1308 supplies the API-only CLI and
end-to-end diagnostic matrix.
Limited initial account routing and minimal GPUI are Slice-1 work. Automatic fallback and
wake remain with XY-1304; remote security, HTTP artifacts, and broader GPUI remain with
their later owners and gates.
The private-artifact API and runtime composition are not implemented and are no
longer vNext targets. At and after the XY-1403 repository effective point, the
[private-artifact archive](specs/private-artifact/README.md) preserves the complete
former design and receipt anchors as historical evidence. Its rules, A0/A1/B/D0a/C/D
delivery graph, CORE-FREEZE, ACC, preparation tasks, mechanical pass, and unified
validation are historical and non-executable. They do not define dependencies or
future work.

XY-1369 and XY-1370 keep their bounded operator checks and produce canonical
privacy-safe Git attestations and digests for XY-1363. XY-1363 consumes the exact
accepted receipt identities and uses the accepted V22 one-shot title path. This
replacement creates no service, schema, storage system, runtime route, platform
layer, issue, compatibility path, or product Artifact. The existing
Artifact/BlobStore boundary remains unchanged, and production dispatch remains
disabled.
