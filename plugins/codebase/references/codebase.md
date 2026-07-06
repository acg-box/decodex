# Codebase Workflow Policy

Read when `$codebase:work` needs exact command, structure, validation, or evidence rules.

## Defaults

- Inspect checked-in authority first: nearest `AGENTS.md`, README/docs, task runner,
  manifests, OKF/LLM Wiki, or repo-memory owner.
- Do not duplicate this routing in host bootstrap files; route to the owning plugin surface instead.
- Prefer repo-native commands: `fmt`, `fmt-check`, `check`, `lint`, `test`, `build`,
  `smoke`, and codegen over raw tool defaults.
- Durable/executable artifacts are English unless explicitly requested, preserved
  source text, or locale/i18n.
- After behavior, command, config, status/help, public contract, architecture, or
  plugin/skill changes, classify docs drift/writeback before ready.
- Before substantial implementation, inspect package/module ownership. Do not leave generated monoliths or files mixing unrelated parsing, state, I/O, rendering, persistence, CLI wiring, or tests.

## Module Boundaries

- Modularization is an ownership decision, not a file-size target. Split or merge
  by responsibility, public contract, state ownership, change cadence, validation
  surface, and reader navigation; do not use fixed line counts as the decision rule.
- Good modules own real concepts: domain behavior, state transitions, protocols,
  persistence, adapters, rendering, policies, validation, or a narrow API shared by
  callers. Small cohesive modules are fine; large cohesive owners are also fine.
- Avoid pseudo-modularization. Do not create files that only wrap one trivial
  helper, one constant, one forwarding function, or re-exports unless that file is
  the canonical owner for the concept.
- Put code and constants in the nearest real owner. For example, keep paths in path
  owners, schemas in schema owners, assets in asset owners, parsing helpers near
  parsers, and policy predicates near policy owners. Avoid dumping unrelated code
  into generic `utils`, `common`, `shared`, or `misc` buckets.
- Do not claim modularization by using textual includes, compatibility shims,
  mechanisms that keep code executing in the old owner's scope, or by moving tests
  away from production code. Generated/FFI includes are documented plumbing, not
  modularization progress.

## Task Runner Structure

- Treat `Makefile.toml` and equivalents as public command APIs.
- Group by action family: `Check`, `Build`, `Format`, `Lint`, `Lint Fix`, `Smoke`,
  `Test`. Sort sections alphabetically; inside a family, primary aggregate first,
  then peers alphabetical.
- Public tasks are action-first: `check-docs`, `lint-rust`, `fmt-rust`, `test-rust`,
  `build-node`, `smoke-*`. Avoid `docs-check`, `typecheck-node`, public `*-smoke`,
  legacy `checks`, and undocumented `[tasks.fix]`.
- Command aliases are not allowed. Keep one canonical command spelling, remove
  compatibility aliases, and update callers/docs instead of preserving duplicate
  command names.
- Typecheck and `cargo check` are `check`; clippy is `lint`; build, smoke, test, and
  mutating fixes stay out of `check-*` composites.
- No substantive shell in task config: no `bash -lc`, heredocs, long scripts, loops,
  temp/db/json flows, or embedded language programs. Put logic in `scripts/` or the
  owning package.
- After task renames/removals, reverse-scan docs, CI, scripts, tests, fixtures,
  reports, help/status text, and examples.
- Task-runner review checklist: authority source, public command names, action-family
  grouping, docs/help references, CI callers, generated artifacts.

## Validation And Claims

- Local validation, remote CI, review handoff, landing, and closeout are separate.
- Use the smallest repo-native proof; broaden for shared behavior, generated outputs,
  runtime/build, public contracts, security, release, landing, or wider failures.
- Before ready/done/fixed claims, verify head/worktree/base and use
  `$codebase:verification`; for material architecture, root cause, review repair,
  generated/large/public contracts, use `$deliberation:skeptic` unless inline applies.

## Drift, Naming, Config

- Drift owner handles docs/code/help/status/config/runtime alignment; tests/help/link
  checks do not prove semantics.
- Names are authority. Before renaming symbols, fields, statuses, commands, or config
  keys, classify internal/external/UI/persisted/telemetry/adapter/migration boundary.
- Prefer one canonical owner/name/config/execution path. Remove obsolete code,
  config, commands, fixtures, docs, aliases, and tests; migrate callers to the
  canonical spelling instead of keeping compatibility aliases.
- Config files are user-facing contracts: missing fields need explicit errors or
  migrations; unknown fields should be rejected unless checked-in policy says
  otherwise.

## Plugin Changes

- Treat Codex skill/plugin edits as executable workflow changes.
- Run `plugin-eval analyze <plugin-root> --format markdown` before ready claims when
  plugin-eval is available; report the score, highest-priority finding, and
  static-vs-measured limitation.

## Evidence To Report

- Checked-in command authority used and task-runner presence.
- Task-runner naming/structure evidence when task config changed.
- Validation scope and touched-file/risk match.
- Drift/debugging/verification owner used.
- Fresh evidence for done/fixed/passing/ready/landed/closed/verified claims, plus gaps.
