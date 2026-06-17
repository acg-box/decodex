# Documentation Log

## 2026-06-18

- Added protocol journal replay idempotency to the runtime contract: protocol events
  now retain payload SHA-256 identity so app-server continuation/recovery can replay
  the same event without failing the lane, while conflicting same-sequence events still
  fail closed.
- Simplified Decodex plugin and docs manual command guidance so agent-facing usage
  examples call `decodex ...` directly instead of teaching source-run or install
  variants.

## 2026-06-17

- Dogfooded the portable `repo-memory-writer` plugin skill on this repository and
  added `docs/reference/build-test-run.md` as the source-backed build/test/run/setup
  route that the first routing probes showed was missing.
- Refreshed the high-level `docs/reference/test-suite.md` snapshot to the current
  1213 default-runnable `nextest` tests plus the one skipped live app-server test.
- Added the portable `repo-memory-writer` plugin skill so Codex can bootstrap or
  improve source-backed repository memory through AI authoring plus OKF route, graph,
  and profile validation.
- Added `docs/reference/docs-knowledge-map.md` to evaluate the practical OKF/LLM Wiki
  value in this repository and connect specialized concepts back into the docs graph.
- Added typed Authority Boundary surfaces and policy decisions so internal
  implementation recovery can continue automatically while high-risk surfaces require
  enhanced evidence, landing blocks, or human decisions.
- Clarified that architecture recovery infers Authority Boundary surfaces from
  retained tracked diffs and that `requires_enhanced_evidence` and `block_landing`
  clear only after a clean review checkpoint for the current lane head.
- Added private `phase_acceptance_check` handling so implementation and repair phases
  require objective coverage, effective delta, non-goal cleanliness, docs readiness,
  and repo-gate evidence before advancing to handoff.
- Recorded XY-978 dogfood coverage: direct Program scheduling now quarantines legacy
  Decision Contract rows with removed flat issue summaries so fresh issue-batch
  Programs and status readback are not blocked by old model fields.
- Added `docs/spec/okf-knowledge-layer.md` to separate portable OKF engine behavior,
  LLM Wiki graph/retrieval behavior, repository-memory anchors, and the strict
  Decodex docs profile.
- Clarified that `decodex okf` is the cross-repository command surface while
  `decodex docs` is the local `docs/` alias, and that `docs okf` command nesting is
  not part of the user-facing model.
- Added `decodex okf check/find/graph/route` and `decodex docs check/find/graph/route`
  command surfaces; `decodex docs lint` remains a compatibility alias.
- Added `decodex okf init` as a turnkey scaffold for portable `core`, `wiki`, and
  `repo-memory` bundles in other repositories.
- Added portable `okf`, `okf-query`, and `okf-maintain` plugin skills and clarified
  that existing `docs-*` skills are Decodex profile wrappers.
- Adopted Docs-as-OKF as the Decodex repo-development documentation knowledge
  standard.
- Defined `docs/` as a Markdown-only OKF bundle with no non-Markdown documentation
  artifacts.
- Added `decodex docs lint` as the repository gate for OKF frontmatter, typed
  enum/date values, routing, local links, Markdown-only artifacts, required concept
  headings, and drift audit evidence anchors.
- Migrated all checked-in docs concepts to required OKF frontmatter.
- Retired checked-in JSON research event logs as an invalid docs shape.
- Split Decodex docs guidance into `docs-method`, `docs-okf`, `docs-wiki`, and
  `docs-drift` references so docs skills stay thin.
- Split Decodex research guidance into `research-lifecycle`, `research-evidence`,
  `research-contract`, and `research-promotion` references and removed the old
  monolithic `research-method` reference.
- Added `plugins/decodex/skills/docs/SKILL.md` as the agent-facing docs maintenance
  router and split detailed maintenance into `docs-okf`, `docs-wiki`, and
  `docs-drift`.
- Added docs impact classification to Decodex lane prompts and private runtime
  checkpoints: `none`, `update_required`, `research_required`, or `drift_required`.
- Tightened terminal finalization so every terminal path, including manual attention,
  requires the latest current-HEAD `issue_progress_checkpoint` to carry
  `docs_impact`.
- Added structured OKF frontmatter validation for `source_refs`, `code_refs`,
  `related`, `promotes_to`, `drift_watch`, and `tags`.
- Clarified that docs impact `research_required` routes into the Decodex `research*`
  skill family, and that `docs/research/` remains latent supporting evidence rather
  than a promotion target.
- Added `docs/evidence/index.md` for reusable public-safe proof concepts and durable
  semantic-drift audit evidence.
- Recorded plugin-eval evidence for the Decodex plugin, repo-memory writer, and new
  docs skill family.
- Clarified the Program Intake public/private boundary so generated Linear issue
  descriptions omit internal Program and node identifiers while SQLite/operator
  readback keeps private mappings.
- Replaced Decision Contract flat issue summaries with structured
  `execution_readiness.proposed_issues[]` as the only issue-shaping input.
- Standardized `OKF` as the all-caps prose form while preserving lowercase `okf` for
  filenames, paths, skill IDs, tags, and URLs.
