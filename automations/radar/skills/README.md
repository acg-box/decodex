# Radar Skills

Purpose: Route repo-local Radar skills for upstream Codex evidence gathering and
signal drafting.

These skills are checked-in repository-development instructions. They are not packaged
with the installable Decodex plugin under `plugins/decodex/`.

## Skill Map

1. `codex-upstream-triage`: read the deterministic upstream review queue or a selected
   source window and group commits by PR when possible.
2. `codex-code-analysis`: read selected upstream code or patch evidence and map it to
   user-visible, Control Plane, and Publisher implications.
3. `codex-release-analysis`: evaluate release or changelog material against commits,
   PRs, release-delta artifacts, and already-published Decodex signals.
4. `github-signal`: turn the reviewed GitHub bundle and analysis result into the
   `analysis_draft` JSON consumed by `radar render-signal`.

## Pipeline Ownership

Only the upstream analysis stage should read upstream Codex source for behavior claims:

- `codex-upstream-triage` selects and groups source candidates.
- `codex-code-analysis` reads upstream PR, commit, file, or patch evidence and produces
  the source-backed interpretation.

Downstream Radar skills are artifact consumers. `codex-release-analysis` and
`github-signal` should start from validated `upstream_review/v1`,
`upstream_impact/v1`, `signal_entry/v1`, `release_delta/v1`, or `analysis_draft`
evidence. If that evidence is missing or too weak, they must return
`upstream_analysis_required` instead of doing ad hoc source analysis.

Checked-in Radar contracts for this workflow are `upstream_review_queue/v1`,
`upstream_review/v1`, `github_change_bundle/v1`, `analysis_draft`,
`signal_entry/v1`, `upstream_impact/v1`, `control_plane_upgrade_candidate/v1`, and
`release_delta/v1`.
