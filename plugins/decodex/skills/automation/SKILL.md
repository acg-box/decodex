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
- When an MCP client is available, use `decodex_observe` and remote-safe resources for
  readback, `decodex_lane_control` for inspect-first steer/interrupt requests, and
  `decodex_project_control` only for project status or future-dispatch pause/resume.
- Use Program Intake for accepted Program work.
- Use `labels` only for ordinary non-Program issue intake.
- Treat `decodex:needs-attention` and `terminal_pending` as stop signals.
- Do not edit runtime DB rows, hidden children, Linear state, or retained worktrees
  from the side.
