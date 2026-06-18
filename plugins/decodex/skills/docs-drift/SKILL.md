---
name: docs-drift
description: Use when auditing Decodex semantic drift across docs claims, code, commands, config, evidence, generated artifacts, status text, and runtime behavior.
---

# Decodex Docs Drift

Audit Decodex docs and public claims against source evidence. For portable OKF graph
or frontmatter quality checks, use `okf` or `okf-query`; this skill owns Decodex
semantic drift and completion blocking.

Read `../../references/docs-drift.md` before judging docs/code alignment.

- Use direct evidence anchors: code, tests, checked-in config, CLI help, command
  output, or runtime observations.
- Reverse-check stale command names, flags, statuses, schemas, labels, and old docs
  artifact names.
- Record `pass`, `fail`, or `needs-human`.
- Treat `fail` as a ready/review handoff blocker.
- Use `../../scripts/semantic_drift_audit.py` when a git diff packet would speed up
  changed-claim, removed-term, or stale-phrase discovery. The helper collects
  candidates only; the agent still owns the verdict.
