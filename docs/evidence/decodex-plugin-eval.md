---
type: Evidence
title: Decodex Plugin Eval
description: Records plugin-eval results for the Decodex, repo-work, and agent-method plugins after splitting companion workflow surfaces out of Decodex core.
status: active
authority: evidence
owner: docs
tags: [plugin-eval, skills, repo-work, docs, research, okf, repo-memory, semantic-drift, debugging, challenge]
source_refs: []
code_refs: [plugins/decodex/.codex-plugin/plugin.json, plugins/decodex/references/routing.md, plugins/decodex/references/okf-layer.md, plugins/decodex/references/docs-drift.md, plugins/decodex/skills/decodex/SKILL.md, plugins/decodex/skills/decodex-ops/SKILL.md, plugins/decodex/skills/okf/SKILL.md, plugins/decodex/skills/okf-query/SKILL.md, plugins/decodex/skills/okf-maintain/SKILL.md, plugins/decodex/skills/repo-memory-writer/SKILL.md, plugins/decodex/skills/repo-memory-evaluator/SKILL.md, plugins/decodex/skills/repo-memory-curator/SKILL.md, plugins/decodex/skills/docs/SKILL.md, plugins/decodex/skills/docs-okf/SKILL.md, plugins/decodex/skills/docs-wiki/SKILL.md, plugins/decodex/skills/docs-drift/SKILL.md, plugins/decodex/skills/research/SKILL.md, plugins/repo-work/.codex-plugin/plugin.json, plugins/repo-work/references/repo-work.md, plugins/repo-work/references/dependency-policy.md, plugins/repo-work/skills/repo-work/SKILL.md, plugins/repo-work/skills/review-feedback/SKILL.md, plugins/repo-work/skills/verification/SKILL.md, plugins/repo-work/skills/debugging/SKILL.md, plugins/agent-method/.codex-plugin/plugin.json, plugins/agent-method/skills/challenge/SKILL.md, plugins/decodex/scripts/semantic_drift_audit.py]
related: [../policy.md, ./docs-self-iteration.md]
last_verified: 2026-06-19
---

# Decodex Plugin Eval

Purpose: Preserve public-safe evidence that Decodex core, repo-work, and agent-method
plugin surfaces passed local plugin evaluation after repo-work and generic challenge
moved out of Decodex core.

Read this when: You need proof that Decodex lifecycle skills, temporary docs/OKF and
repo-memory skills, repo-work contracts, generic challenge, and plugin invocation
policy were evaluated before landing.

Not this document: A runtime benchmark, coverage report, or replacement for
`plugin-eval` output.

Covers: Static plugin-eval commands, score results, and the invocation-policy decision.

## Commands

Current plugin gates:

```sh
node ~/.codex/plugins/cache/openai-curated-remote/plugin-eval/0.1.2/scripts/plugin-eval.js analyze plugins/decodex --format markdown
node ~/.codex/plugins/cache/openai-curated-remote/plugin-eval/0.1.2/scripts/plugin-eval.js analyze plugins/repo-work --format markdown
node ~/.codex/plugins/cache/openai-curated-remote/plugin-eval/0.1.2/scripts/plugin-eval.js analyze plugins/agent-method --format markdown
```

## Results

| Target | Score | Grade | Risk | Checks | Fix First |
| --- | ---: | --- | --- | --- | --- |
| `plugins/decodex` | 95 | A | medium | 0 fail, 1 warn, 2 info | Deferred token budget remains heavy while docs/OKF and repo-memory stay here. |
| `plugins/repo-work` | 100 | A | low | 0 fail, 0 warn, 2 info | No urgent fixes. |
| `plugins/agent-method` | 100 | A | low | 0 fail, 0 warn, 2 info | No urgent fixes. |

Static budget snapshot:

- Decodex active invocation budget: 865 tokens.
- Decodex deferred skill budget: 8433 tokens.
- Decodex explicit-only invocation budget: 6165 tokens.
- Decodex plugin skill count: 17 skills, with one implicit router and 16
  explicit-only skills.
- Repo-work active invocation budget: 392 tokens; deferred budget: 2471 tokens; four
  explicit-only skills.
- Agent-method active invocation budget: 318 tokens; deferred budget: 11 tokens; one
  explicit-only skill.

## Invocation Policy

The plugin keeps the top-level router skills implicit and marks specialist skills as
explicit-only through local `agents/openai.yaml` files. This preserves direct
invocation while keeping plugin active-context cost bounded.

Implicit skills:

- `decodex`

Explicit-only skills:

- `commit`
- `decodex-ops`
- `docs`
- `docs-drift`
- `docs-okf`
- `docs-wiki`
- `land`
- `okf`
- `okf-maintain`
- `okf-query`
- `planning`
- `repo-memory-curator`
- `repo-memory-evaluator`
- `repo-memory-writer`
- `research`
- `research-promote`

Companion plugin skills:

- `$repo-work:repo-work`
- `$repo-work:review-feedback`
- `$repo-work:verification`
- `$repo-work:debugging`
- `$agent-method:challenge`

## Limits

The evaluation is static plugin analysis, not a measured real-usage benchmark. The
2026-06-19 rerun after splitting repo-work and agent-method out of Decodex reported
Decodex at 95/100, grade A, medium risk, zero failing checks, one warning, and two
informational notes. Active invocation cost dropped to 865 tokens; deferred skill cost
dropped to 8433 tokens; explicit-only invocation cost dropped to 6165 tokens. The
remaining Decodex warning is `deferred_cost_tokens-budget-high`, expected while
docs/OKF and repo-memory remain temporarily inside Decodex pending CLI migration.
Repo-work and agent-method both evaluated at 100/100, grade A, low risk.

Only the top-level `decodex` router remains implicit. Repo-work and agent-method
specialist skills are explicit-only and routed by host `AGENTS.md`.

Directly evaluating the installed cache path
`~/.codex/plugins/cache/hack-ink/decodex/0.2.0` reports an additional
`manifest-name-directory-mismatch` warning because the plugin manager stores the
plugin under a version directory. Use the source root `plugins/decodex` for the
canonical plugin-eval score.
