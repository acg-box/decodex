---
name: x-post-publisher
description: Use when publishing or terminalizing a checked social_candidate/v1 for @decodexspace through the bounded xurl publisher.
---

# Decodex X Post Publisher

Use this skill only after a checked `social_candidate/v1` exists. The checked-in
`decodex-publisher` binary is the sole X endpoint client.

## Read First

- `../references/social-release-publisher-gates.md`
- `automations/decodex/skills/x-post-quality-system/SKILL.md`

## Authority

- Consume checked candidate evidence. Do not create new technical analysis.
- Do not invoke `xurl`, X MCP, direct HTTP, Computer Use, or account-control tools
  from the automation prompt.
- Do not inspect credentials, cookies, browser profiles, local storage, or raw API
  responses.
- Keep every generated artifact private under
  `.agent/automations/decodex/cache` with mode `0600`.
- Never commit or upload generated social state.

## Publication Gate

Publish only when all conditions are true:

- `decision.worthiness` is `publish`.
- The candidate contains one text item with at least 80 Unicode characters and
  at most 260 X-weighted characters under the conservative official
  twitter-text v3 ranges.
- The public text contains no URL.
- Every factual claim has durable official or landed evidence.
- The candidate embeds a verified `radar_content_eligibility/v1` receipt and exact
  queue, review, and impact source references.
- The one public text item is the exact ordered claim composition. It contains no
  factual text outside an evidence-bound claim.
- The text states a concrete operator-visible change and consequence.
- The text is not generic, copied, vague, or primarily promotional.
- No published post already has the same idempotency key.
- The daily count is zero and the shared monthly reserved cost ceiling remains at or below
  1,250,000 micro-USD.

Use `social terminalize-skip` for a checked quality skip. It performs no X call.

## Atomic Publish

1. Run `validate-social`.
2. Create one reservation with `social reserve-publish`.
3. Publish only with `social publish-xurl`.
4. Require xurl app `default`, OAuth2 `decodexspace`, exact version `1.3.1`, and
   the approved binary SHA-256.
5. Require one create response and one post-ID readback.
6. Verify the exact ID, text, and `@decodexspace` author.
7. Write the publication and consume the reservation under one lock.
8. Run `validate-social` again.

The normal publication ceiling is 30,000 micro-USD ($0.030). It includes paid
identity read, create, and initial readback. If create may have succeeded
but no trusted post ID exists, do not retry automatically. Keep the attempt
unresolved for reconciliation.

## Outcomes

Use `social observe-xurl` for one due window:

- `24h`: 23 to 48 hours after publication.
- `7d`: 167 to 192 hours after publication.

Each observation reserves 5,000 micro-USD, reads by exact post ID, verifies text
and author, and writes one immutable `social_outcome/v1`.

## Memory

Record only date, bounded result code, artifact IDs, API call counts, micro-USD
cost ceilings, and next due check. Do not store public text, raw responses,
credentials, personal data, or absolute paths.

## Completion

Seal the successful task with its exact terminal post or outcome evidence. A
validated skip, duplicate, or proven no-op can also be sealed. Keep uncertain,
failed, blocked, or invalid work visible.
