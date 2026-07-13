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
| [XY-1261](https://linear.app/hack-ink/issue/XY-1261)-[XY-1264](https://linear.app/hack-ink/issue/XY-1264) | Freeze v0.2; prove Codex ownership/continuation, pinned GPUI, and PostgreSQL/blob/cache operation. These are the M0 prerequisites. |
| [XY-1265](https://linear.app/hack-ink/issue/XY-1265)-[XY-1269](https://linear.app/hack-ink/issue/XY-1269) | Workspace ownership boundaries, `decodexd` protocol, PostgreSQL persistence, `~/.decodex`/API-only CLI, and GPUI shell/cache. |
| [XY-1270](https://linear.app/hack-ink/issue/XY-1270)-[XY-1276](https://linear.app/hack-ink/issue/XY-1276) | Typed app-server adapter; Conversations/RuntimeSessions/history; shared-home reconciliation; account vault/runner pool; typed quota failover; user profiles/plugin readiness; Quick Task slice. |
| [XY-1277](https://linear.app/hack-ink/issue/XY-1277)-[XY-1286](https://linear.app/hack-ink/issue/XY-1286) | Projects/Advisor/Lead, context, messages/collaboration, decision queues, Programs/Objectives, WorkItems, ManagedRuns, repository services, Task-owned independent review/repair/landing, and Project/Program authority policy. |
| [XY-1287](https://linear.app/hack-ink/issue/XY-1287)-[XY-1290](https://linear.app/hack-ink/issue/XY-1290) | Automation definitions/firings, materiality/loop safety, removal of manager agents, and PubFi/SEO/GEO/Radar/Publisher dogfood. |
| [XY-1291](https://linear.app/hack-ink/issue/XY-1291)-[XY-1297](https://linear.app/hack-ink/issue/XY-1297) | GPUI conversations, project/run workspace, graph/timeline, operational surfaces, multi-GB pagination/cache/search, thin menubar, and accessibility/interaction gates. |
| [XY-1298](https://linear.app/hack-ink/issue/XY-1298)-[XY-1303](https://linear.app/hack-ink/issue/XY-1303) | Observability/retention, authenticated remote security/backups, E2E and fault injection, performance budgets, empty-state legacy cutover/removal, and package/dogfood/release reconciliation. |

Each issue is accepted only for its own stated scope and blocked-by relations. The ranges
are navigation, not permission to collapse tasks or skip gates. Linear relations are
planning metadata, not product/runtime identity.

## Required architecture and implementation gates

1. GPUI exact-revision build/package/test/accessibility spike (XY-1263).
2. Shared-home persistent thread visibility in Codex (XY-1262).
3. Ownership isolation between Codex-created and Decodex-created threads (XY-1262).
4. Real two-account continuation after quota failure, including crash points (XY-1262).
5. Typed 5h/7d quota matrix with unknown, stale, and reversed windows (XY-1262).
6. App-server capability/schema negotiation, thread read/list, and native collaboration
   behavior (XY-1262).
7. Empty PostgreSQL bootstrap, backup/rollback, and concurrent lease/outbox tests
   (XY-1264). The scoped proof choices, measurements, recovery procedure, and downstream
   boundary are recorded in [vNext storage feasibility evidence](../evidence/vnext-storage-feasibility.md).
8. WebSocket reconnect, cursor resume, command idempotency, and current/previous-minor
   compatibility tests (XY-1266 and regression owner XY-1300).
9. Large-history pagination/cache test proving multi-GB history is never eagerly loaded
   (XY-1263, implementation XY-1295, and regression owner XY-1300).
10. ManagedRun restart and side-effect reconciliation fault injection, including the
    Task-owned independent review loop and typed reviewer wait/failure states (XY-1283,
    XY-1285, and regression owner XY-1300).
11. Real Program/Automation/Lead/Task/Reviewer dogfood, using PubFi or equivalent
    (XY-1290 and release dogfood owner XY-1303).
12. Remote binding stays disabled until authentication, TLS, authorization, and
    redaction gates pass (XY-1299).

### XY-1262 evidence status

The [Codex runtime proof](../evidence/vnext-codex-runtime-proof.md) at
`f9d6c4e70198e94e5b9461b8cac7518ae14d41ef` supplies partial evidence for shared-home
persistence, creation-receipt ownership isolation, exact-ID Codex Desktop readback,
healthy-account same-thread continuation, Context-Pack mechanics, explicit archive
readback, live schema negotiation, native run-local collaboration shape, and the
duration-typed quota decision table.

The full visibility gate remains failed: the persistent probe thread was returned by
app-server `thread/list(searchTerm=...)` and was readable by exact ID through Codex
Desktop, but app-server `thread/search` and Codex Desktop global title query did not
return it during the experiment. Dependent implementation must not equate rollout/list
visibility with sidebar/global-search discovery. A desktop restart/indexing proof that
finds the retained title is required before this sub-gate is accepted.

The generated schema also advertised paginated thread history while live
`thread/start(historyMode=paginated)` returned JSON-RPC `-32601`; adapter capability is
therefore a negotiated live result keyed by Codex build, not a schema-only inference.
The full XY-1262 gate also remains failed because the available accounts were not
depleted: no real quota-failure exclusion/failover was observed, and the Context-Pack
fallback followed an authentication rejection rather than a same-thread resume denial.

## Cutover gate

Cutover may occur only after replacement behavior has accepted tests and the v0.2
inventory is frozen. The accepted procedure stops v0.2, verifies the trusted tag/cold
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
