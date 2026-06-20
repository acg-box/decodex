---
type: Evidence
title: Decodex Plugin Eval
description: Records plugin-eval results for the Decodex, Knowledge, repo-work, and agent-method plugins after splitting knowledge and companion workflow surfaces out of Decodex core.
status: active
authority: evidence
owner: docs
tags: [plugin-eval, skills, repo-work, docs, research, okf, repo-memory, semantic-drift, debugging, challenge]
source_refs: []
code_refs: [plugins/decodex/.codex-plugin/plugin.json, plugins/decodex/references/routing.md, plugins/decodex/references/research-promotion.md, plugins/decodex/skills/decodex/SKILL.md, plugins/decodex/skills/decodex-ops/SKILL.md, plugins/decodex/skills/commit/SKILL.md, plugins/decodex/skills/land/SKILL.md, plugins/decodex/skills/planning/SKILL.md, plugins/decodex/skills/research/SKILL.md, plugins/decodex/skills/research-promote/SKILL.md, plugins/knowledge/.codex-plugin/plugin.json, plugins/knowledge/references/docs-drift.md, plugins/knowledge/references/docs-method.md, plugins/knowledge/references/docs-okf.md, plugins/knowledge/references/docs-wiki.md, plugins/knowledge/references/okf-layer.md, plugins/knowledge/skills/docs/SKILL.md, plugins/knowledge/skills/docs-drift/SKILL.md, plugins/knowledge/skills/okf/SKILL.md, plugins/knowledge/skills/repo-memory/SKILL.md, plugins/knowledge/scripts/semantic_drift_audit.py, plugins/repo-work/.codex-plugin/plugin.json, plugins/repo-work/references/repo-work.md, plugins/repo-work/references/dependency-policy.md, plugins/repo-work/skills/repo-work/SKILL.md, plugins/repo-work/skills/dependency-policy/SKILL.md, plugins/repo-work/skills/review-feedback/SKILL.md, plugins/repo-work/skills/verification/SKILL.md, plugins/repo-work/skills/debugging/SKILL.md, plugins/agent-method/.codex-plugin/plugin.json, plugins/agent-method/skills/challenge/SKILL.md]
related: [../policy.md, ./docs-self-iteration.md]
last_verified: 2026-06-21
---

# Decodex Plugin Eval

Purpose: Preserve public-safe evidence that Decodex core, Knowledge, repo-work, and
agent-method plugin surfaces passed local plugin evaluation after knowledge, repo-work,
and generic challenge moved out of Decodex core.

Read this when: You need proof that Decodex lifecycle skills, Knowledge docs/OKF and
repo-memory skills, repo-work contracts, generic challenge, and plugin invocation policy
were evaluated before landing.

Not this document: A runtime benchmark, coverage report, or replacement for
`plugin-eval` output.

Covers: Static plugin-eval commands, score results, and the invocation-policy decision.

## Commands

Current plugin gates:

```sh
node ~/.codex/plugins/cache/openai-curated/plugin-eval/202e9242/scripts/plugin-eval.js analyze plugins/decodex --format markdown
node ~/.codex/plugins/cache/openai-curated/plugin-eval/202e9242/scripts/plugin-eval.js analyze plugins/knowledge --format markdown
node ~/.codex/plugins/cache/openai-curated/plugin-eval/202e9242/scripts/plugin-eval.js analyze plugins/repo-work --format markdown
node ~/.codex/plugins/cache/openai-curated/plugin-eval/202e9242/scripts/plugin-eval.js analyze plugins/agent-method --format markdown
```

## Results

| Target | Score | Grade | Risk | Checks | Fix First |
| --- | ---: | --- | --- | --- | --- |
| `plugins/decodex` | 100 | A | low | 0 fail, 0 warn, 2 info | No urgent fixes. |
| `plugins/knowledge` | 100 | A | low | 0 fail, 0 warn, 2 info | No urgent fixes. |
| `plugins/repo-work` | 100 | A | low | 0 fail, 0 warn, 2 info | No urgent fixes. |
| `plugins/agent-method` | 100 | A | low | 0 fail, 0 warn, 2 info | No urgent fixes. |

Static budget snapshot:

- Decodex active invocation budget: 735 tokens.
- Decodex deferred skill budget: 3826 tokens.
- Decodex explicit-only invocation budget: 1974 tokens.
- Decodex plugin skill count: seven skills, with one implicit router and six
  explicit-only skills.
- Knowledge active invocation budget: 378 tokens; deferred budget: 4355 tokens;
  explicit-only invocation budget: 1293 tokens; four explicit-only skills.
- Repo-work active invocation budget: 407 tokens; deferred budget: 2596 tokens;
  explicit-only invocation budget: 2631 tokens; five explicit-only skills.
- Agent-method active invocation budget: 318 tokens; deferred budget: 11 tokens;
  explicit-only invocation budget: 501 tokens; one explicit-only skill.

## Invocation Policy

The plugin keeps the top-level router skills implicit and marks specialist skills as
explicit-only through local `agents/openai.yaml` files. This preserves direct
invocation while keeping plugin active-context cost bounded.

Implicit skills:

- `decodex`

Explicit-only skills:

Decodex lifecycle:

- `commit`
- `decodex-ops`
- `land`
- `planning`
- `research`
- `research-promote`

Knowledge:

- `$knowledge:docs`
- `$knowledge:docs-drift`
- `$knowledge:okf`
- `$knowledge:repo-memory`

Repo-work:

- `$repo-work:repo-work`
- `$repo-work:dependency-policy`
- `$repo-work:review-feedback`
- `$repo-work:verification`
- `$repo-work:debugging`

Agent-method:

- `$agent-method:challenge`

## Limits

The evaluation is static plugin analysis, not a measured real-usage benchmark. The
2026-06-21 rerun after adding dynamic fresh-context scout/skeptic support-agent
routing still reported all four local plugins at 100/100, grade A, low risk, with
zero failing checks and zero warnings.

Only the top-level `decodex` router remains implicit. Knowledge, repo-work, and
agent-method specialist skills are explicit-only and routed by host `AGENTS.md`.

Directly evaluating the installed cache path
`~/.codex/plugins/cache/hack-ink/decodex/0.2.0` reports an additional
`manifest-name-directory-mismatch` warning because the plugin manager stores the
plugin under a version directory. Use the source root `plugins/decodex` for the
canonical plugin-eval score.
