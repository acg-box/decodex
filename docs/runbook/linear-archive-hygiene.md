---
type: "Runbook"
title: "Linear Archive Hygiene"
description: "Procedure for archiving old terminal Linear issues without disturbing active Decodex lanes."
status: active
authority: procedural
owner: automation
tags: [runbook]
last_verified: 2026-06-16
---
# Linear Archive Hygiene

Goal: Archive old terminal Linear issues without touching active Decodex lanes,
queued intake, review handoff, recovery ownership, or unrelated repo labels.

Read this when:

- Linear issue volume is high before a demo, large issue seed, or backlog reset.
- You need a dry-run list of terminal issues that are old enough to archive.
- You need to scope cleanup to a repo label such as `repo:decodex` or
  `repo:ashen-vale`.

Preconditions:

- Register the target project first with `decodex project add <config>` or pass `--config`.
- The registered project config tracker credential must point at the routed Linear
  workspace identity, such as `LINEAR_API_KEY_HACKINK` for the `y`/`hackink`
  route.
- The issue must carry a repo label beginning with `repo:`.

Depends on:

- Terminal states from `WORKFLOW.md` `[tracker].terminal_states`.
- Protected labels from the same workflow policy:
  `decodex:queued:<service-id>`, `decodex:active:<service-id>`,
  `decodex:needs-attention`, and `decodex:manual-only`.

Verification:

- Dry run first and inspect every candidate.
- Re-run the dry run after execution if you need an empty candidate list.

## Dry Run

Use the dry-run default before demos or large issue seeding:

```sh
decodex archive-linear --repo-label repo:decodex --older-than-days 30
```

The command prints the terminal issues that would be archived, using `updatedAt`
as the age cutoff. It does not mutate Linear unless `--execute` is present.

For another Decodex-managed repo, run from that registered checkout or pass its centralized config:

```sh
decodex archive-linear --config ~/.codex/decodex/projects/ashen-vale --repo-label repo:ashen-vale --older-than-days 30
```

## Execute

After the dry run shows only issues that should leave the active tracker view,
repeat the command with `--execute`:

```sh
decodex archive-linear --repo-label repo:decodex --older-than-days 30 --execute
```

This archives issues through Linear `issueArchive` with `trash = false`. It does
not delete issues.

## Exclusions

The archive plan skips an issue when any of these are true:

- The issue state is not one of the configured terminal states, such as `Done`,
  `Canceled`, or `Duplicate`.
- The issue was updated after the cutoff.
- The issue still has `decodex:active:<service-id>` ownership.
- The issue is still queued with `decodex:queued:<service-id>`.
- The issue is marked `decodex:needs-attention`.
- The issue is marked `decodex:manual-only`.

These exclusions keep active, queued, in-review, needs-attention, manual-only,
and retained recovery lanes out of archive hygiene.
