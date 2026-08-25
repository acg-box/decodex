---
type: "Reference"
title: "Decodex Content Automation"
openwiki_generated: true
---

# Decodex Content Automation

This runbook defines the autonomous research, editorial, publication, and
measurement loop for `@decodexspace`.

## Roles

`automations/portfolio.toml` declares two content roles:

| Role | Model | Effort | Frequency |
| --- | --- | --- | --- |
| Content Manager | `gpt-5.6-luna` | `max` | Daily at 09:50 |
| Xurl Publisher | `gpt-5.6-luna` | `max` | Daily at 10:20, 16:20, and 22:20 |

Both use the clean primary checkout as scheduled cwd. They do not use Decodex
server, runtime, queue, planner, or MCP.

## Activation State

`PAUSED` is the initial acceptance state. Native status must always match the
current manifest exactly; while it is `PAUSED`, Manager must not activate. First
land the portfolio with `status = "PAUSED"` and run live acceptance only by
explicit one-shot manual invocation. After all non-activation acceptance evidence
passes, signed-land the one-line promotion to `status = "ACTIVE"`; Manager/native
sync can then activate all five. No activation workflow engine or extra state exists.

## Editorial Loop

The Content Manager researches three source classes in this order:

1. official Codex releases, documentation, source, and protocol changes;
2. landed Decodex changes with verified practical consequences;
3. CodexRadar as secondary discovery and editorial comparison.

At least one factual source must be `official_codex` or `landed_decodex`.
`radar_secondary` cannot support a candidate by itself.

The agent compares new evidence with recent Decodex posts and recorded outcomes.
It selects one useful angle or decides that silence is better. Quality requires:

- a concrete consequence for a Codex or Decodex user;
- claims that each bind one declared source URL;
- original text that is useful without a link;
- no unsupported benchmark, roadmap, or availability claim;
- no repeated topic or empty release paraphrase;
- one text between 80 and 260 weighted characters.

The agent writes one private `decodex/content-evidence/1` staging document with
`decision.worthiness` set to `publish` or `no_op`. It invokes one write boundary:

```sh
decodex-publisher social record-candidate \
  --staging <private-json> \
  --run-id "$CODEX_THREAD_ID"
```

The Publisher inserts the immutable content identity and stores at most one
pending candidate. This is the complete write boundary. There is no content queue,
review pair, impact document, eligibility receipt, strategy layer, or activation
workflow.

## Publication Loop

The Xurl Publisher first validates private state and runs the free readiness
probe:

```sh
decodex-publisher validate-social
decodex-publisher social refresh-pricing
decodex-publisher social probe-xurl
```

`refresh-pricing` performs exactly one bounded ordinary HTTPS GET to the official
X pricing Markdown. It uses no OAuth or token and reports zero X API calls and
zero X API cost. A current receipt can survive a temporary network failure. A
missing or stale receipt, parser failure, or changed official rate blocks paid
work before `probe-xurl` can report ready.

It then performs at most one high-level paid operation. Due measurement has priority:

```sh
decodex-publisher social observe-due --run-id "$CODEX_THREAD_ID"
```

Proceed to editorial review only when the exact successful status is
`no_due_outcome`. This result is continuation-only, is never a terminal outcome,
and is never sufficient to archive. The Publisher must complete the candidate
path through `publish-next`. Any other successful `observe-due` status is a
completed observation that ends paid work for the run, followed by validation
and cost reporting. The review either publishes or records a quality skip:

```sh
decodex-publisher social publish-next \
  --run-id "$CODEX_THREAD_ID" \
  --decision publish

decodex-publisher social publish-next \
  --run-id "$CODEX_THREAD_ID" \
  --decision skip \
  --reason "$SKIP_REASON"
```

Set `SKIP_REASON` to a bounded, evidence-backed reason and quote it as one shell
argument.

The command owns selection, idempotency, reservation, account verification,
create, exact readback, journal recovery, and terminal evidence. The prompt does
not call lower-level steps.

## X Boundary

Only `decodex-publisher` may invoke xurl. Browser control, X MCP, and direct X API
calls are forbidden. The boundary enforces:

- xurl app `default` with sealed authorization for account label
  `decodexspace`;
- exact paid identity proof for `@decodexspace` before create;
- one post per UTC day;
- no URL, domain, email, IP address, or link-like text;
- `$1.25` monthly reserved-cost cap;
- `$0.030` normal publication ceiling;
- exact post ID, author, and text readback;
- no retry when create may have succeeded but no trusted post ID exists;
- one 24-hour and one 7-day outcome read per post; overdue reads remain due after
  machine or app downtime.

The local budget ledger and uncertain-write journal are safety roots. They are
not editorial workflow state and must not be deleted by ordinary cleanup.

## Cost And Outcomes

```sh
decodex-publisher social cost-report
```

A normal publish reserves at most 30,000 micro-USD. Each outcome read reserves at
most 5,000 micro-USD. The monthly cap is 1,250,000 micro-USD. The report includes
used and reserved ceilings, remaining ceiling, and paid-call counts.

The 24-hour and 7-day reads provide evidence for the next Content Manager run.
The Manager reviews publication rate, quality skips, repeated themes, exact
readback failures, due-outcome completion, and cost. It selects a concrete
editorial improvement and verifies its later effect.

## Task Cleanup

Each content role archives its current Codex task through native
`set_thread_archived` only after a terminal successful result and all required
validation, readback, and report evidence are complete. Content Manager success
is a validated candidate or no-op. Publisher success is a completed observation,
a publish with exact readback, a durable quality skip, or a validated no-candidate
no-op reached only after `publish-next` completes its candidate path.
`no_due_outcome` alone is continuation-only and never sufficient to archive. The
current task ID is implicit; the role does not supply another task ID.

A task stays visible for failed validation or checks, missing OAuth or authority,
an ambiguous external effect, damaged safety state, an unresolved user decision,
or required work not durably handed off. Manager may enforce the policy for one
known completed task when bounded exact-task readback is available, but cleanup
does not depend on a global scan. There is no retention receipt, archive queue,
or local archive state.

## Stop Conditions

The content agents stop for human input only when:

- xurl OAuth is missing and interactive login is required;
- a create result is unknown and no trusted post ID exists;
- immutable X safety state is damaged;
- a genuine public policy choice cannot be inferred from project goals.

Unsupported or weak content is an autonomous `no_op` or skip, not a blocker.

## Validation

```sh
cargo test -p decodex-publisher
cargo clippy -p decodex-publisher --all-features --all-targets -- -D warnings
python3 -m unittest automations.decodex.scripts.config.tests.test_portfolio
```

Tests use fake xurl responses. They do not publish a live post.
