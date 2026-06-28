# Codebase Workflow Policy

Read when `$codebase:work` needs exact command, structure, validation, or evidence rules.

## Defaults

- Inspect checked-in authority first: nearest `AGENTS.md`, README/docs, task runner,
  manifests, OKF/LLM Wiki, or repo-memory owner.
- Do not duplicate this routing in host bootstrap files; route to the owning plugin
  surface instead.
- Prefer repo-native commands: `fmt`, `fmt-check`, `check`, `lint`, `test`, `build`,
  `smoke`, and codegen tasks over raw tool defaults.
- Use English for durable or executable artifacts unless the user requests another
  language, source text is being preserved, or the file is a locale/i18n fixture.
- After behavior, command, config, status/help, public contract, architecture, or
  plugin/skill changes, classify docs drift or writeback before ready.
- Before substantial implementation, inspect existing package/module ownership. Do not leave generated monoliths or files mixing unrelated parsing, state, I/O, rendering, persistence, CLI wiring, or tests.

## Task Runner Structure

- Treat `Makefile.toml` and equivalents as public command APIs.
- Group by action family, not domain/toolchain: `Check`, `Build`, `Format`, `Lint`,
  `Lint Fix`, `Smoke`, `Test`.
- Sort action-family sections alphabetically unless a checked-in template says
  otherwise. Inside a family, primary aggregate first, then peer tasks alphabetical.
- Name public tasks action-first: `check-docs`, `lint-rust`, `fmt-rust`,
  `test-rust`, `build-node`, `smoke-*`. Avoid `docs-check`, `typecheck-node`,
  public `*-smoke`, legacy `checks`, and undocumented `[tasks.fix]`.
- Classify by what the task proves: typecheck and `cargo check` are `check`; clippy
  is `lint`; format verification is `fmt-check`; build, smoke, lint, test, and
  mutating fixes stay out of `check-*` composites.
- Keep substantive shell out of task config: no `bash -lc`, heredocs, long multi-line
  scripts, loops over repo files, temp/db/json flows, or embedded language programs.
  Put real logic in `scripts/` or the owning package.
- After task renames/removals, reverse-scan docs, CI, scripts, tests, fixtures,
  reports, help/status text, and examples for stale commands.
- Task-runner review checklist: authority source, public command names, action-family
  grouping, docs/help references, CI callers, and generated artifacts.

## Validation And Claims

- Treat local validation, remote CI, review handoff, landing, and closeout as separate
  lifecycle surfaces.
- Select the smallest repo-native evidence that proves the claim; broaden for shared
  behavior, generated outputs, runtime/build, public contracts, security, release,
  landing, or failures wider than the touched file.
- Before positive ready/done/fixed claims, verify current head/worktree/base state and
  use `$codebase:verification`; for material architecture, root-cause, review-repair,
  generated, large, or public-contract claims, use `$deliberation:skeptic` unless the
  inline exception clearly applies.

## Drift, Naming, Config

- Docs/code/help/status/config/runtime alignment belongs to the drift owner; tests,
  help output, link checks, or generated summaries do not prove semantic consistency.
- Treat names as authority. Before renaming a symbol, field, status, command, or
  config key, classify its boundary: internal, external, UI, persisted schema,
  telemetry/status, adapter, or migration.
- Prefer the root-cause fix and one canonical owner/name/config/execution path.
  Remove obsolete code, config, commands, fixtures, docs, aliases, and tests in the
  same change unless an external contract or migration requires compatibility.
- Config files are user-facing contracts: missing fields need explicit errors or
  migrations; unknown fields should be rejected unless a checked-in policy says
  otherwise.

## Plugin Changes

- Treat Codex skill/plugin edits as executable workflow changes.
- Run `plugin-eval analyze <plugin-root> --format markdown` before ready claims when
  plugin-eval is available; report the score, highest-priority finding, and
  static-vs-measured limitation.

## Evidence To Report

- Checked-in command authority used and whether a task runner existed.
- Task-runner naming/structure evidence when task config changed.
- Validation scope and why it matches the touched files and risk.
- Drift/debugging/verification owner used when relevant.
- Fresh evidence for done, fixed, passing, ready, landed, closed out, or verified
  claims, plus honest remaining gaps.
