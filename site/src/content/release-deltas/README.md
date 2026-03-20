This directory stores generated `release_delta/v1` artifacts for the homepage.

The current artifact is a bounded comparator payload:

- a default stable-versus-preview pair for no-JS rendering
- a limited stable release option set
- a limited preview release option set
- precomputed compare entries for homepage switching

Build the latest `openai/codex` artifact with:

```bash
python3 tools/github/build_release_delta.py \
  --repo openai/codex \
  --signals-dir site/src/content/signals \
  --out site/src/content/release-deltas/openai-codex-latest.json
```
