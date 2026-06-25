---
type: Evidence
title: Decodex Plugin Eval
description: Records current plugin-eval results and open budget findings for the Decodex, Knowledge, Codebase, and Deliberation plugins.
status: active
authority: evidence
owner: docs
tags: [plugin-eval, skills, codebase, docs, research, okf, repo-memory, semantic-drift, debugging, scout, grill, challenge]
source_refs: []
code_refs: [plugins/decodex/.codex-plugin/plugin.json, plugins/decodex/references/routing.md, plugins/decodex/references/research-lifecycle.md, plugins/decodex/references/research-contract.md, plugins/decodex/references/research-promotion.md, plugins/decodex/skills/decodex/SKILL.md, plugins/decodex/skills/decodex-ops/SKILL.md, plugins/decodex/skills/commit/SKILL.md, plugins/decodex/skills/land/SKILL.md, plugins/decodex/skills/planning/SKILL.md, plugins/decodex/skills/research/SKILL.md, plugins/decodex/skills/research-promote/SKILL.md, plugins/knowledge/.codex-plugin/plugin.json, plugins/knowledge/references/docs-drift.md, plugins/knowledge/references/docs-method.md, plugins/knowledge/references/docs-okf.md, plugins/knowledge/references/docs-wiki.md, plugins/knowledge/references/okf-layer.md, plugins/knowledge/skills/docs/SKILL.md, plugins/knowledge/skills/docs-drift/SKILL.md, plugins/knowledge/skills/okf/SKILL.md, plugins/knowledge/skills/repo-memory/SKILL.md, plugins/knowledge/skills/writeback/SKILL.md, plugins/knowledge/scripts/semantic_drift_audit.py, plugins/codebase/.codex-plugin/plugin.json, plugins/codebase/hooks/hooks.json, plugins/codebase/references/codebase.md, plugins/codebase/references/dependency-policy.md, plugins/codebase/scripts/codex_lifecycle_hook, plugins/codebase/scripts/test_codex_lifecycle_hook.py, plugins/codebase/skills/work/SKILL.md, plugins/codebase/skills/dependency-policy/SKILL.md, plugins/codebase/skills/review-feedback/SKILL.md, plugins/codebase/skills/verification/SKILL.md, plugins/codebase/skills/debugging/SKILL.md, plugins/deliberation/.codex-plugin/plugin.json, plugins/deliberation/references/deliberation-gate.md, plugins/deliberation/skills/scout/SKILL.md, plugins/deliberation/skills/grill/SKILL.md, plugins/deliberation/skills/challenge/SKILL.md]
related: [../policy.md, ./docs-self-iteration.md]
last_verified: 2026-06-25
---

# Decodex Plugin Eval

Purpose: Preserve public-safe evidence from local plugin evaluation and make open
budget findings visible instead of relying on stale score claims.

Read this when: You need proof that Decodex lifecycle skills, Knowledge docs/OKF,
repo-memory/writeback skills, Codebase contracts, Deliberation scout/grill/challenge
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
| `plugins/knowledge` | 81 | C | high | 1 fail, 1 warn, 2 info | Deferred cost is excessive; invoke cost is heavy. |
| `plugins/codebase` | 72 | C | high | 2 fail, 0 warn, 2 info | Invoke and deferred costs are excessive. |
| `plugins/deliberation` | 95 | A | medium | 0 fail, 1 warn, 2 info | Invoke cost is heavy. |

Static budget snapshot from the 2026-06-25 source-root run:

| Target | Active | Trigger | Invoke | Deferred | Explicit-only | Total |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `plugins/decodex` | 889 | 50 | 839 | 2045 | 2836 | 2934 |
| `plugins/knowledge` | 2108 | 227 | 1881 | 4340 | 0 | 6448 |
| `plugins/codebase` | 3415 | 234 | 3181 | 4714 | 0 | 8129 |
| `plugins/deliberation` | 2031 | 195 | 1836 | 337 | 0 | 2368 |

## Invocation Policy

Decodex keeps lifecycle specialist skills explicit-only. Knowledge, Codebase, and
Deliberation skills are intentionally available for implicit routing through concise
frontmatter descriptions, while host `AGENTS.md` still names the owner skills at
task start, review, commit, handoff, landing, or ready-claim gates.

Implicit skills:

- `decodex`
- `$knowledge:docs`
- `$knowledge:docs-drift`
- `$knowledge:okf`
- `$knowledge:repo-memory`
- `$knowledge:writeback`
- `$codebase:work`
- `$codebase:dependency-policy`
- `$codebase:review-feedback`
- `$codebase:verification`
- `$codebase:debugging`
- `$deliberation:scout`
- `$deliberation:grill`
- `$deliberation:challenge`

Explicit-only skills:

Decodex lifecycle:

- `commit`
- `decodex-ops`
- `land`
- `planning`
- `research`
- `research-promote`

## Hook Boundary

`plugins/codebase/hooks/hooks.json` adds auxiliary Codex lifecycle hints for
`UserPromptSubmit`, `PreToolUse`, `PostToolUse`, and `PreCompact`. It reminds Codex
about `decodex/commit/1` commit-message style before commit or push operations, warns
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
2026-06-25 rerun after compacting Decodex research references improved
`plugins/decodex` from a failing 86/high-risk report to 95/medium-risk, but did not
prove an all-plugin 100/100 state. `plugins/knowledge` and `plugins/codebase` still
need budget-focused slimming or measured-usage evidence before any ready claim about
plugin-eval quality.

Directly evaluating the installed cache path
`~/.codex/plugins/cache/hack-ink/decodex/0.2.0` reports an additional
`manifest-name-directory-mismatch` warning because the plugin manager stores the
plugin under a version directory. Use the source root `plugins/decodex` for the
canonical plugin-eval score.
