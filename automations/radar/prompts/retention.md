Apply Decodex Radar artifact retention from this repo checkout.

Authority and boundaries:
- This is Codex app automation, not GitHub Actions.
- Repo-local automation source is `automations/radar`.
- Generated state must stay under `.agent/automations/radar/cache`.
- Do not mutate Linear, publish social content, open or land PRs, or write generated archive state into tracked source.

Preflight:
Before any archive manifest write or cleanup action, run `pwd`, `git status --short --branch`, and `git rev-parse HEAD`. Report cwd, branch, HEAD, and dirty state. If the checkout is dirty, the cwd is not the automation checkout, required repo-local source files are missing, or archive validation is unavailable, fail closed before mutating cache state.

Required reads:
- `openwiki/integrations/plugins-automations-and-auxiliary-tools.md`

Workflow:
1. Start with a dry-run-first pass.
2. Identify hot raw artifacts older than the retention window under `.agent/automations/radar/cache/github/bundles`, `.agent/automations/radar/cache/github/reviews`, and `.agent/automations/radar/cache/generated/analysis`.
3. Preserve curated Radar artifacts that still have automation value: current queue, impact records, release deltas, signals, and archive manifests.
4. If cleanup is needed, write an archive manifest under `.agent/automations/radar/cache/archive/index` before removing matching raw cache files.
5. If external release storage is required, report an explicit handoff instead of using GitHub Actions.
6. Validate archive manifests when practical.

Terminal report:
Report selected files, manifest paths, removal paths, validation evidence, persistence evidence, skipped/no-op reason, and blockers. Archive the run thread after a terminal no-op, dry-run-no-archive-needed, archive-contract-persisted, skipped, or blocked-with-handoff outcome when no human handoff remains.
