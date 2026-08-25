---
type: "Reference"
title: "Radar And Publisher Contracts"
openwiki_generated: true
---

# Radar And Publisher Contracts

Radar and Publisher are separate auxiliary tools. They do not form a workflow
state machine.

## Radar Boundary

Radar owns optional repository-local research artifacts:

- `github_change_bundle/v1` and its exact-byte build receipt;
- `upstream_review_queue/v1` for deterministic GitHub discovery;
- `upstream_review/v1` and `upstream_impact/v1` as standalone analysis formats;
- `signal_entry/v1` and `release_delta/v1` for static content;
- the bounded private Radar cache and disposable ledger.

Radar has no native scheduled role in the exact-five portfolio. The Maintainer
and Content Manager may use its output as discovery or supporting editorial
input. They can also research official sources directly.

Radar does not own a content handoff, candidate selection, review pair,
eligibility decision, activation marker, or Publisher path. Its metadata cannot
authorize an X post.

## Publisher Boundary

Publisher owns:

- `decodex/content-evidence/1` candidates;
- `social_publish_reservation/v1` daily and duplicate exclusion;
- `social_post/v1` publication or skip evidence;
- `social_outcome/v1` 24-hour and 7-day observations;
- xurl authorization, pricing, budget ledger, attempt journal, and exact
  external readback.

Content Manager sends Publisher one private candidate or no-op through
`social record-candidate`. Source URLs are direct evidence. A primary source is
required and has class `official_codex` or `landed_decodex`. A CodexRadar URL may
have class `radar_secondary`, but it cannot be the only evidence.

Publisher does not read Radar cache paths or digests. It does not require a
queue, review, impact, pair, or eligibility receipt. This separation lets the
agent use the best current official evidence without ceremony.

## External Effects

Only Publisher may invoke xurl. It enforces the fixed `@decodexspace` account,
one post per day, link-free text, the monthly cost cap, exact author/text/post
readback, and uncertain-create non-retry.

Publisher also owns `social refresh-pricing`, a deterministic preflight that
makes one bounded ordinary HTTPS GET to the exact official pricing Markdown. It
uses no OAuth or token and makes zero X API calls. Strict parser failure or rate
drift blocks paid work through the private pricing receipt boundary.

The Content Manager and Publisher prompts never invoke a browser, X MCP, or
direct HTTP API for X. Radar never invokes X.

## Local Data

Radar state is under `.agent/automations/radar/cache`. Publisher state is under
`.agent/automations/decodex/cache/social`. Both roots are local, owner-only, and
excluded from public artifacts.

The X authorization contract, pricing receipt, budget ledger, and uncertain
attempt journal are preserved safety roots. Ordinary cleanup must not remove
them. No generated private state is uploaded to GitHub.

## Static Site

The public site may consume reviewed static signal or release files from Git. It
must not read live Radar cache, Publisher cache, xurl credentials, Codex task
state, or Decodex runtime state.

## Validation

```sh
cargo test -p radar
cargo test -p decodex-publisher
python3 automations/decodex/scripts/config/evaluate_automations.py --repo-only --json
```

Radar tests cover its standalone cache and artifact boundaries. Publisher tests
cover content evidence and the X external-effect boundary with a fake xurl
executable.
