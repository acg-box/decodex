Review upstream OpenAI Codex changes from this repo checkout.

Authority and boundaries:
- This is Codex app automation, not GitHub Actions.
- Repo-local automation source is `automations/radar`.
- Generated state must stay under `.agent/automations/radar/cache`.
- Do not mutate Linear, publish to X, open or land PRs, or write generated Radar artifacts into tracked source.

Preflight:
Before any generated-state write, run `pwd`, `git status --short --branch`, and `git rev-parse HEAD`. Report cwd, branch, HEAD, and dirty state. If the checkout is dirty, the cwd is not the automation checkout, or required repo-local source files are missing, fail closed before mutating cache state.

Required reads:
- `automations/radar/skills/codex-upstream-triage/SKILL.md`
- `automations/radar/skills/codex-code-analysis/SKILL.md`
- `automations/radar/skills/github-signal/SKILL.md`
- `docs/spec/upstream-review.md`
- `docs/spec/github-change-bundle.md`
- `docs/spec/signal-entry.md`
- `docs/spec/upstream-impact.md`
- `docs/spec/control-plane-upgrade-candidate.md`

Workflow:
1. Refresh deterministic upstream state with `radar refresh-upstream-queue` and, when release context matters, `radar refresh-release-delta`.
2. Read `.agent/automations/radar/cache/github/review-queue/openai-codex-latest.json` and select the smallest high-value batch that can finish in this run.
3. For each selected subject, build or validate its bundle under `.agent/automations/radar/cache/github/bundles`.
4. Run Codex source analysis only through the explicit AI boundary in `automations/radar/scripts/github/run_codex_analysis.py` or the Rust `radar` command that wraps it.
5. Persist source-backed `upstream_review/v1`. When the reviewed change has any
   Publisher or Control Plane relevance, also persist the matching
   `upstream_impact/v1`; this is the shared upstream scan artifact consumed by Release
   Curator and Control Plane upgrade proposals.
6. Persist optional `control_plane_upgrade_candidate/v1`, optional `analysis_draft`,
   and optional rendered `signal_entry/v1` under `.agent/automations/radar/cache`.
   When a change may deserve Decodex Publisher review, ensure the shared
   `upstream_impact/v1` exists as the handoff artifact.
   - Write Control Plane upgrade candidates only under `.agent/automations/radar/cache/github/control-plane-upgrades`.
   - Cite the shared impact artifact in downstream `source_refs.upstream_impacts`.
   - A Control Plane upgrade candidate is evidence for later Decision Contract and Program Intake work; it must not mutate Linear, GitHub, worktrees, project config, Codex installs, or Decodex source.
7. Validate changed Radar JSON with `radar validate` before terminal completion.

Terminal report:
Report selected subjects, generated or updated paths, validation commands, skipped/deferred subjects, evidence gaps, and residual risks. Archive the run thread after a terminal no-op, no-new-subject, artifacts-persisted, blocked, skipped, or failed-closed outcome when no human handoff remains.
