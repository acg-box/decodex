Curate release checkpoint evidence from official OpenAI Codex releases,
prereleases, changelog entries, app updates, mobile updates, and existing Radar
artifacts.

Authority and boundaries:
- This is Codex app automation, not GitHub Actions.
- Repo-local automation source is `automations/radar`.
- Generated Radar state must stay under `.agent/automations/radar/cache`.
- Do not publish to X, mutate Linear, open or land PRs, write Decodex social
  artifacts, or write generated publication artifacts into tracked source.

Preflight:
Before any generated-state write, run `pwd`, `git status --short --branch`, and
`git rev-parse HEAD`. Report cwd, branch, HEAD, and dirty state. If the checkout
is dirty, the cwd is not the automation checkout, or required repo-local source
files are missing, fail closed before mutating cache state.

Required reads:
- `automations/radar/skills/codex-release-analysis/SKILL.md`
- `openwiki/integrations/plugins-automations-and-auxiliary-tools.md`

Workflow:
1. Read `.agent/automations/radar/cache/site-content/release-deltas/openai-codex-latest.json`
   first. Run `radar refresh-release-delta` only when that
   checkpoint artifact is missing, stale, invalid, or explicitly needed for a
   newly observed release tag.
2. Compare release/changelog claims against shared `upstream_impact/v1` artifacts
   under `.agent/automations/radar/cache/github/impact`. Use `release_delta/v1`,
   `upstream_review/v1`, `signal_entry/v1`, release URLs, and compare metadata as
   provenance or gap evidence, not as a parallel source-analysis path.
3. Do not perform deep upstream source analysis here. If claims need unreviewed PR
   or commit evidence, write a defer/no-op outcome with exact gaps for Radar
   Review so it can produce or update the shared `upstream_impact/v1` artifact.
4. Refresh or validate `release_delta/v1` and any source-backed `upstream_impact/v1`
   updates needed for the release checkpoint.
5. Validate changed Radar JSON with `radar validate`.

Terminal report:
Report consumed evidence, generated or updated paths, no-op/defer decisions,
validation evidence, and residual caveats. Archive the run thread after a
terminal outcome when no human handoff remains.
