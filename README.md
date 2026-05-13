<div align="center">

# Decodex

Repo-native agent orchestration and public Codex signal publishing.

[![License](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Language Checks](https://github.com/hack-ink/decodex/actions/workflows/language.yml/badge.svg?branch=main)](https://github.com/hack-ink/decodex/actions/workflows/language.yml)
[![Release](https://github.com/hack-ink/decodex/actions/workflows/release.yml/badge.svg)](https://github.com/hack-ink/decodex/actions/workflows/release.yml)
[![GitHub tag (latest by date)](https://img.shields.io/github/v/tag/hack-ink/decodex)](https://github.com/hack-ink/decodex/tags)
[![GitHub last commit](https://img.shields.io/github/last-commit/hack-ink/decodex?color=red&style=plastic)](https://github.com/hack-ink/decodex)
[![GitHub code lines](https://tokei.rs/b1/github/hack-ink/decodex)](https://github.com/hack-ink/decodex)

</div>

## Feature Highlights

- Rust CLI and runtime for repo-native retained coding-agent lanes.
- Explicit project registry under `~/.codex/decodex/projects/<service-id>/`.
- Local operator listener with a read-only dashboard at `/` and `/dashboard`, plus
  `GET /state` for JSON snapshots.
- Static Astro site that publishes GitHub-backed Codex change signals.
- Deterministic GitHub signal pipeline for change bundles, release deltas, rendered
  signal entries, and content validation.
- Repo-local Radar skills for upstream Codex triage, code analysis, release analysis,
  signal drafting, and X post drafting.
- Publisher workflow for checked-in upstream impact classification and reviewable X
  drafts for `@decodexspace`.
- Installable Decodex plugin with reusable agent-facing skills for manual CLI,
  automation, commit, land, and labels.
- Repository documentation split by question type into spec, runbook, reference, and
  decision lanes.

## Status

Prototype / in active development.

This repository now integrates the former runtime repository and the static signal site
into one Decodex workspace. The static site remains the public surface by default.
Runtime and operator behavior does not become a public site backend just because both
surfaces live in one workspace.

Supported runtime host targets are macOS and Linux. Windows remains unsupported for the
runtime.

## App identity

- Product display name: **Decodex**.
- Runtime CLI binary: **decodex**.
- Public site: <https://decodex.space>.
- Lower-case `decodex` remains only for stable technical identifiers such as the
  repository slug, Cargo package name, CLI binary, plugin package, config keys, and
  schema names.

## Workspace posture

- `apps/decodex/` owns the Rust package that builds the `decodex` CLI and runtime.
- `site/` owns the Astro static site and checked-in public content.
- `scripts/github/` owns deterministic GitHub bundle, release-delta, render, and
  validation scripts.
- `artifacts/github/` owns checked-in GitHub bundles and editorial analysis drafts.
- `artifacts/social/` owns checked-in Publisher social draft artifacts.
- `plugins/decodex/` owns the installable Decodex plugin and reusable agent-facing
  skills.
- `dev/skills/` owns repository-development skills for Radar analysis and Publisher
  drafting. They are not packaged with the installable Decodex plugin.
- `docs/` remains the authoritative documentation surface.

Runtime authority stays in `apps/decodex/src/`, the registered project contracts under
`~/.codex/decodex/projects/<service-id>/`, and the governing specs under `docs/spec/`.
Public site authority stays in `site/`, `scripts/github/`, `artifacts/github/`, and
the site/content specs.

## Runtime platform support

- The Decodex runtime contract is Unix-only: macOS and Linux.
- Windows is outside the runtime contract.
- The public site is static and deploys through GitHub Pages.
- Starting `decodex serve` without `--config` loads enabled projects from the explicit
  registry only. It does not scan Codex history, repo-local config files, or currently
  open worktrees to infer projects.

## Usage

### Runtime CLI

From the workspace root:

```sh
cargo run -p decodex -- --help
cargo run -p decodex -- probe stdio://
cargo run -p decodex -- project list
cargo run -p decodex -- status
cargo run -p decodex -- diagnose --json
cargo run -p decodex -- run --dry-run
cargo run -p decodex -- serve --interval 60s --listen-address 127.0.0.1:8912
```

### Install from Source

```sh
git clone https://github.com/hack-ink/decodex
cd decodex

cargo install --path apps/decodex --force
decodex --version
```

### Project contracts

Project contracts are managed outside checkouts under
`~/.codex/decodex/projects/<service-id>/` with fixed filenames:

- `project.toml` for service paths and credential environment-variable names
- `WORKFLOW.md` for execution policy

The redacted template for a project config lives at `decodex.example.toml`.
When a project enables `[codex.accounts]`, the shared ChatGPT account pool is
`~/.codex/decodex/accounts.jsonl`; it is global Decodex state, not a project-local
file, and project configs do not own an account-pool path override. Set
`[codex.accounts].fixed_account` in `~/.codex/decodex/config.toml` to pin all new
account-pool runs to one account. When that global selector is absent, Decodex balances
new runs across the pool. The operator dashboard Accounts UI writes and clears the same
global selector; project configs do not pin specific accounts.

`decodex diagnose --json` writes the local agent evidence index under
`~/.codex/decodex/agent-evidence/<service-id>/` and prints the same handoff index for
repair agents.

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

The static-site boundary is recorded in `docs/decisions/static-public-site.md`. GitHub
Pages setup for `https://decodex.space` lives in `docs/runbook/github-pages-deploy.md`.

## GitHub Signal Pipeline

The GitHub-first public signal path stays deterministic and reviewable:

- `scripts/github/build_change_bundle.py` builds normalized GitHub bundles under
  `artifacts/github/bundles/`.
- `dev/skills/README.md` routes the repo-local Radar and editorial instructions. They
  are not part of the installable Decodex plugin distribution.
- `scripts/github/sync_latest_signals.py` discovers recent merged PRs and refreshes
  content artifacts.
- `scripts/github/sync_prerelease_signals.py` starts from the latest stable-to-prerelease
  compare so Decodex can explain Codex prereleases even when upstream release notes are
  sparse.
- `docs/spec/upstream-impact.md` records how upstream Codex changes are classified for
  public signals and Control Plane follow-up work.
- `scripts/github/render_signal_entry.py` renders reviewed analysis drafts into site
  content.
- `scripts/github/validate_signal_entry.py` validates the published signal collection.
- `docs/spec/social-post-draft.md` and
  `docs/runbook/social-publishing-workflow.md` govern optional checked-in X drafts
  before external publication.
- `.github/workflows/refresh-github-signals.yml` refreshes GitHub-backed signals every
  hour from a trusted runner.
- `.github/workflows/deploy-pages.yml` publishes the Astro site to GitHub Pages on
  pushes to `main`.

The governing workflow lives at `docs/runbook/local-github-signal-workflow.md`.

## Operator Dashboard

`decodex serve` owns the local operator listener. It serves one read-only operator
console from `GET /` and `GET /dashboard`, plus the same JSON status snapshot model from
`GET /state`.

For dashboard UI development, use the mock operator state server:

```sh
node dev/operator-dashboard-mock.mjs --listen-address 127.0.0.1:57399
node dev/operator-dashboard-mock.mjs --listen-address 127.0.0.1:57399 --use-codex-auth
```

The dashboard semantics and local-vs-external state boundary live in
`docs/reference/operator-control-plane.md`.

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

## Workspace Layout

The tracked workspace currently keeps:

- `apps/decodex/` as the Rust package that builds the `decodex` CLI and runtime
- `site/` as the Astro static site for the public Decodex signal surface
- `scripts/github/` as the deterministic GitHub collection, normalization, render, and
  validation script surface
- `artifacts/github/` as checked-in GitHub bundle and analysis artifacts
- `plugins/decodex/` as the canonical installable Decodex plugin source
- `dev/skills/` as repo-development Radar analysis and Publisher drafting skills that
  are not packaged with the installable Decodex plugin
- `docs/spec/` as the normative runtime, workflow, site, and content contract lane
- `docs/runbook/` as the operator procedures, validation sequences, deployment steps,
  and content workflow lane
- `docs/reference/` as the current repository and artifact surface map lane
- `docs/decisions/` as the durable design-rationale lane
- `docs/research/` as machine-authored research artifacts used by shipped research
  tooling
- `docs/plans/` as historical saved plan artifacts from the static-site bootstrap
- `dev/` as local development helpers outside `dev/skills/`, such as the operator
  dashboard mock server
- `assets/` as shared static assets that are not owned by the Astro app's generated
  output
- `.github/` as CI, release, Pages deployment, and content-refresh workflows

Generated or local-only directories such as `target/`, `site/dist/`, `site/.astro/`,
`.worktrees/`, `.workspaces/`, and `.codex/` are not part of the tracked repository
structure. For the authoritative layout and ownership map, read
`docs/reference/workspace-layout.md`.

## Documentation

- Product and development overview: this `README.md`
- Unified documentation router: `docs/index.md`
- Normative specs: `docs/spec/index.md`
- Procedural runbooks: `docs/runbook/index.md`
- Current implementation references: `docs/reference/index.md`
- Durable design rationale: `docs/decisions/index.md`
- Documentation policy and placement rules: `docs/policy.md`

## Support Me

If you find this project helpful and would like to support its development, you can buy me a coffee!

Your support is greatly appreciated and motivates me to keep improving this project.

- **Fiat**
    - [Ko-fi](https://ko-fi.com/hack_ink)
    - [Afdian](https://afdian.com/a/hack_ink)
- **Crypto**
    - **Bitcoin**
        - `bc1pedlrf67ss52md29qqkzr2avma6ghyrt4jx9ecp9457qsl75x247sqcp43c`
    - **Ethereum**
        - `0x3e25247CfF03F99a7D83b28F207112234feE73a6`
    - **Polkadot**
        - `156HGo9setPcU2qhFMVWLkcmtCEGySLwNqa3DaEiYSWtte4Y`

Thank you for your support!

## Appreciation

We would like to extend our heartfelt gratitude to the following projects and contributors:

- The Rust community for their continuous support and development of the Rust ecosystem.

## Additional Acknowledgements

- TODO

<div align="right">

### License

<sup>Licensed under [GPL-3.0](LICENSE).</sup>

</div>
