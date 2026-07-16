# Decodex vNext Authority Decision

Status: accepted repository authority for vNext.

Tracking issue: [XY-1260](https://linear.app/hack-ink/issue/XY-1260/promote-the-vnext-authority-contract-and-supersede-lane-authority-v2)

Source planning record: [Decodex vNext Design Baseline](https://linear.app/hack-ink/document/decodex-vnext-design-baseline-681f7d8ad284), accepted 2026-07-12 against repository snapshot `5546dcb3b2eb0a8aecf7d6a3b117d2605ea315b8`.

Authority amendment: Manager decision accepted 2026-07-13. The decision accepts the
conceptual XY-1262 foundation/live-enablement split proposed by merged PR #1098 at
`687605583817eca32cbdfb1107f3ee18d3106cea`, but only through this independently
reviewed normative amendment. The merged proposal was evidence, not authority by
itself. Repository authority remains normative; Linear is planning metadata.

XY-1271 storage-boundary amendment accepted 2026-07-14: retain the migration and
daemon-private runtime identities, and treat `decodexd`, that identity, and BlobStore access as one
trusted service boundary. PostgreSQL owns committed metadata/domain/receipt/activity/outbox state;
local CAS owns large bytes, which PostgreSQL alone cannot attest. Blob-backed commands use a
receipt-first, fenced, exact-response saga and dedicated session-level hash plus per-shard
coordination around create-only synchronized CAS publication. Arbitrary/manual use of the private
runtime credential is unsupported and equivalent to daemon compromise.

Readiness authority correction accepted 2026-07-16: Codex 0.144.2 and 0.144.4 expose
no stable passive complete account-owned plugin, skill, and MCP readiness receipt. Passive
inventory is therefore withdrawn from the accepted XY-1262 foundation evidence. Host files,
manifests, configuration, remote catalogs, process binding, and user declarations do not become
observed account readiness. The first-release doctor result remains typed `unknown` for plugins;
`plugin_unready` is inert reserved state. XY-1336 tracks the upstream receipt gap outside the
vNext critical path and neither closes nor blocks XY-1275.

## Decision

Decodex vNext is a rebuild of the agent workspace, not an incremental extension of the
current Linear-lane and SQLite runtime. The normative product and runtime contract is
[the vNext authority contract](../specs/vnext-authority.md); its ordered proof and
implementation boundaries are in [the vNext gate manifest](../specs/vnext-gates.md).

The accepted planning baseline is provenance for this decision. Where the baseline and
these repository documents agree, these repository documents own implementation. A
concrete contradiction must stop the affected milestone for explicit resolution; later
workers must not silently reinterpret either source.

## Decision hierarchy

Within vNext work, authority descends in this order:

1. explicit user direction and checked-in project policy;
2. this decision, the vNext authority contract, and the vNext gate manifest;
3. accepted project policies and versioned domain/protocol contracts created under
   those documents;
4. source, tests, migrations, and operational runbooks implementing an accepted gate;
5. OpenWiki navigation and current-runtime descriptions;
6. Linear plans and research as provenance or candidate changes.

Current source remains authority for current v0.2 behavior but cannot override the
accepted vNext target. Conversely, target documents do not claim that vNext behavior is
already implemented. Git/filesystem and GitHub remain the authorities for repository
content and provider readback respectively; PostgreSQL becomes product-state authority
only when its bootstrap/cutover gate is accepted.

## Supersession

Lane Authority v2 and its C1-C7 program are superseded and frozen. Its decision,
contract, effect registry, gate manifest, checkpoint ledger, fixtures, scripts, review
findings, and incident scenarios remain useful historical evidence. They must not be
continued, repaired, activated, or treated as vNext acceptance gates. In particular,
vNext rejects Lane Authority v2's SQLite authority, Linear issue/lane identity, local
Unix authority ledger, and compatibility migration target.

PR #1092 is likewise historical/incident evidence: freeze or close it, do not merge it,
and do not wholesale cherry-pick its implementation. Reuse a behavior only after the
vNext owner and a focused gate/test make that behavior authoritative.

## Accepted shape

- PostgreSQL owns Decodex product state; a transactional outbox and leases support
  commands and recovery. It is not event sourced.
- A shared normal `~/.codex` owns persistent Codex rollout/thread continuity and Codex
  UI visibility. Decodex maps only threads that it created.
- `decodexd` alone owns scheduling, app-server children, product mutations, repository
  side effects, and adapters. GPUI, the menubar app, CLI, and MCP use the same versioned
  application protocol and are not parallel authority paths.
- Decodex owns Project, stable Advisor/Lead, execution-scoped Task/Reviewer, Program,
  Objective, WorkItem, Conversation, RuntimeSession, ManagedRun, Automation,
  ContextRevision, AgentMessage, Artifact, RoleProfile, and their policies.
- One Project has exactly one stable Lead in V1. The Lead serializes decisions while
  Task and Reviewer execution may be parallel. Advisor is global and advisory only.
- A managed implementation Task owns its implement/review/repair/revalidate loop by
  spawning an independent read-only reviewer subagent. Lead/Manager owns dispatch,
  final acceptance, and merge; Quick Tasks are exempt, and missing/failed review is a
  typed ManagedRun wait and optional WorkItem block rather than reviewed success.
- A Project policy grants bounded unattended authority once; repository, tool, path,
  merge, budget, approval, parallelism, and quiet-period limits remain hard stops.
- Work lands through focused task branches/PRs directly to `main`; there is no long-lived
  vNext branch.

## XY-1262 gate split

The XY-1262 foundation gate is accepted only for the evidence scope enumerated in the
[gate manifest](../specs/vnext-gates.md): shared-home and one-account-per-process
boundaries, creation-receipt ownership, negotiated app-server contracts, supported
exact-ID/list/read/archive operations, lossy-read/divergence policy, native run-local
collaboration normalization, process-scoped authentication/redaction, read-only
integrity evidence, and pure duration-typed quota policy. This acceptance does not
claim global Codex title search, live quota-driven routing, automatic continuation, or
release readiness.

The separate [XY-1262 live account-routing enablement gate](https://linear.app/hack-ink/issue/XY-1304)
remains failed and fail-closed. The bounded foundation work named by the manifest may
proceed after its own dependencies, but no live-routing, managed production execution,
dogfood, cutover, or release path may use the disabled capabilities until that later gate
passes through another explicit repository authority amendment. Unknown or stale quota
facts never establish eligibility.

## Rejected alternatives and falsifiers

Rejected V1 alternatives are enumerated normatively in the contract. The decision may
change only for evidence that cross-account continuation cannot preserve useful
continuity, GPUI cannot meet pinned build/package/accessibility/test gates, PostgreSQL
cannot be operated reliably on the intended host, app-server cannot provide required
ownership/read/list/collaboration behavior, or real dogfood proves one Lead cannot keep
decision latency within policy. Such evidence blocks or revises the owning gate; it does
not authorize a compatibility facade or silent fallback.
