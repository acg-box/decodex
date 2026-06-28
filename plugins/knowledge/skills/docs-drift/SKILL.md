---
name: docs-drift
description: Use when docs claims may drift from code, commands, config, evidence, generated artifacts, status text, runtime behavior, or agent instructions.
---

# Docs Drift

Audit docs and public claims against source evidence. For portable OKF graph or
frontmatter quality checks, use `$knowledge:okf`; this skill owns semantic drift
verdicts and completion blocking.

Read `../../references/docs-drift.md` before judging docs/code alignment.

- Use direct evidence anchors: code, tests, checked-in config, CLI help, command
  output, or runtime observations.
- Reverse-check stale command names, flags, statuses, schemas, labels, and old docs
  artifact names.
- Record `pass`, `fail`, or `needs-human`.
- Treat `fail` as a ready/review handoff blocker.
- If the repository has `scripts/semantic-drift/semantic_drift_audit.py`, use it
  when a git diff packet would speed up changed-claim, removed-term, or stale-phrase
  discovery. The helper collects candidates only; the agent still owns the verdict.
