# Commit Message Specification

Purpose: Define the minimal machine-readable `decodex/commit/1` contract for Decodex-managed local commits and Decodex-directed manual landing change records.
Status: normative
Read this when: You are authoring, validating, or consuming `decodex/commit/1` records intended to describe repository changes in machine-managed history or manual landing receipts.
Not this document: Landing policy, PR merge rules, CI policy, or post-merge closeout state.
Defines: The `decodex/commit/1` schema, required fields, optional fields, and forbidden process-state content.

## Scope

- `decodex/commit/1` describes one tree change.
- `decodex/commit/1` does not describe how that change lands, whether CI passed, or how tracker closeout finished.
- Landing, CI, and closeout are separate runtime concerns.
- `decodex commit` writes `decodex/commit/1` into the local Git commit message.
- `decodex land` reuses the same `decodex/commit/1` shape for the landed-change record it writes during manual closeout, and manual landing uses that same record as the admin-merge subject.

## Canonical shape

Commit messages should be a single-line JSON object with this shape:

```json
{
  "schema": "decodex/commit/1",
  "summary": "inline retained closeout policy",
  "authority": "XY-180",
  "related": ["XY-181", "XY-209"],
  "breaking": true
}
```

## Required fields

- `schema`
  - type: string
  - required exact value: `"decodex/commit/1"`
- `summary`
  - type: string
  - required
  - meaning: the stable semantic summary of the tree change
- `authority`
  - type: string
  - required
  - allowed values:
    - a Linear issue identifier such as `XY-180`
    - reserved literal `"manual"` when the commit or land was created through explicit manual-authority mode
  - meaning: the primary work item that authorizes the change, or an explicit manual lane with no authoritative Linear issue

## Optional fields

- `related`
  - type: array of string
  - optional
  - meaning: additional related work items
- `breaking`
  - type: boolean
  - optional
  - default interpretation when omitted: `false`

## Forbidden content

The commit contract must not encode process-state fields such as:

- landing mode or merge method
- CI or check status
- validation digests
- closeout mode or lifecycle phase
- mirroring state
- execution logs or retry state

Those values are runtime or integration-event concerns, not tree-change semantics.

## Manual-authority mode

- `decodex commit --manual-authority ...` and `decodex land --manual-authority ...` are the only supported ways to produce `authority = "manual"`.
- `--authority manual` is not a supported synonym; the reserved literal exists so downstream consumers can distinguish an intentional manual lane from a malformed issue identifier.
- `related` entries remain issue identifiers even when `authority = "manual"`.
