# Repo Workflow Policy

Read when `$repo-work:repo-work` needs exact command authority, task-runner structure,
validation, configuration, landing, or evidence-reporting rules.

## Tool Authority

- Inspect the nearest checked-in task runner first, especially `Makefile.toml`.
- Prefer repo-native commands over raw tool defaults when the repo defines `fmt`,
  `fmt-check`, `lint`, `test`, `build`, `smoke`, or codegen tasks.
- Do not edit task-runner config unless the request is about tooling behavior or the
  change requires it.

## Engineering Defaults

- Follow checked-in language, runtime, and bootstrap authority before personal
  defaults.
- Treat language/library preferences as local policy only when checked-in docs,
  manifests, or user instructions establish them.

## Task Runner Structure

- Treat `Makefile.toml` and equivalent config as the repository's public command API,
  not an index of every helper script.
- Use action families as the primary grouping. Do not create Docs, Node, Rust, app,
  or package-manager sections just to group by domain/toolchain; keep the domain in
  the task name, `cwd`, or table metadata.
- For vibe-mono-style cargo-make files, sort action-family section headings
  alphabetically unless a checked-in template says otherwise.
- Inside a section or table, put that action family's primary aggregate first, then
  sort peer tasks alphabetically. Required execution order must stay explicit and
  local to the aggregate.
- "Primary aggregate first" is local to its action family; it does not force global
  `Check` to be the first section.
- Name public tasks action-first: `check-docs`, `check-node-types`, `lint-rust`,
  `fmt-rust`, `test-rust`, `build-node`, `smoke-*`. Avoid `docs-check`,
  `typecheck-node`, and public `*-smoke` cargo-make names.
- Classify by what the task proves: typecheck and `cargo check` belong to `check`;
  `cargo clippy` belongs to `lint`; formatting verification belongs to `fmt-check`
  or the repo's established format-check family.
- Keep build, smoke, lint, test, and mutating fix behavior out of `check-*`
  composites. A top-level default `check` may compose action aggregates only when CI
  or docs already define that contract; each task still belongs in its own family.
- Keep non-mutating checks separate from mutating repair. Lint auto-fix tasks should
  be `lint-fix`, `lint-fix-rust`, or similar; generic `fix` is valid only as a
  documented multi-family repair aggregate.
- Keep substantive shell out of `Makefile.toml`: no `bash -lc`, heredocs, long
  multi-line scripts, `set -euo pipefail`, loops over repo files, temp/db/json flows,
  or embedded language programs. Put that logic in `scripts/` or the owning package.
- A thin task wrapper around one script is fine when it creates a stable repo command,
  aggregate membership, normalized cwd/env, or CI/developer entrypoint.
- Remove stale aliases, empty aggregates, misleading names, and wrappers with no
  semantic contract.
- After task renames/removals, scan README, docs, CI, scripts, tests, fixtures,
  generated reports, help text, status text, and examples for stale commands.
- Task-runner review checklist: no legacy `checks`; no undocumented public
  `[tasks.fix]`; no public `[tasks.*-smoke]`; no long inline shell; sorted action
  sections; sorted peer tasks; summary tables match task order; dependencies are
  deterministic unless documented order is required.

## Validation And Lifecycle

- Treat local validation, remote CI, review handoff, landing, and closeout as separate
  lifecycle surfaces.
- Select validation by touched surface and risk. Docs-only or metadata-only changes
  should use targeted checks when available.
- Broaden validation for executable docs, generated code, shared behavior,
  build/runtime behavior, or failure evidence pointing wider than the touched file.
- For landing, remote CI is acceptance authority only after verifying exact head SHA,
  base state, and checks; rerun local gates when local changes followed CI or CI is
  stale/missing.
- Use the repository's owning merge, landing, queueing, tracker, or automation
  surface. Stop when that authority is unavailable or ambiguous.

## Semantic Drift And Naming

- Route docs/code/help/status/config/runtime claim alignment to the owning drift
  workflow. Tests, help output, link checks, or generated summaries alone do not prove
  semantic consistency.
- Treat names as semantic authority. Before renaming a symbol, field, status phrase,
  command, or config key, classify its boundary: internal fact, external contract, UI
  wording, persisted schema, telemetry/status, compatibility adapter, or migration.
- Rename end-to-end only inside one owned boundary. Keep translations local to API,
  UI, persistence, generated artifact, or adapter boundaries with a compatibility
  reason.
- When public fields, status text, CLI flags, telemetry, config, generated artifacts,
  or persisted schemas change, reverse-check docs, fixtures, examples, tests,
  migrations, and readbacks.

## Architecture And Configuration

- Prefer the root-cause fix and a single canonical owner/name/config/execution path.
- Delete obsolete code, config fields, commands, fixtures, docs, aliases, and tests in
  the same change that replaces their authority.
- Do not add permanent compatibility shims, legacy aliases, hidden fallback defaults,
  or parallel implementations unless an external contract, persisted migration,
  rollout boundary, or user instruction requires them.
- Config files are user-facing contracts. Missing fields need explicit errors or
  migrations; unknown fields should be rejected instead of ignored.
- When config policy changes, update schemas, examples, templates, fixtures, and
  readbacks in the same change.

## Skill And Plugin Changes

- Treat Codex skill and plugin edits as executable workflow changes.
- Run `plugin-eval analyze <plugin-root> --format markdown` before done/fixed/ready
  claims for plugin changes when plugin-eval is available; report the score,
  highest-priority finding, and limits.

## Evidence To Report

- Checked-in command authority used and whether `Makefile.toml` existed.
- Task-runner naming/structure evidence when task config changed.
- Validation scope and why it matches the touched files and risk.
- Drift/debugging/verification owner used when relevant.
- Fresh evidence for any done, fixed, passing, ready, landed, closed out, or verified
  claim, plus honest remaining gaps.
