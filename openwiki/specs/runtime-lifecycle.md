# Runtime Lifecycle

This page covers Decodex runtime authority, lane lifecycle, review lifecycle, app-server protocol, tracker tools, agent evidence, loop runtime, and autonomy boundaries.

## Authority model

Decodex coordinates coding-agent work through explicit project configs, workflow policy, local runtime state, Linear issue mirrors, and GitHub PRs. Source code, tests, project `WORKFLOW.md`, runtime SQLite records, and accepted Decision Contracts are authority. Linear comments and GitHub PR metadata are collaboration projections unless the runtime explicitly records them as lifecycle evidence.

Important rule: do not reconstruct lifecycle truth from branch names, PR titles, Linear comments, or current `HEAD` alone. Recovery paths must use explicit evidence and persist a reviewed projection before normal dispatch resumes.

## Runtime and lane concepts

A lane is one retained unit of issue-scoped work. Its durable shape combines:

- a Linear issue identity
- a branch/worktree mapping
- a local lease
- one or more run attempts
- protocol summaries and private execution events
- review lifecycle records after PR handoff

The scheduler decides eligibility from queue labels, retry budget, retained post-review work, Program Intake nodes, and runtime recovery state. It must not run the same issue through multiple local active owners.

## App-server protocol

Decodex starts Codex through `codex app-server --listen stdio://` for lane execution. The generated app-server schema and protocol tests are the source for request/notification shapes. Required capabilities include initialize, thread start/resume, turn start, dynamic tool calls, command exec health checks, thread archive, and phase-goal methods.

Phase goals are required for retained lane execution. If a Codex app-server build lacks the required protocol, Decodex should fail with a compatibility diagnostic rather than silently falling back to ordinary continuation. `decodex probe stdio://` should report `PROBE_OK` for a compatible runtime.

## Lane control

Lane control is inspect-first. Steer and interrupt operations require current run identity and, for steer, the expected active turn id. Soft interrupt uses the app-server lane-control protocol when available. Forced interrupt may hard-kill only after the soft path is unavailable, rejected, or timed out under the runtime rules.

MCP lane control exposes the same model as a typed facade. It does not bypass run/turn preconditions, capability profile, project enablement, or issue ownership.

## Tracker tools and public ledger

The tracker bridge gives the child agent narrow issue-scoped tools: transition, comment, label add, progress checkpoint, review checkpoint, review handoff, review repair complete, closeout complete, and terminal finalize.

The bridge owns the public Linear execution ledger format. Public comments are structured projections and must not include credentials, auth material, local database paths, raw protocol payloads, hidden reasoning, private evidence bodies, or account identifiers. Private checkpoint details belong in runtime SQLite.

Terminal completion requires a valid terminal signal. Decodex should fail closed rather than infer completion from ordinary prose.

## Agent evidence

Agent evidence under `~/.codex/decodex/agent-evidence` supports diagnosis and handoff. Evidence can include handoff indexes, blocker snapshots, run capsules, protocol summaries, and private execution readback. It is not a public tracker record and should be summarized before any public projection.

Use `decodex diagnose` and `decodex evidence` for readback. Treat missing, stale, or mismatched run evidence as a recovery input, not as permission to guess lifecycle state.

## Review lifecycle

After PR handoff, lifecycle authority is represented by normalized review lifecycle records and append-only lifecycle events in the local runtime store. The pure lifecycle kernel classifies facts into handoff, repair, ready-to-land, closeout, or manual-attention outcomes; adapters perform GitHub/Linear/local side effects.

Review rounds must read the current head, not memory of an older branch state. Each round needs both an implementation pass and an adversarial pass. Outcomes are limited to clean, findings, needs architecture review, or blocked. Repeated new findings after multiple rounds should escalate instead of producing patch churn.

Landing requires a non-draft PR, clear review blockers, acceptable merge state, configured required statuses when present, and Decodex-owned `decodex land` closeout. Raw `gh pr merge` is not the Decodex-owned landing path.

## Loop runtime and Program Intake

The loop runtime is above individual issue lanes. Accepted Decision Contracts can be materialized through Program Intake into private Execution Programs and public issue briefs. Program dispatch is direct from runtime state, not queue-label polling.

Decision Contracts distinguish draft latent proposals, accepted promoted authority, human-decision blockers, and rejected superseded records. Only accepted authority can feed executable Program Intake. Public issue briefs may summarize objectives, dependencies, validation, risks, and acceptance criteria; they must not leak internal graph ids, proposal ids, private evidence refs, or runtime row details.

## Autonomy control plane

Autonomy objectives, signals, and proposals are local planning surfaces. Signals are evidence, proposals are not executable by themselves, and promotion requires accepted authority. Capability-profiled MCP tools can draft objectives, submit signals, compile proposals, challenge proposals, and request promotion, but they do not replace human/project-policy acceptance.

Autonomy must stay bounded to explicit allowed surfaces and signal kinds. It must not create hidden self-modifying authority, bypass review/landing gates, or mutate other projects without accepted project authority.

## Workflow policy

Project `WORKFLOW.md` owns tracker state names, labels, read-first paths, canonicalization commands, validation commands, and worktree hooks. Runtime code owns lease, run, retry, recovery, lifecycle, and closeout behavior. When workflow syntax changes, update parser tests and the operator documentation for command consequences.
