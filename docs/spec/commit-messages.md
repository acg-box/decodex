---
type: "Spec"
title: "Commit Message Specification"
description: "Define the minimal machine-readable `decodex/commit/2` contract for Decodex-managed local commits and landed merge subjects. Status: normative Read this when: You are authoring, validating, or consuming `decodex/commit/2` records intended to describe repository changes in machine-managed history. Not this document: Landing policy, PR merge rules, CI policy, related issue modeling, or post-merge closeout state. Defines: The `decodex/commit/2` schema, required fields, and forbidden process-state content."
status: active
authority: normative
owner: runtime
tags: [spec]
last_verified: 2026-07-07
---
# Commit Message Specification

Purpose: Define the minimal machine-readable `decodex/commit/2` contract for Decodex-managed local commits and landed merge subjects.
Status: normative
Read this when: You are authoring, validating, or consuming `decodex/commit/2` records intended to describe repository changes in machine-managed history.
Not this document: Landing policy, PR merge rules, CI policy, related issue modeling, or post-merge closeout state.
Defines: The `decodex/commit/2` schema, required fields, and forbidden process-state content.

## Scope

- `decodex/commit/2` describes one tree change.
- `decodex/commit/2` is commit-local and contains only `change`, `authority`, and
  `impact`.
- Related issues, source/base/head branches, PR URLs, landing state, CI/check state,
  closeout state, cleanup state, and lifecycle transitions belong to runtime authority
  records or issue/project metadata, not commit subjects.
- `decodex commit` writes `decodex/commit/2` into the local Git commit message.
- `decodex land` may use a `decodex/commit/2` subject for the merge commit, but that
  subject remains a tree-change description and does not become landing authority.

## Canonical shape

Commit messages should be a single-line JSON object with this shape:

```json
{
  "schema": "decodex/commit/2",
  "change": "inline retained closeout policy",
  "authority": "XY-180",
  "impact": "compatible"
}
```

## Required fields

- `schema`
  - type: string
  - required exact value: `"decodex/commit/2"`
- `change`
  - type: string
  - required
  - meaning: the stable semantic summary of the tree change
- `authority`
  - type: string
  - required
  - allowed values:
    - a Linear issue identifier such as `XY-180`
    - reserved literal `"baseline"` for Decodex-owned baseline normalization commits
      and merge subjects that are intentionally not backed by a Linear issue
    - reserved literal `"manual"` when the commit or land was created through explicit manual-authority mode
  - meaning: the primary work item that authorizes the change, or an explicit manual lane with no authoritative Linear issue
- `impact`
  - type: string
  - required values: `compatible` or `breaking`
  - meaning: whether the tree change is expected to preserve downstream compatibility

## Forbidden content

The commit contract must not encode process-state fields such as:

- landing mode or merge method
- related issues or source issue relationships
- source, base, or head branch names
- PR URL or PR number
- CI or check status
- validation digests
- closeout mode or lifecycle phase
- closeout or cleanup status
- mirroring state
- execution logs or retry state

Those values are runtime or integration-event concerns, not tree-change semantics.

## Manual-authority mode

- `decodex commit --manual-authority ...` and `decodex land --manual-authority ...` are the only supported ways to produce `authority = "manual"`.
- `--authority manual` is not a supported synonym; the reserved literal exists so downstream consumers can distinguish an intentional manual lane from a malformed issue identifier.
- Non-issue `decodex land --manual-authority --pr <URL>` may leave only a local
  receipt outside issue lifecycle authority. Issue-authority landing must use runtime
  lifecycle authority records for final landing and closeout state.
- `decodex commit` is an operator helper, not an active child-run writer. When the
  current checkout matches a runtime-recorded Decodex lane worktree and that issue has a
  live runtime claim, the helper must refuse the commit, including
  `--manual-authority` commits. The operator should steer or interrupt the owning run,
  wait for it to finish, or clear retained ownership before using the helper.
