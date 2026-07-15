# Decodex vNext Gate Manifest

Status: normative sequencing and acceptance boundary.

Owner: [vNext authority decision](../decisions/vnext-authority.md). Contract:
[vNext authority contract](vnext-authority.md).

## Sequencing rules

XY-1260 establishes authority only. It does not implement PostgreSQL, app-server/Codex
adapters, GPUI, protocol, runtime services, or migration. No later milestone may begin by
reinterpreting a superseded Lane Authority v2 C1-C7 checkpoint. Each gate must record the
exact source revision, command/test evidence, contradictions, and accepted outcome before
its dependent implementation uses the result.

## Downstream ownership

| Range | Accepted downstream ownership |
| --- | --- |
| [XY-1261](https://linear.app/hack-ink/issue/XY-1261)-[XY-1264](https://linear.app/hack-ink/issue/XY-1264), with live continuation moved to [XY-1304](https://linear.app/hack-ink/issue/XY-1304) | v0.2 freeze and PostgreSQL/blob/cache proof are accepted; the XY-1262 foundation is accepted, live continuation remains failed in XY-1304, and GPUI remains failed on the separate XY-1263 accessibility gate. |
| [XY-1265](https://linear.app/hack-ink/issue/XY-1265)-[XY-1269](https://linear.app/hack-ink/issue/XY-1269) | Workspace ownership boundaries, `decodexd` protocol, PostgreSQL persistence, `~/.decodex`/API-only CLI, and GPUI shell/cache. |
| [XY-1270](https://linear.app/hack-ink/issue/XY-1270)-[XY-1276](https://linear.app/hack-ink/issue/XY-1276), plus [XY-1304](https://linear.app/hack-ink/issue/XY-1304) | Typed app-server, Conversation/RuntimeSession/history, shared-home, vault/runner-binding, quota-calculation, and profile/readiness foundations; XY-1304 separately owns live routing enablement and blocks the Quick Task slice. |
| [XY-1277](https://linear.app/hack-ink/issue/XY-1277)-[XY-1286](https://linear.app/hack-ink/issue/XY-1286) | Projects/Advisor/Lead, context, messages/collaboration, decision queues, Programs/Objectives, WorkItems, ManagedRuns, repository services, Task-owned independent review/repair/landing, and Project/Program authority policy. |
| [XY-1287](https://linear.app/hack-ink/issue/XY-1287)-[XY-1290](https://linear.app/hack-ink/issue/XY-1290) | Automation definitions/firings, materiality/loop safety, removal of manager agents, and PubFi/SEO/GEO/Radar/Publisher dogfood. |
| [XY-1291](https://linear.app/hack-ink/issue/XY-1291)-[XY-1297](https://linear.app/hack-ink/issue/XY-1297) | GPUI conversations, project/run workspace, graph/timeline, operational surfaces, multi-GB pagination/cache/search, thin menubar, and accessibility/interaction gates. |
| [XY-1298](https://linear.app/hack-ink/issue/XY-1298)-[XY-1303](https://linear.app/hack-ink/issue/XY-1303) | Observability/retention, authenticated remote security/backups, E2E and fault injection, performance budgets, empty-state legacy cutover/removal, and package/dogfood/release reconciliation. |

Each issue is accepted only for its own stated scope and blocked-by relations. The ranges
are navigation, not permission to collapse tasks or skip gates. Linear relations are
planning metadata, not product/runtime identity.

## Required architecture and implementation gates

1. GPUI exact-revision build/package/test/accessibility spike (XY-1263).
2. The accepted XY-1262 foundation gate: shared-home/process isolation,
   creation-receipt ownership, negotiated app-server contracts, supported
   exact-ID/list/read/archive behavior, lossy-read/divergence policy, native run-local
   collaboration normalization, process-scoped authentication/redaction, read-only
   plugin/skill inventory, and pure duration-typed quota policy.
3. The separate failed XY-1262 live account-routing enablement gate (XY-1304): natural
   quota depletion, durable exclusion before fallback, crash-safe exactly-one
   continuation, real resume-denied Context-Pack fallback, all-depleted wait/wakeup
   readback, side-effect reconciliation, and supported Codex Desktop title discovery.
4. Empty PostgreSQL bootstrap, backup/rollback, and concurrent lease/outbox tests
   (XY-1264). The scoped proof choices, measurements, recovery procedure, and downstream
   boundary are recorded in [vNext storage feasibility evidence](../evidence/vnext-storage-feasibility.md).
5. WebSocket reconnect, cursor resume, command idempotency, and current/previous-minor
   compatibility tests (XY-1266 and regression owner XY-1300).
6. Large-history pagination/cache test proving multi-GB history is never eagerly loaded
   (XY-1263, implementation XY-1295, and regression owner XY-1300).
7. ManagedRun restart and side-effect reconciliation fault injection, including the
    Task-owned independent review loop and typed reviewer wait/failure states (XY-1283,
    XY-1285, and regression owner XY-1300).
8. Real Program/Automation/Lead/Task/Reviewer dogfood, using PubFi or equivalent
    (XY-1290 and release dogfood owner XY-1303).
9. Remote binding stays disabled until authentication, TLS, authorization, and
    redaction gates pass (XY-1299).

### XY-1262 foundation acceptance

Manager accepted this split on 2026-07-13. The decision provenance is merged PR #1098 at
`687605583817eca32cbdfb1107f3ee18d3106cea`; that proposal becomes authority only through
this independently reviewed amendment. Repository authority is normative and Linear is
planning metadata.

The [Codex runtime proof](../evidence/vnext-codex-runtime-proof.md), including the merged
reconciliation receipt, accepts the foundation gate for exactly these observed or pure
evidence boundaries:

- one shared normal `~/.codex`, with each app-server process bound to one account and no
  credential switching under a live process;
- Decodex ownership only from a durable creation receipt, never from arbitrary Codex
  history;
- generated typed schema plus negotiated live method results keyed by the Codex build;
- persistent exact-ID, filtered-list, read, explicit archive, and restart readback;
- lossy `thread/read` handling and a fail-closed ManagedRun `diverged` policy;
- native collaboration/subagent events normalized only as run-local actors;
- process-scoped authentication, redaction, and integrity-preserving read-only
  plugin/skill inventory; and
- pure quota decisions keyed by duration 300/10080, including unknown, stale, reversed,
  and all-depleted synthetic cases.

Healthy-account same-thread continuation and a manually started Context-Pack session are
mechanism evidence only. They do not accept automatic cross-account routing or fallback.
No global Codex title-search contract is accepted: exact-ID and filtered-list ownership
readback are the supported boundary.

### Permitted foundation work

Permission is issue-scoped and does not bypass each issue's own dependencies:

| Issue | Permitted boundary |
| --- | --- |
| XY-1265 | Workspace ownership cutover and composition roots; no compatibility facade. |
| XY-1266 | Loopback protocol, idempotency, reconnect, backpressure, and non-loopback refusal. |
| XY-1267 | PostgreSQL transactions, leases, outbox, and inert account/window schemas. |
| XY-1268 | Owned `~/.decodex` paths and API-only diagnostics that report unavailable/unknown honestly. |
| XY-1269 | No work while the separate GPUI accessibility gate in XY-1263 is failed; this is its sole feasibility blocker. |
| XY-1270 | Generated typed app-server contracts, live capability negotiation, redaction, and one-account-per-process supervision; no task scheduling or account choice. |
| XY-1271 | Conversation/RuntimeSession/history and inspectable Context-Pack persistence; no automatic rollover, assignment, or fallback dispatch. |
| XY-1272 | Transactional creation mappings, exact-ID/list reconciliation, explicit retention, and the ManagedRun `diverged` stop transition; no global title-search claim. |
| XY-1273 | Credential-vault metadata and immutable runner/account binding; no sticky or policy assignment. |
| XY-1274 | Pure duration-typed quota/wake calculations and durable exclusion transaction tests using synthetic fixtures only; no live exclusion, fallback assignment, or wake scheduling. |
| XY-1275 | User-owned profile snapshots and read-only plugin readiness audits; no installation or routing decision. |

### Post-V4 authority order and writer map

This amendment is based on landed `main` at
`2f5e637a2c65ee88c1946df22d5c3649f664f467`. That tree contains the XY-1273
`V4__account_readiness.sql` migration and no later migration. An unlanded migration
name or number is not a reservation. Rebase each schema owner onto the then-current
ledger and allocate its migration version only in the exact candidate that is ready to
land.

Repository decisions, specifications, migrations, source contracts, and tests are the
normative authority. Linear issue descriptions and relations are executable planning
metadata and must be kept aligned with that authority; they cannot amend it. The
post-V4 serial schema and semantic order is:

1. landed XY-1273/V4 account readiness and immutable runner binding;
2. XY-1315 inert canonical Project and Agent identity, including one global Advisor
   identity and one canonical Lead identity per active Project, without live
   Conversation or Codex behavior;
3. XY-1316 minimal Project-owned, versioned Policy identity and immutable accepted
   revisions, without effective policy application;
4. repaired XY-1281 Program and finite Objective persistence, importing the canonical
   Project, Agent, and exact Policy revision authorities; and
5. XY-1282 WorkItem identity and persistence plus its normalized, project-scoped
   Objective-WorkItem association.

Project, Agent, and Policy identity therefore precede Program persistence. WorkItem,
not Program or Objective, owns the normalized association. Both sides must be
foreign-key backed, must resolve to the same Project, and must reject cross-Project
links. A `uuid[]`, JSON array, unconstrained UUID, placeholder identity, or equivalent
denormalized shortcut is not authorized.

The executable dependency edges that mirror this order are XY-1273 -> XY-1314;
XY-1314 -> XY-1315, XY-1317, and XY-1318; XY-1315 -> XY-1316; XY-1315 and XY-1316 ->
XY-1281; the additional direct edge XY-1273 -> XY-1281; and XY-1315, XY-1316, and
XY-1281 -> XY-1282. The direct XY-1273 block is recorded independently rather than
treated as transitively satisfied through XY-1314. The repository order governs if
planning metadata drifts. No fixed Domain Agent hierarchy is part of this order:
additional agents remain a later policy/workload decision.

#### Conflict domains and integration order

PostgreSQL migrations, the embedded migration inventory, schema/authority digest,
aggregate PostgreSQL test harness, and migration evidence are one non-commutative
writer domain. Exactly one schema owner may be active: XY-1315, then XY-1316, then the
repaired XY-1281 persistence slice, then the XY-1282 WorkItem/relation persistence
slice. Pure application work may run beside that lane only when it does not edit or
claim those surfaces.

XY-1317's intended conflict domain is the Codex exact-ID/list/read/archive contract.
On this landed tree, however, typed thread identity/projections are in
`crates/decodex-codex/src/protocol.rs`, while request execution and the only scripted
fake app-server fixture are in
`crates/decodex-runtime/src/account_launch/process.rs`,
`crates/decodex-runtime/src/account_launch/protocol.rs`, and
`crates/decodex-runtime/tests/fixtures/fake_app_server.py`. Its current
"Codex-adapter-only" brief does not authorize that runtime-owned production and fixture
surface. XY-1317 is therefore not dispatch-ready until its planning metadata is
rebriefed to name the runtime transport/fixture ownership or the acceptance contract is
split so the adapter-only candidate is independently testable. Do not duplicate the
fake server under `decodex-codex` to evade this boundary.

XY-1318 owns only pure, side-effect-free quota value and decision contracts in
`decodex-core`. It does not own PostgreSQL quota rows, exclusion receipts, app-server
rate-limit decoding, account assignment, scheduling, wakeup, continuation, or any
live-routing entry point. The existing
`openwiki/evidence/fixtures/xy-1262-quota-matrix.json` is read-only accepted input for
its tests, not a child-owned evidence file.

The landed-tree path-and-contract ownership map for the post-amendment wave is below.
Every concrete path named in it exists at the pinned snapshot. The map intentionally
does not choose filenames for future Project, Agent, quota, PostgreSQL, or evidence
modules. Each active child must re-derive and freeze its exact additions from the then-
landed tree before dispatch; a proposed new filename has no authority merely because it
fits the directory and contract owner recorded here.

| Surface | XY-1315 Project/Agent identity | XY-1317 exact-ID adapter | XY-1318 pure quota algebra | Serial integration or exclusion |
| --- | --- | --- | --- | --- |
| Production domain source | Sole contract writer for canonical Project/Agent identity, lifecycle, validation, and repository ports added within the existing `crates/decodex-core/src/` owner directory; no account, quota, or Conversation contract. Exact additions require the child pre-dispatch re-derivation above. | Intended existing owner is `crates/decodex-codex/src/protocol.rs`, but no production path is active while the runtime execution gap above holds dispatch. | Sole contract writer for pure duration-typed quota values and policy algebra added within the existing `crates/decodex-core/src/` owner directory; no Project/Agent types or persistence. Exact additions require the child pre-dispatch re-derivation above. | `crates/decodex-core/src/lib.rs` is a shared crate export root: XY-1315 integrates its exports and lands first; only after that landing may rebased XY-1318 become the serial writer for its quota export. Neither child is finally acceptable before its own exact export is integrated and reviewed. |
| PostgreSQL production source | Sole wave writer for Project/Agent persistence within the existing `crates/decodex-postgres/src/` owner directory, including required integration in the existing `lib.rs`, `types.rs`, `authority.rs`, and `migrations.rs`. Exact additions require the child pre-dispatch re-derivation above. | Excluded. | Excluded. | No other active task may edit any `crates/decodex-postgres/src/` file during the XY-1315 schema slice. |
| Migration and schema contract | Sole owner of exactly one new migration under `crates/decodex-postgres/migrations/`, versioned only after rebase onto the landed ledger. | Excluded. | Excluded. | The directory, embedded ledger, runtime grants, schema/authority inventory, clean-install/restore contract, and migration number are one serial domain. |
| Focused tests | Project/Agent core tests stay with the child-selected owning source under `crates/decodex-core/src/`; Project/Agent store cases belong in the existing `crates/decodex-postgres/tests/postgres_store.rs`. | Adapter-owned typed-result tests would belong beside its production owner; live request/response tests currently require the held runtime surface. | Pure table, boundary, fake-clock, overflow, ordering, and property-style tests stay with the child-selected owning source under `crates/decodex-core/src/`. | `crates/decodex-postgres/tests/postgres_store.rs` belongs exclusively to XY-1315 during this wave. Shared architecture acceptance remains serial. |
| Fixtures | Project/Agent SQL/harness data only inside the PostgreSQL aggregate test or its existing harness. | `crates/decodex-runtime/tests/fixtures/fake_app_server.py` is required but not authorized by the present brief; no active writer until rebrief. | Reads, but must not edit, `openwiki/evidence/fixtures/xy-1262-quota-matrix.json`. | No child may repurpose another child's fixture or copy an existing fixture to create a nominally disjoint path. |
| Migration harness | Sole writer for `scripts/vnext/postgres_store_test.py` and any Project/Agent assertions required by the existing PostgreSQL 18 harness. | Excluded. | Excluded. | Harness edits land with the schema candidate, never from a parallel pure-core or adapter branch. |
| Evidence | Sole contract owner for any Project/Agent-specific receipt under the existing `openwiki/evidence/` directory, containing only commands actually run against its exact candidate; no new evidence filename is authorized until the child map is re-derived. Existing XY-1273 evidence remains immutable provenance. | No evidence path is active while dispatch is held. | Test output is validation; it must not rewrite XY-1262 evidence or claim natural-depletion/live acceptance. | Existing files under `openwiki/evidence/`, including `xy-1273-account-runner-binding.md` and `vnext-codex-runtime-proof.md`, are not wave integration scratch space. |
| Contract surface | Canonical Project/Agent IDs, role uniqueness, lifecycle/revision, Project repository/root/default-cwd facts, and inert repository operations. | Exact-ID/list/lossy-read/archive typed facts only; no mapping persistence, divergence, protocol DTO, or live continuation. | Duration-typed 300/10080 observations, fail-closed pure eligibility/exclusion facts, and a hypothetical side-effect-free earliest-ready value. | Cross-domain application composition, runtime WebSocket, protocol, GPUI, and live behavior are excluded from this wave. |
| Crate export roots | `crates/decodex-postgres/src/lib.rs` belongs solely to the active schema writer. | `crates/decodex-codex/src/lib.rs` and `crates/decodex-runtime/src/lib.rs` have no active writer while XY-1317 is held. | Becomes serial writer for its `decodex-core` export only after XY-1315 lands and this child rebases. | `crates/decodex-core/src/lib.rs` follows the XY-1315-then-XY-1318 landing sequence; `crates/decodex-protocol/src/lib.rs` is excluded from the wave. Any required runtime module-root edit must be named by a repaired XY-1317 brief before dispatch. |
| Architecture registry | Serial writer for its Project/Agent guards in the first landing candidate. | No active writer while dispatch is held. | Becomes serial writer for its quota guards only after XY-1315 lands and this child rebases. | `tests/scripts/test_vnext_architecture.py` is a shared registry. Each landing candidate adds only its exact-owner guards, reruns the registry and full check, and receives fresh review before landing. |
| Crate manifests | No dependency edit without separate authority. | No dependency edit without separate authority. | No dependency edit without separate authority. | `crates/decodex-core/Cargo.toml`, `crates/decodex-codex/Cargo.toml`, `crates/decodex-postgres/Cargo.toml`, and `crates/decodex-runtime/Cargo.toml` are excluded unless a separately reviewed serial change authorizes the exact dependency/feature contract. |
| Root build graph | Excluded. | Excluded. | Excluded. | `Cargo.toml`, `Cargo.lock`, and `Makefile.toml` are serial shared integration surfaces. Opportunistic dependency, workspace, task, root-manifest, or lockfile edits are prohibited in the parallel wave. |
| Normative OpenWiki | No child owns final authority edits. | No child owns final authority edits. | No child owns final authority edits. | `openwiki/decisions/vnext-authority.md`, `openwiki/specs/vnext-authority.md`, and this manifest are shared normative authority. A separately authorized serial authority step owns any required semantic amendment; ordinary evidence must not rewrite them. |

Parallel implementation and landing are different gates. The landing order for the safe
subset is XY-1315 first, then XY-1318. XY-1315 integrates its Project/Agent exports and
architecture guards, passes fresh review, and lands as the sole schema writer. XY-1318
then rebases onto that exact landing, integrates its quota export and architecture guard
as the now-serial owner, reruns its focused tests plus canonical validation, receives a
fresh exact-candidate review, and lands second. An excluded or deferred shared file may
be absent during parallel implementation, but the affected child cannot claim final
acceptance or landing readiness until its ordered serial integration is complete. Any
upstream landing that changes a mapped path, crate boundary, schema ledger, fixture
owner, contract, manifest, or test registry invalidates this table for the affected
child and requires the map to be re-derived from the new landed tree before work
resumes.

At this snapshot, the maximum safe concurrent implementation subset is **XY-1315 and
XY-1318**. Their owned production paths and contracts are disjoint once the core export
root and architecture registry are deferred to serial integration. XY-1317 is excluded
from dispatch until its adapter/runtime ownership contradiction is repaired. This is not
permission to land XY-1315 and XY-1318 concurrently or to call either complete before
the shared integration and fresh exact-candidate review.

XY-1304 remains the sole owner of live account-routing enablement. Nothing in this
ordering or writer map enables sticky or policy assignment, quota-driven fallback
assignment, `waiting_usage` scheduling/wakeup, automatic cross-account resume,
automatic Context-Pack fallback, or replay after ambiguous side effects. Every one of
those paths remains hard default-disabled, and unknown or stale quota remains
ineligible, until XY-1304 passes through an independently reviewed repository authority
amendment.

All XY-1270-XY-1275 capabilities must be mechanically inert or default-disabled at their
live boundary. Synthetic fixtures can validate representation, calculation, and
transaction ordering but cannot satisfy the live gate.

### Failed live account-routing enablement gate

The [live gate issue](https://linear.app/hack-ink/issue/XY-1304) remains failed and
fail-closed. Before any gated capability is enabled, an account must become **naturally
depleted**; quota must not be deliberately consumed to manufacture acceptance. A fixed
no-tool marker must then return a typed provider quota failure. Durable readback must
show the submitted turn and unknown side-effect state, followed by the specific
duration-typed 300/10080 account/window exclusion committed and crash-recoverable before
any fallback assignment.

A different fresh eligible account must produce exactly one useful continuation on the
same thread when supported. Otherwise, a real denied/incompatible response must end the
old RuntimeSession and create exactly one Context-Pack RuntimeSession. Injected crashes
at each exclusion, assignment, resume, and fallback boundary must read back exactly one
continuation, correct `waiting_usage` plus the earliest ready time when all accounts are
freshly depleted, and no duplicate tool, worktree, Git, or artifact effect. Normal auth,
account-pool, and installed/enabled plugin state must remain unchanged. Separately, the
retained title must be returned by supported Codex Desktop discovery after normal
indexing before any global visibility claim.

Until all of that evidence passes and a later explicit repository amendment enables the
path, sticky or policy assignment, quota-driven exclusion causing another assignment,
`waiting_usage` scheduling/wakeup, automatic cross-account same-thread resume, automatic
Context-Pack fallback, and replay after an ambiguous or possibly side-effecting outcome
are hard default-disabled. Unknown, missing-duration, stale, or low-confidence quota is
not eligibility evidence and remains fail-closed.

XY-1276 remains blocked by XY-1304. The same direct live-gate block is required for later
issues whose stated acceptance would exercise live routing: XY-1277-XY-1280, XY-1283,
XY-1285, XY-1287, XY-1289-XY-1292, XY-1300, XY-1302, and XY-1303. Their presence in a
later milestone, a synthetic test, or an otherwise completed dependency cannot authorize
managed production routing. Other later foundation or UI work may proceed only when its
own scope can remain inert and its other gates pass; it cannot claim live-routing,
dogfood, cutover, or release acceptance.


## Cutover gate

Cutover may occur only after replacement behavior has accepted tests, XY-1304 has passed
through explicit repository authority, and the v0.2 inventory is frozen. The accepted
procedure stops v0.2, verifies the trusted tag/cold
backup, initializes empty PostgreSQL state, explicitly recreates selected Projects and
Automations, and starts only vNext. It imports no legacy execution history and enables no
dual authority. Removal of old Linear/SQLite/Goal/operator transport follows replacement
proof, not speculative deletion.

The repository-owned XY-1261 receipt is
[the v0.2 freeze receipt](../evidence/v0.2-freeze.md). A destructive-removal task must
verify its exact external readbacks and resolve every recorded stop condition first. In
particular, the receipt records that the legacy SQLite database was already absent before
the freeze; its retirement sentinel is not a database backup, and later work must not
silently treat that acceptance gap as restored evidence.

## Stop conditions

Stop the owning gate on any contradiction with the authority contract, any unproven
authority boundary, credentials entering ordinary PostgreSQL rows, a second mutation
path around `decodexd`, possible side-effect replay without reconciliation, unbounded UI
history loading, or attempted remote binding before security acceptance. Decision-level
falsifiers are listed in the owning decision and require explicit architecture revision.
