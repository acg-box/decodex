# Decodex vNext Authority Contract

Status: normative target contract; implementation is gate-controlled.

Owner: [vNext authority decision](../decisions/vnext-authority.md). Gates:
[vNext gate manifest](vnext-gates.md).

## Product entities

| Entity | Contract |
| --- | --- |
| Project | Decodex-owned repository identity and policy; `active`, `paused`, or `archived`. |
| Agent | Stable global Advisor or exactly one stable Lead per Project; `active`, `paused`, or `retired`. |
| RoleProfile | Versioned user choice of model, reasoning effort, service tier, and instructions for `advisor`, `lead`, `task`, or `reviewer`. |
| Program | Open-ended responsibility/context; `active`, `needs_attention`, `blocked`, `paused`, or `retired`. It is not an agent. |
| Objective | Finite outcome in a Program or Project; `proposed`, `active`, `blocked`, `achieved`, or `abandoned`. |
| WorkItem | Concrete board item/execution request; `inbox`, `planned`, `ready`, `running`, `review`, `blocked`, `done`, or `canceled`. |
| Conversation | Durable logical dialogue presented by Decodex; `open` or `archived`. |
| RuntimeSession | Codex thread segment bound to an account, process, and immutable RoleProfile snapshot; `starting`, `active`, `ended`, or `diverged`. |
| ManagedRun | Controlled WorkItem execution with independent lifecycle, phase, and wait reason as defined below. |
| Automation | Deterministic trigger targeting a Program, WorkItem, Advisor, or Lead; `enabled`, `paused`, or `retired`. It is not an agent. |
| ContextRevision | Immutable, inspectable, provenance-linked long-term context snapshot. |
| AgentMessage | Durable sender/recipient envelope with correlation, causation, dedupe, artifact, response, and loop-budget fields; `pending`, `delivered`, `acknowledged`, or `expired`. |
| Artifact | Content-addressed large output/evidence with PostgreSQL metadata; `active`, `expired`, or `deleted`. |

There is no Domain Agent, automatic multi-Lead topology, arbitrary durable role, or Goal
product entity in V1. Task and Reviewer agents are execution-scoped. Codex-native
subagents are run-local runtime actors normalized into the same activity/message graph;
durable cross-run and cross-project routing belongs to Decodex.

Program owns an open-ended Project responsibility, its active canonical Lead owner, an
exact accepted Project Policy revision, review cadence, and bounded provenance-bearing
metrics/signals. Program context compilation is a pure deterministic data operation: it
may preserve ordinary text that mentions a conversation or thread, but it has no typed
Conversation, RuntimeSession, thread, or agent-creation field and performs no such side
effect. Objective owns a finite Project outcome, optional same-Project Program relation,
criteria, target horizon, lifecycle, and optimistic revision. `achieved` is available only
through one immutable Objective-level acceptance-and-validation record bound to the exact
Objective, Project, prior Objective revision, canonical accepting/validating Agent authority,
provenance, and chronology. That record establishes Objective outcome acceptance only; it
does not claim WorkItem or ManagedRun success. Objective abandonment remains independent.
ManagedRun may reach successful terminal completion only from explicit authoritative
WorkItem acceptance and validation. Objective achievement or evidence and any external
Codex Goal state cannot establish WorkItem acceptance or ManagedRun success.
The evidence persists the exact prior Objective `updated_at`; acceptance cannot predate that
revision, including through direct SQL. Program and Objective mutation receipts are scoped
from the caller's canonical Project authority and PostgreSQL verifies the stored Project
match, so not-found replay remains deterministic even if the identity is created later.

## Interaction and work

Advisor is the global default, owns consultation and cross-project recommendations, and
cannot modify project code or call project write tools. A Project Lead owns project
context, user decisions, WorkItems, execution-mode selection, dispatch, and result
acceptance through one serial decision queue. Task and Reviewer agents may execute in
parallel. Reviewer output never mutates code; for a managed implementation task, the
owning Task thread performs accepted repairs under the same WorkItem. Separately
dispatched non-implementation review remains an execution-scoped Reviewer run.

For a managed independent implementation task, the Task thread owns its inner quality
loop: implement, spawn an independent read-only reviewer subagent, evaluate every
finding, repair valid findings, revalidate, and hand the result back to the Lead/Manager.
The Lead/Manager owns dispatch, final acceptance, and merge. Quick Tasks are exempt. A
missing or failed reviewer cannot produce a successful reviewed run; the ManagedRun
remains `waiting` and the WorkItem may become `blocked`, with a typed
`reviewer_unavailable` or `reviewer_failed` wait reason.

A Quick Task is an ordinary multi-turn Codex conversation with no WorkItem, ManagedRun,
reviewer, PR, harness, or Goal. A ManagedRun separates:

- lifecycle: `queued`, `active`, `waiting`, `terminal`;
- phase: `prepare`, `execute`, `validate`, `review`, `repair`, `land`, `close`;
- wait reason: `usage`, `auth`, `plugin`, `dependency`, `approval`, `user`, `external`,
  `reviewer_unavailable`, `reviewer_failed`.

Project/Program policy is versioned authority over allowed repositories, tools, paths, merge
behavior, parallelism, budgets, approvals, and quiet periods. Commands use expected
revisions and idempotency keys. Side effects require receipts and authoritative readback;
an outcome that may already have caused side effects is reconciled, never blindly
replayed.

## Runtime and state authority

| State/surface | Authority |
| --- | --- |
| Projects, agents, policies, Programs, Objectives, WorkItems, ManagedRuns, Automations, profiles, context, messages, mappings, and UI-visible conversation/activity projections | PostgreSQL domain tables with optimistic revisions, leases, append-only activity projection, and transactional outbox |
| Codex thread continuation and Codex UI visibility | persistent Codex rollout under the shared normal `~/.codex` |
| Repository files and worktrees | Git/filesystem on the `decodexd` host |
| PR/check/merge readback | GitHub |
| Large tool output and evidence bytes | content-addressed local blob store, with PostgreSQL metadata |
| GPUI local state | bounded disposable cache only; SQLite is permitted only here |
| Credentials | host credential-vault boundary; PostgreSQL stores account metadata/health, never ordinary credential rows |
| v0.2 state | cold backup/tag and historical evidence only; vNext never reads it as runtime input |

PostgreSQL is not event sourced and no graph database is used. Stable IDs plus correlated
activity derive graph/timeline projections. `decodexd` is the sole product scheduler,
app-server child owner, mutation coordinator, and repository-side-effect owner. GPUI,
SwiftUI menubar, CLI, and MCP are clients/adapters over common application services; they
never read PostgreSQL, rollout files, blobs, or repositories directly. V1 is single-host
and has no worker registry or distributed mesh. Remote UI may be added only through the
protocol security gate.

`decodexd`, its daemon-private PostgreSQL runtime identity, and its BlobStore access form one
trusted service boundary. PostgreSQL owns committed metadata, domain state, command receipts,
activity, and outbox records; local content-addressed storage owns large bytes. PostgreSQL alone
does not attest external bytes, and arbitrary/manual use of the daemon credential is unsupported
and equivalent to daemon compromise. Blob-backed commands use a durable receipt-first saga: a
committed pending receipt binds protocol, operation, project/scope/entity, request digest, expected
revision, and payload hashes/lengths before publication; sorted session hash locks and per-shard
admission serialize create-only verified publication; transaction B atomically registers
metadata/domain references/evidence, stores the exact response bytes, and completes the fenced
receipt. Exact replay returns those bytes; conflicting reuse fails before effects.

## Conversation, context, and communication

Every meaningful Decodex-created thread uses `ephemeral=false`, the shared normal
`~/.codex`, the repository `cwd`, and a title/provenance marker retained for supported
exact-ID and filtered-list ownership readback. Advisor and Lead threads are never
auto-archived. Task/Reviewer threads remain discoverable through that supported boundary
and are archived only by explicit retention policy; probes may be ephemeral. Decodex
never imports Codex-created threads and persists mappings only for Decodex-created
threads. No global Codex or Codex Desktop title-search/indexing contract is claimed until
the separate live enablement gate records the required supported discovery readback.

A logical Conversation may span RuntimeSessions when size, resume latency, compatibility,
or account failure requires it. Each mapping records conversation, session, Codex thread,
account, profile snapshot, and last known turn. Decodex persists normalized visible
messages/items for UI and remote access and offloads large payloads to blobs. Issued history cursors
bind both membership high-water and an append-only immutable item-version sequence, so later
streaming mutation cannot change a page or replay. Cursor chains are opaque, Conversation-bound,
fixed-page-size, one-hour expiring, and bounded to 512 rows per Conversation and 4,096 globally;
bounded pruning retains versions required by active cursors or exact receipt replay. Successful
reads verify every direct and transitive referenced blob before returning typed metadata.
The daemon preserves a canonical media type for inline and offloaded entries and exposes only a
flat credential-negative metadata map: at most 32 fields, 64-byte keys, and boolean or 256-byte
string values. Core owns this typed representation and every Rust boundary reuses it. Keys whose
ASCII-alphanumeric normalized form ends in a credential-bearing suffix are rejected, as are concrete
authorization schemes, known token/key formats, credential assignments, embedded URL passwords, and
private-key headers. Ordinary prose containing words such as `secret`, `token`, or `session` is not
credential material. PostgreSQL enforces the equivalent closed predicate. Nested/raw app-server JSON
and unsupported, oversized, or credential-shaped forms are rejected.
`thread/read(includeTurns=true)` is a lossy reconciliation source. External Codex activity
may be provenance-imported for ordinary Quick/Advisor/Lead conversations; on an active
ManagedRun it marks the session `diverged` and blocks side effects until tool/repository
readback reconciles them.

Long-term context consists of immutable Project, Advisor, and Program revisions. Project
context records decisions, constraints, repository facts, active Programs/Objectives,
unresolved risks, and accepted handoffs. Advisor briefs compact cross-project status and
risk. Program context records metrics/signals, recent decisions, quiet periods, and next
review. A Context Pack contains the current revision, recent raw window, relevant
artifacts, and repository instructions/OpenWiki. Summaries never silently replace
sources; users can inspect pinned memory and provenance. V1 uses structured PostgreSQL
queries and full-text search, not vectors.

AgentMessage carries logical endpoints, project, correlation and causal parent, dedupe
key, artifact refs, requested response, hop count, and response budget. Deterministic
dedupe, budgets, quiet periods, and causal chains prevent loops. Automation results cannot
recursively wake themselves without a new material signal.
Agents communicate directly only when capability and Project policy permit. Stable
cross-run communication is delivered by Decodex as turns to recipient Conversations.

## Account continuity and profiles

Each app-server process is bound to one account. Shared `~/.codex` supplies configuration
and plugins; per-process credentials are never switched under a live runner. Account
state is `unavailable`, `available`, `depleted`, `unknown`, `auth_failed`,
`plugin_unready`, or `disabled`. Each quota window stores its class/duration, remaining amount, reset time,
observation time, and confidence; 5-hour and 7-day windows are never inferred from
positional primary/secondary ordering.

The dormant manual account observation path checks an exact PostgreSQL account revision in the
`available` state before mechanics and checks the same predicate again after cleanup. Each check
releases its row and pooled client before arbitrary caller, vault, or process work. The result is a
post-cleanup non-live observation: readiness is not claimed to remain true while the child runs, and
any final stale, non-ready, or unavailable observation fails closed. A blocked synchronous vault can
retain local mechanical capacity, but never a PostgreSQL transaction, row lock, or client checkout.
Runtime owns the sole sibling-adapter composition and all child/vault/protocol mechanics in private,
non-reexported modules. Codex remains a pure capability/schema adapter and does not depend on
PostgreSQL. The private concrete permit moves through every
sequential preflight and the final child, returning to the attempt only after confirmed group death
and child reaping. Uncertain cleanup installs it in a hard-capped fair quarantine and ends the launch
attempt without freeing capacity or synchronously entering an unbounded loop. A stuck group cannot
starve later cleanup. Capacity exists only after its persistent cleanup owner starts successfully;
per-iteration unwind and in-flight-job guards retain ownership across panic, poison recovery, and
wakeup. Cleanup progress never depends on a later launch or external maintenance call. The janitor
belongs to the live capacity lifecycle rather than process-global static ownership: a weak registry
preserves one live daemon authority, and a finite coordinator stops and joins the worker after the
last capacity, permit, and cleanup job is gone. Each permit reserves
one fixed cleanup slot before spawn, so admission has no shared-lock deadline and a 65th group is
rejected before it exists. Cargo metadata proves the current
workspace production dependency graph and absence of synthetic features on normal edges; compile-fail
contracts prove the launcher and capacity authority are not crate API. Neither proves call provenance,
the absence of future wrappers, or Rust friend visibility against arbitrary new downstream
dependencies. Daemon/host crash can orphan OS descendants; restart reconstructs neither assignment
nor launch authority and requires fresh exact PostgreSQL observations.

Routing honors an available sticky Advisor/Lead account, otherwise chooses an available
compatible account by user policy and quota facts. Every known depleted window excludes
an account until reset; unknown is distinct and receives bounded probe/backoff. When all
accounts are depleted, persist `waiting_usage` and the earliest ready time. Persist the
specific account/window exclusion before rate-limit failover.

Cross-account resume of the same Codex thread is allowed only after the two-account E2E
gate. Otherwise start a new RuntimeSession from a Context Pack while preserving the
Conversation and ManagedRun. Never replay a possibly side-effecting turn without receipt,
worktree/Git, and artifact reconciliation.

Those paragraphs define the target behavior, not current enablement. Until the separate
[XY-1262 live account-routing enablement gate](https://linear.app/hack-ink/issue/XY-1304)
passes and repository authority explicitly enables it, all of the following are hard
default-disabled in every production, dogfood, cutover, and release configuration:

- sticky-account assignment and policy-based account assignment;
- a quota-driven exclusion causing selection or assignment of another account;
- `waiting_usage` scheduling or wakeup;
- automatic same-thread continuation on another account;
- automatic creation or dispatch of a Context-Pack fallback RuntimeSession; and
- replay of a turn after an ambiguous outcome or any possible side effect.

Foundation code may represent these states, persist inert metadata, calculate pure
decisions, and test transactions with synthetic fixtures only where the gate manifest
permits it. It must not submit a turn, assign a fallback runner, schedule a wake, or
transition a live ManagedRun through these paths. Unknown, missing-duration, stale,
low-confidence, auth-failed, plugin-unready, and disabled account/quota facts never imply
availability. In particular, unknown or stale quota is fail-closed: no assignment and no
automatic fallback is permitted; the unavailable/unknown condition must be surfaced for
human resolution or bounded observation.

Users exclusively select the four global RoleProfiles. Runtime cannot alter model,
reasoning, or service tier. Each RuntimeSession snapshots its profile. Decodex keeps a
desired plugin manifest and audits account inventories; V1 reports readiness differences
and guides supported login/install/OAuth work rather than claiming file replacement can
install cloud-bound plugins.

## Automation and protocol

An Automation deterministically turns a schedule, webhook, metric, or repository event
into a deduplicated/materiality- and budget-checked delivery to a Program, WorkItem,
Advisor, or Lead inbox. Lead decision may create a WorkItem/ManagedRun. PubFi, Radar, and
Publisher workflows become Programs plus triggers only when explicitly adopted.

One authenticated, versioned WebSocket multiplexes `control/ack/result`,
`conversation/stream`, `project/work`, `run/activity`, `agent/message`,
`automation/firing`, `accounts/health`, and `system/health`. Commands carry client command
ID, idempotency key, and optional expected revision. Events carry protocol major/minor,
server ID, resumable monotonic sequence, entity ID/revision, correlation/causation, and
payload type. Reconnect is snapshot plus cursor-resumed deltas with backpressure. Major
versions match exactly; server supports current and previous minor for UI/server rollout.
Large artifacts use authenticated HTTP, never WebSocket snapshots. Non-loopback binding
remains disabled until authentication, TLS, authorization, and redaction gates pass.

GPUI is the primary workspace and exposes the Advisor inbox; Projects with persistent
Lead Conversations; Quick Tasks; Program/Objective/WorkItem board; Run, review, repair,
and landing state; agent/thread/automation graph and causal timeline; accounts, plugin
readiness, global RoleProfiles, and system health. Users can always talk to Advisor or a
Project Lead, start Quick Tasks, intervene in WorkItems/ManagedRuns, and inspect all
agent/message/automation relationships. SwiftUI is a thin accounts/run-health menubar
client over the restricted protocol. GPUI caches are bounded, disposable,
cursor-paginated, and keyed by server/schema/content hash; project opening never eagerly
loads all history.

## Migration and delivery

Cutover has no availability requirement. Stop v0.2, tag the trusted `main`, and preserve
cold copies of old SQLite/config/automation inventory plus incident scenarios. Start
vNext with empty PostgreSQL product state. Do not import old Codex sessions, SQLite
execution state, Linear lanes, or Codex-created tasks. Recreate Projects and Automations
explicitly from reviewed inventory.

Freeze/close PR #1092 and do not cherry-pick its implementation wholesale. Every task
uses a focused worktree branch and PR directly into `main`; there is no long-lived vNext
branch. Product-incomplete `main` is acceptable during replacement only when each merge
compiles, tests, and states current capability. Remove Linear, SQLite product authority,
Goal, and old operator transport after replacement behavior and gates exist; do not add
dual writes, dual reads, or compatibility facades. Radar, Publisher, and the static site
may remain outside the runtime until explicitly adopted.

## V1 non-goals

- Pi as a second runtime; per-run/per-agent `CODEX_HOME`; Codex Project sync.
- Linear import, projection, identity, lane authority, or compatibility.
- SQLite product authority, dual writes, historical Codex/SQLite execution migration.
- Domain Agents, automatic multi-Lead, arbitrary durable roles, or Goal as general
  planning/review/development state.
- Graph/vector databases, event sourcing, CRDT/DeltaDB worktrees, distributed workers.
- Unauthenticated remote control or runtime-selected model/reasoning/service tier.

## Decision-changing evidence

Only the falsifiers in the owning decision may revise this contract. A failing gate
freezes the affected milestone and records the contradiction; it does not authorize a
silent legacy fallback.
