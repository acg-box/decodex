# Automatic review and retained records

The user accepted Chief-managed review on September 16, 2026. The user should not
select reviewers, carry context, route fixes, or maintain a review log.

## Default behavior

- Chief inspects results and relevant validation for small reversible changes.
- Substantial changes, cross-module behavior, persistent data, and consequential
  external effects get a separate review worker before acceptance.
- Review scope identifies the actual change or artifact and acceptance criteria.
- Actionable findings return to the original execution worker. A focused recheck
  covers the repaired result. Explicit user scope constraints take precedence.
- Repeated unsuccessful repair produces a concise blocker, not an infinite loop.

## Records and navigation

Use the existing durable worker conversations, instruction events, completion
receipts, and Chief disposition notes. The assessment identifies scope, version,
reviewer, findings, handling, validation evidence, and remaining uncertainty.
Earlier adverse findings remain in their original conversations. A later change
can invalidate an earlier review. Do not confuse completion, no findings, passing
tests, and Chief acceptance.

The main conversation gives a short result and identifies the review work. Users
open that worker from the existing agent tree. This change adds no Review page,
new database schema, or automatic task deletion. A dedicated structured review
card, findings index, and automatic finding-to-diff navigation are not implemented.

## Implementation boundary

`chief/instructions.md` defines model-driven coordination policy. It is installed
in new manager threads and native resume parameters. It is not a deterministic
mandatory-review gate and does not grant external-action authority. Existing
storage and tool checks remain authoritative. Live compliance by an actual
reviewer has not been tested by creating a new user task.
