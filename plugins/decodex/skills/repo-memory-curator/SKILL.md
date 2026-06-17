---
name: repo-memory-curator
description: Use when improving an existing repo-memory OKF/LLM Wiki bundle after route misses, noisy top results, orphan concepts, duplicate claims, stale links, or graph decay.
---

# Repo Memory Curator

Maintain repository memory as a living, source-backed wiki after the first writer pass.

Basis: OKF is Markdown plus YAML frontmatter, not a required query runtime. LLM Wiki is
the method on top: maintain an interlinked Markdown artifact through ingest, query, and
lint. llms.txt reinforces concise Markdown navigation, clear link descriptions, fixed
processing where possible, and test questions.

- Use `repo-memory-writer` for first-pass creation; use this skill for growth and
  repair after real usage exposes misses.
- Use `repo-memory-evaluator` first when the task is to judge bundle quality, produce
  a route benchmark report, or compare before/after curation.
- Treat `docs check` or `okf check` as shape validation only. Prove usefulness with
  `graph`, `find`, and representative `route` probes.
- Prefer metadata and link repairs before creating new concepts.
- Improve top-result quality by editing the owner concept's `title`, `description`,
  `tags`, routing header, and links; do not pad unrelated concepts.
- Treat a metadata-only repair as successful when the same real question moves to the
  expected owner without increasing noise elsewhere.
- Triage orphans as missing edge, intentional leaf, duplicate, stale concept, or
  low-value generated summary before editing.
- Keep one owner concept per durable claim; link instead of copying.
- Record benchmark evidence and remaining misses in the bundle log or final answer.

Growth loop:

1. Capture a real usage signal: failed route, noisy top result, orphan report,
   duplicate claim, stale command, or user confusion.
2. Identify the owner concept. If none exists, create the smallest source-backed
   concept that can own the claim.
3. Repair in this order: owner `description`, `tags`, routing header, "Not this"
   boundary, Markdown links/`related`, lane index. Split, merge, or delete only after
   ownership is proven wrong.
4. Re-run the failing probe plus a small regression set. If the set is not already
   defined, use `repo-memory-evaluator` to create one.
5. Record material navigation changes in `log.md`.

Route benchmark rules:

- Use representative task questions, not keyword lists.
- Map each question to one or more acceptable owner concepts.
- Top-3 is the minimum practical bar; top-1 matters for common operational questions.
- If top-3 passes but top-1 is noisy, strengthen the expected owner metadata first.
- If a noisy result overclaims scope, narrow that concept's description or boundary.
- Preserve before/after counts for real question sets, especially top-1 and top-3.

Orphan triage:

- Treat orphan count as a maintenance queue, not a vanity metric; prioritize concepts
  tied to common workflows, failed route probes, or stale ownership claims.
- Missing edge: add a real Markdown link or `related` edge.
- Intentional leaf: keep if `find` or route can discover it and it has a narrow role.
- Duplicate: merge into the canonical owner.
- Stale concept: delete or mark superseded according to bundle policy.
- Generated summary: remove unless it owns a durable sourced claim.

Metadata rules:

- `description` is the strongest routing sentence; include natural task language.
- `tags` carry cross-cutting lookup terms and common synonyms.
- `source_refs` cite external evidence; `code_refs` point at driftable repo files.
- `related` should help a future task, not decorate the graph.
- `drift_watch` names concrete files, commands, or evidence checks.

Done evidence: report check result, graph counts, top-1/top-3 benchmark results,
metadata/find probes when relevant, and any remaining orphan or route noise.
