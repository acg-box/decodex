Create release and checkpoint publication candidates from official OpenAI Codex releases, prereleases, changelog entries, app updates, mobile updates, and existing Radar artifacts.

Authority and boundaries:
- This is Codex app automation, not GitHub Actions.
- Repo-local automation source is `automations/decodex`.
- Generated state must stay under `.agent/automations/decodex/cache`.
- Do not publish to X, mutate Linear, open or land PRs, or write generated publication artifacts into tracked source.

Preflight:
Before any generated-state write, run `pwd`, `git status --short --branch`, and `git rev-parse HEAD`. Report cwd, branch, HEAD, and dirty state. If the checkout is dirty, the cwd is not the automation checkout, or required repo-local source files are missing, fail closed before mutating cache state.

Required reads:
- `automations/decodex/skills/codex-release-analysis/SKILL.md`
- `automations/decodex/skills/x-post-quality-system/SKILL.md`
- `automations/decodex/skills/references/social-release-publisher-gates.md`
- `docs/spec/release-delta.md`
- `docs/spec/upstream-review.md`
- `docs/spec/upstream-impact.md`
- `docs/spec/social-candidate.md`
- `docs/spec/social-publishing.md`

Workflow:
1. Read `.agent/automations/decodex/cache/site-content/release-deltas/openai-codex-latest.json` first. Run `cargo run -p radar -- refresh-release-delta` only when that checkpoint artifact is missing, stale, invalid, or explicitly needed for a newly observed release tag. Do not refresh the upstream review queue or perform deep source analysis here.
2. Compare release/changelog claims against the shared `upstream_impact/v1` artifacts
   produced by Decodex Radar Review under
   `.agent/automations/decodex/cache/github/impact`. Use `release_delta/v1`,
   `upstream_review/v1`, `signal_entry/v1`, release URLs, and compare metadata as
   provenance or gap evidence, not as a parallel source-analysis path.
3. Do not perform deep upstream source analysis here. If claims need unreviewed PR or
   commit evidence, write a defer/no-op outcome with exact gaps for Radar Review so it
   can produce or update the shared `upstream_impact/v1` artifact.
4. When publication is justified, write or update `social_candidate/v1` under
   `.agent/automations/decodex/cache/github/social-candidates`; for Radar-derived
   candidates, include the consumed shared impact artifact in
   `source_refs.upstream_impacts`.
5. Validate changed JSON with `cargo run -p radar -- validate`.

Terminal report:
Report consumed evidence, selected mode, generated or updated paths, candidate worthiness, no-op/defer/skip decisions, validation evidence, style/reference gates used, and residual caveats. Archive the run thread after a terminal outcome when no human handoff remains.
