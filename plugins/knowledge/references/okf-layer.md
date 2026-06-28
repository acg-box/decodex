# Portable OKF Layer

Use for portable OKF bundles, LLM Wiki navigation, and cross-repository memory.

OKF is Markdown/frontmatter/links; LLM Wiki adds indexes, owners, logs. A repo may
define a stricter Decodex docs profile; portable bundles do not inherit runtime lanes
or docs-impact checkpoints.

Profiles: `core`, `wiki`, `repo-memory`, `decodex`. `repo-memory` adds source-backed repository memory anchors `source_refs`, `code_refs`, `related`, `drift_watch`. Commands: `decodex okf init`, `decodex okf check`, `decodex okf find`, `decodex okf graph`. Do not create or recommend `decodex docs okf ...`.

The CLI does not replace the LLM: agents judge owners/concepts and prove usefulness with owner coverage, graph health, and agent-read review.
