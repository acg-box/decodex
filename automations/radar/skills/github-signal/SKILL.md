---
name: github-signal
description: Use when turning a reviewed GitHub bundle and code-analysis result into a Decodex signal draft, especially for writing or updating the local editorial analysis JSON that feeds `radar render-signal`.
---

# Decodex GitHub Signal

Use this skill for the final local editorial step in the GitHub-first Decodex workflow.
This is a Decodex repository-development instruction surface, not a complete
user-facing plugin skill, and it must not be packaged with the installable Decodex
plugin.

This skill does not replace the deterministic Radar CLI or upstream source analysis.
It consumes a reviewed bundle plus source-backed analysis and drafts the JSON that
`radar render-signal` renders into a final `signal_entry/v1`.

## Read before drafting

- `openwiki/integrations/plugins-automations-and-auxiliary-tools.md`
- `automations/radar/skills/codex-upstream-triage/SKILL.md`
- `automations/radar/skills/codex-code-analysis/SKILL.md`

## Inputs

- A normalized bundle JSON under `.agent/automations/radar/cache/github/bundles/`
- A source-backed `upstream_review/v1` or code-analysis result from
  `automations/radar/skills/codex-code-analysis/SKILL.md`
- An output path under `.agent/automations/radar/cache/generated/analysis/`
- Optional upstream impact output under `.agent/automations/radar/cache/github/impact/`
- Optional Control Plane upgrade candidate output under
  `.agent/automations/radar/cache/github/control-plane-upgrades/`

## Companion Skill Routing

- Use `codex-upstream-triage` before this skill when the candidate still needs to be
  selected from latest commits, PRs, releases, or changelog entries.
- Use `codex-code-analysis` before this skill when the behavior path or Control Plane
  impact is not already clear.
- Use `codex-release-analysis` before this skill when the source is release-shaped.
- Decodex Publisher may later consume the rendered signal or upstream-impact artifact,
  but this skill does not create social artifacts or publish posts.

## Boundaries

- Do not perform fresh upstream source analysis here. If the behavior path or Control
  Plane impact is not clear from the reviewed artifacts, return
  `upstream_analysis_required`.
- Treat the PR as the main narrative container.
- Treat commits, files, and patch excerpts as evidence.
- Do not summarize every commit as if it were independently important.
- Publish only when the change introduces a capability, changes user-visible behavior, or offers a clear try-now path.
- Classify Control Plane impact separately from public signal worthiness when the
  change touches Codex app-server, plugins, browser automation, MCP, permissions,
  sandboxing, or config behavior.
- Keep `why_it_matters` focused on user value, not internal mechanics.
- If `how_to_try` is present, make it concrete and pair it with `expected_effect`.
- When a feature is gated by `config.toml`, prefer canonical user-facing toggles over raw patch constants or PR-local token strings.
- When evidence is weak or the change is mostly internal cleanup, lower confidence or skip publication.

## Editorial Gate

Use the signal-entry spec and local GitHub signal runbook for detailed editorial rules.
Make only these decisions here:

- signal-worthy: user-visible capability, behavior change, try path, or migration value
- `try_now`: concrete short-session try path plus observable expected effect
- homepage highlight: confirmed, concrete, and high reader value

Skip internal cleanup, telemetry, plumbing, groundwork, and weak evidence. Keep
`why_it_matters`, `how_to_try`, and `expected_effect` focused on reader value.

## Draft shape

Write a JSON analysis draft with these fields:

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

## Workflow

1. Validate the bundle first.
2. Read the reviewed bundle metadata plus the source-backed upstream review or
   code-analysis result.
3. Decide whether the change is signal-worthy.
4. Draft the `analysis_draft` JSON under `.agent/automations/radar/cache/generated/analysis/`.
5. Draft or update an `upstream_impact/v1` artifact when the change affects Control
   Plane or Publisher follow-up. This impact artifact is the shared handoff that
   release publishing and Control Plane upgrade proposals should consume.
6. Draft or update `control_plane_upgrade_candidate/v1` only when source-backed
   evidence shows Decodex may need adoption, compatibility mitigation, or discovery;
   cite the shared `upstream_impact/v1` and do not create Linear issues or mutate
   runtime state from this skill.
7. Render the final signal entry with `radar render-signal`.
8. Validate the published signal collection and site build.

## Commands

Validate a bundle:

```bash
radar bundle validate .agent/automations/radar/cache/github/bundles/<bundle>.json
```

Render the final signal entry after drafting:

```bash
radar render-signal \
  --bundle .agent/automations/radar/cache/github/bundles/<bundle>.json \
  --analysis .agent/automations/radar/cache/generated/analysis/<bundle>.analysis.json \
  --out .agent/automations/radar/cache/site-content/signals/<bundle>.json
```

Validate the published output:

```bash
radar validate .agent/automations/radar/cache/site-content/signals
npm run build --prefix site
npm run check --prefix site
```
