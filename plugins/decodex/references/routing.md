# Decodex Routing Reference

Use this when a Decodex task crosses research, promotion, planning, runtime ops,
commit, or landing boundaries. Repository execution policy, knowledge/docs, and
generic challenge live in companion plugins.

## Mode Map

- Context intake: for non-trivial repo work, use `$knowledge:docs` to read the
  smallest owner from `docs/index.md`, `docs/policy.md`, lane indexes, or explicit
  concept links when docs/knowledge shape the plan.
- Repo work: use `$codebase:work` for checked-in command authority, task-runner
  structure, configuration contracts, dependency policy, review repair, validation
  evidence, completion claims, debugging, and dynamic support-agent boundaries.
- Docs and OKF: use `$knowledge:docs` for checked-in docs workflows,
  `$knowledge:okf` for portable OKF/LLM Wiki bundles, and
  `$knowledge:repo-memory` for source-backed repository memory.
- Semantic drift: use `$knowledge:docs-drift` for docs/code/help/status/config/
  runtime claim alignment. It owns drift verdicts even when the trigger is a command,
  config key, status phrase, generated artifact, or runtime readback rather than a
  prose doc.
- Debugging: use `$codebase:debugging` for repository bugs, original-symptom checks,
  and repeated failed fixes. Use `decodex-ops` for Decodex runtime readback,
  retained-lane control, or operator diagnostics. Debugging may feed research only
  when the result becomes a decision-ready comparison.
- Research/design: use `research`. The compact loop is probe, evidence, options,
  judgment, challenge, decision. Results are latent Decision Contract candidates only.
  This research compact loop is separate from runtime `compact_current_head_review`;
  read runtime compact review quality from `issue_review_checkpoint.review_cost_control`
  and `decodex evidence`, not from research output.
- Challenge: use `$deliberation:challenge` for generic skeptic review of plans,
  claims, evidence sufficiency, option framing, and ready/decision-ready assertions.
  Challenge does not create execution authority.
- Promotion: use `research-promote` only after explicit acceptance such as "arrange
  this", "push this forward", "proceed with this", or "implement this".
- Planning: use `planning` after promotion or another explicit execution instruction.
  Planning owns issue briefing and Program readiness.
- Decodex ops: use `decodex-ops` for retained automation, human-driven CLI commands,
  ordinary non-Program tracker intake, service labels, lane control, recovery
  inspection, operator readback, and `missing_review_handoff_record` diagnosis before
  any dry-run rebind or adopt.
- Commit and land: use `commit` or `land` for human-driven Git history creation or PR
  landing; keep these high-risk authority surfaces narrower than general ops.
- MCP gateway: use stdio locally and Streamable HTTP only behind the operator's chosen
  listener with `--bearer-token-env`, tunnel, relay, network ACL, reverse proxy, or
  protected-resource auth boundary. Treat `--allow-origin` as CORS trust, not
  authentication; direct non-loopback listeners require both `--allow-origin` and
  `--bearer-token-env`, and Streamable HTTP profiles above `observe` require
  `--bearer-token-env`. The built-in bearer guard is not OAuth Protected Resource
  Metadata. MCP is a typed facade, not a bypass around Decision Contract,
  lane-control, tracker, review, landing, or closeout gates. Plan tools are
  `research_compile`, `research_promote`, and
  `intake_goal`; operate/admin remains inspect-first with current run/turn
  preconditions.

## First Reads

- In this repo, read `README.md`, `docs/index.md`, `docs/policy.md`, and
  `Makefile.toml` before docs or validation work.
- For registered projects, read the project `project.toml` and `WORKFLOW.md`.
- For runtime semantics, prefer `docs/spec/` and `docs/runbook/` over host policy.

## Natural-Language Research Routing

Keep Decodex natural-language-first. Requests such as `research X` route through the
`research` compact loop, including an `$deliberation:challenge` pass before terminal
decision, before promotion.

Research never queues work, mutates Linear, starts implementation, creates Codex
goals, or dispatches Program nodes. Promotion preserves accepted objectives,
constraints, validation expectations, proposed issues, non-goals, and stop
conditions. A result is a latent Decision Contract candidate only.

## Program Versus Label Intake

- Program Intake dispatches ready mapped nodes directly from the persisted Execution
  Program; queue labels are not the Program scheduler.
- Ordinary issue intake starts from `decodex:queued:<service-id>` and must still pass
  `WORKFLOW.md` eligibility, terminal-state, dependency, opt-out, and active-lease
  checks.
- `decodex:active:<service-id>` is runtime ownership, not "please start work".
- `decodex:manual-only` opts out of automation.
- `decodex:needs-attention` is a human-required stop.

## Commit And Land

For human-driven commits, inspect the diff, stage intended files, run touched-surface
validation, then use `decodex commit`, adding `--manual-authority` for deliberate
non-issue work.

For human-driven PR landing, confirm PR/base/head/mergeability/checks, then use
`decodex land`; add `--manual-authority --pr <URL>` for non-issue work. If
issue-authority landing lacks retained handoff state, run
`decodex recover review-handoff diagnose <ISSUE> --json` first. Use dry-run `rebind`
only for a Decodex-owned retained lane whose PR, retained worktree, branch, and head
lineage match. Use dry-run `adopt` only for a human-owned PR takeover from the current
managed worktree. Neither recovery command lands the PR.
Non-issue `--manual-authority --pr` landing is not project-registry authority: without
`--config`, it may use the current Git checkout plus `GH_TOKEN`, `GITHUB_TOKEN`, or
`gh auth token` and must skip runtime/Linear closeout. Use `--config` when the operator
wants configured GitHub credentials or workspace hooks.

## Hard Boundaries

- Do not use global `AGENTS.md` as Decodex runtime, tracker, identity, landing,
  closeout, or cleanup authority.
- Do not route Decodex issue briefing through an external delivery workflow; planning
  owns Decodex issue shaping after execution authority exists.
- Do not hand-edit runtime DB rows, kill hidden `_attempt` children, or mutate Linear
  state to simulate lane controls.
- Do not use MCP tools to bypass Decision Contract promotion, lane-control
  preconditions, tracker tools, review handoff, landing, closeout, or runtime
  authority.
- Do not substitute raw GitHub merge, merge queue, `gh pr merge`, direct API mutation,
  or hand-assembled merge commits for `decodex land` when Decodex owns landing.
- Do not expose graph ids, DAG edge editing, hidden goal ids, or Program dispatch
  mechanics as the ordinary user workflow.
