---
name: codex-upstream-triage
description: Use when scanning latest upstream OpenAI Codex commits, PRs, releases, or changelog entries to decide which items deserve a GitHub bundle, code analysis, upstream-impact classification, site signal, or social draft.
---

# Decodex Codex Upstream Triage

Use this skill before deep analysis. Its job is to keep Radar fast and selective: find
candidate upstream Codex changes, group them correctly, and choose the next artifact.

This is a Decodex repository-development instruction surface, not an installable
Decodex plugin skill.

## Read Before Triage

- `docs/spec/github-change-bundle.md`
- `docs/spec/upstream-impact.md`
- `docs/runbook/local-github-signal-workflow.md`
- `dev/skills/codex-code-analysis/SKILL.md`
- `dev/skills/codex-release-analysis/SKILL.md`

## Inputs

- Upstream repository, normally `openai/codex`
- A time window, release tag, PR number, commit SHA, or changelog URL
- Optional existing Decodex signal or release-delta artifacts

## Retrieval Order

Use the lightest source that can answer the triage question:

1. GitHub release or compare metadata when the user asks about a release.
2. GitHub PR metadata when a PR number is known.
3. GitHub commit metadata when only a SHA is known.
4. Upstream changelog or browser observation when the question is about public product
   framing.

For a latest-commit pass, list recent upstream commits first, then resolve promising
commits back to PRs before building bundles. A commit list is a queue, not final
evidence.

## Candidate Ladder

Classify each item as exactly one:

| Decision | Meaning | Next step |
| --- | --- | --- |
| `skip` | Internal churn, no safe user or Decodex implication. | Record nothing durable. |
| `watch` | Interesting but too weak, too hidden, or too broad. | Optional `upstream_impact/v1` with `control_plane_impact = "watch"`. |
| `bundle` | Enough GitHub context exists for code analysis. | Build or reuse a `github_change_bundle/v1`. |
| `release_review` | Release or changelog framing needs comparison against commits and signals. | Use `codex-release-analysis`. |
| `style_reference` | Useful only as style or audience evidence. | Save no technical artifact; use only as optional style context when a separate source-backed draft exists. |

## Grouping Rules

- Prefer PR-first grouping over individual commit grouping whenever a commit maps to a
  merged PR.
- Group adjacent commits when they share the same PR, feature area, or release note.
- Do not split a multi-commit PR into separate signals unless the PR clearly ships
  multiple independently useful user paths.
- Treat sparse release bodies such as a title-only prerelease as an index into commits,
  not as enough evidence for `confirmed` claims.

## Radar Triggers

Escalate to `codex-code-analysis` when changed files or release text mention:

- app-server, app-server protocol, remote control, or websocket transport
- plugins, MCP, tool search, browser automation, or Chrome integration
- sandboxing, permissions, approval policy, hooks, or config schemas
- model providers, auth, accounts, or rate-limit behavior
- CLI/TUI behavior visible to a normal Codex user

Escalate to `codex-release-analysis` when the source is a release, prerelease, app
update, or public changelog.

Escalate to `x-post-draft` only after there is technical source evidence and a clear
Publisher angle. Style references from X must not start a social draft by themselves.

## Output

Return a compact triage note with:

- source URLs and timestamps
- grouped candidate IDs
- triage decision for each group
- why skipped items were skipped
- next skill to use
- confidence limits

Do not draft `signal_entry/v1` or `social_post_draft/v1` directly from this skill.
Do not treat this note as a durable repository artifact unless a later change adds a
schema, path, and validator for it.
