# Repo Workflow Policy

Read when `$decodex:repo-work` needs exact command, task-runner structure,
exact Makefile/cargo-make task naming, validation, landing, or evidence-reporting
rules.

## Tool Authority

- Inspect the nearest checked-in `Makefile.toml` first.
- Prefer repository-native task-runner commands over raw tool defaults. If
  `Makefile.toml` defines `fmt`, `fmt-check`, `lint`, `test`, build, or codegen, use
  that task instead of guessing language-specific commands directly.
- Do not edit `Makefile.toml` or equivalent task-runner config unless the request is
  about validation/tooling behavior or the task requires that change.

## Task Runner Structure

- Treat `Makefile.toml` and equivalent task-runner config as the repository's small
  public command API, not as an index of every helper script or tool command.
- Start from the repository's existing clear structure before inventing a new one. For
  a vibe-mono-style cargo-make file, preserve established sections and summary tables
  such as Check, Format, Lint, and Test; extend with Build or Smoke only when that
  action family is established in the repo.
- Use action families as the primary grouping. Do not create Docs, Node, Rust, app, or
  package-manager sections merely to group tasks by domain or toolchain; keep the
  domain in the task name, `cwd`, or table metadata.
- Add a new section, table field, task family, or top-level aggregate only when it
  represents a distinct language, package manager, toolchain, lifecycle, consumer,
  validation contract, or actual execution boundary.
- Keep section blocks, summary table rows, task definitions within a section, and
  dependency lists deterministic. For vibe-mono-style cargo-make files, order
  action-family section headings alphabetically unless a checked-in template says
  otherwise. Inside each section or table, put that action family's primary aggregate
  first, then sort peer tasks alphabetically by task name unless execution order is
  required; when order is required, keep it explicit and local to that aggregate.
- "Primary aggregate first" is a local rule inside its action-family section or table;
  it does not force the global Check section to be first when global section headings
  are otherwise sorted alphabetically.
- Name tasks action-first, then the domain or toolchain. Prefer names such as
  `check-rust`, `check-node`, `check-docs`, `check-docs-json`, `lint-rust`,
  `lint-clippy`, `fmt-rust`, `test-rust`, and `build-node`. Avoid noun-first names
  such as `docs-check` or `docs-json-check`.
- Classify tasks by what they prove, not by the literal command name. Type checking
  belongs in the `check` family; Rust `cargo check` belongs in `check`; Rust
  `cargo clippy` belongs in `lint`; formatting verification belongs in the format
  check family already used by the repo, such as `fmt-check` or `check-fmt`.
- Use action-family names for specialized checks too: prefer `check-node-types` over
  `typecheck-node`, and keep docs lint/JSON validation under names such as
  `check-docs` and `check-docs-json`.
- Keep build, smoke, lint, test, and mutating fix behavior out of `check-*` composites.
  A `check-node` aggregate may depend on `check-node-types`; it must not hide
  `build-node`, `lint-node`, `smoke-node`, or `test-node`. Put those tasks in their
  own Build, Lint, Smoke, or Test action family.
- If the repository intentionally treats `check` as the full default validation gate,
  the top-level `check` aggregate may compose action aggregates such as `fmt-check`,
  `lint`, `test`, `build`, or `smoke` only when checked-in CI or docs already make that
  contract explicit. Even then, keep each task defined and summarized in its own action
  family instead of moving it under Check.
- Keep non-mutating verification and mutating repair separate. A lint task should
  report lint findings; auto-fix commands belong in a fix-oriented task family or a
  clearly named fix task, not in `check-*` or the default non-mutating lint gate.
- Name mutating repair tasks by the action owner. Lint auto-fix tasks should be named
  `lint-fix`, `lint-fix-rust`, or similar; formatting fixes should stay under the
  format family. A generic top-level `fix` is valid only when it is a documented
  aggregate spanning multiple repair families.
- Public smoke tasks must be action-first with `smoke-*` names. Reject cargo-make
  public task names ending in `*-smoke` unless that name belongs to a native script
  file, package script, or external tool outside the public task API.
- Avoid business, app, or directory labels such as `check-site` when the real boundary
  is a toolchain like Node/frontend, Rust, Python, or docs.
- A task must earn its place by providing at least one real contract: a stable
  developer or CI entrypoint, cross-toolchain composition, required ordering, shared
  environment or toolchain normalization, portability, or repo-specific policy.
- Keep `check` as the primary aggregate when the repository has a task-runner default
  gate. Avoid parallel top-level aggregates unless each one has a distinct lifecycle
  contract.
- Remove aliases, aggregate tasks, and wrapper tasks that add no semantic contract.
  Examples include an aggregate that only aliases another aggregate or a wrapper that
  only invokes a script unchanged without adding toolchain-aligned naming, ordering,
  environment, portability, or policy value.
- Keep narrow one-off operations in their native surface, such as scripts, package
  scripts, or direct tool commands. Promote them into the task runner only when they
  become part of the repo-wide command contract.
- Keep substantive implementation logic out of `Makefile.toml`. Prefer `command` plus
  `args`; use `script` only for trivial one-line shell that cannot be expressed
  cleanly otherwise. Long shell bodies, `bash -lc`, multi-line `script`, `set -euo
  pipefail`, heredocs, embedded Python/Node/Ruby, loops over repo files, temp-directory
  setup, JSON parsing, database bootstrap/export flows, and other multi-step
  imperative logic belong in purpose-named files under `scripts/`, the owning package's
  scripts, or the owning language module. The task runner should expose the stable
  command contract and call that script.
- A thin task-runner wrapper around one script is allowed, and often preferred, when
  it names a stable repo command, puts it under the correct aggregate, normalizes
  environment or cwd, or gives CI and developers one public entrypoint. Do not remove
  or reject a task merely because it calls a single script.
- Before restructuring a messy task runner, inventory every task in a table:
  `task name | current behavior | purpose or consumer | value added | recommended action`.
- Remove stale, empty, historical, misleading, or name/behavior-mismatched tasks
  instead of preserving them as compatibility aliases.
- When task names or boundaries change, update README, docs, CI, and script references
  in the same change.
- After cargo-make task renames or removals, scan every executable command reference
  surface for stale names: README, docs, CI, scripts, tests, fixtures, generated
  reports, help text, status text, and examples.
- Review task-runner restructures against a small loophole checklist: no legacy
  `checks` aggregate, no undocumented public `[tasks.fix]`, no public `[tasks.*-smoke]`,
  no long inline shell red flags such as `bash -lc`, heredocs, multi-line scripts, or
  `set -euo pipefail`, sorted action-family sections, sorted peer tasks, summary table
  rows matching task order, and deterministic dependency arrays unless required order
  is documented.
- Validate task-runner structure changes with the repository's format check, the
  primary Makefile/cargo-make `check` entry when present, and the smallest reasonable
  affected language or toolchain checks.

## Lifecycle And Landing

- Before a push, pull request, or review handoff, run the smallest project-native
  local checks that can prevent avoidable CI failures.
- Use the repository's owning landing, queueing, or automation surface when it defines
  one. Do not substitute ad hoc merge, enqueue, or raw Git/GitHub merge tools for a
  checked-in landing authority.
- If the configured merge, landing, or automation path is unavailable, blocked, or
  ambiguous, stop.
- When landing after remote CI is already green for the exact head and base, treat CI as the acceptance authority. Verify CI status, head SHA, and base state instead of rerunning local default gates solely because landing is starting.
- Re-run local gates at landing only when local changes followed CI, remote CI evidence is missing or stale, or a failure needs local diagnosis. Do not treat pre-push, PR, review-handoff, or generic default gates as landing gates.

## Validation

- Treat local validation and remote CI as different lifecycle surfaces.
- Select validation by touched surface and risk. Docs-only or metadata-only changes
  should use targeted checks when available instead of full code test suites.
- Broaden beyond targeted checks only for executable docs, generated code,
  build/runtime behavior, or failure evidence points at a code surface.
- A generic checked-in default gate is not by itself evidence that docs-only or metadata-only changes need local full-suite execution.
- When documentation, help output, config examples, telemetry/status wording, runtime
  behavior, or executable behavior can affect each other's truth claims, route to the
  repository's owning drift workflow. Decodex repo-work selects validation scope; it
  does not own drift methodology.
- Passing tests, help output, link checks, smoke scripts, or generated summaries alone
  do not prove semantic consistency. The owning drift workflow must compare claims
  against concrete code, script, help, config, telemetry, or runtime evidence anchors.

## Semantic Naming And Boundary Contracts

- Names are semantic authority, not cosmetic labels. Before renaming a symbol, field,
  status phrase, command, or config key, classify it as an internal fact, external
  contract, UI wording, persisted schema, telemetry/status wording, compatibility
  adapter, or migration surface.
- Rename end-to-end only inside one semantic boundary. Do not mechanically rename
  internal facts to match external presentation contracts, or external contracts to
  match internal storage names, unless the semantic owner really changed.
- Keep distinct names when they describe distinct concepts. Do not force cosmetic
  consistency across runtime facts, UI/readback contracts, persisted schemas, or
  compatibility surfaces.
- Prefer one canonical name inside each owned boundary. Do not create permanent
  alias, translation, or mapping layers to paper over avoidable naming disagreement.
- Allow translation only at explicit contract boundaries such as API compatibility,
  UI presentation, persistence migrations, generated artifacts, or adapter layers.
  Keep that translation local to the boundary and document the compatibility reason.
- When a rename changes a public field, emitted status, CLI flag, telemetry key,
  config key, generated artifact, or persisted schema, reverse-check docs, fixtures,
  examples, tests, migrations, and readbacks for stale or over-broad terms.

## Architecture And Cutover Defaults

- Default to the root-cause fix and the simplest single canonical design that fits
  the current checked-in contract. This is the default single-canonical-design and
  no-permanent-compatibility-layer policy. Do not wait for the user to ask for
  "no compat" or "clean architecture" before challenging fallback paths, duplicate ownership, or partial repairs.
- Prefer deleting obsolete code, config fields, commands, fixtures, docs, aliases,
  and tests in the same change that replaces their authority. Do not leave old paths
  around only to reduce short-term diff size or preserve agent comfort.
- Do not introduce permanent compatibility shims, legacy aliases, dual-read/dual-write
  paths, hidden fallback defaults, or parallel implementations unless an explicit
  external contract, persisted-data migration, production rollout boundary, or user
  instruction requires them.
- When compatibility is required, keep it local to the boundary it protects, document
  the reason, provide an explicit migration or removal condition, and test both the
  protected boundary and the canonical path.
- Before calling a fix complete, ask whether the change removed the underlying
  ambiguity or merely added another layer. If it added another layer without a
  bounded compatibility reason, keep simplifying toward one owner, one name, one config contract, and one execution path.

## Skill And Plugin Changes

- Treat Codex skill and plugin edits as executable workflow changes, not docs-only
  prose. They change future agent behavior even when they are Markdown-only.
- After local checks and any owner-required drift review, run plugin evaluation before
  claiming a skill or plugin change is done, fixed, ready, or verified.
- For a standalone skill, run `plugin-eval analyze <skill-root> --format markdown`.
- For a plugin that contains multiple skills or shared references/scripts, run
  `plugin-eval analyze <plugin-root> --format markdown` so nested skills and manifest
  routing are evaluated together.
- If `plugin-eval` is unavailable, use the repository's checked-in fallback command
  when one exists. Otherwise, report the missing eval tool as a limitation instead of
  silently substituting an unrelated test.
- Include the eval verdict or main score, the highest-priority finding, and any
  remaining limitation in the final evidence.

## Configuration Contracts

- Config files are explicit user-facing contracts, not hidden implementation hints.
- Treat the missing-field, unknown-field, and fallback-default policy as one
  configuration contract.
- For every supported configuration contract, every supported field must appear in the
  schema, documented examples, templates, fixtures, and readbacks that teach operators
  what the config means.
- missing fields must not use hidden fallback defaults. Prefer a clear "add this field" error
  that names the field, the owning config file, and the smallest safe migration.
- unknown fields must be rejected instead of ignored so operators know exactly what
  they are configuring.
- When changing a missing-field, unknown-field, or fallback-default policy, update
  examples, templates, schemas, fixtures, and readbacks in the same change.
- Existing configs that need a new required field must get an explicit migration path;
  never silent inference.

## Evidence To Report

- Whether `Makefile.toml` existed for the touched project and which task, if any,
  owned formatting, linting, testing, building, or code generation.
- Whether task-runner edits preserved the primary `check` aggregate and kept
  toolchain-aligned naming across language, package-manager, or toolchain boundaries.
- Whether validation was local pre-push, PR, or review-handoff risk reduction; remote CI acceptance for landing; or targeted docs/metadata verification.
- Why the chosen validation scope matched the touched files, risk, and current CI evidence.
- Whether an owning drift workflow was required, with pass, fail, or needs-human
  verdict when it was run.
- For Codex skill or plugin changes, the plugin-eval command and result summary.
- Which owner workflow supplied debugging evidence, boundary, hypothesis,
  original-symptom check, or honest gap for a bug-fix path when one was required.
- Which verification evidence supports any done, fixed, passing, ready, landed, closed out, or verified claim, and which claims were intentionally downgraded.
