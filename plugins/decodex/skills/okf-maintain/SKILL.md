---
name: okf-maintain
description: Use when creating or updating OKF/LLM Wiki concepts, indexes, logs, links, frontmatter fields, or repo-memory anchors in any repository.
---

# OKF Maintain

Produce and maintain OKF bundles without coupling them to Decodex-specific policy.

Read `../../references/okf-layer.md` before creating concepts, moving files, or
repairing graph quality.

- Choose the target profile first: `core`, `wiki`, `repo-memory`, or `decodex`.
- For a new portable bundle, run
  `decodex okf init <root> --profile core|wiki|repo-memory` before adding
  repository-specific concepts.
- Keep each concept focused on one topic and give it a useful `type`.
- Add `title`, `description`, `resource`, `tags`, and `timestamp` when they help
  retrieval.
- For repository memory, add `source_refs`, `code_refs`, `related`, and `drift_watch`
  only when they carry real retrieval or maintenance value.
- Update `index.md` and `log.md` when navigation or maintenance history changes.
- Do not overwrite existing scaffold files with different content; inspect and merge
  manually.
- Run the matching `decodex okf check <root> --profile <profile>` before claiming the
  bundle is ready.
