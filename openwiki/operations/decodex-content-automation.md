# Decodex Content Automation

The content loop has two scheduled owners:

- `decodex-content-manager` discovers source-backed opportunities, uses Radar evidence,
  performs one bounded weekly read-only X editorial benchmark under the shared lease,
  produces one checked candidate or one quality skip, and writes bounded daily and
  weekly `social_strategy/v1` decisions from terminal evidence.
- `decodex-x-browser-publisher` is the only X operator. It validates, reserves,
  publishes through browser control, reads the final URL, restores the initial account,
  and records 24-hour or seven-day outcomes.

The upstream code loop remains independent. A content failure must not block upstream
maintenance, review, or landing. Content may consume a landed result, but it must not
describe an unlanded candidate as shipped.

## Evidence Flow

Use this order:

1. Official OpenAI documentation, GitHub release data, or `openai/codex` source.
2. Validated Radar review, impact, release-delta, signal, or analysis artifacts.
3. Landed Decodex `main` evidence.
4. External sites such as Codex Radar only as topic leads or editorial benchmarks.

X posts and community measurements are not technical authority.
Once per seven-day strategy period, Content Manager reads at most 12 public posts in
total from `@CodexReleases`, `@Codex_Changelog`, and `@decodexspace` with browser
control. It uses the shared browser lease, does not switch accounts for the benchmark,
does not copy text, and stores only public URLs and bounded editorial observations in
the weekly strategy record. Every weekly record contains exactly one
`weekly_editorial_benchmark` decision and one schema-bound `editorial_benchmark`
object. A completed object contains at most 12 supported public X status URLs and at
most 12 short observations. A deferred object contains one bounded lowercase reason
code and the matching `benchmark:deferred:<reason-code>` evidence reference. It
verifies the initial account again before lease release. This benchmark can change
editorial preferences only under the existing minimum outcome-sample gate. Health
reports `weekly_benchmark_missing` when the current weekly record does not contain
that bounded result.

## Publication Flow

The manager writes `social_candidate/v1` for a publish or justified quality skip
decision. Publisher runs `social terminalize-skip` to turn a quality skip into one
deterministic, atomically created `social_post/v1` terminal record without opening X
or acquiring a browser lease. Concurrent or resumed runs read back the same terminal
record and cannot overwrite it. For publish decisions, Publisher creates
`social_publish_reservation/v1`, publishes at most once, and writes `social_post/v1`.
It writes `social_outcome/v1` for due 24-hour and seven-day browser readback.
The 24-hour window is 23 to 48 hours after publication. The seven-day window is 167
to 192 hours after publication.

The Publisher uses no X MCP or X API. X API call count and spend are always zero.
Generated social and strategy records are local-only under `.agent/`. They are never
committed, uploaded, or archived to GitHub. Browser-session records contain only the
two schema-bound public handles needed to prove account switching and restoration;
they contain no cookies, tokens, storage, profile data, or raw API responses.

Before X compose, Publisher:

1. Acquires one crash-recoverable X browser lease, then renews and verifies it before
   opening X and before every browser action.
2. Captures the initial visible X account.
3. Switches to `@decodexspace` when needed.
4. Verifies the target profile and checks live duplicates.
5. Persists one create-only, idempotency-derived reservation.
6. Repeats target-account and duplicate checks, renews the browser lease, and verifies
   it immediately before the public write.

After any terminal outcome, Publisher restores the initial account when it was
`@hackink`. A restore failure after a confirmed post does not erase publication
evidence, but it makes the run unhealthy and requires a visible handoff.
The Publisher also renews after a public click, before final readback, and before
account restoration. A resumed run must renew and verify before it touches X. This
keeps ownership fenced through the complete browser session. It releases the exact
browser lease only after restoration and terminal validation.

## Coordination

The browser lease serializes all X account use, including Content Manager's weekly
read-only benchmark. Publisher remains the only X writer. The candidate idempotency
key is the publication identity. Its create-only reservation path prevents a second
writer for the same identity. A separate short-lived local state lock serializes
quality-skip terminalization with publication reservation, so those two outcomes
cannot both succeed for one identity. A terminal post consumes the claim only after
confirmed publication or a consuming policy outcome.

Scheduled tasks use the primary `main` checkout. They never use a worktree cwd.
Development worktrees are not runtime bindings.
Content Manager runs `cargo build --locked -p radar -p decodex-publisher` with the
primary checkout's `target` directory. It uses the exact `target/debug/radar` and
`target/debug/decodex-publisher` paths, refreshes the official upstream queue and
release delta, and validates the Radar cache before drafting. Publisher and Health
build Publisher and use the exact `target/debug/decodex-publisher` path. No content
owner depends on a global command in `PATH`.

## Cadence

Content Manager runs every six hours. Browser Publisher runs every two hours. The
Publisher handles at most one candidate, one due outcome, or one proven no-op per run.
It terminalizes a quality skip through the atomic Publisher command without browser
work. Daily and weekly
learning uses unique strategy cycle keys and persisted timestamps, not separate
scheduled tasks. Strategy decisions can change only bounded topic weights, format
preferences, or quality thresholds. Evidence, privacy, idempotency, account, and
publication gates remain unchanged. Numerical topic or format changes require at least
three published posts with valid 24-hour outcomes. Raw views alone cannot change
strategy.

Upstream Health is the portfolio supervisor. It reconciles the five managed task
definitions and validates content contracts without opening X. It queues one
`content_loop_degraded` improvement when strategy, candidate handling, outcome
collection, account restoration, or social validation misses its bounded service
level. The existing Maintainer and Reviewer tasks then reproduce, repair, test, and
land the improvement through the same autonomous code path as upstream compatibility
work.

No fixed posting quota exists. A justified quality skip is valid. Filler content is
not.

## Run Thread Retention

Each scheduled execution creates a separate Codex task. The terminal role classifies
its task only after terminal validation and external-effect readback:

- Mark a successful terminal result, proven no-op, quality skip, duplicate block,
  persisted retry, or failed result with a durable automatic repair owner. The result
  must have no remaining lease, handoff, or human decision.
- Escalate invalid or unpersisted state, login or CAPTCHA, missing
  human-only authority, unknown publication state, lost browser ownership after a
  public write, account restoration failure, a retained handoff tab, or failed final
  readback without automatic ownership.

For a terminal result with durable automatic ownership, the role calls native
`set_thread_archived` with `archived = true` and no `threadId`. This archives only the
current scheduled task. An ambiguous, human-only, or unowned result stays visible.
Archival affects only Codex task-list visibility. It does not pause or delete the
recurring automation and does not delete local evidence.

## Validation

Run:

```sh
python3 automations/decodex/scripts/config/evaluate_automations.py \
  --manifest automations/decodex/automations.toml --repo-only
cargo test -p decodex-publisher
CARGO_TARGET_DIR="$PWD/target" cargo build --locked -p decodex-publisher
target/debug/decodex-publisher validate-social
```
