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
