# Decodex Content Automation

## Scope

The content loop has two scheduled tasks:

- `decodex-content-manager` runs once per day at 09:50.
- `decodex-xurl-publisher` runs at 10:20, 16:20, and 22:20.

Both use the primary clean `main` checkout, local execution, and `high`
reasoning. Content Manager uses `gpt-5.6-terra`; Xurl Publisher uses
`gpt-5.6-luna`. They do not use Decodex server surfaces and do not run from
worktree paths.
Runtime memory is bounded, mode `0600`, advisory only, and cannot override current
repository, artifact, Publisher, or cost-report state. A fresh cutover deletes old
memory; it does not migrate it.

The three Publisher windows are required because one task processes at most one
operation: one publication, one due 24-hour outcome, or one due seven-day
outcome. More task windows do not increase public cadence. The Publisher still
permits at most one post per UTC day.

## Content Manager

Content Manager owns product operations and marketing decisions. Each run:

1. Refreshes the Radar upstream queue first, binds the successful report's exact
   `queue_sha256`, and then refreshes the release delta.
2. Validates current Radar and social state.
3. Reads official OpenAI sources, `openai/codex`, landed Decodex evidence, and
   current outcomes.
4. Uses CodexRadar at most once per business day as a secondary discovery source.
5. Runs `radar review-next --expected-queue-sha256 <refresh-receipt-sha256>` once.
   The command compares the receipt with the locked queue bytes before selection. It
   selects one current queue subject but cannot make it publish eligible.
6. For `needs_source_review`, builds one GitHub change bundle and performs one
   bounded source-reading pass. It follows the runtime path and requires a concrete
   implementation, test, documentation, or schema anchor plus a user or operator
   path. Titles and filenames are not evidence.
7. Writes one create-only, mode-`0600` pair staging record. It then calls
   `radar content-pair-commit`, which materializes the exact review digest and
   atomically commits the review and impact in one run-owned directory.
   `review-next` skips a handled subject with the same normalized commit set. It
   runs `radar content-eligibility` once only when the committed pair supports a
   public claim. Metadata-only selection, invalid staging, or invalid lineage
   cannot produce a candidate.
8. Records the daily operations review in bounded memory without any queue SHA-256.
   It creates one private
   mode-0600 staging artifact only for a weekly strategy checkpoint, an
   evidence-backed strategy change, one `social_candidate/v1`, or one precise
   skip candidate. A no-op creates no artifact.
9. Calls Publisher `social record-manager`, which derives and atomically creates
   the run-owned candidate or strategy destination under the shared mutation lock.
   Content Manager never writes an authoritative social store directly.
10. Runs social validation and updates bounded memory.

It never calls X endpoints. Ordinary web research can inspect public editorial
sources without X API cost. Community claims require official confirmation.

## Quality Gate

A publish-worthy candidate:

- contains one text item with at least 80 Unicode characters and at most 260
  X-weighted characters under the conservative official twitter-text v3 ranges;
- contains no URL;
- states one concrete change and operator consequence;
- uses source-backed Radar review and impact evidence, not queue metadata;
- embeds the exact `radar_content_eligibility/v1` receipt and exact private queue,
  review, and impact references;
- binds every factual claim to one verified Radar review or impact;
- reconstructs public text exactly from ordered claims and fixed non-factual
  connective segments;
- avoids generic announcements, hype, copied text, and vague monitoring;
- uses stable idempotency lineage.

Cadence does not lower this gate. Weekly numeric threshold changes require at least
three valid 24-hour outcomes.

## Xurl Publisher

Publisher is the only X writer. The automation prompt does not invoke xurl directly.
The checked-in `decodex-publisher` auxiliary:

1. Validates all social contracts.
2. Reserves one checked candidate atomically.
3. Verifies exact xurl `1.3.1` and the approved binary SHA-256.
4. Allows unrelated local OAuth2 labels, then uses a paid `whoami` read to verify
   the exact `decodexspace` identity before create.
5. Enforces one post per day, no URL in public text, and the monthly budget.
6. Creates one post, reads it by exact ID, and verifies exact text and author.
7. Writes one immutable `social_post/v1` and consumes the reservation under one
   state lock.
8. Collects one due `social_outcome/v1` at 24 hours or seven days.

It does not retry an uncertain create without a trusted post ID.
Publisher `social cost-report` is the only ledger reader used for reporting.
It validates v4 attempts and emits only monthly cost ceilings, the cap, remaining
budget, and bounded call counts.

## Cost

The local ledger uses micro-USD:

| Operation | Maximum |
| --- | ---: |
| Identity read, URL-free create, initial readback | 30,000 ($0.030) |
| 24-hour read | 5,000 ($0.005) |
| Seven-day read | 5,000 ($0.005) |
| Full measured lifecycle | 40,000 ($0.040) |
| Monthly hard cap | 1,250,000 ($1.25) |

At one post per day, X API use remains at most $1.20 in 30 days and $1.24 in 31
days, below the hard $1.25 calendar-month cap. The three daily Publisher task
windows do not multiply this spend because each post still has only one
publication, one 24-hour read, and one seven-day read. Public text with a URL is
rejected because it is lower quality for this format and materially more
expensive. Competitor research never uses paid X reads.

## Local Evidence

All generated staging, strategy, candidate, reservation, attempt, post, outcome,
and usage records are mode `0600` under `.agent/automations/decodex/cache`. A
successful manager record removes its staging file. These files are not committed
or uploaded. Stored API evidence is limited to response digests, exact post
identity, verified target account, version, app, and reserved or recorded cost
ceilings. It does not claim the provider's final bill. Credentials, raw responses,
personal data, and public text are excluded from automation memory.

## Social Artifact Retention

Health is the only scheduled GC owner. Each Health cycle runs one `social gc`
first, so journal recovery and the GC-owned bounded validation complete before
ordinary validation. Health then runs one full `validate-social` readback. A GC
validation failure or the final validation failure prevents a successful cleanup
result.

The fixed strategy window keeps the 14 most recent valid evidence-backed daily
strategy changes and the 8 most recent valid weekly checkpoints. An additional
strategy is eligible when its `reviewed_at` is at least 10 days old. Strategy
pruning is planned before retained strategy references are applied to social
lineages.

A lineage is eligible only when its newest trusted schema timestamp is at least
10 days old. A published lineage must have one candidate, one consumed
reservation, one verified published post, due 24-hour and seven-day outcomes, one
successful publication attempt, and two matching successful observation
attempts. A quality-skip lineage must have one checked candidate and one matching
skipped post, with no reservation, outcome, or xurl attempt. GC deletes all files
in an eligible lineage as one planned component.

GC preserves active or unconsumed candidates, active reservations, failed posts,
failed, uncertain, or inflight attempts, missing outcome windows, inconsistent
lineages, current UTC billing-month usage and its whole lineage, and retained
strategy references. Task-retention receipts store evidence digests only and do
not control social artifact retention.

The scanner accepts at most 8,192 entries, 4,096 files, and 64 MiB. It requires
owned mode-`0700` directories and owned mode-`0600`, one-link files. It uses
no-follow, descriptor-relative access and pins each directory through preflight
and unlink. A symlink, unexpected entry, malformed JSON, unknown schema,
replacement race, or exceeded bound stops cleanup. Business retention uses schema
timestamps and never filesystem modification time.

GC returns only bounded counts and fixed reason codes. It never returns post text,
metrics, raw API responses, usernames beyond the fixed target, or absolute paths.
It does not delete Radar or upstream evidence. Social cache stays local and is
never archived to GitHub.

## Health Management

`codex-upstream-health` is the manager for all five automations. Daily it checks:

- live definition drift and missing app metadata;
- primary-cwd and `high` reasoning invariants;
- unresolved candidates, reservations, and xurl attempts;
- overdue outcomes and lineage errors;
- daily and monthly cost limits;
- publication conversion, skip causes, and failure rates;
- task cleanup backlog;
- upstream detection-to-land latency and adaptation count.

Weekly it compares topic coverage and usefulness with CodexRadar and public release
sources, then queues concrete reason-specific improvements. Maintainer implements
them through the trusted ephemeral Codex child wrapper. Reviewer independently
validates and lands them
with local `decodex commit` and `decodex land` authority boundaries.

## Task Retention

Each terminal run uses `task-retention-seal` and
`Task retention: manager_archive`. The owner receipt stores the exact task ID,
automation ID, allowlisted terminal result, nullable evidence kind, digest of the
validated evidence bytes, timestamp, and status. It stores no evidence path, task
text, rollout, tool-call history, personal data, or absolute path.
An evidence-bearing seal uses the current owned Publisher binary to run the
canonical full-store `validate-social` command. It then reads the evidence again
and requires exact byte equality before it writes the receipt. A missing binary,
invalid store, changed evidence file, timeout, or unexpected validator output
fails closed.

Health plans at most 50 task records bound to the owner, result, and evidence
projection. It calls native `read_thread`, archives only a verified terminal success through
`set_thread_archived`, and verifies exact readback before settling the receipt.
Failed, blocked, needs-attention, user-continued, ambiguous, and human-decision
tasks stay visible. Python does not inspect Codex databases or invoke native task
tools. Archiving does not disable the recurring automation or delete evidence.
