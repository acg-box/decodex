# GitHub Scripts

This directory owns deterministic GitHub-first Decodex scripts.

Current scripts:

- `build_change_bundle.py`
- `run_codex_analysis.py`
- `sync_latest_signals.py`
- `validate_change_bundle.py`
- `render_signal_entry.py`
- `validate_signal_entry.py`

Contract ownership:

- input bundle shape: `docs/spec/github-change-bundle.md`
- output signal shape: `docs/spec/signal-entry.md`

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

These scripts stay deterministic on purpose. Local Codex analysis produces the
editorial draft JSON consumed by `render_signal_entry.py`. Trusted automation may
invoke the Codex analysis step as long as `auth.json` is injected into
`CODEX_HOME` and no credentials are logged or persisted into the repo.
