This directory stores generated `release_delta/v1` artifacts for the homepage.

Build the latest `openai/codex` artifact with:

```bash
python3 tools/github/build_release_delta.py \
  --repo openai/codex \
  --signals-dir site/src/content/signals \
  --out site/src/content/release-deltas/openai-codex-latest.json
```
