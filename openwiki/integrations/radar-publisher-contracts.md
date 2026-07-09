# Radar Publisher Contracts

This page is the compact contract guide for Radar-generated upstream evidence, Decodex Publisher social handoff, and the static public site boundary. Use it when changing `apps/radar/`, `automations/radar/`, `apps/decodex-publisher/`, `automations/decodex/`, or site content that consumes generated artifacts.

## Contract owners

- Radar owns upstream evidence and validation for `github_change_bundle/v1`, `upstream_review_queue/v1`, `upstream_review/v1`, `upstream_impact/v1`, `analysis_draft`, `signal_entry/v1`, `release_delta/v1`, `control_plane_upgrade_candidate/v1`, archive manifests, the local Radar ledger, and bundle/signal/release operations (`apps/radar/src/constants.rs`, `apps/radar/src/cli/commands.rs`, `apps/radar/src/artifact_validation/core/dispatch.rs`).
- Decodex Publisher owns `social_candidate/v1`, `social_publish_reservation/v1`, `social_post/v1`, social validation, and publication reservation workflows (`apps/decodex-publisher/src/lib.rs`, `apps/decodex-publisher/src/social_validation.rs`, `apps/decodex-publisher/src/social_publish.rs`).
- `automations/radar/radar.toml` is the path handoff contract from Radar cache to Publisher cache. Generated Radar state belongs under `.agent/automations/radar/cache`; generated Publisher state belongs under `.agent/automations/decodex/cache/social` (`automations/radar/README.md`, `automations/decodex/README.md`).
- The public site under `site/` is static Astro output and must not depend on live Decodex daemon state, runtime SQLite, tracker credentials, account-pool state, or local evidence (`site/README.md`, `site/package.json`).

## Radar artifact chain

Radar starts from deterministic GitHub evidence. `radar bundle build` writes `github_change_bundle/v1` for PR-first or commit-only source material; bundle validation requires owner/name repo, analysis mode, default branch, non-empty commit/file lists, and PR fields when `analysis_mode = "pr_first"` (`apps/radar/src/operations.rs`, `apps/radar/src/artifact_validation/bundle.rs`).

`radar refresh-upstream-queue` writes `upstream_review_queue/v1` and records inspected commits in the local Radar ledger unless `--no-ledger` is used. The queue is routing evidence only: it may carry surface hints, attention flags, review priority, and `next_step = ai_review_required`, but it must not make final public-value or compatibility claims (`apps/radar/src/operations.rs`, `apps/radar/src/artifact_validation/upstream/queue.rs`, `apps/radar/src/ledger.rs`).

`upstream_review/v1` is the source-backed AI review boundary. It records the subject, source refs, observed change, changed surfaces, confidence, evidence, and next actions. Current validation accepts actions such as promotion to upstream impact, signal entry, or control-plane upgrade candidate; historical `linear_followup` is allowed only for old review paths before the configured cutoff (`apps/radar/src/artifact_validation/upstream/review.rs`, `apps/radar/src/constants.rs`). AI review output is evidence, not mutation authority.

`upstream_impact/v1` is the shared handoff from Radar Review into both Publisher and Control Plane reasoning. It carries `public_signal_decision`, `control_plane_impact`, `publisher_angle`, confidence, and evidence. New Radar-derived Publisher candidates and Control Plane upgrade candidates should cite the matching upstream impact instead of independently reinterpreting release notes or raw reviews (`apps/radar/src/artifact_validation/upstream/impact.rs`).

## Release deltas and signals

`release_delta/v1` compares selected upstream releases and prereleases, preserves release identity, compare metadata, supported release options, precomputed comparisons, and tracked signal slugs. `radar refresh-release-delta` should reuse validated signal entries and compare evidence; sparse release notes alone are not enough for behavioral claims (`apps/radar/src/release_delta.rs`, `apps/radar/src/artifact_validation/release.rs`).

`signal_entry/v1` is the public site signal shape. Signals require source references, proof points, confidence, impact, and user-facing value. Homepage inclusion is reserved for entries with material value such as medium/high impact, a try path, config flags, confirmed capability, or migration/deprecation relevance; low-impact internal churn can remain traceable without dominating the public surface (`apps/radar/src/operations.rs`, `apps/radar/src/artifact_validation/signal.rs`).

Generated analysis drafts are not final signals until rendered and validated by Radar. Use `radar render-signal` only after the bundle and analysis draft validate, then run `radar validate` over the relevant artifact paths (`apps/radar/src/operations.rs`, `apps/radar/src/paths.rs`).

## Control-plane upgrade candidates

`control_plane_upgrade_candidate/v1` proposes Decodex Control Plane work from upstream evidence. It must include source refs, `upstream_impacts`, target Codex version/tag/commit/release information, affected surfaces, validation gates, stop conditions, and an authority object with `decision_contract_required = true`, `program_intake_required = true`, and `mutation_allowed = false` (`apps/radar/src/artifact_validation/upstream/control_plane_upgrade.rs`).

A candidate is not executable work by itself. Promote implementation only through the Decodex runtime's Decision Contract and Program Intake boundaries, and keep Radar from creating Linear issues or code mutations directly.

## Publisher and social reservations

`social_candidate/v1` is a pre-publication handoff artifact. It may cite upstream reviews, upstream impacts, signals, release deltas, or URLs; current validation rejects Radar-derived candidates that cite upstream reviews or release deltas without the shared `upstream_impact/v1` handoff (`apps/decodex-publisher/src/social_validation/candidate.rs`). Candidate producers must not publish to X or write terminal post records.

`social_publish_reservation/v1` is a pre-compose lease. Publisher automation must create active reservations through `decodex-publisher social reserve-publish`, not by hand-writing JSON. The command checks slug, idempotency key, daily limit, candidate/URL refs, duplicate keys, existing active reservations, terminal posts, schema validity, and create-new write semantics before persisting under `.agent/automations/decodex/cache/social/x/reservations` (`apps/decodex-publisher/src/cli/social.rs`, `apps/decodex-publisher/src/social_publish.rs`).

`social_post/v1` is the durable terminal record for published, blocked, failed, or skipped social output. Validation enforces channel/account constants, modes, status-specific payloads, evidence notes, claims, source refs, and cross-file idempotency conflicts between active reservations and terminal posts (`apps/decodex-publisher/src/social_validation/post.rs`, `apps/decodex-publisher/src/social_validation/reservation.rs`, `apps/decodex-publisher/src/social_validation/cross_file.rs`).

Publisher is an artifact consumer. It must not refresh upstream state, perform fresh upstream source analysis, bypass reservations, exceed the social cap, publish unsupported claims, expose private/local details, or treat X search/social engagement as technical evidence (`automations/decodex/README.md`).

## Static site boundary

The site may render checked-in static product content and public assets from `site/src/` and `site/public/`. It must not become a runtime dashboard, hosted operator control plane, monitoring feed, or live publishing queue. Validate site changes with Astro checks/builds and keep public claims aligned with runtime source and validated artifacts (`site/README.md`, `site/package.json`, `openwiki/integrations/radar-publisher-site.md`).

Radar and Publisher artifacts can inform static content only after accepted handoff and validation. Do not make the site read local `.agent/` cache state at request time or depend on a running `decodex serve` process.

## Retention and evidence boundaries

Use the local Radar ledger for high-frequency trace, skipped/low-value subjects, commit-to-PR mappings, and artifact links. Use Git only for curated warm artifacts such as public signals, current release deltas, upstream impacts, control-plane candidates, social publication records when intentionally retained, and archive manifests (`apps/radar/src/ledger.rs`, `apps/radar/src/paths.rs`).

Raw bundles and analysis drafts are hot working artifacts. Archive manifests preserve cold release-asset pointers, checksums, source commit, release URL, and file metadata; compressed archives themselves are not committed (`apps/radar/src/artifact_validation/archive.rs`). Generated media is local by default; source control should keep small JSON records and public URLs rather than raw generated images unless an operator explicitly requests a sample.

Do not read or document secret values. Configuration may reference token environment variable names, but OpenWiki and public artifacts must not include credentials, private issue details, hidden reasoning, local runtime paths, or account material.

## Stop conditions

Stop and route back to the owning boundary when any of these are true:

- Radar evidence lacks source-backed review, upstream impact, or compare evidence for the claim being made.
- A Publisher candidate cites Radar reviews or release deltas but has no shared `upstream_impact/v1` handoff.
- A control-plane candidate lacks required Decision Contract/Program Intake authority or sets mutation authority outside those paths.
- Social publication lacks an active validated reservation, has an idempotency/daily-cap conflict, cannot verify duplicate state, or would publish unsupported/private claims.
- Site content would require live daemon state, local cache reads, private runtime evidence, or claims not backed by current source or validated artifacts.
- Validation fails for `radar validate`, `radar bundle validate`, `decodex-publisher validate-social`, or the relevant site check/build.
