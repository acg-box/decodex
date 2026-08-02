# Decodex Publisher

`decodex-publisher` is the deterministic boundary between autonomous content
agents and X. Agents decide what is useful and how it should be written. The
Publisher owns atomic local writes, idempotency, account enforcement, paid-call
budgets, uncertain-write recovery, and immutable readback.

## Agent Surface

```text
decodex-publisher validate-social [PATH ...]
decodex-publisher social record-candidate --staging <FILE> --run-id <UUID>
decodex-publisher social publish-next --run-id <UUID> --decision <publish|skip> [--reason <TEXT>]
decodex-publisher social observe-due --run-id <UUID>
decodex-publisher social refresh-pricing
decodex-publisher social probe-xurl
decodex-publisher social cost-report [--month <YYYY-MM>]
decodex-publisher social seal-xurl-auth
```

Commands that expose reservation, create, read, reconciliation, skip, journal,
or garbage-collection internals are not part of the CLI. `publish-next` and
`observe-due` compose those primitives so the prompt does not reproduce a state
machine.

Every mutating scheduled command requires `--run-id` to equal the current
`CODEX_THREAD_ID`.

## Candidate Recording

`record-candidate` is the only candidate writer. The input is a private,
create-only staging JSON document with schema `decodex/content-evidence/1`.
Staging omits `decision.idempotency_key`; the writer derives it from canonical
candidate content and inserts `content-publication:<sha256>`.

The write boundary:

1. verifies the staging file and private directory chain;
2. rejects an invalid source, unsupported claim, wrong account, link, or invalid
   post text;
3. rejects a second candidate while one unconsumed candidate remains available
   for normal processing; a candidate with a durable uncertain create stays
   lineage-blocked but does not consume this global backpressure slot;
4. holds the social lock through creation and readback;
5. creates one mode-`0600` candidate without overwrite;
6. verifies the immutable identity from installed bytes;
7. removes the exact staging file after successful readback.

An exact retry returns the existing result. Changed content conflicts.

## Publication

`publish-next` first checks the immutable attempt journal for one recoverable
interruption. Otherwise, it selects the oldest unconsumed candidate by its
stable candidate path. Normal recording backpressure keeps this set at one.

- A `no_op` candidate is terminalized without an X call.
- `--decision skip` requires a reason and is terminalized without an X call.
- `--decision publish` reuses the exact active reservation or creates one under
  the daily limit, then executes the xurl boundary.

The xurl boundary accepts only the fixed operating-system installation and a
content-addressed runtime copy. It verifies the approved binary identity and the
sealed OAuth contract. It makes the paid identity read before create and requires
the exact account `@decodexspace`.

After create, the Publisher reads the exact post ID and accepts success only when
the author and text match the candidate. It stores the post, attempt, reservation,
call counts, xurl version, and budget evidence as private immutable JSON.
If recovery finds a post written before local terminalization, the validated
attempt is authoritative. The Publisher consumes the reservation only when the
post exactly matches that attempt, its request, candidate, and reservation.

If create may have succeeded but no trusted post ID is available, the attempt is
marked uncertain and create is never retried. Later candidates with that lineage
remain blocked until a human resolves the unknown external effect. This block is
limited to that publication lineage. It does not block unrelated candidates.

Publisher writes a durable attempt before each paid xurl call. On restart, it
releases an owner-mismatched reservation that has no attempt. If a durable
`reserved` attempt has no call, Publisher terminalizes it as no-create and releases
the reservation. These actions are idempotent before and after reservation expiry.
Identity recovery permits one extra identity read. A second identity failure
terminalizes the no-create lineage so later unrelated work can continue.

## Observation

`observe-due` recovers one interrupted read when possible. It then selects at
most one published post whose observation checkpoint is due:

- `24h`: at least 23 elapsed hours;
- `7d`: at least 167 elapsed hours.

It reads the exact post ID through xurl, verifies the author and original text,
and records one outcome for that post and checkpoint. A missed checkpoint stays
due after machine or app downtime. Existing outcomes make retries idempotent.
An observation permits at most three durable reads. If the final read fails,
Publisher records a terminal read-recovery result and allows later checkpoints to
continue.

## Safety And Cost

The compiled and checked-in policy enforces:

- xurl only for X API operations; no browser, X MCP, or direct X API client;
- xurl app `default`, account label `decodexspace`, target `@decodexspace`;
- one post per UTC day;
- immediately before content create, actual UTC must match the reservation day
  and more than two minutes must remain before UTC midnight;
- one text item, at least 80 Unicode characters, at most 260 X-weighted
  characters, and no URL or link-like form;
- exact claim-to-source URL binding and at least one primary source;
- 1,250,000 micro-USD (`$1.25`) monthly reserved-cost cap;
- 30,000 micro-USD (`$0.030`) normal publish ceiling;
- 5,000 micro-USD observation ceiling;
- 60,000 micro-USD (`$0.060`) per-lineage ceiling, sufficient for one interrupted identity read,
  one safe identity reconciliation, one normal publication, and both observations;
- at most five paid calls in one publication attempt and three in one observation
  attempt; a further recovery is rejected before xurl runs or the ledger changes;
- immutable budget ledger and uncertain-write journal;
- bounded process duration and response size;
- owner-only paths, no symlink traversal, no overwrite, and exact readback.

`refresh-pricing` makes one ordinary HTTPS GET to the exact official pricing
Markdown. It uses root-owned `/usr/bin/curl`, follows no redirect, has a
10-second total deadline and 1 MiB response limit, sends no OAuth or token, and
makes zero X API calls at zero X API cost. It strictly parses the named pricing
section and operation tables. A newer parse failure or changed rate blocks paid
work; a network failure can defer only while a prior receipt remains current.

`probe-xurl` is free and verifies readiness without an X API call.
`cost-report` reads the local ledger and reports the monthly used and reserved
ceilings, remaining ceiling, and call counts. `seal-xurl-auth` is an explicit
operator action after interactive login; scheduled agents do not start OAuth.

## Private State

All state is local under `.agent/automations/decodex/cache/social`:

```text
x/candidates/
x/reservations/
x/posts/
x/outcomes/
x/xurl-attempts/
x/locks/
x/xurl-authorization-contract.json
x/x-pricing-receipt.json
x/x-pricing-failure.json
x/xurl-cost-ledger/
```

The auth contract contains no token. xurl owns OAuth credentials outside this
repository. Publisher evidence must not be committed, uploaded, or copied into
task memory.

## Validation

```sh
cargo test -p decodex-publisher
cargo clippy -p decodex-publisher --all-features --all-targets -- -D warnings
cargo run -p decodex-publisher -- validate-social
```

The test suite uses private temporary directories and a fake xurl executable. It
does not make a live X call.
