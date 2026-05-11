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
| `site/` | Astro static site for the public Decodex signal surface. It renders checked-in content and generated JSON from `site/src/content/`; it is not backed by a live Decodex daemon. |
| `scripts/github/` | Deterministic GitHub collection, normalization, render, validation, and sync scripts for public signal content. |
| `scripts/config/` | Repository automation scripts for config-derived artifacts. |
| `artifacts/github/` | Checked-in GitHub change bundles and editorial analysis drafts used by the public signal pipeline. |
| `dev/skills/` | Repository-development skill-like instructions that are not part of installable plugin distribution. |
| `plugins/decodex/` | Canonical installable Decodex plugin source and reusable agent-facing skills, including manual CLI, automation, commit, land, and labels. |
| `docs/spec/` | Normative runtime, workflow, site, and content contracts. |
| `docs/runbook/` | Operator procedures, validation sequences, deployment steps, and content workflows. |
| `docs/reference/` | Current repository and artifact surface maps. |
| `docs/decisions/` | Durable rationale for repository-level design choices. |
| `docs/research/` | Machine-authored research run artifacts used by shipped research tooling. |
| `docs/plans/` | Historical saved plan artifacts from the static-site bootstrap. These are not primary authority. |
| `dev/` | Local development helpers outside `dev/skills/`, such as the operator dashboard mock server. |
| `assets/` | Shared static assets that are not owned by the Astro app's generated output. |
| `.github/` | CI, release, Pages deployment, and content-refresh workflows. |
| `Makefile.toml` | Repo-native task names and automation entrypoints. |
| `decodex.example.toml` | Redacted template for a project `project.toml`; live project contracts live under `~/.codex/decodex/projects/<service-id>/`. |

## Rust workspace

The root `Cargo.toml` is a workspace manifest. It does not define a root package.

`apps/decodex/Cargo.toml` is the only checked-in Rust package in this first integrated
layout. Use package-qualified commands when invoking the runtime from the workspace root:

```sh
cargo run -p decodex -- --help
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

`scripts/github/` owns deterministic content scripts. It may call Codex for the
editorial drafting step through the repo-local instructions at
`dev/skills/github-signal/`, but that surface is not part of the installable Decodex
plugin distribution. Generated GitHub bundles and analysis drafts live under
`artifacts/github/` and must stay explicit and checked into the repository.

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
  projects.
- `logs/` stores Decodex process logs.
- `projects/<service-id>/project.toml` stores the central service config for one
  registered project.
- `projects/<service-id>/WORKFLOW.md` stores that project's execution policy.
- Project discovery comes from explicit registration, not from scanning Codex history
  or repo-local config files.

This local control-plane state chooses registered projects. Once a checkout is selected,
the matching project directory's `WORKFLOW.md` remains the execution contract for gates,
tracker routing, and policy.

## Boundary notes

- Runtime authority stays in `apps/decodex/src/`, the registered project contract under
  `~/.codex/decodex/projects/<service-id>/`, and the governing specs under
  `docs/spec/`.
- Public site authority stays in `site/`, `scripts/github/`, `artifacts/github/`, and
  the site/content specs.
- Reusable agent-facing Decodex usage instructions live under `plugins/decodex/`.
- `docs/runbook/`, `docs/reference/`, and `docs/decisions/` must not override runtime or
  workflow authority.
- `docs/research/` and `docs/plans/` are supporting evidence only. They do not become
  policy until their conclusions are promoted into governing docs.

## Local-only and generated directories

These paths are intentionally ignored and should not be treated as tracked repository
structure:

- `target/`: Rust build products and local analysis artifacts
- `site/dist/`: Astro build output
- `site/.astro/`: Astro local cache
- `.worktrees/`: local Git worktree lanes
- `.workspaces/`: local clone-backed workspace lanes from older workflows
- `.codex/`: local agent/runtime state
