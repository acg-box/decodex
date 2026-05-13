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

- Treat Codex prereleases as a primary Radar source because their release bodies may be
  empty or title-only.
- Treat release notes as discovery, not proof, when they are sparse.
- Use GitHub compare data and PR mappings to explain what changed between stable and
  prerelease tags.
- Prefer already-published `signal_entry/v1` items when they match the compare commit
  or PR evidence.
- Do not imply a feature is broadly available when the source says alpha, beta,
  rollout, platform-gated, or config-gated.
- Do not write a release recap that only duplicates a release bot unless there is no
  deeper evidence-backed angle.

## Codex Prerelease-First Path

When the target is an OpenAI Codex prerelease:

1. Refresh or read `release_delta/v1`.
2. Select the top-level `stable_release` -> `prerelease` comparison unless the user
   asks for a specific tag pair.
3. Use `compare.pr_numbers` and `compare.commit_shas` as the discovery queue.
4. Remove PRs that already have published `signal_entry/v1` coverage.
5. Prioritize the remaining PRs by Radar triggers: app-server/protocol, plugins, MCP,
   browser/Chrome, tool search, hooks, permissions, sandboxing, config, auth,
   providers, and visible CLI/TUI behavior.
6. Build PR-first bundles for the selected unpublished PRs and run
   `codex-code-analysis` before `github-signal`.
7. Refresh `release_delta/v1` after new signals are rendered so the homepage can map
   prerelease deltas to the new tracked signals.

Use `scripts/github/sync_prerelease_signals.py` for the default latest-prerelease
automation path.

## Analysis Modes

Use exactly one primary mode:

| Mode | Use when | Output |
| --- | --- | --- |
| `release_pulse` | The release headline is the story and evidence is thin. | Short awareness note or social draft. |
| `delta_explainer` | Compare commits map to existing signals or clear PRs. | Refresh existing `release_delta/v1` and summarize the evidence. |
| `operator_impact` | Release changes app-server, plugins, browser, MCP, permissions, sandbox, hooks, config, auth, or providers. | `upstream_impact/v1` plus possible follow-up issue. |
| `watch_note` | The release is interesting but evidence is incomplete. | Watch note with caveats. |

For sparse Codex prereleases, prefer `delta_explainer` or `operator_impact` over
`release_pulse`; the release version alone is rarely the useful story.

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
  `operator_impact`, or `watch_note`

Promote durable conclusions into existing artifacts only: `upstream_impact/v1`,
`analysis_draft` plus rendered `signal_entry/v1`, refreshed `release_delta/v1`, or
`social_post_draft/v1`.
