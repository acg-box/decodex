---
type: Evidence
title: Decodex Plugin Eval
description: Records plugin-eval results for the Decodex plugin, routing reference, repo-work, portable OKF, repo-memory, docs drift, debugging, and research skills.
status: active
authority: evidence
owner: docs
tags: [plugin-eval, skills, repo-work, docs, research, okf, repo-memory, semantic-drift, debugging]
source_refs: []
code_refs: [plugins/decodex/.codex-plugin/plugin.json, plugins/decodex/references/routing.md, plugins/decodex/references/repo-workflow.md, plugins/decodex/references/dep-roll-policy.md, plugins/decodex/references/okf-layer.md, plugins/decodex/references/docs-drift.md, plugins/decodex/skills/decodex/SKILL.md, plugins/decodex/skills/repo-work/SKILL.md, plugins/decodex/skills/dep-roll/SKILL.md, plugins/decodex/skills/dep-style/SKILL.md, plugins/decodex/skills/python/SKILL.md, plugins/decodex/skills/review-feedback/SKILL.md, plugins/decodex/skills/rust/SKILL.md, plugins/decodex/skills/verification/SKILL.md, plugins/decodex/skills/okf/SKILL.md, plugins/decodex/skills/okf-query/SKILL.md, plugins/decodex/skills/okf-maintain/SKILL.md, plugins/decodex/skills/repo-memory-writer/SKILL.md, plugins/decodex/skills/repo-memory-evaluator/SKILL.md, plugins/decodex/skills/repo-memory-curator/SKILL.md, plugins/decodex/skills/docs/SKILL.md, plugins/decodex/skills/docs-okf/SKILL.md, plugins/decodex/skills/docs-wiki/SKILL.md, plugins/decodex/skills/docs-drift/SKILL.md, plugins/decodex/skills/debugging/SKILL.md, plugins/decodex/skills/research/SKILL.md, plugins/decodex/skills/research-challenge/SKILL.md, plugins/decodex/scripts/semantic_drift_audit.py]
related: [../policy.md, ./docs-self-iteration.md]
last_verified: 2026-06-19
---

# Decodex Plugin Eval

Purpose: Preserve public-safe evidence that the Decodex plugin, routing reference,
portable OKF init and skill family, repo-work skill family, repo-memory skill family,
docs drift skill, debugging skill, and research skill family passed local plugin
evaluation without failures.

Read this when: You need proof that the OKF init/split, repo-memory skills, docs
drift, debugging, repo-work migration, research skill split, and plugin invocation
policy were evaluated before landing.

Not this document: A runtime benchmark, coverage report, or replacement for
`plugin-eval` output.

Covers: Static plugin-eval commands, score results, and the invocation-policy decision.

## Commands

Current full-plugin gate:

```sh
node ~/.codex/plugins/cache/openai-curated-remote/plugin-eval/0.1.2/scripts/plugin-eval.js analyze plugins/decodex --format markdown
```

## Results

| Target | Score | Grade | Risk | Checks | Fix First |
| --- | ---: | --- | --- | --- | --- |
| `plugins/decodex` | 91 | B | medium | 0 fail, 2 warn, 2 info | Reduce invoke and deferred token budget in a future slimming pass. |

## Invocation Policy

The plugin keeps the top-level router skills implicit and marks specialist skills as
explicit-only through local `agents/openai.yaml` files. This preserves direct
invocation while keeping plugin active-context cost bounded.

Implicit skills:

- `decodex`
- `docs`
- `planning`
- `repo-memory-curator`
- `research`

Explicit-only skills:

- `automation`
- `commit`
- `debugging`
- `dep-roll`
- `dep-style`
- `docs-drift`
- `docs-okf`
- `docs-wiki`
- `labels`
- `land`
- `manual-cli`
- `okf`
- `okf-maintain`
- `okf-query`
- `python`
- `repo-memory-evaluator`
- `repo-memory-writer`
- `repo-work`
- `review-feedback`
- `rust`
- `research-challenge`
- `research-decision`
- `research-evidence`
- `research-judgment`
- `research-options`
- `research-probe`
- `research-promote`
- `verification`

## Limits

The evaluation is static plugin analysis, not a measured real-usage benchmark. The
2026-06-19 full-plugin rerun reported score 91/100, grade B, medium risk, zero
failing checks, two warnings, and two informational notes. The warnings are
`invoke_cost_tokens-budget-high` and `deferred_cost_tokens-budget-high`, which are
known static token-budget cleanup items for a future slimming pass after the repo-work
migration, not routing, safety, or progressive-disclosure failures. The manifest
default prompt count remains within the three-prompt Codex limit. `repo-work` is
explicit-only because host `AGENTS.md` names it directly instead of relying on passive
implicit triggering.
