---
name: decodex-github-signal
description: Use when turning a normalized GitHub bundle under `tools/github/bundles/` into a Decodex signal draft, especially for requests to analyze a PR-first bundle, decide if a change is signal-worthy, or write/update the local editorial analysis JSON that feeds `tools/github/render_signal_entry.py`.
---

# Decodex GitHub Signal

Use this skill for the local editorial step in the GitHub-first Decodex workflow.

This skill does not replace the deterministic scripts. It tells Codex how to read a
bundle, decide whether it deserves publication, and draft the analysis JSON that the
repo already renders into a final `signal_entry/v1`.

## Read before drafting

- `docs/spec/github_change_bundle.md`
- `docs/spec/signal_entry.md`
- `docs/guide/local_github_signal_workflow.md`

## Inputs

- A normalized bundle JSON under `tools/github/bundles/`
- An output path under `tools/github/analysis/`

## Boundaries

- Treat the PR as the main narrative container.
- Treat commits, files, and patch excerpts as evidence.
- Do not summarize every commit as if it were independently important.
- Publish only when the change introduces a capability, changes user-visible behavior, or offers a clear try-now path.
- Keep `why_it_matters` focused on user value, not internal mechanics.
- If `how_to_try` is present, make it concrete and pair it with `expected_effect`.
- When evidence is weak or the change is mostly internal cleanup, lower confidence or skip publication.

## Draft shape

Write a JSON analysis draft with these fields:

- `kind`
- `title`
- `summary`
- `why_it_matters`
- `confidence`
- `impact`
- `proof_points`
- optional `how_to_try`
- optional `expected_effect`
- optional `config_flags`

## Workflow

1. Validate the bundle first.
2. Read `primary_pr.title`, `primary_pr.body`, `files`, and `commits`.
3. Decide whether the change is signal-worthy.
4. Draft the editorial JSON under `tools/github/analysis/`.
5. Render the final signal entry with the repo script.
6. Validate the published signal collection and site build.

## Commands

Validate a bundle:

```bash
python3 tools/github/validate_change_bundle.py tools/github/bundles/<bundle>.json
```

Render the final signal entry after drafting:

```bash
python3 tools/github/render_signal_entry.py \
  --bundle tools/github/bundles/<bundle>.json \
  --analysis tools/github/analysis/<bundle>.analysis.json \
  --out site/src/content/signals/<bundle>.json
```

Validate the published output:

```bash
python3 tools/github/validate_signal_entry.py site/src/content/signals
npm run build --prefix site
npm run check --prefix site
```
