---
name: github-signal
description: Use when turning a reviewed GitHub bundle and code-analysis result into a Decodex signal draft, especially for writing or updating the local editorial analysis JSON that feeds `scripts/github/render_signal_entry.py`.
---

# Decodex GitHub Signal

Use this skill for the final local editorial step in the GitHub-first Decodex workflow.
This is a Decodex repository-development instruction surface, not a complete
user-facing plugin skill, and it must not be packaged with the installable Decodex
plugin.

This skill does not replace the deterministic scripts. It tells Codex how to read a
reviewed bundle and in-session code-analysis result, decide whether the change deserves
publication, and draft the analysis JSON that the repo already renders into a final
`signal_entry/v1`.

## Read before drafting

- `docs/spec/github-change-bundle.md`
- `docs/spec/upstream-impact.md`
- `docs/spec/signal-entry.md`
- `docs/spec/social-publishing.md`
- `docs/runbook/local-github-signal-workflow.md`
- `dev/skills/codex-upstream-triage/SKILL.md`
- `dev/skills/codex-code-analysis/SKILL.md`

## Inputs

- A normalized bundle JSON under `artifacts/github/bundles/`
- A code-analysis result from `dev/skills/codex-code-analysis/SKILL.md`, when the
  behavior path is not already clear from the bundle
- An output path under `artifacts/github/analysis/`
- Optional upstream impact output under `artifacts/github/impact/`

## Companion Skill Routing

- Use `codex-upstream-triage` before this skill when the candidate still needs to be
  selected from latest commits, PRs, releases, or changelog entries.
- Use `codex-code-analysis` before this skill when the behavior path or Control Plane
  impact is not already clear.
- Use `codex-release-analysis` before this skill when the source is release-shaped.
- Use `x-post-publisher` after this skill only when the rendered signal or
  upstream-impact artifact supports a social post.

## Boundaries

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

## Editorial decision ladder

Do not collapse everything into one "worth trying" bucket. Make three separate decisions.

### 1. Signal-worthy at all

Publish a signal only when the change crosses at least one of these bars:

- It exposes a new user-facing capability.
- It changes user-visible behavior in a meaningful way.
- It gives users a concrete new path they can validate now.

Do not publish purely for internal cleanup, invisible refactors, telemetry, plumbing, or groundwork unless the user-facing effect is already clear.

### 2. Should this be `kind = "try_now"`?

Use `kind = "try_now"` only when the answer to "should a reader actively go try this now?" is yes.

Require all of these:

- The try path is concrete, bounded, and realistic for a normal product reader.
- The expected effect is directly observable by that reader.
- The payoff is user-facing, not just implementation-facing.
- The change feels newly reachable now, not merely documented or exposed as metadata.

Do not use `try_now` just because a command exists. Keep the signal as `capability` or `behavior_change` when the change is mainly informative, low-stakes, contributor-facing, operator-facing, or too niche to recommend broadly.

### 3. Should this surface as a homepage highlight?

Do not use a numeric score. Use a simple gate plus amplifier rule.

Hard gates: all of these must be true.

- `how_to_try` is present and concrete.
- `expected_effect` is present and concrete.
- `confidence = "confirmed"`.
- A normal prerelease reader can try it in one short session.
- The payoff is clear enough that the reader would care today, not just note it for later.

Amplifier rule: at least one of these must also be true.

- It unlocks a newly reachable workflow or product surface.
- It removes noticeable friction from a common workflow.
- It changes visible behavior or output in a way a user can directly confirm.

Good shortcut:

- If the reader can answer "I should go try this" after one sentence, it is probably highlight material.
- If the reader only thinks "good to know", it probably belongs in the full feed instead.

Treat a signal as not homepage-highlight material when any of these are true:

- It is mostly internal refactor, cleanup, telemetry, groundwork, or API surface bookkeeping.
- It is useful to know but not worth interrupting the homepage reading flow for.
- The try path is too indirect, too expensive, too environment-specific, or too admin-only.
- The expected effect is vague enough that the reader would not know whether it worked.
- It is a low-impact capability detail that belongs in the full feed, even if it includes a demo path.

Editorial tie-breakers:

- If several signals describe the same user journey, pick the clearest user payoff as the highlight and leave sibling details for the full feed.
- Prefer one obvious workflow win over multiple small implementation-adjacent deltas.
- `why_it_matters` should explain the user payoff, not restate the patch.
- `how_to_try` should stay short and runnable.
- `expected_effect` should describe success in reader terms.

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
2. Read `primary_pr.title`, `primary_pr.body`, `files`, `commits`, and the companion
   in-session code-analysis result when one was produced.
3. Decide whether the change is signal-worthy.
4. Draft the `analysis_draft` JSON under `artifacts/github/analysis/`.
5. Draft or update an `upstream_impact/v1` artifact when the change affects Control Plane or
   Publisher follow-up.
6. Render the final signal entry with the repo script.
7. Validate the published signal collection and site build.

## Commands

Validate a bundle:

```bash
python3 scripts/github/validate_change_bundle.py artifacts/github/bundles/<bundle>.json
```

Render the final signal entry after drafting:

```bash
python3 scripts/github/render_signal_entry.py \
  --bundle artifacts/github/bundles/<bundle>.json \
  --analysis artifacts/github/analysis/<bundle>.analysis.json \
  --out site/src/content/signals/<bundle>.json
```

Validate the published output:

```bash
python3 scripts/github/validate_signal_entry.py site/src/content/signals
npm run build --prefix site
npm run check --prefix site
```
