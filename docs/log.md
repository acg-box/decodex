# Documentation Log

## 2026-06-17

- Added `docs/spec/okf-knowledge-layer.md` to separate portable OKF engine behavior,
  LLM Wiki graph/retrieval behavior, repository-memory anchors, and the strict
  Decodex docs profile.
- Clarified that `decodex okf` is the cross-repository command surface while
  `decodex docs` is the local `docs/` alias, and that `docs okf` command nesting is
  not part of the user-facing model.
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
- Recorded plugin-eval evidence for the Decodex plugin and new docs skill family.
- Clarified the Program Intake public/private boundary so generated Linear issue
  descriptions omit internal Program and node identifiers while SQLite/operator
  readback keeps private mappings.
- Standardized `OKF` as the all-caps prose form while preserving lowercase `okf` for
  filenames, paths, skill IDs, tags, and URLs.
