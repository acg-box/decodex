---
name: codex-release-analysis
description: Use when evaluating upstream Codex releases, prereleases, app updates, or changelog entries and deciding what Decodex should publish, watch, or absorb from the release.
---

# Decodex Codex Release Analysis

Use this repo-local skill when GitHub releases, prerelease tags, changelogs, app update
notes, or release-focused social posts need interpretation from already-reviewed Radar
evidence.

## Read Before Analysis

- `docs/spec/release-delta.md`, `docs/spec/signal-entry.md`,
  `docs/spec/upstream-impact.md`, `docs/spec/social-candidate.md`, and
  `docs/spec/social-publishing.md`
- `docs/runbook/local-github-signal-workflow.md`
- `dev/skills/codex-upstream-triage/SKILL.md`
- `dev/skills/codex-code-analysis/SKILL.md` only when explicitly routing missing
  release-window gaps back to source analysis

## Inputs

- Release tag, changelog URL, or app update URL
- Existing `release_delta/v1`, `upstream_review/v1`, `upstream_impact/v1`, and
  `signal_entry/v1` artifacts
- GitHub compare metadata for gap detection and PR/commit-title grouping, not as a
  substitute for source review
- Official Codex changelog entries from `https://developers.openai.com/codex/changelog`
  when the update is app-shaped, mobile-shaped, or otherwise not represented by an
  `openai/codex` GitHub release
- Historical style lessons from `@CodexReleases` and `@Codex_Changelog` only when
  they are already encoded in repo docs or supplied by the operator. Recurring
  automation must not browse those accounts, use their coverage as evidence, or decide
  publish/skip state from whether they posted.

This advisory pass does not replace deterministic `release_delta/v1` generation.

## Source Analysis Boundary

Do not perform fresh upstream source analysis. Use compare metadata only to find
unreviewed PR/commit gaps, then return `upstream_analysis_required` for
`codex-upstream-triage` and `codex-code-analysis`. Publish rollups only from
source-backed artifacts plus release/compare metadata.

## Release Reading Rules

- Treat release and prerelease tags as reporting checkpoints over the commit/PR stream,
  not as a separate higher-priority intake lane.
- Keep release and prerelease channels separate:
  - Stable release posts compare the current stable release against the previous stable
    release in the same tag channel.
  - Prerelease posts compare the current prerelease against the previous prerelease in
    the same upcoming release train, such as `0.138.0-alpha.3` ->
    `0.138.0-alpha.4`.
  - The first prerelease after a stable release compares against that stable release
    and does not quote a prior prerelease post.
- Do not use the homepage top-level stable-versus-prerelease `release_delta/v1` pair
  as the copy basis for every prerelease post. It is an index. Select the adjacent
  channel pair that matches the post being written.
- Treat release notes as discovery, not proof, when they are sparse.
- Treat official Codex app and mobile changelog entries as first-class release-shaped
  sources. They can support a `release_pulse` or `watch_note` without GitHub compare
  evidence when the post only summarizes the changelog entry and links that source.
- Use GitHub compare data and PR mappings to explain what changed between stable and
  prerelease tags. For prerelease posts, extract named PR/commit clusters first:
  user-visible or anticipated workflow changes, protocol/API/schema changes,
  plugin/config/tooling changes, and removals/deprecations. A generic theme summary
  is not enough when the compare window contains named PRs or commit titles.
- When a candidate names important PRs in public copy, include direct GitHub PR URLs
  on first mention. Raw `#12345` shorthand is allowed only as secondary notation after
  the URL is already present or when the exact compare URL is the sole source link.
- Prefer already-published `signal_entry/v1` items when they match the compare commit
  or PR evidence.
- Do not imply a feature is broadly available when the source says alpha, beta,
  rollout, platform-gated, or config-gated.
- Do not write a release recap that only duplicates a release bot. Prefer a summary
  built from accumulated Decodex signal, upstream-impact, and commit/PR analysis.
- Do not let sparse prerelease bodies disappear silently. If a new prerelease is the
  latest checkpoint but source analysis is incomplete, emit an explicit watch decision:
  either a source-backed `watch_note` candidate that says only what is proven, or a
  durable `needs_upstream_analysis` / no-op record with the gap list.
- Treat prerelease interpretation as Decodex's differentiated lane because official
  release metadata, compare metadata, PR titles, and existing Radar artifacts can
  support early theme reads. The advantage comes from source-backed interpretation, not
  from reading or reacting to other accounts' coverage.

## Release Rollup Path

When the target is an OpenAI Codex stable release:

1. Refresh or read release metadata.
2. Select the current stable release and the immediately previous stable release in
   the same tag channel.
3. Compare current stable against previous stable. Do not mix prerelease-only
   checkpoint copy into the stable release post unless it is already part of the
   stable release delta.
4. Start from existing `signal_entry/v1`, `upstream_impact/v1`, and recent commit/PR
   analyses that match the stable-to-stable compare range.
5. Use `release_rollup` only after accumulated evidence explains the useful changes.

When the target is an OpenAI Codex prerelease:

1. Refresh or read `release_delta/v1`.
2. Select the adjacent prerelease comparison for the same train. For example, if the
   current checkpoint is `0.138.0-alpha.4`, prefer `0.138.0-alpha.3` ->
   `0.138.0-alpha.4`. If it is the first prerelease after a stable release, compare
   the current stable release -> first prerelease.
3. Start from existing `signal_entry/v1`, `upstream_impact/v1`, and recent
   commit/PR analyses that match the compare range.
4. Use `decodex radar backfill-release-range --dry-run` to find `compare.pr_numbers`
   gaps that still need code analysis.
5. Group findings by reader value: useful now, important for Decodex Control Plane,
   anticipated user workflow changes, protocol/API changes, deprecated/removed
   behavior, and watch-only changes.
6. Publish `release_rollup` only after the summary is grounded in those historical
   analyses and passes the daily cap.
7. Publish a prerelease `watch_note` only when the latest checkpoint itself is useful
   to readers and every claim is limited to release metadata, compare metadata, PR
   titles, source URLs, and explicit caveats. Do not summarize unreviewed code behavior
   in this mode.
8. Prefer a short prerelease-read thread when there is enough metadata to identify
   themes. The first post should name the highest-value PR/commit cluster, follow-ups
   should separate protocol/API changes from user-facing workflow changes, and the
   final post should carry source and alpha caveats.
9. Record the channel lineage in candidate evidence: previous checkpoint, current
   checkpoint, compare URL, previous prerelease post URL when one exists, and whether
   this is the first prerelease after a stable release.
10. If material gaps remain, stop with `upstream_analysis_required` instead of filling
    them inside this skill.
11. Write `social_candidate/v1` with `decision.worthiness = "publish"`, `"defer"`, or
    `"skip"` when the checkpoint needs a durable Publisher decision.
12. Refresh `release_delta/v1` after new signals are rendered so the homepage can map
   the release window to tracked signals.

When the target is an official Codex app or mobile changelog entry:

1. Read the current official changelog entry and preserve the version/date.
2. Use repo-local style lessons only; do not read benchmark accounts or use their
   coverage to decide urgency or skipping.
3. Use `release_pulse` for a source-led update card when the changelog lists concrete
   user-visible changes.
4. Use `watch_note` when the changelog is useful but platform, plan, or rollout limits
   should be highlighted.
5. Do not require GitHub compare evidence unless the post claims repository behavior.

## Analysis Modes

Use exactly one primary mode:

| Mode | Use when | Output |
| --- | --- | --- |
| `release_pulse` | The release headline is the story. | Short awareness note or social post. |
| `delta_explainer` | Compare commits map to existing signals or clear PRs. | Refresh existing `release_delta/v1` and summarize the evidence. |
| `operator_impact` | Release changes app-server, plugins, browser, MCP, permissions, sandbox, hooks, config, auth, or providers. | `upstream_impact/v1` plus follow-up if needed. |
| `watch_note` | The release is interesting but evidence is incomplete. | Watch note with caveats. |

## Prerelease Intro Path

For sparse Codex prereleases, prefer `delta_explainer`, `operator_impact`, or a
source-backed release rollup over `release_pulse`; the release version alone is rarely
the useful story. A timely prerelease-read `watch_note` is acceptable when it tells
readers what direction the metadata suggests, what is not yet reviewed, and where to
follow the exact checkpoint.

Do not treat sparse prerelease notes as automatic silence. For every new prerelease
checkpoint, choose one outcome:

- `release_pulse`: short candidate intro from public release and compare metadata.
- `watch_note`: a cautious preview when the checkpoint is interesting but release-window
  source analysis is incomplete.
- `release_rollup`: a stronger post when existing upstream reviews, impacts, and signals
  explain the useful changes.
- `defer` or `skip` candidate decision: when a post would only repeat a version tag or
  would require unreviewed code claims.

The intro must name the tag, source, timing, and evidence gap. Do not invent feature
claims from the tag name, sparse body, or social style references.

Style constraints:

- Release-bot style is useful for speed: version, three bullets, source link.
- Human analysis style is useful for value: what changes in a real workflow, why it
  matters, what to try, and where the limit remains.
- Decodex should prefer the human-analysis shape whenever source evidence supports it.
- For app updates, the best shape is a concise update card: version/date, three
  concrete user-visible changes, rollout caveat if present, and the official changelog.
- For prerelease alpha checkpoints, avoid excitement language. Use a careful watch
  shape that names the exact tag, compare window, metadata-derived themes, and analysis
  gap.
- Preserve scan-friendly formatting in every X draft: short headline, blank line,
  two to four compact bullets or lines, then source/caveat. Do not publish a dense
  paragraph when the source naturally splits into important commits, features, protocol
  changes, and caveats.

## Output

Return:

- source/timestamp, release-body quality, compare/PR evidence, matching signal slugs
- chosen mode, takeaway, Control Plane impact, and `social_candidate/v1`
  `decision.worthiness`

Promote durable conclusions into existing artifacts only: `upstream_impact/v1`,
Codex-owned `analysis_draft` plus `decodex radar render-signal` output, refreshed
`release_delta/v1`, `social_candidate/v1`, or terminal `social_post/v1`.
