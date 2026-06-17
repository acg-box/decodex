---
name: repo-memory-writer
description: Use when bootstrapping or improving source-backed repo-memory OKF/LLM Wiki knowledge for a code repository, including create repo knowledge, organize repo docs, make this repo an LLM wiki, or bootstrap OKF for another repo.
---

# Repo Memory Writer

Build source-backed repository knowledge. The AI writes the concepts; `decodex okf`
only scaffolds, checks, routes, and graphs them.

Operating rules:

- Treat repository memory like persistent project guidance: concise, specific,
  broadly reusable, and scoped close to the work.
- Treat OKF as a portable Markdown/YAML exchange format, not a platform runtime.
- Treat LLM Wiki routing as progressive disclosure: small entrypoints point to
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
   `code_refs`, `related`, and `drift_watch` only when they improve retrieval or
   maintenance.
6. Update `index.md` and `log.md`; link neighboring concepts instead of repeating
   broad summaries.
7. Validate with:

   ```sh
   decodex okf check <root> --profile repo-memory
   decodex okf graph <root>
   decodex okf route <root> "<representative task>"
   ```

   Pick route probes from likely user intents and revise until the owning concept is
   a top result.

Quality bar:

- Good: a new agent can find setup, tests, architecture boundaries, automation
  resources, and high-risk drift points without broad repo reading.
- Bad: generated summaries, README duplication, unverified architecture claims,
  orphan concepts, or knowledge that cannot answer a task intent.
