# OpenWiki Quickstart

Decodex is cutting over to the accepted vNext agent-workspace architecture. The active
Rust workspace currently contains the ownership skeleton and explicitly unavailable
composition roots; it does not provide the old Linear/SQLite runtime. The v0.2 source is
preserved under `apps/decodex/` as frozen provenance and is excluded from the active
workspace. Radar, Publisher, the static site, plugins, and automation source remain
outside the runtime rewrite until their owners adopt them.

OpenWiki is the repo-local project knowledge surface for agents and maintainers. Runtime authority lives in source, project contracts, tests, manifests, and local runtime state; OpenWiki explains where to start and what to watch before editing.

## Start here

- [Runtime architecture](architecture/runtime-architecture.md): process topology, CLI bootstrap, app-server runs, operator HTTP/MCP, and state ownership.
- [Design rationale](decisions/design-rationale.md): why Decodex keeps loop graphs internal, autonomy authority typed, MCP/skills split, the site static, and Radar/Publisher bounded.
- [vNext authority decision](decisions/vnext-authority.md): the accepted product, ownership, state-authority, cutover, and delivery decision for the rebuild.
- [vNext authority contract](specs/vnext-authority.md): normative entities, runtime boundaries, protocol, account continuity, non-goals, and migration contract for later implementation.
- [vNext gate manifest](specs/vnext-gates.md): ordered feasibility and implementation gates, downstream issue ownership, and decision-changing falsifiers.
- [XY-1262 Codex runtime proof](evidence/vnext-codex-runtime-proof.md): shared-home, ownership, schema, collaboration, cross-account, fallback, crash, and typed-quota evidence for the Codex feasibility gate.
- [XY-1345 exact-command authority proof](evidence/xy-1345-exact-command-authority.md): corrected pure-PostgreSQL command authority, deterministic/concurrency schedules, privilege/catalog closure, restore receipt, and V9/V10 ownership order.
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
- [Plugins, automations, and auxiliary tools](integrations/plugins-automations-and-auxiliary-tools.md): installable plugin lifecycle, hook guardrails, automation sync, Radar, Publisher, native App, and site boundaries.
- [Radar, Publisher, and site contracts](integrations/radar-publisher-site.md): Radar artifacts, upstream review, release deltas, social publishing, site contract, and retention.
- [Radar Publisher contracts](integrations/radar-publisher-contracts.md): artifact contracts, upstream handoff, control-plane candidates, Publisher reservations, static-site boundary, retention, and stop conditions.

## Repository map

- `crates/decodex-core/` owns domain/application authority contracts, including logical
  Conversation/RuntimeSession/history and deterministic inspectable Context Pack types, plus the XY-1306
  typed `~/.decodex` root, bounded/redacted config profiles, stable server identity,
  content-addressed blobs, and disposable bounded cache foundation.
- `crates/decodex-protocol/` owns the vNext version and loopback-only endpoint contract shared with clients.
- `crates/decodex-postgres/` owns the PostgreSQL product-state adapter: explicit
  connection configuration, embedded immutable migrations, optimistic transactions,
  leases, append-only activity, transactional outbox delivery, inert account/window
  metadata, bounded history pagination, immutable snapshots, blob references, Context Pack
  revisions, and inert rollover/fallback proposals. XY-1307 wires the typed connection data through runtime composition into the
  existing verification/migration boundary; every bootstrap failure remains fail-closed.
- `crates/decodex-codex/` owns typed app-server contracts, exact-build capability profiles, redacted normalized events, fixed and bounded read-only launch/probe behavior, and immutable one-account process supervision. Its live dispatch guard remains fail-closed on XY-1304.
- `crates/decodex-runtime/` owns `decodexd` service assembly and is the only library owner that composes protocol and infrastructure adapters.
- `apps/decodexd/`, `apps/decodex-cli/`, and `apps/decodex-gpui/` are composition roots. The client roots depend only on the protocol crate; the GPUI binary remains a disabled print-and-exit stub while the authority-defined XY-1269 P/K/L/S slices are still unimplemented. Its checked-in diagnostic still says XY-1263 remains failed; that text is stale, and P must align it with the accepted foundation and disabled slice posture before P validation.
- `apps/decodex/` is the frozen v0.2 package. It remains in Git for provenance but is excluded from Cargo workspace membership and must not be used by vNext.
- `apps/radar/` is the Radar auxiliary tool for upstream review queues, release deltas, artifact validation, signal rendering, and bundle generation (`apps/radar/README.md`, `apps/radar/src/lib.rs`).
- `apps/decodex-publisher/` validates and reserves Decodex-owned social artifacts (`apps/decodex-publisher/README.md`, `apps/decodex-publisher/src/lib.rs`).
- `apps/decodex-app/` is a native macOS UI over local Decodex account-pool state and may launch `decodex serve` when no default local server is available (`apps/decodex-app/README.md`).
- `site/` is the static Astro product site; it must not depend on live daemon state (`site/package.json`, `openwiki/integrations/plugins-automations-and-auxiliary-tools.md`).
- `plugins/decodex/` contains the installable Decodex plugin, narrow routing skills, and lifecycle guardrail hooks (`plugins/decodex/.codex-plugin/plugin.json`).
- `automations/decodex/` and `automations/radar/` contain portable Codex App automation source; live machine-local configs are generated from these manifests (`automations/decodex/README.md`, `automations/radar/README.md`).
- `scripts/` contains repo maintenance helpers including plugin sync and macOS app staging.
- `tests/scripts/test_vnext_architecture.py` enforces the exact vNext dependency graph, client isolation, and exclusion-with-preservation of the legacy package.

## Runtime in one minute

`apps/decodexd` composes the PostgreSQL and Codex adapter boundaries through
`decodex-runtime` and serves the typed V1 protocol at loopback-only
`ws://127.0.0.1:49152/v1/ws`. It opens no repository or Codex process. It attempts only
the explicitly configured PostgreSQL Unix socket and otherwise retains a typed unavailable
adapter. The protocol supports V1.2/V1.1 negotiation, typed command receipt/result and
event envelopes, bounded snapshots/queues/wire text, fixed per-version-capacity in-lifetime
idempotency whose lookup and capacity namespace are bound to the negotiated protocol version (so V1.2
and V1.1 mutation keys cannot consume or poison one another),
publication-epoch-bound cursor resume,
snapshot fallback, stable
server-identity pinning, bounded doctor/status results, and a bounded typed Conversation-history
query. The `decodex` and GPUI roots
compile against `decodex-protocol` only. `decodex status` and `decodex doctor` are active
API-only V1.2 diagnostic clients; GPUI still reports its disabled state.

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
source bodies matching the canonical embedded migrations. The twenty-nine expected safety/state/retention
triggers must also remain enabled, correctly shaped, and bound to their canonical functions; no
additional user trigger, rule, policy, RLS mode, or noncanonical expression dependency may add an
indirect execution path on a runtime relation. One canonical PostgreSQL 18 schema manifest also
attests every shipped relation/column, default, constraint, index, enum label, and internal
constraint-trigger binding together with each stable catalog dependency identity. It includes
foreign keys whose Decodex relation is either the child or
the referenced parent, so external cascades and internally generated execution paths fail closed.
Extension authority follows `pg_depend` membership,
not extension schema, so a runtime-controlled extension cannot own or drop a Decodex member.
`public.refinery_schema_history` is always schema-qualified and must have table SELECT only. Its
ordered versions, names, and checksums must exactly equal the embedded migration inventory;
missing SELECT is incompatible, while ownership, SET-reachable authority, table/column grant
options, writes, and table DDL privileges are unsafe. All canonical database functions have an exact
function-local `pg_catalog, decodex` search path. Exactly three narrowly scoped functions are
security definers: bounded cursor issuance inserts opaque issued-only continuations, bounded snapshot
pruning removes expired cursor/version chains under serialized caps, and trigger-only history-version
capture appends immutable item versions. Runtime cannot insert cursor rows or execute capture directly.
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

Doctor/status is a V1.2 read-only query served only by `decodexd`. Queries have client observation
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
Unsupported or mutating product CLI operations remain unavailable and belong to later slices, as do
scheduling, account routing, a PostgreSQL installation or administration plane, live Codex dispatch,
an authenticated HTTP artifact path, remote binding, and GPUI product behavior. Authentication and
TLS are disabled; loopback refusal is the enforced network boundary until the later remote-security gate.

## First commands

Use these as discovery and validation entrypoints:

```sh
cargo run -p decodexd
cargo run -p decodex-cli -- status
cargo run -p decodex-cli -- doctor --output json
cargo run -p decodex-gpui
cargo test -p decodex-core --all-targets --all-features
cargo make test-vnext-architecture
cargo make test-vnext-postgres-store
cargo make check
```

`decodexd` starts the loopback protocol service and runs until stopped. The CLI selects
the configured active profile by default; `--profile NAME` selects an explicit declared
profile and `--root PATH` selects a typed Decodex root. Human output is the default and
`--output json` emits `decodex/cli-diagnostics/1`. GPUI still reports its disabled state.
For a targeted Rust gate,
prefer
`cargo check --all-features --all-targets --workspace` or
`cargo nextest run --workspace --all-targets --all-features` (`Makefile.toml`,
`openwiki/operations/commands-and-validation.md`).

## Authority and safety rules

- Do not read `.env` files or live secret-bearing config. `decodex.example.toml` is the
  bounded vNext setup model and stores only a PostgreSQL credential environment-variable
  name, never its value.
- Do not route vNext through `apps/decodex`, legacy SQLite, Linear lanes, or the legacy operator transport.
- Use `decodex commit` and `decodex land` for Decodex-owned commit/landing authority; the installable plugin hook blocks raw `git commit` and `gh pr merge` inside Decodex scope (`plugins/decodex/scripts/decodex_lifecycle_hook`).
- PostgreSQL is the vNext product-state authority when explicitly configured; unavailable is the only supported service state otherwise, with no fallback authority.
- For project knowledge work, update OpenWiki directly and keep it aligned with source, tests, and manifests.

## Recent development context

XY-1265 established compile-time ownership and composition. XY-1266 established the
loopback protocol foundation; XY-1270 implements the bounded Codex adapter foundation
without live dispatch. XY-1267 established PostgreSQL-backed product state and durable
transactions. XY-1306 established the typed `~/.decodex` path/config/blob/cache child of
XY-1268; XY-1307 supplied daemon bootstrap/doctor; XY-1308 supplies the API-only CLI and
end-to-end diagnostic matrix.
Account routing, remote security, HTTP artifacts, and GPUI product work remain with their
later owners and gates.
