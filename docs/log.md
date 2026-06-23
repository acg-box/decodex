# Documentation Log

## 2026-06-23

- Added Phase 8 self-dogfood production-gate readback: autonomy lineage now projects
  public-safe PR handoff, validation, and post-land replay evidence from scoped
  private evidence pointers. PR evidence now also requires a matching retained review
  lifecycle row for the same generated issue, run, attempt, and PR ref, while
  `post_land` denotes lifecycle-tail proof after normal landing, install, restart, or
  plugin-sync authority without granting those actions.
- Added the Phase 7 autonomy MCP and external-agent surface to the autonomy spec,
  MCP capability-gateway decision, and Decodex router skill: observe resources expose
  objective, signal, proposal, and evidence summaries, while plan tools can draft
  objectives, submit signals, compile/challenge proposals, and request explicit
  proposal promotion without bypassing Decision Contract, Program Intake, review, or
  landing authority. The surface now fails closed on caller-supplied accepted-policy
  bodies and redacts local-private signal refs while preserving ref counts.
- Added the retained review-repair terminalization contract: after local validation
  passes, Decodex now owns pushing the repaired head to the retained PR branch,
  records typed push failures before marker refresh, and only refreshes retained
  review lineage after PR readback confirms the remote head matches local `HEAD`.
- Tightened the Codebase Codex lifecycle hook so Codex-owned commit/push attempts
  receive the `decodex/commit/1` JSON message contract, large implementation diffs
  trigger module-boundary challenge guidance before ready claims, public
  code/config/command/status/plugin surface changes stay coupled to docs or durable
  knowledge checks, non-trivial repo work starts from nearby source-backed docs,
  OKF/LLM Wiki, or repo-memory context when present, and future hook event logs no
  longer persist prompt samples.
- Finalized the four-plugin Codex workflow architecture: `decodex` owns Decodex
  lifecycle and runtime authority, `knowledge` owns docs/OKF/LLM Wiki/semantic
  drift/repo-memory/writeback, `codebase` owns repository engineering contracts and
  verification/debugging/dependency policy, and `deliberation` owns portable
  scout/grill/challenge fresh-context support-agent methods.
- Narrowed repo-gate tracked-rewrite handling so validation-satisfied phase goals can
  continue to a commit-capable handoff when canonicalize rewrites only pre-gate
  implementation paths, while unsafe or strict-boundary rewrites still preserve
  `repo_gate_tracked_rewrites_left`.
- Split accepted-review repair validation from ordinary handoff: after
  `repair_accepted_review_findings` passes repo-gate acceptance, Decodex now advances
  to `review_repair_evidence` so the retained PR repair head, PR readback,
  `issue_review_repair_complete`, and `review_repair` terminal finalize stay on the
  post-review lifecycle path.

## 2026-06-22

- Tightened operator review-control projection so ordinary running lanes no longer
  synthesize pending Decodex Review checkpoints, stale repair checkpoint rows cannot
  override newer clean checkpoint events, and clean review-repair terminal writeback
  gaps surface deterministic missing/stale lifecycle-marker reasons instead of
  reviving old repair control.
- Tightened operator status project metrics and protocol readback after live-history
  audit: ordinary fresh model/tool/repo-gate execution no longer inflates
  project-level waiting counts, continuation-pending attempts no longer inherit
  synthetic review prompts, and newer current-attempt marker events supersede stale
  durable maintenance events such as archive readback.
- Promoted the final autonomy architecture into authoritative docs: Decodex autonomy
  is now specified as an objective-driven project control plane with first-class
  Objective Contracts, typed signals, non-executable proposals, explicit
  Decision-Contract promotion, normal Program Intake execution, an MCP action matrix,
  memory-adapter limits, references-only project config, explicit draft/accepted
  Objective Contract lifecycle state, and a staged implementation roadmap.
- Added the workflow `context.read_first` dispatch preflight and agent-evidence
  live-wait projection boundary so stale workflow paths fail before lease/attempt
  ownership while normal running waits remain run capsules instead of blockers.
- Documented the ordinary dispatch guard for retained review handoff lanes:
  matching retained worktree plus review lifecycle record blocks normal, Program, and
  retry intake with `review_handoff_state_transition_pending` while post-review
  recovery, review repair, landing, and closeout remain the progress owners.

## 2026-06-21

- Required non-trivial scout/search and skeptic/challenge work to use fresh dynamic
  read-only support-agent context when tool support exists, while keeping static
  `scout.toml`/`skeptic.toml` role config out of the plugin contract.

## 2026-06-20

- Tightened Decodex Review orchestration so `issue_review_checkpoint` records only
  clean committed lane heads, persists an explicit registered-workflow review
  contract plus head-tree binding, and narrows retained repair review to verification
  of accepted findings plus contract regressions.
- Added codebase implementation-structure guardrails so substantial or generated
  implementation code must follow existing module ownership and cannot be claimed
  ready while unrelated responsibilities remain in one file.

## 2026-06-19

- Split the knowledge/docs surface into `plugins/knowledge` with four public skills:
  `docs`, `docs-drift`, `okf`, and `repo-memory`. Decodex now owns only lifecycle
  routing, research, planning, ops, commit, and land.
- Split generic agent-facing method and repository execution policy out of Decodex
  core: `plugins/deliberation` now owns challenge/skeptic review and
  `plugins/codebase` owns repo command, task-runner, review, verification, debugging,
  and the direct `dependency-policy` skill for dependency roll/style contracts.
- Consolidated Decodex research phase skills into the `research` compact loop and
  promoted the research-specific challenge entry into a generic `challenge` skeptic
  skill; scout-style evidence gathering remains dynamic read-only support-agent work
  rather than a configured static role.
- Documented idempotent `ghost_lane_cleanup` audit readback for missing-issue ghost
  lanes so stale PubFi fixture cleanup evidence no longer projects as current or
  retained attention when no retained work, live execution, or review lineage remains.
- Documented stale local issue-id isolation during retained worktree refresh so ghost
  rows cannot abort registered project status or dry-run candidate selection.
- Added the terminal app-server archive barrier contract: late non-terminal protocol
  events after `thread/archive` or `thread/archive/discarded` are recorded as
  discarded post-archive diagnostics in a non-conflicting runtime journal namespace,
  while operator status preserves failed child-run recovery context separately from
  parent journal or closeout handling.
- Documented Program retryable failed-start cleanup: clean no-diff failed starts now
  release stale active ownership and status names remaining clean active ownership as
  cleanup debt rather than retained partial progress.
- Refreshed Decodex plugin-eval evidence after the packaged plugin manifest prompt
  surface was consolidated back to the first-three prompt limit.

## 2026-06-18

- Documented the narrow `mcp_test_fixture_ghost_lane` recovery classification for the
  historical PubFi MCP fixture lane and preserved the fail-closed boundary for real
  private/control/protocol evidence.
- Added missing-issue ghost-lane recovery docs for `decodex recover ghost-lane`,
  including the live/cached status readback states `ghost_lane`,
  `runtime_recovery_required`, and `runtime_recovery_blocked`, plus the fail-closed
  cleanup boundary.
- Closed the MCP remote-control productization drift: Streamable HTTP now documents
  `--bearer-token-env` as the direct-listener boundary for non-loopback and elevated
  profiles, records the process-level HTTP smoke evidence, refreshes test-suite
  counts to 1282 runnable tests, and narrows remaining research to OAuth Protected
  Resource Metadata, operator-loop-hosted scan, and future protocol compatibility.
- Promoted MCP remote-control docs drift into the runtime spec, operator reference,
  remote MCP runbook, decision record, evidence index, and Decodex plugin routing;
  documented that `--allow-origin` is not authentication, loopback `observe` is the
  safe default, direct remote/elevated Streamable HTTP needs an operator auth
  boundary, and built-in MCP protected-resource auth plus process-level HTTP smoke
  were active research-backed gaps at that point.
- Added active research for MCP remote-control productization, covering remote
  access docs, authorization or relay boundaries, public-safe observation, process
  smoke coverage, high-risk control refusals, and protocol compatibility.
- Corrected MCP operator-control docs to describe checked-in research as Markdown
  research concepts, matching the current OKF research contract.
- Added the OKF research knowledge-lifecycle decision and updated research policy,
  reference, index, checker, and plugin guidance so promotion now routes rationale to
  decisions, truth to owner lanes, reusable proof to evidence, and superseded
  research out of active LLM Wiki routing.
- Removed the public OKF/LLM Wiki lexical route scorer from Decodex docs/OKF commands
  and clarified that OKF owns the Markdown/frontmatter contract while LLM Wiki owns
  agent navigation, owner concepts, indexes, links, and graph maintenance.
- Moved Decodex OKF/LLM Wiki context intake, `Context anchors`, docs completion gate,
  and late docs-skill recovery ownership into `plugins/decodex/references/routing.md`
  so host instructions can compose installed plugins without copying Decodex-specific
  procedures.
- Updated the MCP decision record, operator-control reference, resource-template
  readback, and Decodex plugin routing so complete remote MCP now points agents toward
  capability-profiled observe/plan/operate/admin resources, prompts, and tools.
- Corrected the MCP research resource alias to expose checked-in Markdown Research
  Contract concepts and removed the stale JSON docs resource contract.
- Replaced the research cleanup audit with `research-runtime-boundary.md`, a current
  Research Contract that states the Markdown docs, runtime Decision Contract, MCP
  resource, and future-research boundaries without carrying a cleanup-audit shape.
- Promoted MCP operate/admin docs from deferred stubs to the implemented
  inspect-first `decodex_lane_control` and future-dispatch-only
  `decodex_project_control` authority model.
- Refreshed the test-suite inventory to 1270 runnable `nextest` tests after adding
  MCP operate/admin coverage.
- Added schema-bound MCP planning tools for `research_compile`, `research_promote`,
  and `intake_goal`; dry-run paths stay read-only while apply/promote paths require
  explicit authority and return structured refusals when authority is missing.
- Added structured app-server schema drift checks for Decodex-owned
  `ClientRequest`, `ServerRequest`, `ClientNotification`, and `ServerNotification`
  method unions so protocol probes catch owned-method removal or params-schema drift.
- Synced Decodex's app-server dynamic tool contract to the Codex 0.141 preview
  schema by documenting tagged `type:function` and `type:namespace` declarations
  instead of the legacy flat `dynamicTools[].namespace` shape.
- Added remote-safe MCP observability projections for live status, activity tail, run
  events, protocol activity, child-agent activity, progress diagnostics, lane inspect,
  and PR/review state without exposing private evidence or raw steer text.
- Added Streamable HTTP to the Decodex MCP gateway: the transport now defaults to
  loopback binding and the `observe` profile, validates browser origins, issues MCP
  sessions, supports JSON and SSE responses, and keeps operate/admin calls behind
  profile-gated structured refusals.
- Added the stdio Decodex MCP primitive surface: initialize now advertises resources,
  resource templates, prompts, tools, logging, and progress compatibility; stdio smoke
  coverage now exercises resources/templates/list, prompts/list, prompts/get,
  tools/list, tools/call, active capability-profile filtering, and stdout cleanliness
  as the first MCP slice before the later Streamable HTTP and operate/admin
  promotions.
- Added `repo-memory-evaluator` so repo-memory OKF/LLM Wiki bundles have a
  plugin-eval-style quality workflow for static checks, graph health, owner coverage,
  source/code evidence, and before/after curation evidence.
- Added `repo-memory-curator` so existing OKF/LLM Wiki bundles have a dedicated
  growth and maintenance skill for weak owner concepts, metadata-only owner tuning,
  orphan triage, graph repair, and link tuning.
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
- Added the initial OKF/docs check, find, and graph command surfaces; `decodex docs
  lint` remains a compatibility alias.
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
- Clarified that docs impact `research_required` routes into the Decodex research
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
- Promoted review checkpoints into canonical evidence-keyed runtime artifacts so
  same-HEAD review evidence can be reused across attempts only when the review phase,
  `HEAD`, review level, and prompt-version key all match; completion and mutation-fence
  checks now read that keyed artifact instead of the run-local projection.
- Clarified that pre-existing, repo-wide, or global-baseline repo-gate failures are
  runtime-owned signals and must not be routed through agent-requested
  `manual_attention`.
