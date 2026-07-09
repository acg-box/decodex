# Radar, Publisher, And Site Contracts

This page covers auxiliary artifact families and public surfaces that surround the Decodex runtime.

## Radar scope

Radar is an auxiliary tool for upstream GitHub/Codex evidence. It owns upstream review queues, upstream impact artifacts, signal entries, release deltas, control-plane upgrade candidates, artifact retention, local ledger operations, validation, and bundle generation.

Radar artifacts live under `.agent/automations/radar/cache` when generated locally. Checked-in source for Radar behavior lives in `apps/radar/`, `automations/radar/`, and related tests.

## Upstream review and impact

Upstream review artifacts capture release/prerelease checkpoints, AI review notes, promotion boundaries, and evidence. Upstream impact artifacts classify control-plane and publisher relevance with source references and an explicit impact ladder.

Required behavior:

- every artifact has stable identity and source references
- stale actions are rejected
- AI review output is evidence, not mutation authority
- promotion requires accepted authority or a follow-up issue
- direct mutation from upstream review output is not allowed

## GitHub change bundles

GitHub change bundles summarize PR or release source material for downstream review. They should preserve PR-first fields, commit/file summaries, analysis boundaries, and source URLs. They should not become a substitute for the target repo's own tests or review.

Radar bundle tests intentionally treat upstream documentation-path and `README.md` references as external source material. That classification does not reintroduce a Decodex repo-local docs surface.

## Signal entries and homepage inclusion

Signal entries are curated summaries of meaningful upstream changes. A signal needs identity, source references, try path, effect, claim boundary, and publication suitability. Homepage inclusion requires a clear user-facing capability or operational value, not just internal churn.

Generated analysis drafts are not final signals until curated and validated.

## Release deltas

Release deltas compare release objects and summarize material changes. They should preserve release identity, compare target, options, reused signals, and explicit evidence. Release deltas can feed social or site updates only through accepted handoff, not by direct publication.

## Radar ledger and retention

The Radar ledger tracks artifact links, statuses, and release/archive state. Retention distinguishes hot raw artifacts, warm curated artifacts, and cold archived artifacts. Generated heavy artifacts stay out of source unless a manifest or curated summary is intentionally checked in.

## Control-plane upgrade candidates

Control-plane upgrade candidates are structured proposals for Decodex control-plane changes. They need source references, target Codex/Decodex surface, impact, validation expectations, and authority guardrails. They do not directly authorize code mutation.

## Publisher scope

`decodex-publisher` owns social candidate, publication reservation, and social post validation. Publisher artifacts live under `.agent/automations/decodex/cache/social` when generated locally.

Publisher consumes Radar handoff evidence and validates social publication readiness. It must not refresh upstream state, scrape new evidence, or bypass the social reservation model.

## Social publishing contract

Social candidates require source references, claim rules, mode guidance, a decision object, and publication boundary. Reservations prevent duplicates and preserve idempotency. Published posts should avoid unsupported claims, private details, local paths, credentials, and hidden reasoning.

## Static site contract

The static site under `site/` is a public product surface. Route budget, homepage obligations, static boundary, and asset boundary are site-owned concerns. Site content should be source-backed and buildable without a live Decodex daemon.

Use `site/src/`, `site/package.json`, and site tests/build output as authority for current behavior. Keep product claims consistent with runtime capability and OpenWiki summaries.
