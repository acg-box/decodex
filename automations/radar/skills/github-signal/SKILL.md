---
name: github-signal
description: Use when turning verified GitHub evidence into a static Decodex signal draft for radar render-signal.
---

# GitHub Signal

Use this repository-local skill for one static site signal. It is optional
editorial guidance and does not create social candidates or workflow authority.

## Inputs

- one validated GitHub change bundle;
- verified official source evidence;
- a source-backed analysis of user value;
- an output path under the private Radar generated-analysis directory.

## Editorial Decision

Write a signal only when the change introduces a user-visible capability,
changes observable behavior, or provides a concrete try-now path. Skip internal
cleanup, telemetry, groundwork, and weak evidence.

Keep the narrative centered on user value. Use changed source and tests as proof.
Do not summarize every commit. Do not invent availability, benchmarks, or future
plans.

## Draft

Produce the `analysis_draft` fields required by Radar:

- `kind`
- `title`
- `summary`
- `why_it_matters`
- `confidence`
- `impact`
- `proof_points`
- optional `how_to_try`
- optional `expected_effect`
- optional `config_flags`

Render and validate with:

```sh
radar render-signal \
  --bundle .agent/automations/radar/cache/github/bundles/<bundle>.json \
  --analysis .agent/automations/radar/cache/generated/analysis/<bundle>.analysis.json \
  --out .agent/automations/radar/cache/site-content/signals/<bundle>.json

radar validate .agent/automations/radar/cache/site-content/signals
```

This skill does not publish to X. Content Manager separately researches current
sources and records one candidate or no-op through Decodex Publisher.
