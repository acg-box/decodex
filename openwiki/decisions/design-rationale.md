# Design Rationale

Scope: current v0.2 rationale and historical decisions. For the vNext target, the
[vNext authority decision](vnext-authority.md) and
[vNext authority contract](../specs/vnext-authority.md) supersede conflicting product,
runtime, state, identity, transport, migration, and delivery claims on this page.

For factory work after the current local Quick Task foundation, the
[adaptive Program and extension architecture](adaptive-program-extension-architecture.md)
supersedes conflicting product-direction claims on this page. In particular, the new
direction does not require Linear or Program Intake, allows a visible semantic graph,
and gives Decodex durable open-ended Program responsibility. It retains the safety rule
that Signals and MCP calls cannot create authority or prove external effects. Historical
sections below remain provenance for the frozen runtime and must not be read as current
implementation evidence.

This page preserves durable "why" decisions that were previously scattered across historical decision records. It is not runtime authority by itself. Current authority lives in source, contracts, tests, checked-in manifests, and local runtime state; this page explains why those authority boundaries exist and where future agents should verify them.

## Private-artifact retirement rationale

XY-1403 selects the smallest design that meets current obligations. At and after
the [repository effective point](../specs/private-artifact/decision.md#repository-effective-point),
the [private-artifact archive](../specs/private-artifact/README.md) is historical
evidence only. Its model, reducer, persistence, executor, platform, controller,
garbage-collection, delivery, CORE-FREEZE, ACC, preparation, and validation
mechanisms have no current named vNext consumer. Keeping them as future authority
would preserve cost and ambiguity without a product requirement.

The accepted Artifact/BlobStore boundary remains unchanged for ordinary product
evidence. The only retained need is evidence transport for XY-1369 and XY-1370 into
XY-1363. Bounded canonical privacy-safe Git evidence meets that need without a new
service, schema, storage system, runtime route, platform layer, issue, compatibility
path, or product Artifact. A new material requirement that Artifact, BlobStore, and
Git cannot meet must return to an explicit architecture decision. Historical package
text cannot authorize that change.

## Current authority and status

- The runtime is natural-language-first at the operator surface, while Decision Contracts, Execution Programs, Program Intake, leases, attempts, and recovery state remain runtime-owned internals (`apps/decodex/src/loop_contract.rs`, `apps/decodex/src/execution_program.rs`, `apps/decodex/src/program_intake.rs`, `apps/decodex/src/state/sqlite_store/schema.rs`).
- Project autonomy is a control-plane layer, not an open-ended product manager or hidden self-repair loop. Objective, signal, and proposal records are versioned runtime concepts (`apps/decodex/src/autonomy_objective.rs`, `apps/decodex/src/autonomy_signal.rs`, `apps/decodex/src/autonomy_proposal.rs`).
- MCP is the typed capability gateway over existing runtime controls. Capability profiles, stdio/Streamable HTTP transports, resources, prompts, and tools are implemented in `apps/decodex/src/mcp.rs` and `apps/decodex/src/mcp/`.
- Radar and Publisher are auxiliary tooling, not runtime lifecycle authority. Radar validates upstream artifacts and handoffs (`apps/radar/src/lib.rs`, `apps/radar/src/artifact_validation/`); Publisher validates and reserves social artifacts (`apps/decodex-publisher/src/lib.rs`).
- The public site is intentionally static and independent of daemon state (`site/README.md`, `site/package.json`).

Historical basis: this page consolidates former `docs/decisions` records for the natural-language loop runtime, project autonomy control plane, MCP gateway and skill slimming, static public site, Codex upstream Radar redesign, Radar/Control Plane/Publisher split, and bounded Radar local retention.

## Account lifecycle ownership

Account routing, account presentation, and secret storage have different security and
recovery requirements. Decodex therefore uses one small three-owner design: former server store
holds credential-negative product state, the HostCredentialStore holds only secret
bundles, and the `decodexd` Account Service coordinates operations across them.

For Mac dogfood, the HostCredentialStore uses one daemon-owned redb file. The former
Keychain adapter forced normal service startup through an app bundle, development
provisioning profile, access-group entitlement, and wrapper process. Those mechanisms
served the backend choice, not the product boundary. Redb keeps atomic transactions,
exact compare-and-swap, restart recovery, and one-writer exclusion while allowing one
direct signed daemon executable. Keeping credentials out of former server store still limits
exposure through SQL access, dumps, backups, and ordinary product-state tools.

This is a deliberate v1 host-trust trade-off. The vault is owner-only and relies on host
disk encryption; it does not claim protection from root or malicious same-user code. An
extra application encryption layer would also need a separate durable key authority and
rotation design. Decodex does not add that system until a named threat requires it. See
[Credential Vault Cutover Evidence](../evidence/credential-vault-cutover.md).

The first Mac dogfood keeps the smallest usable set: enrollment/import, refresh and
rotation, enable/disable, logout, quota-aware fixed/balanced initial selection, explicit
order, and manual recovery. Full usage/profile/history presentation, ambient `Use in
Codex`, Linux secrets, automatic fallback/wake, and broad matrices remain later final
obligations. This does not preserve `accounts.jsonl` as runtime authority.

A finite per-account operation journal and exact credential compare-and-swap close the
required crash boundary. A generic transaction coordinator, event-sourced account domain,
new process/effect ledger, per-account daemon, or per-run/per-account Codex home would add
lifecycle cost without an accepted obligation.

The shared normal `~/.codex` remains Codex authority for configuration, plugins, rollout
files, and thread visibility. Decodex runner binding is one account per process. An
explicit `Use in Codex` command is ambient-auth projection only, never routing authority,
and is not a MacDogfoodReady prerequisite. See
[Account Lifecycle Authority](../specs/account-lifecycle-authority.md).

## Natural-language loop runtime

Decodex keeps graph semantics backstage. Users and agents should work through ordinary conversation, accepted Decision Contracts, and normal Linear issue lanes instead of editing DAG ids or internal goal state directly.

Why:

- The existing runtime already owns issue eligibility, retained worktrees, tracker writes, validation gates, review handoff, landing, closeout, cleanup, and operator status.
- Loop execution needs dependency, ordering, conflict-domain, drift, and readiness semantics, but exposing those mechanics as the daily workflow would make Decodex harder to use and duplicate Linear lanes.
- Natural language remains the operator interface; the runtime translates accepted direction into structured execution state only after an explicit acceptance boundary.

Current shape:

- Accepted direction becomes a Decision Contract (`apps/decodex/src/loop_contract.rs`).
- Decision Contracts can materialize internal Execution Programs with dependency, conflict-domain, stage, lifecycle, queue intent, and Linear issue mapping data (`apps/decodex/src/execution_program.rs`).
- Program Intake turns accepted work into normal issue lanes rather than replacing tracker workflows (`apps/decodex/src/program_intake.rs`).
- The daemon reconciles active children, idle recovery, post-review orchestration, archive backlog, and due child spawns without exposing internal graph state as user control (`apps/decodex/src/orchestrator/daemon.rs`).

Do not introduce user-facing graph editing, dry-run/apply DAG commands, or direct Codex goal mutation as the ordinary interface unless a new accepted design changes this boundary.

## Project autonomy control plane

Autonomy exists to turn accepted objectives and typed evidence into bounded proposals and normal Decodex execution. It is not a generic backlog manager, memory product, or hidden Decodex-only repair loop.

Why:

- Durable authority must live in Decodex records, not in chat history, memory retrieval, external-agent output, or MCP access.
- Runtime health is useful dogfood input, but autonomy has to be project-general: user feedback, validation failures, review findings, telemetry, spec drift, protocol drift, and metric regressions can all become typed signals when policy allows.
- Proposal generation needs explicit objectives, non-goals, allowed surfaces, evidence, contradictions, validation gates, and acceptance boundaries before execution.

Current shape:

- Objective Contracts represent project-level autonomy authority above individual Decision Contracts (`apps/decodex/src/autonomy_objective.rs`).
- Autonomy signals are read-only evidence with provenance, freshness, confidence, privacy, and source classification (`apps/decodex/src/autonomy_signal.rs`).
- Autonomy proposals are dry-run proposal evidence until accepted through the Decision Contract/Program Intake path (`apps/decodex/src/autonomy_proposal.rs`).
- MCP tools expose autonomy drafting, signal, challenge, proposal, and promotion-request surfaces, but they refuse unsupported authority shortcuts (`apps/decodex/src/mcp.rs`, `apps/decodex/src/mcp/tools.rs`).

Do not let a signal, report, memory hit, review comment, or MCP caller execute work directly. Accepted proposals must still pass through Decision Contract acceptance, Program Intake or issue lanes, validation, review, landing, and closeout.

## MCP gateway and skill slimming

Decodex uses MCP as a small typed capability gateway and keeps installable skills as static routing, authority, and safety entrypoints.

Why:

- Skills are good at trigger routing and policy reminders, but poor at carrying fresh runtime state, large reference bodies, and structured mutation contracts.
- MCP separates resources, prompts, and tools, which maps cleanly to Decodex docs/state, reusable workflows, and authority-checked operations.
- A typed gateway lets agents interoperate without learning SQLite internals, hidden graph ids, worktree conventions, or one-off CLI wrappers.

Current shape:

- Stdio MCP defaults to the `admin` profile for local desktop/CLI use; Streamable HTTP defaults to `observe`, binds to loopback by default, validates origins, manages MCP sessions, and requires bearer authorization for unsafe direct exposure or elevated profiles (`apps/decodex/src/mcp.rs`, `apps/decodex/src/mcp/types.rs`).
- Capability profiles are `observe`, `plan`, `operate`, and `admin`; mutating tools require the appropriate profile and explicit authority inputs (`apps/decodex/src/mcp/types.rs`, `apps/decodex/src/mcp/tools.rs`).
- The tool catalog is deliberately small: observe, plan, intake/autonomy surfaces, lane control, and project control route through existing runtime guards instead of mirroring every CLI command (`apps/decodex/src/mcp/tools.rs`).
Do not make MCP a new source of truth, expose raw private evidence by default, or add broad mutation tools that bypass Decision Contract, lane-control, review, landing, tracker, project-enable, or runtime-state checks.

## Static public site boundary

The public `site/` stays static by default. Runtime orchestration, operator state, tracker writes, app-server integration, account pools, Radar, and Publisher remain outside the public-site runtime boundary.

Why:

- The site is a product surface and app download entry, not a local operator dashboard.
- Static deployment keeps the public surface cacheable and buildable without a live Decodex daemon.
- Runtime changes can evolve in `apps/decodex/` without turning the website into an operational dependency.

Current shape:

- `site/README.md` states that the Astro/TypeScript site must stay independent from live daemon state.
- `site/package.json` exposes only site build/check/dev scripts.
- Operator state and dashboard routes live under the local control plane in `apps/decodex/src/orchestrator/operator_http.rs`.

Do not add login, personalized views, live daemon queries, paid/private access, or public runtime mutation to `site/` without a new accepted decision and an explicit backend/security design.

## Agent-led automation

The five Codex tasks use capable agents for research, diagnosis, planning,
implementation, review, writing, and iteration. Standard GitHub and native Codex task
state replace repository-owned orchestration state.

Why:

- Upstream and editorial decisions need current context and engineering judgment.
- A script that selects work, routes repairs, or decomposes analysis duplicates the
  agent and creates stale state.
- Deterministic code remains valuable at irreversible boundaries such as signed Git
  history, X writes, budget enforcement, and exact readback.

Current shape:

- Maintainer and Reviewer use GitHub PRs, refs, signed `decodex commit` commits, and
  exact `decodex land` merge readback.
- Manager audits the exact-five native portfolio and archives only successful completed
  tasks through native task tools.
- Content Manager records one direct source-backed candidate or no-op.
- Publisher owns xurl identity, daily and monthly limits, uncertain writes, and outcome
  reads.
- Radar remains optional research and static-signal tooling. It has no mutation or
  content-handoff authority.

Do not add a candidate queue, lease, handoff, repair router, content review ceremony,
or migration reader around these agents.

## Radar local cache retention

Radar raw bundles, reviews, impacts, analysis drafts, and ledger records stay in
owner-only bounded local cache. They are disposable working state, not source artifacts
or remote recovery assets.
Publisher social candidates, reservations, posts, outcomes, strategy records, xurl
attempts, and generated media follow the same local-only rule.

Why:

- Continuous Radar must not turn the repository or a remote release store into a
  permanent raw-data warehouse.
- High-frequency evidence has short operational value and can be rebuilt from upstream
  public sources.
- A deterministic local policy makes privacy and disk use observable without a
  historical compatibility path.

Current shape:

- Collection retention is 30 days, 256 files, and 64 MiB per collection.
- Ledger retention is 30 days, 10,000 rows per table, and 64 MiB total. The disposable
  ledger prunes oldest-first. If it cannot meet the byte limit, Radar preserves it and
  fails with `RADAR_LEDGER_OVERSIZE`. Radar reads the bounded SQLite image through the
  fixed cache descriptor, operates on it in memory, and atomically replaces it through
  that descriptor while the cache lock remains held.
- Cache directories use mode `0700`; JSON and SQLite files use mode `0600`.
  Descriptor-relative no-follow traversal rejects symbolic-link ancestors, `..`,
  wrong owners or modes, unexpected hard links, and path replacement. One process
  lock serializes every writer with retention.
- Default daily validation runs retention before it requires current queue and release
  snapshots and includes bounded retention counts. First-run empty-cache validation
  requires explicit `--bootstrap`; any partial generated cache fails closed. Explicit
  validation paths cannot be combined with bootstrap mode.

Do not commit or upload local Radar cache state to GitHub.

## Stop conditions for future changes

Stop and require a new accepted decision, architecture review, or explicit human authority when a change would:

- expose internal Execution Program graph ids, DAG operations, or hidden Codex goal state as the ordinary operator workflow;
- let autonomy execute from signals, reports, memory retrieval, external-agent output, or MCP calls without Decision Contract and Program Intake authority;
- make MCP or skills bypass capability profiles, inspect-first lane-control preconditions, tracker boundaries, review policy, landing policy, project enablement, or private-evidence boundaries;
- make `site/` depend on a live Decodex daemon or add dynamic public capabilities without a backend/security decision;
- let Radar mutate runtime/tracker state directly or let Publisher publish from unaccepted upstream evidence;
- make unbounded Radar cache growth part of normal operation or upload local Radar
  working state.

Runtime stop evidence also exists in source: authority-boundary checks and architecture-recovery events preserve when an automated lane must change strategy, collect enhanced evidence, block landing, or require human decision before continuing (`apps/decodex/src/orchestrator/execution_architecture_recovery.rs`, `apps/decodex/src/orchestrator/types/authority/`, `apps/decodex/src/orchestrator/status/post_review/authority_boundary.rs`).
