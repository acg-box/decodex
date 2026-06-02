# Decodex Dev Skills

Purpose: Route repo-local development skills for the Decodex Radar and Publisher
pipeline.

These skills are checked-in repository-development instructions. They are not packaged
with the installable Decodex plugin under `plugins/decodex/`.

## Skill Map

Use these skills in order when turning upstream Codex activity into Decodex content or
follow-up work:

1. `codex-upstream-triage`: read the deterministic upstream review queue or a selected
   source window and group commits by PR when possible.
2. `codex-code-analysis`: read the selected upstream code or patch evidence and map it
   to user-visible, Control Plane, and Publisher implications.
3. `codex-release-analysis`: evaluate release or changelog material against commits,
   PRs, release-delta artifacts, and already-published Decodex signals.
4. `github-signal`: turn the reviewed GitHub bundle and analysis result into the
   `analysis_draft` JSON consumed by `scripts/github/render_signal_entry.py`.
5. `x-post-publisher`: turn evidence-backed Radar output into a low-frequency
   `social_post/v1` publication, block, skip, or failure record for `@decodexspace`.
6. `rate-limit-reset-watch`: review today's `@thsottiaux` X posts with AI semantic
   judgment and refresh the homepage `reset_status/v1` artifact.

Use only the skills needed for the current artifact. Do not publish a social post just
because a signal exists.

Default posture: track every upstream Codex commit as a possible evidence unit. Resolve
commits back to PRs when possible, decide whether the change matters to Decodex Control
Plane or the wider Codex community, and only then promote important, useful, or
deprecated behavior into a signal, upstream-impact artifact, follow-up issue, or X
post.

For upstream releases and prereleases, use `codex-release-analysis` as a rollup over
the accumulated commit/PR analysis. Codex prerelease notes are often too sparse to
explain what changed by themselves.

Checked-in contracts for this workflow are `upstream_review_queue/v1`,
`upstream_review/v1`, `github_change_bundle/v1`, `analysis_draft`, `signal_entry/v1`,
`upstream_impact/v1`, `release_delta/v1`, `social_post/v1`, and
`reset_status/v1`. The triage, code-analysis, release-analysis, and reset-watch skills
are reasoning passes unless their conclusions are promoted into one of those
contracts.
