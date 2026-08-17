# Radar Skills

Purpose: Route repo-local Radar skills for upstream Codex evidence gathering and
signal drafting.

These skills are checked-in repository-development instructions. They remain repo-local
and must not be copied into global `$CODEX_HOME/skills`.

## Skill Map

1. `codex-upstream-triage`: read the deterministic upstream review queue or a selected
   source window and group commits by PR when possible.
2. `codex-code-analysis`: read selected upstream code or patch evidence and map it to
   user-visible, Control Plane, and Publisher implications.
3. `codex-release-analysis`: evaluate release or changelog material against commits,
   PRs, release-delta artifacts, and already-published Decodex signals.
4. `github-signal`: turn the reviewed GitHub bundle and analysis result into the
   `analysis_draft` JSON consumed by `radar render-signal`.

## Use

Agents select only the skills that help the current research question. Direct official
evidence is valid input; a Radar queue or intermediate artifact is not required. A skill
does not create workflow authority or require another skill to run first.

Checked-in Radar contracts for this workflow are `upstream_review_queue/v1`,
`upstream_review/v1`, `github_change_bundle/v1`, `analysis_draft`,
`signal_entry/v1`, `upstream_impact/v1`, and `release_delta/v1`.
