---
name: docs-drift
description: Use when auditing this repository's docs claims against code, evidence, and runtime behavior.
---

# Decodex Docs Drift

Audit Decodex docs claims against source evidence. For portable OKF graph or
frontmatter quality checks, use `okf` or `okf-query`; this skill owns Decodex
semantic drift and completion blocking.

Read `../../references/docs-drift.md` before judging docs/code alignment.

- Use direct evidence anchors: code, tests, checked-in config, CLI help, command
  output, or runtime observations.
- Reverse-check stale command names, flags, statuses, schemas, labels, and old docs
  artifact names.
- Record `pass`, `fail`, or `needs-human`.
- Treat `fail` as a ready/review handoff blocker.
