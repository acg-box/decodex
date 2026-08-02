# Codex Upstream Adaptation

This runbook defines the agent-led loop that keeps Decodex compatible with the
latest official Codex behavior. It is independent of the Decodex server and
runtime.

## Authority

`automations/portfolio.toml` is the only checked-in portfolio authority. The
upstream loop has three roles:

| Role | Model | Effort | Frequency |
| --- | --- | --- | --- |
| Maintainer | `gpt-5.6-sol` | `max` | Every 6 hours |
| Reviewer | `gpt-5.6-sol` | `max` | Every 12 hours |
| Manager | `gpt-5.6-terra` | `high` | Twice daily |

## Activation State

`PAUSED` is the initial acceptance state. Runtime evaluation always compares
native status with the current manifest exactly, and Manager repairs all five
native definitions to that value. If the current value is `PAUSED`, Manager must
not activate a definition. `ACTIVE` is valid only after the signed promotion.

First land the portfolio with `status = "PAUSED"`. Run live acceptance only by
explicit one-shot manual invocation. After all non-activation acceptance evidence
passes, signed-land the one-line manifest change to `status = "ACTIVE"`.
Manager/native sync can then activate all five definitions. No activation
workflow engine or extra state exists.

All roles start in the clean primary checkout at
`/Users/x/code/y/hack-ink/decodex`. Temporary task worktrees are execution
details, never scheduled cwd or durable workflow state.

Agents own technical judgment. Bounded code validates the exact-five portfolio,
signed commits, signed landing, and external effects. No repository script owns
research, issue selection, implementation planning, review, or repair routing.

## Workflow State

The loop uses standard Git and GitHub state:

- the official upstream Codex ref and release identity;
- branch `xv/codex-upstream-<12-lowercase-head-hex>`;
- one open PR for the upstream head;
- PR checks, review comments, base ref, and exact head ref;
- signed implementation commits;
- signed merge commit and exact merge-tree readback.

There is no candidate file, cursor database, lease, handoff receipt, effect
registry, or repair queue. A rerun reconstructs current work from upstream, Git,
GitHub, and tests.

## Maintainer Run

1. Read `AGENTS.md`, OpenWiki, this runbook, and the relevant project tests.
2. Require clean primary `main` equal to `origin/main`.
3. Inspect official Codex releases, source changes, protocol schemas, and
   documentation since the latest Decodex adaptation.
4. Compare upstream behavior with current Decodex code and tests. State the
   concrete user or operator consequence.
5. Search GitHub for the deterministic branch, upstream-head trailer, and an
   existing matching PR.
6. Return a source-backed no-op when no compatibility or adoption change is
   useful.
7. For one change, create a temporary task worktree and dispatch one native
   ephemeral Sol/max subagent. Give it the exact upstream head, expected outcome,
   affected boundaries, required tests, and stop conditions.
8. Review the result. Run focused tests first and broaden tests in proportion to
   the affected surface.
9. Use `decodex commit` for every implementation commit. Do not use Decodex
   server, runtime, queue, planner, or MCP.
10. Create or update the one matching PR. Verify its exact remote head and the
    trailer `Upstream-Codex-Head: <oid>`.
11. Remove the temporary worktree only after the remote PR state is read back.

A failed check, stale base, or Reviewer request is ordinary autonomous work on
the same PR. The Maintainer updates that PR and does not create a parallel task
record.

## Reviewer Run

1. Enumerate open PRs with the upstream trailer and deterministic branch.
2. Read the complete diff, upstream evidence, review history, and check results.
3. Record exact base and head OIDs. Create a temporary review worktree at that
   head.
4. Review independently for correctness, scope, removed obsolete support,
   security, and test quality.
5. Run the focused tests and the appropriate repository gate.
6. When a defect exists, submit precise GitHub review feedback with a required
   outcome. Leave the PR open for Maintainer repair.
7. When the head is acceptable, invoke only:

```sh
decodex land --manual-authority --pr <url> \
  --expected-base-oid <base> \
  --expected-head-oid <head>
```

8. Read remote `main` again. Require the signed merge parents to be the reviewed
   base and head, and require the merge tree to equal the reviewed head tree.
9. Remove the review worktree.

Exact base/head arguments and merge readback make a retry adopt the same result
instead of creating a second landing effect.

## Manager Run

The Manager evaluates the complete operating system, not only prompt syntax.
Every run checks:

- all five native definitions against `automations/portfolio.toml`;
- exact models, efforts, schedules, local execution, status, and primary cwd;
- duplicate managed definitions and missing native metadata;
- upstream release or protocol changes not yet investigated;
- detection-to-PR and PR-to-land latency;
- stale PRs, failed checks, review feedback, and merged outcomes;
- content evidence, X results, due observations, and cost;
- repeated failure causes and whether the previous repair improved the result.

The target service levels are:

- detect an actionable official change within 6 hours;
- open or update its PR within 12 hours;
- land a passing reviewed change within 24 hours.

The Manager repairs native-definition drift directly through native automation
tools and verifies full readback. Repository repairs use one ephemeral Sol/max
subagent and the normal Maintainer/Reviewer PR path.

The Manager uses native task listing and readback. It archives only a completed
successful task that has terminal evidence and no unresolved external effect,
failure, continuation, or decision request. Failed, active, ambiguous, and
human-decision tasks stay visible.

## Human Stop Conditions

Routine implementation defects are not human blockers. Human attention is valid
only when an external authority is unavailable or a real policy decision is
required. Examples are:

- GitHub or repository authority is unavailable;
- required OAuth is missing;
- an X create may have succeeded but its result is unknown;
- two valid product directions need an owner decision.

## Validation

```sh
python3 automations/decodex/scripts/config/evaluate_automations.py --repo-only --json
cargo make test-automations
cargo make check-automations
```

The first two commands do not read or change native automation state. The
Manager may run the evaluator without `--repo-only` for read-only drift evidence.
