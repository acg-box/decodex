# Decodex Dev Skills

Purpose: Route repo-local development skills for the Decodex Radar and Publisher
pipeline.

These skills are checked-in repository-development instructions. They are not packaged
with the installable Decodex plugin under `plugins/decodex/`.

## Skill Map

Use these skills in order when turning upstream Codex activity into Decodex content or
follow-up work:

1. `codex-upstream-triage`: choose which upstream commits, PRs, releases, or changelog
   entries deserve deeper analysis.
2. `codex-code-analysis`: read the selected upstream code or patch evidence and map it
   to user-visible, Control Plane, and Publisher implications.
3. `codex-release-analysis`: evaluate release or changelog material against commits,
   PRs, release-delta artifacts, and already-published Decodex signals.
4. `github-signal`: turn the reviewed GitHub bundle and analysis result into the
   `analysis_draft` JSON consumed by `scripts/github/render_signal_entry.py`.
5. `x-post-draft`: turn evidence-backed Radar output into a reviewable
   `social_post_draft/v1` artifact for `@decodexspace`.

Use only the skills needed for the current artifact. Do not create a social draft just
because a signal exists.

Only the existing checked-in contracts are durable artifacts today:
`github_change_bundle/v1`, `analysis_draft`, `signal_entry/v1`, `upstream_impact/v1`,
`release_delta/v1`, and `social_post_draft/v1`. The triage, code-analysis, and
release-analysis skills are reasoning passes unless their conclusions are promoted
into one of those contracts.
