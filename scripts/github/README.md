# GitHub Scripts

This directory owns deterministic GitHub-first Decodex scripts.

Current scripts:

- `build_change_bundle.py`
- `build_release_delta.py`
- `backfill_release_range.py`
- `run_codex_analysis.py`
- `sync_latest_signals.py`
- `validate_change_bundle.py`
- `render_signal_entry.py`
- `validate_signal_entry.py`

Current checked contracts:

- `analysis_draft.schema.json`
- `release_delta/v1` is validated by `contracts.py`
- `upstream_impact.schema.json`
- `social_post_draft.schema.json`

Contract ownership:

- input bundle shape: `docs/spec/github-change-bundle.md`
- output signal shape: `docs/spec/signal-entry.md`
- upstream impact shape: `docs/spec/upstream-impact.md`
- social post draft shape: `docs/spec/social-post-draft.md`

Example flow:

```bash
python3 scripts/github/build_change_bundle.py \
  --repo openai/codex \
  --pr 15222 \
  --out artifacts/github/bundles/openai-codex-pr-15222.json

python3 scripts/github/render_signal_entry.py \
  --bundle artifacts/github/bundles/openai-codex-pr-15222.json \
  --analysis artifacts/github/analysis/openai-codex-pr-15222.analysis.json \
  --out site/src/content/signals/openai-codex-pr-15222.json

python3 scripts/github/validate_signal_entry.py \
  site/src/content/signals/openai-codex-pr-15222.json
```

Continuous commit sync:

```bash
python3 scripts/github/sync_latest_signals.py \
  --repo openai/codex \
  --search-limit 20 \
  --max-new-prs 3
```

Release-window gap fill:

```bash
python3 scripts/github/backfill_release_range.py \
  --repo openai/codex \
  --stable-tag rust-v0.130.0 \
  --preview-tag rust-v0.131.0-alpha.9 \
  --max-prs 3
```

These scripts stay deterministic on purpose. Local Codex analysis produces the
editorial draft JSON consumed by `render_signal_entry.py`. Trusted automation may
invoke the Codex analysis step as long as `auth.json` is injected into
`CODEX_HOME` and no credentials are logged or persisted into the repo.

Repo-local skills under `dev/skills/` are reasoning instructions for the Codex
analysis step and for manual Radar/Publisher work. They do not introduce extra
intermediate artifact schemas unless the conclusion is promoted into one of the
checked-in contracts listed above.
