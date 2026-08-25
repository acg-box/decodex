---
type: "Reference"
title: "Radar, Publisher, And Site Contracts"
openwiki_generated: true
---

# Radar, Publisher, And Site Contracts

This page covers auxiliary evidence and public surfaces that surround the Decodex
runtime. For the compact automation contract, read [Radar Publisher
contracts](radar-publisher-contracts.md). For the product boundary, read [Runtime
architecture](../architecture/runtime-architecture.md).

## Radar scope

Radar is an optional auxiliary tool for GitHub and Codex evidence. It owns change
bundles, review queues, upstream impact artifacts, signal entries, release deltas,
bounded local retention, validation, and bundle generation. Radar has no native
schedule and is not workflow state for the five managed automations.

Radar artifacts live under `.agent/automations/radar/cache` when generated locally. Checked-in source for Radar behavior lives in `apps/radar/`, `automations/radar/`, and related tests.

## Upstream review and impact

Upstream review artifacts capture release or prerelease checkpoints, review notes,
and evidence. Upstream impact artifacts classify possible Decodex and editorial
relevance. They are advisory inputs only.

Required behavior:

- every artifact has stable identity and source references
- stale actions are rejected
- AI review output is evidence, not mutation authority
- an owning agent independently verifies official sources before code or content work
- direct mutation or publication from Radar output is not allowed

## GitHub change bundles

GitHub change bundles summarize PR or release source material for downstream review. They should preserve PR-first fields, commit/file summaries, analysis boundaries, and source URLs. They should not become a substitute for the target repo's own tests or review.

Radar bundle tests intentionally treat upstream documentation-path and `README.md` references as external source material. That classification does not reintroduce a Decodex repo-local legacy documentation tree.

## Signal entries and homepage inclusion

Signal entries are curated summaries of meaningful upstream changes. A signal needs identity, source references, try path, effect, claim boundary, and publication suitability. Homepage inclusion requires a clear user-facing capability or operational value, not just internal churn.

Generated analysis drafts are not final signals until curated and validated.

## Release deltas

Release deltas compare release objects and summarize material changes. They preserve
release identity, compare targets, reused signals, and explicit evidence. They can
help discovery, but the Maintainer and Content Manager must verify the official
sources themselves.

## Radar ledger and retention

The Radar ledger is bounded, disposable, owner-only working state for current artifact
links, review status, commit observations, and source-cache trace. It has no remote
retention or recovery role. Raw generated artifacts and the ledger stay outside source.
Curated public content can enter Git only through a separate reviewed source change.
Radar prunes local collections and ledger rows oldest-first and fails closed instead of
resetting an oversized ledger. Retention covers every current Radar writer
collection and removes abandoned internal temporary files while the cache-wide lock
is held.

## Publisher scope

`decodex-publisher` owns content evidence recording, publication reservation, xurl
effects, exact readback, outcome observation, and social validation. Publisher
artifacts live under `.agent/automations/decodex/cache/social` when generated
locally.

The Content Manager researches official Codex and landed Decodex sources directly.
CodexRadar can be secondary editorial input, but it cannot be the only factual
source. Publisher does not research or choose topics. It enforces the deterministic
write boundary.

## Social publishing contract

One `decodex/content-evidence/1` document records a publish or no-op decision.
Publisher derives its immutable content identity and allows at most one unresolved
candidate. The `publish-next` and `observe-due` commands hide reservation, ledger,
journal, recovery, and readback steps from the X Publisher agent. Published posts
must contain no URL, unsupported claim, private detail, local path, credential, or
hidden reasoning.

## Static site contract

The static site under `site/` is a public product surface. Route budget, homepage obligations, static boundary, and asset boundary are site-owned concerns. Site content should be source-backed and buildable without a live Decodex daemon.

Use `site/src/`, `site/package.json`, and site tests/build output as authority for current behavior. Keep product claims consistent with runtime capability and OpenWiki summaries.
