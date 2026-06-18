---
name: repo-memory-writer
description: Use when bootstrapping or improving source-backed repo-memory OKF/LLM Wiki knowledge for a code repository, including create repo knowledge, organize repo docs, make this repo an LLM wiki, or bootstrap OKF for another repo.
---

# Repo Memory Writer

Build source-backed repository knowledge. The AI writes the concepts; `decodex okf`
only scaffolds, checks, finds, and graphs them. Use `repo-memory-curator` after
real graph, owner, or orphan evidence shows an existing bundle needs repair. Use
`repo-memory-evaluator` when the question is whether the bundle is useful enough.

Operating rules:

- Treat repository memory like persistent project guidance: concise, specific,
  broadly reusable, and scoped close to the work.
- Treat OKF as a portable Markdown/YAML exchange format, not a platform runtime.
- Treat LLM Wiki navigation as progressive disclosure: small entrypoints point to
  detailed canonical concepts.
- For this repository's strict `docs/` profile, use the Decodex `docs-*` skills
  after this skill identifies the repo-memory shape.

Workflow:

1. Identify the target root and profile. For a new portable bundle, run
   `decodex okf init <root> --profile repo-memory`.
2. Probe evidence before writing: README, `AGENTS.md` or project instructions,
   package/build manifests, CI, task runners, entrypoints, config, tests, docs
   indexes, and recent command output when available.
3. Write only claims backed by a checked file, command result, external source, or
   explicit user statement. Put unknowns in the final answer, not durable docs.
4. Prefer this first-pass concept set: `overview.md`,
   `reference/workspace-layout.md`, `reference/build-test-run.md`,
   `reference/automation-resources.md`, focused `spec/` files for real contracts,
   `runbook/` files for executable procedures, and `decisions/` only for recorded
   decisions.
5. Keep each concept one-topic. Add `description`, useful `tags`, `source_refs`,
   `code_refs`, `related`, and `drift_watch` only when they improve navigation or
   maintenance; leave later graph and owner tuning to `repo-memory-curator`.
6. Update `index.md` and `log.md`; link neighboring concepts instead of repeating
   broad summaries.
7. Validate with:

   ```sh
   decodex okf check <root> --profile repo-memory
   decodex okf graph <root>
   decodex okf find <root> --text "<known owner phrase>"
   ```

   Use `find` only for concrete field/text lookups; do not turn it into a ranking
   benchmark. For a fuller quality report, switch to `repo-memory-evaluator`.

Quality bar:

- Good: a new agent can find setup, tests, architecture boundaries, automation
  resources, and high-risk drift points without broad repo reading.
- Bad: generated summaries, README duplication, unverified architecture claims,
  orphan concepts, or knowledge whose owner cannot be discovered from indexes, links,
  or concrete field lookup.
