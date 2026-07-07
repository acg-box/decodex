---
type: Evidence
title: Decodex Plugin Eval
description: Records current plugin-eval results and open budget findings for the Decodex, Knowledge, Codebase, and Deliberation plugins.
status: active
authority: evidence
owner: docs
tags: [plugin-eval, skills, codebase, docs, research, okf, repo-memory, semantic-drift, debugging, scout, grill, skeptic]
source_refs: []
code_refs: [plugins/decodex/.codex-plugin/plugin.json, plugins/decodex/references/routing.md, plugins/decodex/references/research-lifecycle.md, plugins/decodex/references/research-contract.md, plugins/decodex/references/research-promotion.md, plugins/decodex/skills/decodex/SKILL.md, plugins/decodex/skills/decodex-ops/SKILL.md, plugins/decodex/skills/commit/SKILL.md, plugins/decodex/skills/land/SKILL.md, plugins/decodex/skills/planning/SKILL.md, plugins/decodex/skills/research/SKILL.md, plugins/decodex/skills/research-promote/SKILL.md, plugins/knowledge/.codex-plugin/plugin.json, plugins/knowledge/references/docs-drift.md, plugins/knowledge/references/docs-method.md, plugins/knowledge/references/docs-okf.md, plugins/knowledge/references/docs-wiki.md, plugins/knowledge/references/okf-layer.md, plugins/knowledge/skills/docs/SKILL.md, plugins/knowledge/skills/docs-drift/SKILL.md, plugins/knowledge/skills/okf/SKILL.md, plugins/knowledge/skills/repo-memory/SKILL.md, plugins/knowledge/skills/writeback/SKILL.md, scripts/semantic-drift/semantic_drift_audit.py, tests/plugins/knowledge/test_semantic_drift_audit.py, plugins/codebase/.codex-plugin/plugin.json, plugins/codebase/hooks/hooks.json, plugins/codebase/references/codebase.md, plugins/codebase/references/dependency-policy.md, plugins/codebase/scripts/codex_lifecycle_hook, tests/plugins/codebase/test_codex_lifecycle_hook.py, plugins/codebase/skills/work/SKILL.md, plugins/codebase/skills/dependency-policy/SKILL.md, plugins/codebase/skills/review-feedback/SKILL.md, plugins/codebase/skills/verification/SKILL.md, plugins/codebase/skills/debugging/SKILL.md, plugins/deliberation/.codex-plugin/plugin.json, plugins/deliberation/references/deliberation-gate.md, plugins/deliberation/skills/scout/SKILL.md, plugins/deliberation/skills/grill/SKILL.md, plugins/deliberation/skills/skeptic/SKILL.md]
related: [../policy.md, ./docs-self-iteration.md]
last_verified: 2026-06-29
---

# Decodex Plugin Eval

Purpose: Preserve public-safe evidence from local plugin evaluation and make open
budget findings visible instead of relying on stale score claims.

Read this when: You need proof that Decodex lifecycle skills, Knowledge docs/OKF,
repo-memory/writeback skills, Codebase contracts, Deliberation scout/grill/skeptic
skills, and plugin invocation policy were evaluated before landing.

Not this document: A runtime benchmark, coverage report, or replacement for
`plugin-eval` output.

Covers: Static plugin-eval commands, score results, token budgets, invocation policy,
and the auxiliary hook boundary.

## Commands

Current plugin gates:

```sh
node ~/.codex/plugins/cache/openai-curated-remote/plugin-eval/0.1.2/scripts/plugin-eval.js analyze plugins/decodex --format markdown
node ~/.codex/plugins/cache/openai-curated-remote/plugin-eval/0.1.2/scripts/plugin-eval.js analyze plugins/knowledge --format markdown
node ~/.codex/plugins/cache/openai-curated-remote/plugin-eval/0.1.2/scripts/plugin-eval.js analyze plugins/codebase --format markdown
node ~/.codex/plugins/cache/openai-curated-remote/plugin-eval/0.1.2/scripts/plugin-eval.js analyze plugins/deliberation --format markdown
```

## Results

| Target | Score | Grade | Risk | Checks | Fix First |
| --- | ---: | --- | --- | --- | --- |
| `plugins/decodex` | 95 | A | medium | 0 fail, 1 warn, 2 info | Deferred cost is heavy. |
| `plugins/knowledge` | 100 | A | low | 0 fail, 0 warn, 2 info | No urgent fixes. |
| `plugins/codebase` | 95 | A | medium | 0 fail, 1 warn, 2 info | Deferred cost is heavy. |
| `plugins/deliberation` | 95 | A | medium | 0 fail, 1 warn, 2 info | Invoke cost is heavy. |

Static budget snapshot from the 2026-06-29 source-root run:

| Target | Active | Trigger | Invoke | Deferred | Explicit-only | Total |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `plugins/decodex` | 901 | 50 | 851 | 2178 | 2983 | 3079 |
| `plugins/knowledge` | 761 | 82 | 679 | 891 | 1210 | 1652 |
| `plugins/codebase` | 1138 | 101 | 1037 | 2187 | 1899 | 3325 |
| `plugins/deliberation` | 1687 | 193 | 1494 | 475 | 0 | 2162 |

## Invocation Policy

Decodex lifecycle specialist skills, Knowledge narrow skills, and Codebase narrow
skills are explicit-only. `docs` and `work` remain the lightweight implicit routers,
while host `AGENTS.md` and hooks name the owner skills at task start, review, commit,
handoff, landing, docs impact, or ready-claim gates. Deliberation stays implicit so
grill/scout/skeptic can be used autonomously for design, research, critique, and
fresh-context subagent review.

Implicit skills:

- `decodex`
- `$knowledge:docs`
- `$codebase:work`
- `$deliberation:scout`
- `$deliberation:grill`
- `$deliberation:skeptic`

Explicit-only skills:

Decodex lifecycle:

- `commit`
- `decodex-ops`
- `land`
- `planning`
- `research`
- `research-promote`

Knowledge:

- `docs-drift`
- `okf`
- `repo-memory`
- `writeback`

Codebase:

- `debugging`
- `dependency-policy`
- `review-feedback`
- `verification`

## Hook Boundary

`plugins/codebase/hooks/hooks.json` adds auxiliary Codex lifecycle hints for
`UserPromptSubmit`, `PreToolUse`, `PostToolUse`, and `PreCompact`. It reminds Codex
about `decodex/commit/2` commit-message style before commit or push operations, warns
about large implementation diffs before ready/commit claims, and keeps development
coupled to source-backed docs, OKF/LLM Wiki, or durable knowledge before non-trivial
repo work and when public code/config/command/status/plugin surfaces change. The
`UserPromptSubmit` hook now emits short conditional reminders for English-only
durable artifacts and the Deliberation Gate without maintaining natural-language
trigger word lists; `PreToolUse` and `PostToolUse` keep structural defenses for
git commands, large diffs, and public workflow surfaces. The hook is not the source
of workflow authority: skill frontmatter and host `AGENTS.md` remain the primary
routing mechanism. A Codex hook trust prompt may still be required before the hook
executes in a live session.

## Limits

The evaluation is static plugin analysis, not a measured real-usage benchmark. The
2026-06-29 source-root run reports `plugins/knowledge` at 100/low-risk and
`plugins/decodex`, `plugins/codebase`, and `plugins/deliberation` at 95/medium-risk.
The remaining 95-point reports are budget tradeoffs, not structural failures; measure
real usage before deleting more workflow contract text for score-only gains.

Directly evaluating the installed cache path
`~/.codex/plugins/cache/hack-ink/decodex/0.2.0` reports an additional
`manifest-name-directory-mismatch` warning because the plugin manager stores the
plugin under a version directory. Use the source root `plugins/decodex` for the
canonical plugin-eval score.
