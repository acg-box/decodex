# Workspace Layout

Purpose: Describe the current top-level repository surfaces and which concerns each one
owns.

Read this when: You need to know where runtime code, static-site code, workflow policy,
GitHub signal tooling, and documentation topics currently live.

Not this document: The normative behavior contract, operator procedures, or durable
design rationale.

Covers: The repository surface map, ownership boundaries, and local directories that
should not be treated as repository source.

## Top-level surfaces

| Path | Role |
| --- | --- |
| `apps/decodex/` | Rust package that builds the `decodex` CLI and runtime. Runtime, orchestration, tracker integration, app-server integration, operator HTTP, and local control-plane behavior live under `apps/decodex/src/`. |
| `apps/decodex-app/` | SwiftPM macOS app for local Decodex Codex account-pool management. It talks to the bundled `decodex-app-helper`, which links the Rust account service directly, and does not own runtime scheduling or operator dashboard state. |
| `site/` | Astro static site for the public Decodex signal surface. It renders checked-in content and generated JSON from `site/src/content/`; it is not backed by a live Decodex daemon. |
| `scripts/github/` | Automation-only Codex AI analysis helper and shared schema support for that helper. Deterministic Radar commands live in the Rust CLI. |
| `scripts/config/` | Repository automation scripts for config-derived artifacts. |
| `artifacts/github/` | Checked-in GitHub change bundles and editorial analysis drafts used by the public signal pipeline. |
| `artifacts/archive/` | Checked-in manifests for cold Radar archive batches stored as GitHub Release assets. |
| `artifacts/social/` | Checked-in Publisher social publication records, blocked-cap records, and generated-media evidence. |
| `dev/skills/` | Repository-development skills for Radar upstream triage, code analysis, release analysis, GitHub signal drafting, and X publishing. These are not part of installable plugin distribution. |
| `plugins/decodex/` | Canonical installable Decodex plugin source and reusable agent-facing skills, including planning, manual CLI, automation, commit, land, and labels. |
| `docs/spec/` | Normative runtime, workflow, site, and content contracts. |
| `docs/runbook/` | Operator procedures, validation sequences, deployment steps, and content workflows. |
| `docs/reference/` | Current repository and artifact surface maps. |
| `docs/decisions/` | Durable rationale for repository-level design choices. |
| `docs/research/` | Supporting JSON research reports and evidence. It does not own runtime authority, policy, current-state reference, or durable rationale until promoted into the matching primary docs lane. |
| `dev/` | Local development helpers outside `dev/skills/`, such as the operator dashboard mock server. |
| `assets/` | Shared static assets that are not owned by the Astro app's generated output. Decodex App icons live under `assets/app-icon/{source,composer,generated}/`; menu bar template assets live under `assets/tray-icon/{source,generated}/`; `scripts/assets/render_decodex_app_icons.swift` regenerates the icon set. |
| `.github/` | CI, release, Pages deployment, and content-refresh workflows. |
| `Makefile.toml` | Repo-native task names and automation entrypoints. |
| `decodex.example.toml` | Redacted template for a project `project.toml`; live project contracts live under `~/.codex/decodex/projects/<service-id>/`. |

## Rust workspace

The root `Cargo.toml` is a workspace manifest. It does not define a root package.

`apps/decodex/Cargo.toml` is the only checked-in Rust package in this first integrated
layout. Use package-qualified commands when invoking the runtime from the workspace root:

```sh
cargo run -p decodex --bin decodex -- --help
cargo build -p decodex
cargo install --path apps/decodex --force
```

Do not add new runtime behavior to a root `src/` directory. If Decodex later needs
shared crates, add them under `packages/` and make the boundary explicit in this
reference document and the root workspace manifest.

## Static public site

`site/` remains the public, static Decodex surface. It owns:

- homepage and feed rendering
- signal cards and release-delta presentation
- checked-in content collections under `site/src/content/`
- Astro build and type-check behavior

The site does not own:

- Decodex runtime scheduling
- local operator state
- tracker writes
- app-server orchestration
- live operator dashboard behavior

Those runtime and operator surfaces stay in `apps/decodex/` and `docs/spec/`.

## GitHub signal tooling

`apps/decodex/src/radar.rs` owns deterministic Radar commands. `decodex radar
refresh-upstream-queue` is the continuous Radar entrypoint: it scans recent upstream
commits, resolves them back to PRs when possible, records local ledger state, and
writes an `upstream_review_queue/v1` artifact for Codex automation. It does not run
Codex or render public signals. `decodex radar refresh-release-delta` refreshes the
current homepage release-delta artifact from release compare metadata and published
signal entries. `decodex radar render-signal` renders published signals from
Codex-owned analysis drafts, and `decodex radar backfill-release-range` fills gaps for
release-window summaries when an operator or automation chooses to generate signal
content. Generated GitHub bundles and analysis drafts live under `artifacts/github/`
and must stay explicit and checked into the repository when promoted into Publisher
content.

Raw bundles and analysis drafts are hot artifacts with a 21-day Git retention window.
Older raw batches move to dedicated GitHub Release assets, with recovery manifests kept
under `artifacts/archive/index/`.

`artifacts/github/impact/` may hold `upstream_impact/v1` classifications when an
upstream Codex change has public-signal, Control Plane, or Publisher implications.
`artifacts/github/review-queue/` may hold the latest deterministic review queue.
`artifacts/github/social-candidates/` may hold `social_candidate/v1` pre-publication
handoffs. `artifacts/social/` holds `social_post/v1` published, blocked, failed, or
skipped records for external publication. Generated media files are not checked-in by
default; records should point to X status/media URLs or optional content hashes instead.
These remain checked-in artifacts; none turns the public site into a live service.

## Installable Codex surface

The installable Codex home surface, including `~/.codex/AGENTS.md`, is not a Decodex
runtime contract and is not tracked in this repository. Do not reintroduce checked-in
`.codex/AGENTS.md` content here to carry Decodex-specific policy.

Global agent guidance should stay portable. Decodex-specific runtime, workflow,
identity, review, landing, closeout, and cleanup policy belongs in `apps/decodex/src/`,
`docs/spec/`, the registered project `WORKFLOW.md`, project `project.toml`, runbooks,
or the Decodex plugin skill that owns a reusable method. The normative split is defined
by [`../spec/installable-agent-policy.md`](../spec/installable-agent-policy.md).

## Local Decodex home

Runtime state that belongs to the local operator, not to this repository, lives under
`~/.codex/decodex/`:

- `runtime.sqlite3` is the single-machine control-plane database for all registered
  projects. It owns run leases, attempts, private execution events, tracker/PR
  caches, retained PR state, retry state, and project registration.
- `agent-evidence/<service-id>/` stores local agent-readable diagnosis artifacts,
  including `handoff-index.json`, `events.jsonl`, `blockers/*.json`, and
  `runs/<yyyy-mm>/<run-id>/capsule.json`. This is a derived handoff view, not the
  runtime source of truth and not a public mirror.
- `accounts.jsonl` stores the optional shared ChatGPT account pool used for
  Codex app-server auth token injection and refresh.
- `account-usage-history.jsonl` stores bounded local usage percentages and non-secret
  capacity weights for the account pool display.
- `logs/` stores Decodex process logs. Logs are diagnostic text; structured execution
  evidence belongs in `runtime.sqlite3`.
- `projects/<service-id>/project.toml` stores the central service config for one
  registered project.
- `projects/<service-id>/WORKFLOW.md` stores that project's execution policy.
- Project discovery comes from explicit registration, not from scanning Codex history
  or repo-local config files.

Repo-local Radar history that belongs to the current checkout, not to Git, lives under
`.decodex/`:

- `radar.sqlite3` is the default SQLite ledger for observed upstream Codex commits,
  skipped candidates, PR mappings, review status, and artifact links.

`.decodex/` is ignored by Git. Public curated artifacts and archive manifests remain in
the checked-in tree.

This local control-plane state chooses registered projects. Once a checkout is selected,
the matching project directory's `WORKFLOW.md` remains the execution contract for gates,
tracker routing, and policy.

## Boundary notes

- Runtime authority stays in `apps/decodex/src/`, the registered project contract under
  `~/.codex/decodex/projects/<service-id>/`, and the governing specs under
  `docs/spec/`.
- Public site authority stays in `site/`, `apps/decodex/src/radar.rs`,
  `artifacts/github/`, and the site/content specs.
- Reusable agent-facing Decodex usage instructions live under `plugins/decodex/`.
- `docs/runbook/`, `docs/reference/`, and `docs/decisions/` must not override runtime or
  workflow authority.
- `docs/research/` remains a supporting JSON report and evidence lane. Removed legacy JSON
  event logs are consolidated in
  [`../research/legacy-research-goal-audit.json`](../research/legacy-research-goal-audit.json).
  Current Decodex runtime research authority still flows through runtime-local
  Decision Contracts until accepted and promoted.

## Local-only and generated directories

These paths are intentionally ignored and should not be treated as tracked repository
structure:

- `target/`: Rust build products and local analysis artifacts
- `site/dist/`: Astro build output
- `site/.astro/`: Astro local cache
- `.worktrees/`: local Git worktree lanes
- `.workspaces/`: local clone-backed workspace lanes from older workflows
- `.codex/`: local agent/runtime state, except the app-local
  `apps/decodex-app/.codex/environments/environment.toml` run action config
