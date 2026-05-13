---
name: codex-code-analysis
description: Use when reading upstream OpenAI Codex PR, commit, file, or patch evidence to understand what changed, whether it is user-visible, and how it affects Decodex Radar, Control Plane, Publisher, or follow-up engineering.
---

# Decodex Codex Code Analysis

Use this skill after upstream triage chooses a candidate. Its job is to turn source
evidence into a defensible interpretation, not to rewrite release notes.

This is a Decodex repository-development instruction surface, not an installable
Decodex plugin skill.

## Read Before Analysis

- `docs/spec/github-change-bundle.md`
- `docs/spec/upstream-impact.md`
- `docs/spec/signal-entry.md`
- `dev/skills/README.md`

## Inputs

- A `github_change_bundle/v1` under `artifacts/github/bundles/`, or enough GitHub PR or
  commit evidence to request or create one
- Optional release or changelog context
- Optional existing Decodex signal, upstream-impact, or release-delta artifacts

This skill does not define a new checked-in artifact. Keep the result in-session
unless it is promoted into an existing `analysis_draft`, `upstream_impact/v1`, or
`social_post_draft/v1` contract.

## Analysis Loop

1. Identify the changed surface.
   - Public API/protocol/schema
   - CLI/TUI/app-server behavior
   - Config, permission, sandbox, auth, provider, hook, or plugin behavior
   - Docs, examples, tests, or internal-only refactor

2. Follow the runtime path.
   - Start from the PR title/body when `analysis_mode = "pr_first"`.
   - Use changed files and patch excerpts to locate the actual behavior boundary.
   - Use tests and docs as confirmation, not as the only proof.
   - Read enough surrounding code to know whether the change is shipped behavior,
     plumbing, guardrail, or cleanup.

3. Map implications.
   - User path: what a normal Codex user can observe or try.
   - Control Plane path: what Decodex runtime, app-server integration, plugin routing,
     tracker tooling, or automation policy may need to adopt or guard.
   - Publisher path: what can be explained publicly without overclaiming.

4. Assign confidence.
   - `confirmed`: source patch plus tests, docs, schema, CLI help, or public release
     evidence point to the same behavior.
   - `likely`: code strongly implies behavior, but no public docs or direct test covers
     the exact user path.
   - `weak`: evidence is names, commit titles, sparse release notes, or incomplete patch
     excerpts.

## Evidence Standards

Prefer concrete anchors:

- changed protocol/schema files
- config-schema or CLI flag changes
- tests that exercise visible behavior
- docs or examples that describe the behavior
- app-server, plugin, MCP, browser, sandbox, hook, auth, provider, or tool-handler code

Do not treat these as enough by themselves:

- internal file names
- generic commit titles
- social engagement
- release bodies that only repeat the version number
- TODOs or comments without behavior

## Output

Return an analysis note that can feed `github-signal`, `codex-release-analysis`, or
`upstream_impact/v1`:

- one-sentence observed change
- changed surface classification
- evidence anchors
- user-visible path, if any
- Control Plane impact, if any
- Publisher angle, if any
- confidence and caveats
- recommended next artifact: `none`, `analysis_draft` through `github-signal`,
  `upstream_impact/v1`, or `social_post_draft/v1`

Keep the note shorter than the source patch. Explain the behavior path, not every
changed file.
