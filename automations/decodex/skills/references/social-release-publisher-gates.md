# Social Release Publisher Gates

Use these gates for every `@decodexspace` candidate and publication.

## Source Authority

- Prefer official OpenAI documentation, `openai/codex` source, release metadata,
  and landed Decodex evidence.
- Treat CodexRadar and public social content as discovery or editorial input only.
- Confirm community claims with official evidence before publication.
- Do not infer implementation behavior from a tag, short release note, or social
  post.

## Candidate Quality

A publish decision requires:

- one concrete change;
- one useful operator consequence;
- one text item with at least 80 Unicode characters and at most 260
  X-weighted characters under the conservative official twitter-text v3 ranges;
- no URL in public text;
- one exact `radar_content_eligibility/v1` receipt for the selected queue, review,
  and impact;
- durable verified Radar evidence for each factual claim;
- canonical public text reconstructed from ordered claims and only approved
  non-factual connective segments;
- wording that stands alone without a link;
- no hype, copied text, vague monitoring language, or generic availability notice.

Create a checked skip when no candidate passes. Cadence does not lower the gate.

## Channel And Lineage

- Stable releases compare with the previous stable release in the same channel.
- Prereleases compare within the same train.
- A Decodex adaptation claim requires a landed result or current-main proof.
- Use stable idempotency keys and immutable candidate-to-reservation-to-post
  lineage.
- Content Manager writes one mode-0600 staging file. Only Publisher can derive and
  create the run-owned candidate or strategy destination under the shared state
  lock.
- Never hand-author publication or outcome evidence.

## X Cost And Authority

- `decodex-publisher` is the only X endpoint client.
- Use xurl app `default` with OAuth2 account `decodexspace`.
- Permit at most one post per day.
- Enforce 1,250,000 micro-USD ($1.25) per calendar month.
- Reserve a 30,000 micro-USD ($0.030) ceiling for paid identity read, URL-free
  publication, and initial readback.
- Reserve 5,000 micro-USD for each due outcome read.
- Do not use paid X reads for competitor research.
- Do not retry an uncertain create without a trusted post ID.

## Acceptance Evidence

A published result requires an exact ID, text, and author readback, one canonical
`https://x.com/decodexspace/status/<id>` URL, response digests, a recorded cost
ceiling, and consumed reservation. An outcome requires the same post identity, a
valid 24-hour or seven-day window, public metrics, response digest, and a recorded
cost ceiling.

Failed, blocked, uncertain, invalid, or over-budget work stays visible.

## Local Retention

Health must run full social validation, `decodex-publisher social gc`, and full
social validation again. Keep 14 valid daily strategies, 8 valid weekly
strategies, and at least 10 days after the newest trusted lineage timestamp.
Delete only complete verified publication or complete checked quality-skip
lineages. Keep active, failed, uncertain, inflight, incomplete, current
billing-month, and retained-strategy evidence. Task-retention receipts store
digests only and do not control social GC.

The scan limit is 8,192 entries, 4,096 files, and 64 MiB. Unsafe entries, unknown
schemas, malformed JSON, and replacement races fail closed. Retention uses schema
timestamps, not filesystem modification time. Cache stays local. Never archive it
to GitHub, and never delete Radar or upstream evidence.
