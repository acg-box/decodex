---
name: codex-upstream-triage
description: Use when scanning latest upstream OpenAI Codex commits, PRs, releases, or changelog entries to decide which items deserve a GitHub bundle, code analysis, upstream-impact classification, or site signal.
---

# Decodex Codex Upstream Triage

Use this skill before deep analysis. Its job is to route the deterministic upstream
review queue: group upstream Codex commits correctly, identify likely surfaces, and
choose the next artifact without deciding final impact.

This is a Decodex repository-development instruction surface, not an installed runtime
capability.

## Read Before Triage

- `automations/radar/skills/codex-code-analysis/SKILL.md`
- `automations/radar/skills/codex-release-analysis/SKILL.md`

## Inputs

- Upstream repository, normally `openai/codex`
- A time window, release tag, PR number, commit SHA, or changelog URL
- Optional existing Decodex signal or release-delta artifacts

## Retrieval Order

Use the lightest source that can answer the triage question:

1. GitHub commit metadata for the upstream commit stream.
2. GitHub PR metadata when a commit maps to a merged PR or a PR number is known.
3. GitHub release-delta or compare metadata when the user asks for a release or
   prerelease rollup.
4. Upstream changelog or browser observation when the question is about public product
   framing.

For normal Radar operation, start from `upstream_review_queue/v1`, scan recent
upstream commits first, and resolve each commit back to a PR when possible. A commit
list is a queue for understanding and classification, not final evidence.

For release or prerelease work, compare metadata is a rollup index over the commit/PR
history already being analyzed. Do not let a release tag displace the underlying commit
and PR evidence.

## Candidate Ladder

Classify each item as exactly one:

| Decision | Meaning | Next step |
| --- | --- | --- |
| `skip` | AI review confirms internal churn with no safe user or Decodex implication. | Keep ledger trace only. |
| `watch` | Interesting but too weak, too hidden, or too broad. | Optional `upstream_impact/v1` with `control_plane_impact = "watch"`. |
| `bundle` | Enough GitHub context exists for code analysis. | Build or reuse a `github_change_bundle/v1`. |
| `release_review` | Release or changelog framing needs comparison against commits and signals. | Use `codex-release-analysis`. |
| `style_reference` | Useful only as style or audience evidence. | Save no technical artifact; use only as optional style context when a separate source-backed publication candidate exists. |

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
update, or public changelog. For Codex releases and prereleases, summarize from prior
commit/PR analysis whenever possible, then use compare data to find gaps.

Record an editorial angle only after there is technical source evidence. It remains
advisory. The Content Manager independently verifies official sources before it
records content evidence.

## Output

Return a compact triage note with:

- source URLs and timestamps
- grouped candidate IDs
- triage decision for each group
- why skipped items were skipped
- next skill to use
- confidence limits

Do not draft `signal_entry/v1` directly from this skill. Do not treat deterministic
queue hints as technical claims. The owning agent must verify official source evidence
before it implements a change or records public content.
