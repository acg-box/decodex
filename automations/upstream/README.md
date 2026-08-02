# Decodex Agent Automations

`automations/portfolio.toml` is the only checked-in definition of the managed
automation portfolio. It declares exactly five native Codex tasks:

| ID | Responsibility | Model | Effort | Schedule |
| --- | --- | --- | --- | --- |
| `codex-upstream-maintainer` | Research and implement Codex compatibility work | `gpt-5.6-sol` | `max` | Every 6 hours |
| `codex-upstream-reviewer` | Independently review, test, and land upstream PRs | `gpt-5.6-sol` | `max` | Every 12 hours |
| `codex-upstream-health` | Manage portfolio quality and repair drift | `gpt-5.6-terra` | `high` | Twice daily |
| `decodex-content-manager` | Research and record one content decision | `gpt-5.6-terra` | `high` | Daily |
| `decodex-xurl-publisher` | Publish or observe through the X safety boundary | `gpt-5.6-luna` | `high` | Three times daily |

No managed task uses `xhigh`. Every native definition has local execution and
uses `/Users/x/code/y/hack-ink/decodex` as its scheduled cwd. A scheduled task
must never use a task worktree as its cwd.

## Operating Model

Agents own research, diagnosis, planning, implementation, review, writing, and
iteration. Repository code does not model these judgments as a queue or state
machine. Scripts exist only for bounded validation and irreversible-effect
boundaries.

Standard external state is sufficient:

- GitHub PRs, refs, review comments, checks, and merge state;
- Git commits and exact tree identities;
- signed `decodex commit` receipts;
- signed `decodex land` merge readback;
- native Codex task definitions and task status;
- private Publisher evidence for X writes and reads.

There is no upstream candidate database, lease, handoff, effect journal,
recursive repair queue, Decodex server, Decodex runtime, planner, or MCP in this
automation loop.

## Maintainer

The Maintainer reads official Codex releases, source, protocol schemas, and
documentation. It compares those sources with current Decodex behavior and
tests. A no-change result is valid only after evidence-based inspection.

For one concrete change, the Maintainer:

1. Refreshes clean primary `main` and records its exact base.
2. Uses branch `xv/codex-upstream-<12-lowercase-head-hex>` for the upstream head.
3. Reuses the matching open PR when it exists. It does not create a duplicate.
4. Creates one temporary task worktree and dispatches one native ephemeral
   Sol/max subagent with a precise outcome brief.
5. Reviews the implementation, runs focused and proportional tests, and uses
   `decodex commit` for signed commits.
6. Creates or updates the deterministic PR and verifies its remote head and
   `Upstream-Codex-Head: <oid>` trailer.
7. Removes the temporary worktree after the remote PR state is verified.

Review feedback, stale bases, and test failures return to this same PR. They do
not create a second workflow record and do not require routine human attention.

## Reviewer

The Reviewer independently reads the PR diff and its upstream evidence. It
checks out the exact remote head in a temporary review worktree, reruns the
required tests, and verifies the signed commit chain.

Defects become precise GitHub review feedback for the Maintainer. An accepted
head lands only through:

```text
decodex land --manual-authority --pr <url> \
  --expected-base-oid <base> --expected-head-oid <head>
```

Success requires remote `main` to contain the signed merge, the merge parents to
match the reviewed base and head, and the merge tree to equal the reviewed head
tree. This exact readback makes landing retries idempotent.

## Manager

The Manager reads and repairs all five native definitions through native
automation tools. It audits:

- exact-five identity, model, effort, schedule, status, execution environment,
  and primary cwd;
- detection-to-PR, PR-to-land, failed checks, review cycles, and merged results;
- upstream adaptation latency and missed official changes;
- content evidence, X publication, 24-hour and 7-day outcomes, and monthly cost;
- repeated causes and configuration drift.

The Manager archives only completed successful Codex tasks with terminal
readback and no unresolved effect or user decision. Failed, active, ambiguous,
or human-decision tasks stay visible. Archiving uses native task tools directly;
there is no receipt or retention state machine.

## Validation

Render the five complete native definitions without changing runtime state:

```sh
python3 automations/decodex/scripts/config/render_automation_plan.py --json
```

Validate checked-in authority:

```sh
python3 automations/decodex/scripts/config/evaluate_automations.py --repo-only --json
cargo make test-automations
```

Validate native state read-only when an operator or Manager needs drift evidence:

```sh
python3 automations/decodex/scripts/config/evaluate_automations.py --json
```

Use `cargo make check-automations` for the complete headless repository gate when
the patch does not touch GPUI or Apple build integration.
