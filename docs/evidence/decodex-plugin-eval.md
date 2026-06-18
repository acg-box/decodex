---
type: Evidence
title: Decodex Plugin Eval
description: Records plugin-eval results for the Decodex plugin, routing skill, portable OKF, repo-memory, docs, and research skills.
status: active
authority: evidence
owner: docs
tags: [plugin-eval, skills, docs, research, okf, repo-memory]
source_refs: []
code_refs: [plugins/decodex/.codex-plugin/plugin.json, plugins/decodex/references/context-gates.md, plugins/decodex/references/routing.md, plugins/decodex/references/okf-layer.md, plugins/decodex/skills/decodex/SKILL.md, plugins/decodex/skills/okf/SKILL.md, plugins/decodex/skills/okf-query/SKILL.md, plugins/decodex/skills/okf-maintain/SKILL.md, plugins/decodex/skills/repo-memory-writer/SKILL.md, plugins/decodex/skills/docs/SKILL.md, plugins/decodex/skills/docs-okf/SKILL.md, plugins/decodex/skills/docs-wiki/SKILL.md, plugins/decodex/skills/docs-drift/SKILL.md, plugins/decodex/skills/research/SKILL.md]
related: [../policy.md, ./docs-self-iteration.md]
last_verified: 2026-06-18
---

# Decodex Plugin Eval

Purpose: Preserve public-safe evidence that the Decodex plugin, routing skill,
portable OKF init and skill family, repo-memory writer skill, and docs skill family
passed local plugin evaluation.

Read this when: You need proof that the OKF init/split, repo-memory writer, docs skill
split, research skill split, and plugin invocation policy were evaluated before
landing.

Not this document: A runtime benchmark, coverage report, or replacement for
`plugin-eval` output.

Covers: Static plugin-eval commands, score results, and the invocation-policy decision.

## Commands

Current full-plugin gate:

```sh
node ~/.codex/plugins/cache/openai-curated/plugin-eval/015c0dff/scripts/plugin-eval.js analyze plugins/decodex --format markdown
```

## Results

| Target | Score | Grade | Risk | Fix First |
| --- | --- | --- | --- | --- |
| `plugins/decodex` | 95 | A | medium | deferred reference budget warning |
| `plugins/decodex/skills/okf` | 100 | A | low | none |
| `plugins/decodex/skills/okf-query` | 100 | A | low | none |
| `plugins/decodex/skills/okf-maintain` | 100 | A | low | none |
| `plugins/decodex/skills/repo-memory-writer` | 100 | A | low | none |
| `plugins/decodex/skills/docs` | 100 | A | low | none |
| `plugins/decodex/skills/docs-okf` | 100 | A | low | none |
| `plugins/decodex/skills/docs-wiki` | 100 | A | low | none |
| `plugins/decodex/skills/docs-drift` | 100 | A | low | none |

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
- `docs-drift`
- `docs-okf`
- `docs-wiki`
- `labels`
- `land`
- `manual-cli`
- `okf`
- `okf-maintain`
- `okf-query`
- `repo-memory-evaluator`
- `repo-memory-writer`
- `research-challenge`
- `research-decision`
- `research-evidence`
- `research-judgment`
- `research-options`
- `research-probe`
- `research-promote`

## Limits

The evaluation is static plugin analysis, not a measured real-usage benchmark. The
2026-06-18 full-plugin rerun reported score 95/100, grade A, medium risk, zero
failing checks, one deferred reference budget warning, and two informational notes.
The directly touched router skills still report 100/100, grade A, low risk. The
remaining full-plugin warning is about aggregate deferred reference cost, not safety,
routing, authority, or progressive-disclosure failure in the changed skills.
