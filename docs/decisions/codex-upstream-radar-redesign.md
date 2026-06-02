# Codex Upstream Radar Redesign

Status: accepted

Date: 2026-06-02

Question: How should Decodex rebuild upstream Codex tracking so it can support both
public X/site updates and Decodex compatibility work?

Decision: Replace the signal-first refresh path with an upstream-review pipeline.

The new pipeline has four layers:

1. Deterministic GitHub sync records every observed upstream commit, resolves PRs when
   possible, assigns routing hints, and writes an `upstream_review_queue/v1` artifact.
2. Codex automation consumes that queue and performs AI source review for each queued
   subject.
3. Source-backed reviews promote only valuable outcomes into `upstream_impact/v1`,
   `signal_entry/v1`, `social_post/v1`, or Linear follow-up work.
4. Release and prerelease summaries roll up accumulated commit and PR analysis instead
   of treating sparse release notes as enough evidence.

GitHub Actions must stay deterministic. It may refresh GitHub metadata, release deltas,
review queues, and validation results, but it must not install Codex, inject Codex auth,
or make AI editorial judgments. AI analysis belongs in Codex automation where the local
operator already manages model access, account state, and review context.

Consequences:

- Title-score skipping is removed from the continuous Radar path.
- Public site signals are no longer the first output of upstream tracking; they are
  Publisher promotions from source-backed review.
- Decodex compatibility risks and adoption opportunities can be tracked before they
  become public content.
- Prerelease rollups can explain changes with prior commit/PR evidence even when the
  upstream prerelease has no release notes.
- Raw bundles and review artifacts remain subject to the 21-day hot-window archive
  policy; curated impacts, signals, social publication records, and archive manifests
  stay in Git.
