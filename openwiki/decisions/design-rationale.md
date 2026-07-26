# Design Rationale

Scope: current v0.2 rationale and historical decisions. For the vNext target, the
[vNext authority decision](vnext-authority.md) and
[vNext authority contract](../specs/vnext-authority.md) supersede conflicting product,
runtime, state, identity, transport, migration, and delivery claims on this page.

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

Historical basis: this page consolidates former `docs/decisions` records for the natural-language loop runtime, project autonomy control plane, MCP gateway and skill slimming, static public site, Codex upstream Radar redesign, Radar/Control Plane/Publisher split, and Radar artifact release archives.

## Account lifecycle ownership

Account routing, account presentation, and secret storage have different security and
recovery requirements. Decodex therefore uses one small three-owner design: PostgreSQL
holds credential-negative product state, the HostCredentialStore holds only secret
bundles, and the `decodexd` Account Service coordinates operations across them.

This design preserves the mature v0.2 login, import, refresh, rotation, usage, history,
selection, logout, and explicit `Use in Codex` behavior without preserving its
`accounts.jsonl` runtime authority. A finite operation saga and exact credential-version
compare-and-swap close the required crash boundary. A generic transaction coordinator,
event-sourced account domain, per-account daemon, and per-run or per-account Codex home
would add lifecycle cost without an accepted obligation.

The shared normal `~/.codex` remains Codex authority for configuration, plugins, rollout
files, and thread visibility. Decodex runner binding is one account per process. An
explicit `Use in Codex` command is ambient-auth projection only and never routing
authority. See [Account Lifecycle Authority](../specs/account-lifecycle-authority.md).

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
- Plugin skills remain the installable policy pack and are checked by plugin surface tests (`apps/decodex/src/plugin_surface_tests.rs`, `plugins/decodex/.codex-plugin/plugin.json`).

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

## Radar upstream redesign

Radar replaced signal-first upstream tracking with an upstream-review pipeline so the same source-backed evidence can support public publishing and Decodex control-plane compatibility work.

Why:

- Sparse release notes and title-score skipping are not enough for compatibility decisions.
- GitHub Actions can deterministically refresh metadata, queues, release deltas, bundles, validation, and ledgers, but AI editorial judgment belongs in local Codex automation where model access and operator context are managed.
- Public signals and control-plane upgrades should share the same reviewed upstream impact conclusion instead of independently reinterpreting source material.

Current shape:

- Radar builds `upstream_review_queue/v1` artifacts from recent upstream commits and PRs, records ledger state, and skips subjects already represented by published signals (`apps/radar/src/review_queue.rs`).
- Artifact validation recognizes bundles, upstream reviews, upstream impacts, release deltas, signal entries, control-plane upgrade candidates, and archive manifests (`apps/radar/src/lib.rs`, `apps/radar/src/artifact_validation/`).
- Control-plane upgrade candidates must reference the shared `upstream_impact/v1` handoff and include affected surfaces, validation gates, authority, and stop conditions (`apps/radar/src/artifact_validation/upstream/control_plane_upgrade.rs`).

Do not let Radar review output directly mutate Decodex runtime state, create Linear issues, publish social content, or claim shipped behavior. It produces evidence and candidates that downstream authority surfaces must accept.

## Radar, Control Plane, and Publisher handoff

Decodex should be described as one product with three capability areas: Radar for upstream intelligence, Control Plane for repo-native retained orchestration, and Publisher for public/static/social publication surfaces.

Why:

- The old A/B labels were useful during discussion but not durable product language.
- Radar can detect upstream Codex implications before they are public content or engineering work.
- Publisher should turn Radar evidence into practical, evidence-backed external angles without coupling the public site to the runtime.
- Control Plane remains the local execution authority even when Radar evidence suggests a Decodex improvement.

Current shape:

- Radar owns upstream review queues, release deltas, artifact validation, signals, ledgers, bundles, and control-plane upgrade candidates (`apps/radar/src/lib.rs`).
- Control Plane owns registered projects, app-server integration, tracker writes, local runtime state, operator status, review handoff, landing, closeout, cleanup, and recovery (`apps/decodex/src/cli.rs`, `apps/decodex/src/orchestrator/`).
- Publisher owns social candidates, reservations, posts, outcomes, strategy validation,
  browser lease serialization, and idempotency or daily-cap checks
  (`apps/decodex-publisher/src/lib.rs`,
  `apps/decodex-publisher/src/social_publish.rs`,
  `apps/decodex-publisher/src/social_validation.rs`).
- The static site consumes reviewed product content and build assets, not live runtime state (`site/README.md`).

Do not describe Radar artifacts as execution authority, Publisher content as shipped runtime proof, or the static site as a live control-plane surface.

## Radar artifact release archives

Radar keeps raw upstream bundles and analysis drafts in Git only for a short hot window, then moves cold raw batches to dedicated GitHub Release assets while retaining checked-in manifests.

Why:

- Continuous Radar may inspect every upstream commit, but the repository should not become a permanent raw-data warehouse.
- Curated public impacts, signal entries, and archive manifests are small enough to
  remain in Git. Raw bundles and drafts are heavier recovery material. Social,
  strategy, browser-session, browser-lease, and generated-media records are local-only
  and are never part of a Radar archive.
- GitHub Release assets preserve a durable download location without filling the Git tree with compressed archives.

Current shape:

- Archive manifests use `radar_archive_manifest/v1` and require archive identity, source commit, release tag/URL, external asset metadata, checksum information, and file entries (`apps/radar/src/artifact_validation/archive.rs`).
- Radar validation treats `.agent/automations/radar/cache/archive/index/` as the checked-in manifest area and permits historical retention-policy exceptions only for recognized archive-manifest paths (`apps/radar/src/artifact_validation/core/paths.rs`).
- Radar constants and ledger schemas include archive artifact kinds and archived statuses (`apps/radar/src/constants.rs`, `apps/radar/src/ledger/schema/`).

Do not include social or account-session records in a Radar archive. Do not commit
compressed raw archives to Git as normal source, prune raw artifacts without updating
the archive manifest, or confuse `radar-archive-*` release tags with Decodex product
releases.

## Stop conditions for future changes

Stop and require a new accepted decision, architecture review, or explicit human authority when a change would:

- expose internal Execution Program graph ids, DAG operations, or hidden Codex goal state as the ordinary operator workflow;
- let autonomy execute from signals, reports, memory retrieval, external-agent output, or MCP calls without Decision Contract and Program Intake authority;
- make MCP or skills bypass capability profiles, inspect-first lane-control preconditions, tracker boundaries, review policy, landing policy, project enablement, or private-evidence boundaries;
- make `site/` depend on a live Decodex daemon or add dynamic public capabilities without a backend/security decision;
- let Radar mutate runtime/tracker state directly or let Publisher publish from unaccepted upstream evidence;
- keep large raw Radar artifacts in Git instead of manifests plus external release assets.

Runtime stop evidence also exists in source: authority-boundary checks and architecture-recovery events preserve when an automated lane must change strategy, collect enhanced evidence, block landing, or require human decision before continuing (`apps/decodex/src/orchestrator/execution_architecture_recovery.rs`, `apps/decodex/src/orchestrator/types/authority/`, `apps/decodex/src/orchestrator/status/post_review/authority_boundary.rs`).
