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
- GitHub compare metadata for gap detection, not as a substitute for source review

This advisory pass does not replace deterministic `release_delta/v1` generation.

## Source Analysis Boundary

Do not perform fresh upstream source analysis. Use compare metadata only to find
unreviewed PR/commit gaps, then return `upstream_analysis_required` for
`codex-upstream-triage` and `codex-code-analysis`. Publish rollups only from
source-backed artifacts plus release/compare metadata.

## Release Reading Rules

- Treat release and prerelease tags as checkpoints over the commit/PR stream.
- Treat release notes as discovery, not proof, when they are sparse.
- Use GitHub compare data and PR mappings to explain what changed between stable and
  prerelease tags.
- Prefer already-published `signal_entry/v1` items when they match the compare commit
  or PR evidence.
- Do not imply a feature is broadly available when the source says alpha, beta,
  rollout, platform-gated, or config-gated.
- Do not duplicate a release bot; add accumulated Decodex analysis or caveats.

## Release Rollup Path

When the target is an OpenAI Codex release or prerelease:

1. Refresh or read `release_delta/v1`.
2. Select the top-level `stable_release` -> `prerelease` comparison unless the user
   asks for a specific tag pair.
3. Start from matching `signal_entry/v1`, `upstream_impact/v1`, and recent analyses.
4. Run `decodex radar backfill-release-range --dry-run` to find unreviewed gaps.
5. Group useful, Control Plane, deprecated/removed, and watch-only changes.
6. If material gaps remain, stop with `upstream_analysis_required` instead of filling
   them inside this skill.
7. Write `social_candidate/v1` with `decision.worthiness = "publish"`, `"defer"`, or
   `"skip"` when the checkpoint needs a durable Publisher decision.
8. Refresh `release_delta/v1` after new signals are rendered so the homepage can map
   the release window to tracked signals.

## Analysis Modes

Use exactly one primary mode:

| Mode | Use when | Output |
| --- | --- | --- |
| `release_pulse` | The release headline is the story. | Short awareness note or social post. |
| `delta_explainer` | Compare commits map to existing signals or clear PRs. | Refresh existing `release_delta/v1` and summarize the evidence. |
| `operator_impact` | Release changes app-server, plugins, browser, MCP, permissions, sandbox, hooks, config, auth, or providers. | `upstream_impact/v1` plus follow-up if needed. |
| `watch_note` | The release is interesting but evidence is incomplete. | Watch note with caveats. |

## Prerelease Intro Path

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

## Output

Return:

- source/timestamp, release-body quality, compare/PR evidence, matching signal slugs
- chosen mode, takeaway, Control Plane impact, and `social_candidate/v1`
  `decision.worthiness`

Promote durable conclusions into existing artifacts only: `upstream_impact/v1`,
Codex-owned `analysis_draft` plus `decodex radar render-signal` output, refreshed
`release_delta/v1`, `social_candidate/v1`, or terminal `social_post/v1`.
