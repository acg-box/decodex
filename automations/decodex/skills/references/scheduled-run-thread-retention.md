# Scheduled Run Thread Retention

This policy controls the Codex thread created for one scheduled run. It does not
archive, pause, or delete the recurring automation definition. It does not delete
local evidence.

Before the final report, classify the current run:

- `auto_archive`: The run reached a terminal outcome. Every intended write and
  external effect has a durable receipt and readback. No lease, browser handoff,
  ambiguous side effect, active tool call, or human decision remains.
- `keep_visible`: The run needs human attention or has uncertain state. This includes
  `needs_attention`, dirty or wrong checkout, missing authority, invalid or
  unpersisted state, unknown push/merge/publication result, login or CAPTCHA,
  permission failure, lost browser ownership, account restoration failure, an
  intentionally retained handoff tab, or failed terminal validation/readback.

Normal successful work, a proven no-op, `no_candidate`, `role_busy`, a quality skip,
a duplicate or daily-cap block, a validated retry or repair handoff, a submitted pull
request with durable ownership, a landed result, and a confirmed publication with
account restoration can use `auto_archive`.

For `auto_archive`, call the native Codex app `set_thread_archived` tool with
`archived = true` and omit `threadId`, so the tool can target only the current run
thread. Make it the final tool action immediately before the final report. Include the
readback result in the report.

For `keep_visible`, do not call `set_thread_archived`. Lead the final report with the
reason that human attention is required. If the archive tool is unavailable or its
readback fails, keep the thread visible and report `run_thread_archive_failed`.

Never archive another thread. Never use run-thread archiving to hide a failed,
ambiguous, or incomplete operation.
