# Decodex Context Gates

Use this when Decodex work needs autonomous OKF/LLM Wiki context intake or the docs
completion gate. These gates are Decodex-owned plugin behavior; host instructions may
route here, but no other plugin is required to know this method.

## Context Intake

- For non-trivial Decodex repository work, route the task intent before planning or
  implementation when the docs bundle is available:
  `decodex okf route docs "<task intent>" --limit 5`.
- Read the clearly relevant owner concept before choosing the change path.
- Carry a short `Context anchors` note into plans, issue briefs, handoffs, or review
  summaries when the route result materially shaped the work.
- Skip the intake route only for tiny single-file tasks, explicit user-provided file
  targets, unavailable bundles, or clearly noisy route results. Record the skip or
  route miss instead of forcing irrelevant context.
- For portable bundles outside this repo, use `decodex okf route <root> "<task
  intent>" --limit 5` and the `okf-query`, `repo-memory-evaluator`, or
  `repo-memory-curator` skill that matches the result.

## Docs Completion Gate

- When Decodex work touches `docs/`, documented behavior, CLI/status/config/workflow
  text, research, semantic names, or repo-memory metadata, use `docs` before changing
  the docs surface when feasible and before any done, fixed, ready, commit, handoff,
  landed, or verified claim.
- The docs gate reads `docs-method.md`, `docs/index.md`, `docs/policy.md`, and the
  owning concept; classifies docs impact as `none`, `update_required`,
  `research_required`, or `drift_required`; and runs `decodex docs check`.
- If the docs skill was skipped and discovered late, treat that as a process gap:
  read the same docs method, index, policy, and owning concept; classify docs impact;
  run `decodex docs check`; and report the recovered evidence.

## Boundary

- Context intake improves retrieval; it is not implementation authority by itself.
- Docs readiness is a completion gate only for Decodex-owned docs or documented
  behavior. Portable OKF bundles use their selected profile check instead.
- Do not copy these gates into generic repo-work plugins. Keep cross-plugin
  composition in host instructions and keep Decodex execution details here.
