# Decodex Dev Skills

Purpose: Route repo-local development skills for the Decodex Radar and Publisher
pipeline.

These skills are checked-in repository-development instructions. They are not packaged
with the installable Decodex plugin under `plugins/decodex/`.

## Skill Map

Use these skills as a pipeline when turning upstream Codex activity into Decodex
content or follow-up work:

1. `codex-upstream-triage`: read the deterministic upstream review queue or a selected
   source window and group commits by PR when possible.
2. `codex-code-analysis`: read the selected upstream code or patch evidence and map it
   to user-visible, Control Plane, and Publisher implications.
3. `codex-release-analysis`: evaluate release or changelog material against commits,
   PRs, release-delta artifacts, and already-published Decodex signals.
4. `github-signal`: turn the reviewed GitHub bundle and analysis result into the
   `analysis_draft` JSON consumed by `radar render-signal`.
5. `x-post-publisher`: consume social candidates whose
   `decision.worthiness = "publish"` or explicit operator handoffs that name checked
   Radar artifacts, then write a low-frequency
   `social_post/v1` publication, block, skip, or failure record for `@decodexspace`.

The scheduled automation boundary is producer-consumer shaped: Radar Review owns
upstream queue refresh and source-backed review/impact artifacts; Release Analysis and
Publisher consume those artifacts. Release Analysis may refresh only the lightweight
release-delta checkpoint when missing or stale. Publisher must not refresh upstream
state or fill evidence gaps.

Use only the skills needed for the current artifact. Do not publish a social post just
because a signal exists.

## Pipeline Ownership

Only the upstream analysis stage should read upstream Codex source for behavior claims:

- `codex-upstream-triage` selects and groups source candidates.
- `codex-code-analysis` reads upstream PR, commit, file, or patch evidence and produces
  the source-backed interpretation.

Downstream skills are artifact consumers. `codex-release-analysis`, `github-signal`,
`x-post-publisher`, and `x-post-quality-system` should start from validated
`upstream_review/v1`, `upstream_impact/v1`, `signal_entry/v1`, `release_delta/v1`,
`social_candidate/v1`, or `analysis_draft` evidence. If that evidence is missing or too
weak, they must return `upstream_analysis_required` for missing source-analysis
evidence or preserve a `social_candidate/v1` with `decision.worthiness = "defer"` or
`"skip"` instead of doing ad hoc source analysis.

Release and prerelease automations may use compare metadata to detect gaps, but filling
those gaps belongs back in the upstream analysis stage.

Default posture: track every upstream Codex commit as a possible evidence unit. Resolve
commits back to PRs when possible, decide whether the change matters to Decodex Control
Plane or the wider Codex community, and only then promote important, useful, or
deprecated behavior into a signal, upstream-impact artifact, Control Plane upgrade
candidate, or X post.

For upstream releases and prereleases, use `codex-release-analysis` as a rollup over
the accumulated commit/PR analysis. Codex prerelease notes are often too sparse to
explain what changed by themselves, but a new prerelease checkpoint can still produce a
cautious `social_candidate/v1` for a `release_pulse` intro or `watch_note` preview when
release metadata, compare metadata, and caveats create real reader value.

Checked-in contracts for this workflow are `upstream_review_queue/v1`,
`upstream_review/v1`, `github_change_bundle/v1`, `analysis_draft`, `signal_entry/v1`,
`upstream_impact/v1`, `control_plane_upgrade_candidate/v1`, `release_delta/v1`,
`social_candidate/v1`, `social_post/v1`, and their supporting generated artifacts. The
triage, code-analysis, and release-analysis skills are reasoning passes unless their
conclusions are promoted into one of those contracts.
