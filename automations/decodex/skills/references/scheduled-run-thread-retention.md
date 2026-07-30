# Scheduled Run Task Retention

This policy cleans completed Codex tasks. It changes only the native archived flag.
It does not disable recurring automations or delete evidence. Never export task
content, rollout data, or Codex database rows.

## Owner Receipt

After terminal state or effect readback, each scheduled owner runs:

```text
task-retention-seal \
  --automation-id <exact-managed-id> \
  --terminal-result-code <bounded-result-code> \
  [--evidence-path <repository-relative-path>] \
  [--keep-visible-reason <bounded-reason-code>] \
  --json
```

The command uses the app-provided `CODEX_THREAD_ID`. The caller cannot select a task
ID. It atomically creates one mode-`0600` receipt named by that exact ID. The receipt
contains only:

- schema `decodex/codex-task-retention-receipt/2`;
- automation ID and task ID;
- bounded terminal result code;
- nullable evidence kind and SHA-256 of the exact validated evidence bytes;
- timestamp;
- status.

Receipt v1 is not accepted. No evidence path, absolute path, evidence content, task
text, prompt, rollout, tool call, database field, personal data, credential, social
text, or raw response is retained.

A `pending_archive` receipt accepts only these successful terminal results:

- `codex-upstream-maintainer`: `no_candidate`, `repair_queued`, `role_busy`, or
  `review_pending`;
- `codex-upstream-reviewer`: `no_candidate`, `repair_queued`, `role_busy`,
  `no_change`, `rejected`, `landed`, `repair_requested`, or
  `stale_decision_requeued`;
- `codex-upstream-health`: `pass`;
- `decodex-content-manager`: `candidate_recorded`,
  `quality_skip_recorded`, `strategy_recorded`, or `proven_no_op`;
- `decodex-xurl-publisher`: `published`, `outcome_observed`, `quality_skip`,
  `duplicate`, or `proven_no_op`.

Evidence is required only for Content Manager `candidate_recorded`,
`quality_skip_recorded`, and `strategy_recorded`, and Publisher `published`,
`outcome_observed`, and `quality_skip`. The seal rejects evidence for all other
successful results.

The evidence path must be repository-relative and name one direct JSON file in one
of these authoritative private collections:

- `.agent/automations/decodex/cache/social/x/candidates`;
- `.agent/automations/decodex/cache/manager/strategy`;
- `.agent/automations/decodex/cache/social/x/posts`;
- `.agent/automations/decodex/cache/social/x/outcomes`.

The reader rejects traversal and symbolic links. The evidence must be an owned,
regular, one-link, mode-`0600` file of at most 1 MiB. The bounded JSON parse and
semantic projection check apply to the same bytes that produce `evidence_sha256`.
`evidence_kind` is `candidate`, `strategy`, `post`, or `outcome`.

For every evidence-bearing result, sealing also requires the current owned,
one-link, non-group-writable, non-world-writable executable at
`target/debug/decodex-publisher`. The seal runs its canonical
`validate-social` command over the full private social store. A missing binary,
timeout, nonzero result, stderr output, unexpected or oversized stdout, or
invalid store prevents the receipt. The seal reads the evidence again after
that validation and requires byte-for-byte equality with the bytes used for the
digest. Content Manager and Publisher build this binary before they create
evidence.

Content Manager candidate and strategy files must be named
`<CODEX_THREAD_ID>.json`. Candidate results require `social_candidate/v1`;
`candidate_recorded` requires `decision.worthiness = publish`, and
`quality_skip_recorded` requires `decision.worthiness = skip`.
`strategy_recorded` requires `social_strategy/v1`.

Publisher `published` requires a `social_post/v1` file named
`<CODEX_THREAD_ID>.json` with status `published`. Publisher `quality_skip` requires
a `social_post/v1` file with status `skipped`. Publisher `outcome_observed`
requires a `social_outcome/v1` file named `<CODEX_THREAD_ID>.json`. Each Publisher
record must have `owner.automation_id = decodex-xurl-publisher` and
`owner.run_id = CODEX_THREAD_ID`.

A completed and independently read-back result uses status `pending_archive`. A
failed, blocked, cancelled, needs-attention, user-continued, human-decision,
invalid, or ambiguous result uses `--keep-visible-reason`. Keep-visible is
fail-safe and can retain another bounded terminal result code. Do not supply
evidence when the owner creates a keep-visible receipt. An unknown external effect
always stays visible. If no terminal readback exists, do not seal.

## Health Plan

The Health Manager is the only cross-task archive owner. It runs:

```text
task-retention-plan --json
```

The planner scans only the bounded receipt directory. It does not inspect Codex
SQLite, rollout files, task text, tool calls, or native tool history. It returns at
most 50 `pending_tasks` records and excludes the active Health ID from
`CODEX_THREAD_ID`. Each record contains only `thread_id`, `automation_id`,
`terminal_result_code`, `evidence_kind`, and `evidence_sha256`. There is no
`pending_thread_ids` compatibility field.

For each returned record:

1. Use the bound record and call native `read_thread` for its exact `thread_id`.
2. Keep active, user-continued, failed, cancelled, blocked, needs-attention,
   ambiguous, or human-decision tasks visible.
3. Otherwise call native `set_thread_archived` with `archived = true` for that
   exact ID.
4. Call native `read_thread` again and require archived readback for the same ID.
5. Run `task-retention-settle --thread-id <id> --result archived --json`.

If the first read requires visibility, run:

```text
task-retention-settle \
  --thread-id <id> \
  --result keep-visible \
  --reason <bounded-reason-code> \
  --json
```

If archive readback fails, restore the exact ID to visible, confirm exact readback,
then settle with reason `archive_readback_failed`. Python never invokes native task
tools. No `list_threads` query is required.

Settled receipts record either `archived_readback_confirmed` or
`keep_visible:<reason>`. The store retains at most 128 settled receipts for at most
30 days. Pending receipts are not inferred from commentary and are never removed by
age.

## Invariants

- Only an owner-sealed terminal task can enter the plan.
- Every plan entry stays bound to its owner result and evidence digest.
- The active Health task is never archived.
- Needs-attention and unresolved tasks stay visible.
- Native exact readback is required before an archived settlement.
- No recurring definition is disabled.
- No scheduled automation is bound to a worktree.
- Decodex server is not used.
