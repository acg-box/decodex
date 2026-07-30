# Decodex Content Automations

This directory is the canonical source for two content tasks:

- `decodex-content-manager`: source research, Radar refresh, daily and weekly
  strategy, candidate creation, and quality review.
- `decodex-xurl-publisher`: skip terminalization, bounded X publication, exact
  readback, and 24-hour or seven-day outcome collection.

Both run from the primary clean `main` checkout with local execution,
`gpt-5.6-sol`, and `high` reasoning. Scheduled definitions never use a worktree
cwd.

Content Manager runs once at 09:50. Publisher runs at 10:20, 16:20, and 22:20.
Each Publisher task handles at most one publication, one due 24-hour outcome, or
one due seven-day outcome. These three windows do not change the one-post-per-day
limit.

## Ownership

Content Manager never calls X. It uses official sources, landed Decodex evidence,
Radar, CodexRadar, and ordinary web research. It creates one private staging
strategy or candidate, or produces a strict no-op. Publisher `social
record-manager` is the only writer for the run-owned authoritative candidate and
strategy stores.

Publisher never invents claims. It consumes one checked candidate and invokes only
the checked-in `decodex-publisher` auxiliary. That binary is the sole xurl and X
endpoint client.

## Publication Contract

- Fixed target: `@decodexspace`.
- xurl app: `default`.
- Authentication: OAuth2 target account must be exactly `decodexspace`.
- Supported xurl version: exactly `1.3.1` with the approved binary SHA-256.
- Public cadence: at most one post per day.
- Public text: one item with at least 80 Unicode characters and at most 260
  X-weighted characters, no URL. Weighting uses the conservative official
  twitter-text v3 ranges.
- Monthly hard cap: 1,250,000 micro-USD ($1.25).
- Maximum modeled X API use: $1.20 per 30 days and $1.24 per 31 days.
- Normal publication: a 30,000 micro-USD ($0.030) recorded ceiling for paid
  identity read, create, and initial readback.
- Outcome observation: 5,000 micro-USD for each due read.
- Competitor research: no paid X reads.

A publication succeeds only after exact post ID, text, and author readback. The
candidate, reservation, attempt, publication, canonical URL, and recorded cost
ceiling must agree. An uncertain create with no trusted post ID is not retried
automatically.

## Quality Contract

Publish only when the text states one concrete change and why it matters, stands
alone without a link, embeds one verified Radar queue-review-impact receipt, and
binds every factual claim to the exact private Radar review or impact. Public text
must equal its ordered claim composition. Only fixed non-factual connective
segments can appear outside claims. Reject generic availability notices, vague
monitoring language, copied source text, hype, unsupported claims, and
cadence-filling content.

`radar review-next` only selects one current subject for investigation. Content
Manager then builds one source bundle and follows the implementation path. It may
stage one source-backed review and matching impact only after finding a concrete
code, test, documentation, or schema anchor and a user or operator path. Radar
materializes the review digest and atomically commits both artifacts in one
run-owned pair directory. `review-next` skips an already handled subject with the
same normalized commit set. Queue titles, paths, hints, and flags cannot become
publishable evidence.

Content Manager records a bounded operations review in memory each day. It writes
a strategy artifact only for the weekly checkpoint or an evidence-backed change,
so daily review cannot consume the candidate slot. Numeric threshold changes
require at least three valid 24-hour outcomes. CodexRadar and public release
content are discovery and editorial inputs, not technical evidence.

## Private State

Generated staging, candidate, strategy, reservation, xurl attempt, publication,
outcome, and usage records are create-only mode `0600` files under
`.agent/automations/decodex/cache`. They are not committed, uploaded, or archived to
GitHub. A successful manager record removes its staging file. Automation memory
stores only bounded result codes, artifact IDs, cost
ceilings, and next checks. It never stores post text, raw responses, credentials,
personal data, or absolute paths.

Health validates the complete social state, runs `decodex-publisher social gc`,
and validates the complete state again. GC keeps 14 recent daily strategies, 8
recent weekly strategies, and a minimum 10-day lineage window. It deletes only a
complete verified publication lineage or a complete checked quality-skip lineage.
It preserves active, failed, uncertain, inflight, incomplete, current billing
month, and retained-strategy evidence. Task receipts keep only evidence digests
and do not control social GC. The scan is fail-closed and
bounded to 8,192 entries, 4,096 files, and 64 MiB. It uses schema timestamps, not
filesystem modification time. It does not delete Radar or upstream evidence, and
it does not archive local cache to GitHub.

## Validation

Build and validate with:

```text
cargo build --locked -p radar -p decodex-publisher
target/debug/decodex-publisher validate-social
target/debug/decodex-publisher social gc
target/debug/decodex-publisher validate-social
python3 automations/decodex/scripts/config/render_automation_plan.py --json
```

Publisher tests cover manager overwrite and backpressure, record crash recovery,
two-writer locking, Radar lineage tampering, unclaimed factual text, wrong-account
rejection before a write, URL rejection, monthly budget exhaustion, one-per-day
reservation, exact create/readback, idempotent retry with a known ID,
uncertain-write handling, outcome windows, shared budget accounting, skip
terminalization, obsolete evidence rejection, complete lineage retention,
fail-closed replacement races, and bounded social GC.

The plan command is read-only. Apply each definition only through the Codex native
automation lifecycle tool. View an existing ID before an update and read back every
create or update. Repository tooling cannot write scheduler TOML or manage Codex App
timestamps. The plan also emits the exact retirement
`decodex-x-browser-publisher`; Health deletes that definition through the native
tool and verifies an exact not-found readback.

## Task Retention

Every terminal run calls `task-retention-seal` and ends with
`Task retention: manager_archive` only after validation. Failed, blocked,
needs-attention, ambiguous, and human-decision tasks use `keep_visible`.

The active task does not archive itself. Health scans only bounded owner receipts,
plans at most 50 task records bound to the owner, allowlisted result, evidence
kind, and evidence-byte digest, calls native `read_thread`, archives only a
confirmed terminal success with `set_thread_archived`, and verifies the exact
readback. Python does not inspect Codex databases or invoke native task tools.
Archiving cleans the Codex task list only. It does not disable recurrence or delete
evidence.
