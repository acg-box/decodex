# Decodex

Decodex is a mono repo for repo-native agent orchestration and public Codex signal
publishing.

The repository has two deliberately separate product surfaces:

- a local Rust runtime and operator control plane for retained coding-agent lanes
- a static public site that publishes GitHub-backed Codex change signals

The static site remains the public surface by default. Runtime and operator behavior
does not become a public site backend just because both surfaces now live in one
workspace.

## Status

This repository now integrates the former runtime repository and the static signal site
into one Decodex workspace:

- `apps/decodex/` owns the Rust package that builds the `decodex` CLI and runtime.
- `site/` owns the Astro static site and checked-in public content.
- `tools/github/` owns deterministic GitHub bundle, release-delta, render, and
  validation scripts.
- `plugins/decodex/` owns the installable Decodex plugin and reusable agent-facing
  skills.
- `docs/` remains the authoritative documentation surface.

Supported runtime host targets are macOS and Linux. Windows remains unsupported for the
runtime.

## Repository Layout

- `apps/decodex/src/` holds the Decodex runtime, orchestration logic, tracker
  integrations, app-server integration, operator HTTP server, and authoritative
  implementation behavior.
- `site/` holds the static Astro application and site-owned content collections.
- `tools/github/` holds deterministic public-signal collection and validation tooling.
- `plugins/decodex/` holds the canonical Decodex plugin source.
- `docs/spec/` holds normative runtime, workflow, site, and content contracts.
- `docs/runbook/` holds operator procedures, validation sequences, deployment steps, and
  content workflows.
- `docs/reference/` holds current repository and artifact surface maps.
- `docs/decisions/` holds durable design rationale and tradeoffs.
- `docs/research/` holds machine-authored research run artifacts used by shipped
  research tooling.
- `docs/plans/` holds historical saved plan artifacts from the static-site bootstrap.
- `scripts/` and `dev/` hold repository-level helpers that are not part of the shipped
  runtime binary.
- `decodex.example.toml` is the redacted project-config template; live project
  contracts live under `~/.codex/decodex/projects/<service-id>/`.

## Runtime CLI

From the workspace root:

```sh
cargo run -p decodex -- --help
cargo run -p decodex -- probe stdio://
cargo run -p decodex -- project list
cargo run -p decodex -- status
cargo run -p decodex -- run --dry-run
cargo run -p decodex -- serve --interval 60s --listen-address 127.0.0.1:8912
```

Install the local runtime binary from the package directory:

```sh
cargo install --path apps/decodex --force
decodex --version
```

Project contracts are managed outside checkouts under
`~/.codex/decodex/projects/<service-id>/` with fixed filenames:

- `project.toml` for service paths and credential environment-variable names
- `WORKFLOW.md` for execution policy

Starting `decodex serve` without `--config` loads enabled projects from the explicit
registry only. It does not scan Codex history, repo-local config files, or currently
open worktrees to infer projects.

## Static Site

The public site is an Astro static site under `site/`. It renders checked-in content and
generated JSON artifacts, then deploys through GitHub Pages.

The public site owns:

- Codex signal cards
- release-delta presentation
- recommended config artifacts
- static assets and public page rendering

The public site does not own:

- retained-lane scheduling
- tracker writes
- local operator state
- app-server orchestration
- the operator dashboard served by `decodex serve`

The static-site boundary is recorded in
[`docs/decisions/static-public-site.md`](docs/decisions/static-public-site.md). GitHub
Pages setup for `https://decodex.space` lives in
[`docs/runbook/github-pages-deploy.md`](docs/runbook/github-pages-deploy.md).

## GitHub Signal Pipeline

The GitHub-first public signal path stays deterministic and reviewable:

- `tools/github/build_change_bundle.py` builds normalized GitHub bundles.
- `plugins/decodex/skills/github-signal/SKILL.md` defines the Codex editorial step.
- `tools/github/sync_latest_signals.py` discovers recent merged PRs and refreshes
  content artifacts.
- `tools/github/render_signal_entry.py` renders reviewed analysis drafts into site
  content.
- `tools/github/validate_signal_entry.py` validates the published signal collection.
- `.github/workflows/refresh-github-signals.yml` refreshes GitHub-backed signals every
  hour from a trusted runner.
- `.github/workflows/deploy-pages.yml` publishes the Astro site to GitHub Pages on
  pushes to `main`.

The governing workflow lives in
[`docs/runbook/local-github-signal-workflow.md`](docs/runbook/local-github-signal-workflow.md).

## Operator Dashboard

`decodex serve` owns the local operator listener. It serves one read-only operator
console from `GET /` and `GET /dashboard`, plus the same JSON status snapshot model from
`GET /state`.

For dashboard UI development, use the mock operator state server:

```sh
node dev/operator-dashboard-mock.mjs --listen-address 127.0.0.1:57399
node dev/operator-dashboard-mock.mjs --listen-address 127.0.0.1:57399 --use-codex-auth --query-usage
```

The dashboard semantics and local-vs-external state boundary live in
[`docs/reference/operator-control-plane.md`](docs/reference/operator-control-plane.md).

## Development

Repo-native validation is owned by `Makefile.toml`.

Runtime checks follow the Decodex task structure:

```sh
cargo make check
cargo make fmt
cargo make lint
cargo make test
```

Whole-workspace checks include runtime, static-site, and content validation:

```sh
cargo make checks
```

Static-site/content checks are available separately:

```sh
cargo make decodex-checks
```

## Documentation

- Repository router: [`docs/index.md`](docs/index.md)
- Documentation policy: [`docs/policy.md`](docs/policy.md)
- Specifications: [`docs/spec/index.md`](docs/spec/index.md)
- Operational runbooks: [`docs/runbook/index.md`](docs/runbook/index.md)
- Reference docs: [`docs/reference/index.md`](docs/reference/index.md)
- Design decisions: [`docs/decisions/index.md`](docs/decisions/index.md)
- Workspace layout: [`docs/reference/workspace-layout.md`](docs/reference/workspace-layout.md)
- Static-site decision: [`docs/decisions/static-public-site.md`](docs/decisions/static-public-site.md)

## License

Licensed under [GPL-3.0](LICENSE).
