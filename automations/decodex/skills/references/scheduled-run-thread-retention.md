# Scheduled Run Task Retention

This policy controls the Codex task created for one scheduled run. Archival cleans
the Codex task list. It does not pause or delete the recurring automation, and it
does not delete local evidence.

Before the final report, classify the current run:

- `auto_archive`: The run reached a terminal outcome for its ownership scope. Every
  intended write and external effect has a durable receipt and readback. A retry,
  blocked result, or failed validation is terminal for the run when an exact durable
  automatic repair owner or successor record exists. An exact persisted upstream
  side-effect intent is terminal for the current run only when it has no unknown
  external result and a bounded automatic retry owner owns that same intent. No
  lease, browser handoff, ambiguous side effect, active tool call, or human decision
  remains.
- `keep_visible`: The run has uncertain or uncontained state that automation cannot
  own. Examples include invalid or unpersisted state; an unknown push, merge, or
  publication result; login or CAPTCHA; missing human-only authority; lost browser
  ownership; account restoration failure; an intentionally retained handoff tab; or
  failed terminal readback without a durable automatic repair owner. An exhausted or
  unowned side-effect intent stays visible. A side effect with an unknown external
  result stays visible even when a generic repair candidate exists.

Normal successful work, a proven no-op, `no_candidate`, `role_busy`, a quality skip,
a duplicate or daily-cap block, a persisted retry, an automatically owned repair, a
submitted pull request with durable ownership, a landed result, and a confirmed
publication with account restoration must use `auto_archive`.

For `auto_archive`, call the native Codex app `set_thread_archived` tool with
`archived = true` and omit `threadId`, so the tool can target only the current task.
Make it the final tool action immediately before the final report. Require a
successful tool result and include that result in the report.

For `keep_visible`, do not call `set_thread_archived`. Lead the final report with the
reason that human attention is required. If the archive tool is unavailable or its
result fails, keep the task visible and report `run_task_archive_failed`.

Never archive another task. Archiving is UI retention, not evidence deletion. Never
archive an ambiguous or incomplete operation without a durable automatic owner.

Repository tests validate this classification contract. They do not prove that a
live scheduled Codex task called the native archive tool. Live scheduled readback is
the acceptance evidence for task-list cleanup.
