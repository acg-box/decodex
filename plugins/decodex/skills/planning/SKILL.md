---
name: planning
description: Use when shaping Decodex-friendly issue sets, queue strategy, project capacity, dependencies, or concurrency. Helps agents split work so retained lanes can run independently without overlapping ownership or bypassing registered project policy.
---

# Planning

## Goal

Shape work so Decodex can run the right lanes in parallel while each issue remains
independently executable, reviewable, and recoverable.

Use this before queueing a broad feature, migration, or cleanup effort into Decodex.
For durable issue text, pair this skill with the delivery plugin's `split` or `issue`
skill, then use the `labels` skill when applying Decodex intake labels.

## Read First

- The registered project `project.toml` for `service_id`, repo path, worktree root,
  and credential environment-variable names.
- The registered project `WORKFLOW.md` for startable states, terminal states,
  dependency policy, validation commands, gate profiles, and context files.
- `docs/spec/runtime.md` for eligibility, one-lane-per-issue ownership, and dispatch
  slots.
- `docs/spec/workflow-file.md` for `max_concurrent_agents`, gate profile semantics,
  and workspace hooks.
- `docs/spec/owned-lane-policy.md` and `docs/spec/post-review-lifecycle.md` for
  retained review, landing, closeout, cleanup, and manual-intervention boundaries.
- `docs/reference/operator-control-plane.md` when using `status` or the dashboard to
  understand current queue, active lanes, review waits, and cleanup debt.

## Good Decodex Issues

Each issue should have:

- one concrete outcome that can land as one PR-backed lane
- required reading and current authority files
- explicit scope and non-goals
- a narrow landing zone, preferably one module, service, docs lane, or workflow seam
- acceptance criteria that can be checked without asking product intent again
- validation commands or the applicable `WORKFLOW.md` gate profile
- explicit blockers or dependency issues when the issue cannot start yet
- enough natural-language briefing in the Linear description for generic dispatch

Do not make the description only a machine-readable fenced block. Generic normal
dispatch requires a usable briefing surface.

## Parallelism Rules

- Split by ownership boundary, validation surface, or deployable behavior, not by
  arbitrary chronology.
- Prefer several independent queued issues over one broad issue when their file sets,
  contracts, and verification can be isolated.
- Put shared contracts, schema changes, and cross-cutting migrations in a foundation
  issue first; queue downstream slices only after the blocking contract lands or mark
  the dependency explicitly in the tracker.
- Avoid queueing issues that are likely to edit the same hot files, branch lineage,
  config authority, or generated artifacts unless the ordering is explicit.
- Keep one issue responsible for any user-facing wording or spec contract that several
  implementation slices depend on, then link the other issues to that authority.
- Use `decodex:queued:<service-id>` only for issues that are startable under the
  registered `WORKFLOW.md`; the label does not bypass state, blocker, terminal-state,
  active-lease, or capacity checks.
- Set `[execution] max_concurrent_agents = 0` only when the project is intentionally
  uncapped. Use a positive value when the repo, accounts, CI budget, or review surface
  needs bounded parallelism.

## Queue Shaping

1. Use `decodex status` or the dashboard to read active lanes, queued candidates,
   retry waits, review waits, landing state, recovery worktrees, cleanup debt, and
   available capacity.
2. Use `decodex run --dry-run` to confirm project loading, issue discovery,
   eligibility, dependency blockers, and worktree planning before live automation.
3. Queue only the next independent slice set. Leave future or blocked slices unqueued
   until the dependency is terminal or the `WORKFLOW.md` policy makes them startable.
4. If capacity is finite, keep the highest-value independent slices queued first
   instead of flooding the queue with dependent work.
5. When a lane stops with `decodex:needs-attention`, resolve the recorded blocker
   before clearing the label or re-queueing.

## Boundaries

- This skill does not replace project-local `WORKFLOW.md` policy or runtime
  eligibility.
- Do not use planning guidance to manually mutate active labels, runtime DB rows, or
  retained worktrees.
- Do not convert a human-driven manual task into retained-lane automation unless the
  user asks for automation or the registered workflow requires it.
- Do not use parallelism as a reason to split one atomic behavior across dependent
  issues that cannot be validated or reviewed independently.
