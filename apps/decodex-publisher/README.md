# Decodex Publisher

`decodex-publisher` validates private social artifacts and is the only component
that can invoke `xurl`. Scheduled prompts never invoke X endpoints directly.

## Commands

```text
decodex-publisher validate-social
decodex-publisher social record-manager \
  --staging <private-candidate-or-strategy-path> \
  --run-id <UUID>
decodex-publisher social reserve-publish \
  --candidate <path> \
  --run-id <UUID>
decodex-publisher social publish-xurl \
  --reservation <path> \
  --run-id <UUID>
decodex-publisher social observe-xurl \
  --post <path> \
  --run-id <UUID> \
  --window <24h-or-7d>
decodex-publisher social cost-report [--month <YYYY-MM>]
decodex-publisher social probe-xurl
decodex-publisher social seal-xurl-auth
decodex-publisher social reconcile-xurl \
  --evidence <reservation-or-outcome-path> \
  --operation-id <UUID>
decodex-publisher social reconcile-xurl \
  --attempt <interrupted-xurl-attempt-path> \
  --operation-id <UUID>
decodex-publisher social terminalize-skip \
  --candidate <path> \
  --run-id <UUID>
decodex-publisher social gc
```

## Safety Contract

- `social record-manager` is the only Content Manager writer for authoritative
  candidate and strategy stores. It derives the destination from the exact
  `CODEX_THREAD_ID`, holds the social mutation lock through prevalidation, atomic
  mode-0600 creation, postvalidation, and staging cleanup, and refuses overwrite,
  multiple effects, and candidate backpressure. An exact retry can finish cleanup
  after a crash between the authoritative write and staging unlink.
- Resolve the operating-system account home with `getpwuid_r`, never from the
  inherited `HOME` variable. Accept only the fixed `$HOME/.local/bin/xurl`
  entrypoint. The home, `.local`, `bin`, and executable must have a trusted
  owner, no group or world write permission, and no symlink component. The
  executable must already exist; the publisher does not install it.
- Pass the same verified operating-system home to xurl for OAuth state, then
  execute a bounded private content-addressed runtime copy.
- Target only `@decodexspace` through xurl app `default` and OAuth2. Other local
  OAuth2 labels are allowed. A paid `/2/users/me` read with the explicit
  `decodexspace` OAuth2 label is the identity proof before create.
- Require the approved official xurl `1.3.1` binary and exact SHA-256.
- Permit at most one post per day.
- Require exactly one public post with at least 80 Unicode characters and at
  most 260 X-weighted characters under the conservative official twitter-text
  v3 ranges.
- Reject public text that contains a URL, bare domain, email, IP address, or
  other link-like form.
- Require every claim to name one declared source reference. Bind each internal
  JSON evidence reference to its SHA-256 digest and expected schema. Recheck the
  private file and digest before reservation, publication, and local recovery.
- Require each publish candidate to embed the exact
  `radar_content_eligibility/v1` output and exact private queue reference. Review
  and impact must be the exact `review.json` and `impact.json` files in one
  `.agent/automations/radar/cache/github/content-review-pairs/<run>--<digest>/`
  directory. Re-read the three files with no-follow access, verify the canonical
  pair-directory digest, raw artifact digests, repo, subject, upstream head,
  exact commit set, review-to-impact binding, and the canonical lineage digest
  before record, reservation, and publication.
- Reconstruct public candidate text from ordered claim segments. Each claim appears
  exactly once and binds a verified Radar review or impact. Only fixed non-factual
  connective segments can appear outside claims.
- Enforce a shared monthly limit of 1,250,000 micro-USD ($1.25).
- Reserve a 30,000 micro-USD ceiling for identity read, create, and initial
  readback.
- Reserve 5,000 micro-USD for each 24-hour or seven-day observation.
- Verify the exact post ID, text, and author after creation.
- Do not retry a create with an unknown result and no trusted post ID.
- Refuse a paid call when the official pricing audit receipt is missing, stale,
  future-dated, malformed, tampered, or inconsistent with the compiled cost
  ceilings.
- Bound the complete Publisher xurl command to 45 seconds, including trusted
  binary setup, all child executions, and output drain. Kill each child process
  group on timeout and retain at most 1 MiB per output stream.
- Require production `run_id` values to equal `CODEX_THREAD_ID`. Outcome files
  use `<run-id>.json` so the task can reference the exact evidence path.

`probe-xurl` is nonbillable. It uses the production descriptor-bound,
content-addressed, environment-cleared runtime. It invokes only
`xurl --version` and `xurl --app default auth status`. Its report contains only
bounded version, digest, app, account label, readiness, the nonsecret
authorization contract, and pricing-policy metadata. It never starts an
authorization or login flow.

## Least-Privilege Authorization Contract

The stock xurl CLI owns OAuth credentials and refresh-token rotation. Publisher
does not read or parse `~/.xurl/auth.yml`, token values, client configuration, or
authorization URLs.

```text
.agent/automations/decodex/cache/social/x/xurl-authorization-contract.json
```

The operator must request and authorize exactly these policy-required scopes for
stock xurl app `default` and account label `decodexspace`, in this order:

```text
tweet.read users.read tweet.write offline.access
```

After authorization, run:

```text
decodex-publisher social seal-xurl-auth
```

The command verifies exact xurl version `1.3.1`, exact binary SHA-256
`7b85a210009db7a3f2d6183684674441fbf81276f1101f73d36d0266ec9aa01e`,
and `xurl --app default auth status`. It then writes a strict, private,
create-only JSON contract:

```json
{
  "schema": "decodex/xurl-authorization-contract/1",
  "policy_id": "xurl-oauth-least-privilege/3",
  "target_account": "decodexspace",
  "xurl_app": "default",
  "required_operator_authorized_scopes": [
    "tweet.read",
    "users.read",
    "tweet.write",
    "offline.access"
  ],
  "xurl_version": "1.3.1",
  "xurl_binary_sha256": "7b85a210009db7a3f2d6183684674441fbf81276f1101f73d36d0266ec9aa01e",
  "sealed_at": "<RFC3339>"
}
```

Unknown, old, or additional fields fail validation. The contract contains no client
ID, token, secret digest, authorization request, URL, or raw API response. It
does not expire on a calendar schedule. Every authenticated operation verifies
the contract, exact xurl binary identity, and the configured xurl account label
through `auth status`. xurl does not expose scope introspection here, so the
contract records a policy requirement and the operator authorization procedure,
not a runtime-verified scope claim. Publication uses the paid `/2/users/me`
check to verify `@decodexspace`; only a successful create proves that
`tweet.write` worked.

`reconcile-xurl --evidence` remains local and nonbillable for a verified effect
that already has durable post or outcome evidence. `reconcile-xurl --attempt`
can make a separately budgeted safe read for an interrupted identity read, a
publication with a known post ID, or an interrupted outcome read. Each recovery
operation ID can appear only once. Every recovery also consumes the immutable
publication lineage's 40,000 micro-USD ceiling, so recovery can prevent a later
24-hour or seven-day read. Identity recovery never creates a post; it releases
the old reservation so a later task can reserve the candidate again.
Publication and outcome recovery verify the exact post ID, text, and author
before terminalizing. Unknown post IDs and every `create_inflight` or
`create_uncertain` state remain permanently ineligible for automatic create
retry.

The Publisher serializes these operations with the social state lock. Each
additional read is reserved against the same
`$1.25` monthly ledger before execution. A recovery call is charged to the
calendar month in which that call runs. An unused reservation from an earlier
month cannot bypass the current-month cap. The operation ID identifies the
current task and must differ from the durable original owner.

The same operating-system UID can read and directly use its own `xurl`
credential. Preventing a malicious same-UID process from bypassing the
Publisher requires an OS capability or separate service identity and is outside
this repository threat model. Repository automation, prompts, and Python code
must not invoke raw `xurl`; the only allowed raw execution paths are the
hardened Rust runtime and the explicit stock xurl authorization ceremony. This
is an audited code-path boundary, not OS credential isolation.

The compiled pricing contract is `x-api-pay-per-usage/2026-07-27`. Before paid
work, the Publisher reads the private
`.agent/automations/decodex/cache/social/x/x-pricing-receipt.json` receipt. The
upstream health automation refreshes this receipt from the
[X API pricing page](https://docs.x.com/x-api/getting-started/pricing.md) with
one ordinary documentation HTTPS request and no X API call. The audit uses the
root-owned system curl with a monotonic 10-second total deadline, no redirects,
HTTPS-only protocols, and a 1 MiB response ceiling. It accepts only the exact
`Credit consumption details` section, its reads-per-resource and
writes-per-request statement, and adjacent `Read operations` and `Write
operations` tables. Their exact headers are `Resource | Unit cost` and `Action |
Unit cost`; their target labels are `Posts: Read`, `User: Read`, `Post: Create`,
and `Post: Create (with URL)`. Each escaped-dollar amount must use `per resource`
or `per request` as applicable. Fenced, split, duplicate, wrong-unit,
per-1,000, or legacy-label tables fail parsing.

The success receipt is valid for a dynamic 36 hours from each fetch. It binds the
source URL, parser version, fetch time, raw source digest, exact integer rates,
and its own integrity digest. There is no calendar expiry. The
Publisher requires 10,000 micro-USD for a user read, 15,000 micro-USD for
URL-free content create, 200,000 micro-USD for content create with a URL, and a
5,000 micro-USD ceiling for a post read. Any rate change blocks paid work until
the compiled contract is reviewed and updated. A parse failure atomically writes
an at-most-16-KiB private `x-pricing-failure.json` marker with a bounded table
diagnostic and digests, never the source page. A failure marker at least as new
as the success receipt immediately blocks paid work, even during the older
receipt's 36-hour window.

Cost fields are conservative reservation ceilings, not provider bills. A
normal full 24-hour and seven-day lifecycle reserves exactly 40,000 micro-USD.
This is also a hard ceiling for one immutable Radar publication lineage across
all billing months and all retries or recovery calls. The separate monthly hard
cap remains 1,250,000 micro-USD. A paid call is not executed unless both limits
can reserve its full ceiling.
`social cost-report` is the only consumer-facing ledger summary. It validates
all current v4 publication and observation attempts before it reports one month.
The bounded report contains used and reserved ceilings, the hard cap, the
remaining ceiling, and fixed call counts. It contains no post text, path,
response digest, or raw response.

Generated artifacts are create-only, mode `0600`, and local to
`.agent/automations/decodex/cache`. Private state and lock mutations use pinned
directory descriptors and no-follow opens. The artifacts are not committed or
uploaded.

## Social State Retention

Health runs `social gc` first so journal recovery completes before any ordinary
validation. GC validates the bounded state under the social mutation lock before
it plans a deletion. Health then runs one full `validate-social` readback. A GC
validation failure or the final validation failure prevents a successful cleanup
result.

GC keeps the 14 most recent valid daily strategies and the 8 most recent valid
weekly strategies. It can remove an additional strategy when `reviewed_at` is at
least 10 days old. GC removes old strategies before it evaluates lineage
references.

GC can remove a lineage only when its newest trusted business timestamp is at
least 10 days old and one of these complete forms applies:

- a candidate, one consumed reservation, one verified published post, both the
  due 24-hour and seven-day outcomes, one successful publication attempt, and
  both successful observation attempts;
- a checked skip candidate and its matching terminal skipped post, with no
  reservation, outcome, or xurl attempt.

GC keeps active or unconsumed candidates, active reservations, failed or
uncertain posts and attempts, inflight attempts, missing outcome windows,
inconsistent lineages, and every lineage referenced by a retained strategy. Any
xurl usage record in the current UTC billing month keeps its complete lineage.
After a complete terminal lineage becomes eligible, GC removes its social
artifacts and its three validated successful xurl attempts as one component.
Unresolved attempts remain durable public-effect idempotency authority.
Task-retention receipts store only an evidence-path digest. They do not protect
strategies or social snapshots, and they do not control social GC.

One scan accepts at most 8,192 entries, 4,096 files, and 64 MiB. Artifact and
attempt files are at most 1 MiB. GC uses schema timestamps, not filesystem
modification time, as retention truth. A
symlink, unsafe mode or owner, extra hard link, malformed JSON, unknown schema,
unexpected entry, replacement race, or exceeded bound stops the operation.
Reports contain only bounded counts and fixed reason codes.

GC never removes Radar or upstream evidence. All social cache remains local. It
is not archived to GitHub or another external service.
