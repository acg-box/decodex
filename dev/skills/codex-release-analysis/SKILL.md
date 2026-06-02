---
name: codex-release-analysis
description: Use when evaluating upstream Codex releases, prereleases, app updates, or changelog entries and deciding what Decodex should publish, watch, or absorb from the release.
---

# Decodex Codex Release Analysis

Use this skill when the source is release-shaped: GitHub releases, prerelease tags,
OpenAI developer changelogs, app update notes, or a release-focused social post.

This is a Decodex repository-development instruction surface, not an installable
Decodex plugin skill.

## Read Before Analysis

- `docs/spec/release-delta.md`
- `docs/spec/signal-entry.md`
- `docs/spec/upstream-impact.md`
- `docs/spec/social-publishing.md`
- `docs/runbook/local-github-signal-workflow.md`
- `dev/skills/codex-upstream-triage/SKILL.md`
- `dev/skills/codex-code-analysis/SKILL.md`

## Inputs

- Release tag, changelog URL, or app update URL
- Existing `release_delta/v1` artifact, when available
- Existing Decodex signals that may explain the release delta
- GitHub compare, commit, or PR evidence for any claim beyond the release headline

This skill is an advisory reasoning pass. It does not define a new checked-in
release-analysis artifact and does not replace deterministic `release_delta/v1`
generation.

## Release Reading Rules

- Treat release and prerelease tags as reporting checkpoints over the commit/PR stream,
  not as a separate higher-priority intake lane.
- Treat release notes as discovery, not proof, when they are sparse.
- Use GitHub compare data and PR mappings to explain what changed between stable and
  prerelease tags.
- Prefer already-published `signal_entry/v1` items when they match the compare commit
  or PR evidence.
- Do not imply a feature is broadly available when the source says alpha, beta,
  rollout, platform-gated, or config-gated.
- Do not write a release recap that only duplicates a release bot. Prefer a summary
  built from accumulated Decodex signal, upstream-impact, and commit/PR analysis.

## Release Rollup Path

When the target is an OpenAI Codex release or prerelease:

1. Refresh or read `release_delta/v1`.
2. Select the top-level `stable_release` -> `prerelease` comparison unless the user
   asks for a specific tag pair.
3. Start from existing `signal_entry/v1`, `upstream_impact/v1`, and recent
   commit/PR analyses that match the compare range.
4. Use `decodex radar backfill-release-range --dry-run` to find `compare.pr_numbers`
   gaps that still need code analysis.
5. Group findings by reader value: useful now, important for Decodex Control Plane,
   deprecated/removed behavior, and watch-only changes.
6. Publish release or prerelease X reporting only after the summary is grounded in
   those historical analyses and passes the daily cap.
7. Refresh `release_delta/v1` after new signals are rendered so the homepage can map
   the release window to tracked signals.

## Analysis Modes

Use exactly one primary mode:

| Mode | Use when | Output |
| --- | --- | --- |
| `release_pulse` | The release headline is the story and evidence is thin. | Short awareness note or social post. |
| `delta_explainer` | Compare commits map to existing signals or clear PRs. | Refresh existing `release_delta/v1` and summarize the evidence. |
| `operator_impact` | Release changes app-server, plugins, browser, MCP, permissions, sandbox, hooks, config, auth, or providers. | `upstream_impact/v1` plus possible follow-up issue. |
| `watch_note` | The release is interesting but evidence is incomplete. | Watch note with caveats. |

For sparse Codex prereleases, prefer `delta_explainer`, `operator_impact`, or a
source-backed release rollup over `release_pulse`; the release version alone is rarely
the useful story.

## Style Lessons

- Release-bot style is useful for speed: version, three bullets, source link.
- Human analysis style is useful for value: what changes in a real workflow, why it
  matters, what to try, and where the limit remains.
- Decodex should prefer the human-analysis shape whenever source evidence supports it.

## Output

Return:

- release source and timestamp
- whether the release body is explanatory or sparse
- compare or PR evidence used
- matching Decodex signal slugs, if any
- chosen mode
- user-facing takeaway
- Control Plane impact, if any
- Publisher recommendation: no post, `release_pulse`, `practical_explainer`,
  `operator_impact`, `release_rollup`, or `watch_note`

Promote durable conclusions into existing artifacts only: `upstream_impact/v1`,
Codex-owned `analysis_draft` plus `decodex radar render-signal` output, refreshed
`release_delta/v1`, or `social_post/v1`.
