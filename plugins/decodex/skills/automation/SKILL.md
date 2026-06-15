---
name: automation
description: Use when Decodex owns retained automation.
---

# Automation

Operate Decodex retained lanes after execution authority exists. Read
`../../references/routing.md` for Program Intake, labels, recovery, and lane-control
details.

- Confirm authority before tracker or runtime mutation.
- Inspect `decodex status` or `decodex lane inspect <ISSUE>` first.
- Use Program Intake for accepted Program work.
- Use `labels` only for ordinary non-Program issue intake.
- Treat `decodex:needs-attention` and `terminal_pending` as stop signals.
- Do not edit runtime DB rows, hidden children, Linear state, or retained worktrees
  from the side.
