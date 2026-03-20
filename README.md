# Decodex

Decodex is a GitHub-first signal layer for Codex changes.

The immediate MVP goal is to turn GitHub PR and commit activity into compact,
actionable signal entries that answer:

- what changed
- why it matters
- how to try it

## Current status

This repository started as an organization Rust template. It is being reshaped into a
static-site plus tooling layout for Decodex.

During the transition:

- `site/` owns the Astro-based static site
- `tools/` owns the deterministic GitHub collection, render, and validation scripts
- `skills/` owns repo-local AI workflow entrypoints such as the GitHub signal drafting skill
- `docs/` remains the authoritative documentation surface
- the root Rust scaffold remains a legacy template surface until it is explicitly
  removed or repurposed

## Documentation entry points

- `docs/index.md` routes documentation reads.
- `docs/spec/` defines normative contracts.
- `docs/guide/` defines repeatable workflows.
- `docs/plans/` stores saved `plan/1` artifacts.

## MVP direction

The first delivery focus is the GitHub lane:

- PR-first analysis
- commit and diff evidence
- local or trusted-runner Codex analysis
- static-site build and deployment through CI

The current seed path is live in-repo:

- `tools/github/build_change_bundle.py` builds a normalized GitHub bundle
- `skills/decodex-github-signal/SKILL.md` defines the Codex editorial step
- `tools/github/sync_latest_signals.py` discovers recent merged PRs and refreshes the content artifacts
- `tools/github/render_signal_entry.py` renders a reviewed analysis draft into site content
- `tools/github/validate_signal_entry.py` validates the published collection
- `.github/workflows/refresh-github-signals.yml` refreshes GitHub-backed signals every hour from a trusted runner
- `.github/workflows/deploy-pages.yml` publishes the Astro site to GitHub Pages on pushes to `main`
- `site/src/content/signals/openai-codex-pr-15222.json` is the first real bundle-backed signal
- `cargo make decodex-checks` runs the current repo-native validation surface for the MVP

## Production domain

The intended public domain is:

- `https://decodex.space`

Repository code now includes the Pages deployment workflow and site URL. The
remaining GitHub Pages source selection, custom-domain, and DNS provider changes
are documented in:

- `docs/guide/github_pages_deploy.md`

## License

Licensed under [GPL-3.0](LICENSE).
