---
name: labels
description: Use when applying, clearing, or interpreting Decodex Linear labels that control automation intake, active ownership, opt-out, or human-required stops. Covers service-scoped queued and active labels plus manual-only and needs-attention labels.
---

# Labels

## Goal

Handle Decodex-related Linear labels without changing runtime ownership by accident.

Labels are retained-lane intake and ownership signals. They are not the user-facing
research/design workflow and they do not promote latent Decision Contracts by
themselves.

## Label Catalog

| Label | Meaning |
| --- | --- |
| `decodex:queued:<service-id>` | Candidate for automatic intake by one registered Decodex service. |
| `decodex:active:<service-id>` | Runtime-owned active lane marker for one registered service. |
| `decodex:manual-only` | Human opt-out from automatic selection. |
| `decodex:needs-attention` | Automation stopped and a human must resolve the blocker. |

## Service ID

For `queued` and `active`, read `<service-id>` from the registered project
`project.toml`. Do not guess it from repository name, branch name, issue number, or
Linear project.

The project config is usually under:

```sh
$HOME/.codex/decodex/projects/<service-id>/project.toml
```

If a project-scoped command supplies `--config <project-dir>`, read that project's
`project.toml` instead.

## Human Label Actions

- Humans may add or clear `decodex:queued:<service-id>` when deciding whether a
  specific service should intake the issue.
- Humans may add or clear `decodex:manual-only`.
- Humans may clear `decodex:needs-attention` only after addressing the underlying
  blocker recorded by the runtime or agent.
- `decodex recover review-handoff rebind` may clear `decodex:needs-attention` itself
  only when a current same-PR same-head handoff marker proves stale failure-state drift.
- Humans should not normally add or clear `decodex:active:<service-id>` unless doing
  explicit recovery and the runtime-owned state has been verified.
- For `recover review-handoff adopt`, do not hand-add
  `decodex:active:<service-id>` just to satisfy the command. The dry-run reports
  missing active ownership and whether live adopt will restore it. The live command
  restores the active label only after validating the issue, service ID, managed
  worktree, PR branch, and PR head belong to the same manual takeover lane.
- For `recover merged-closeout`, queue, active, and needs-attention labels must already
  be absent. The command reconciles stale runtime/ledger attention after merged PR
  proof; it does not clear labels as a shortcut.

## Queue an Issue

1. Read the target service ID from the registered project config.
2. Read the registered project `WORKFLOW.md` for startable states, terminal states,
   dependency policy, and other eligibility constraints.
3. Ensure the issue is intended to be startable for that service; the queued label is
   only an intake signal and does not bypass `WORKFLOW.md` eligibility, terminal-state
   checks, dependency checks, or active-lease checks.
4. Ensure `decodex:manual-only` is absent.
5. Ensure any prior blocker behind `decodex:needs-attention` is resolved.
6. For research-to-execution work, ensure the source is an accepted/promoted Decision
   Contract or an equivalent explicit human execution instruction, not only a plain
   `research X` result or latent contract.
7. Add `decodex:queued:<service-id>`.

## Pause or Opt Out

1. Remove `decodex:queued:<service-id>`.
2. Add `decodex:manual-only` if the issue should stay manual until someone deliberately
   opts it back in.

## Resume After Attention

1. Read the failure or attention comment first.
2. Resolve the underlying blocker.
3. Clear `decodex:needs-attention`, or use `decodex recover review-handoff rebind`
   when the blocker is verified current-marker failure-state drift.
4. Re-add `decodex:queued:<service-id>` only when the issue should resume automation.

## Boundaries

- Do not use `decodex:active:<service-id>` to mean "please start work".
- Do not clear `decodex:needs-attention` just to silence a failed lane.
- Do not add a service-scoped label for the wrong registered service.
- Do not add `decodex:active:<service-id>` just to make `decodex land` pass; use it
  only for verified retained-lane recovery or manual PR takeover adopt.
- Do not ask ordinary users to apply queue labels, mention DAG/goal mechanics, or
  manage internal readiness state just to move from research to execution; route that
  through promotion, planning, and automation policy.
- Use `land` when the task is really about human-driven PR landing.
- Use `commit` when the task is really about human-driven commit creation.
