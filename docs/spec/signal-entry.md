---
type: "Spec"
title: "Signal Entry"
description: "Define the published Decodex signal-entry schema for the GitHub-first MVP."
status: active
authority: normative
owner: runtime
tags: [spec]
last_verified: 2026-06-16
---
# Signal Entry

Purpose: Define the published Decodex signal-entry schema for the GitHub-first MVP.

Status: normative

Read this when:
- You are generating or validating signal content.
- You are rendering signal cards or detail views.
- You need to know which fields are required for publication.

Not this document:
- The GitHub input bundle schema.
- The site route contract.
- The manual publishing workflow.

Defines:
- The canonical `signal_entry/v1` shape.
- Required publication fields.
- Field-level rules for confidence, impact, proof, and try paths.

## Entry identity

The canonical schema identifier is:

- `signal_entry/v1`

## Required fields

| Field | Type | Notes |
| --- | --- | --- |
| `schema` | string | Must be `signal_entry/v1`. |
| `slug` | string | URL-safe identifier unique within the collection. |
| `lane` | string | Must be `github` for the MVP. |
| `kind` | string | `capability`, `behavior_change`, or `try_now`. |
| `title` | string | User-facing title. |
| `published_at` | string | Publication timestamp. |
| `summary` | string | Short description of what changed. |
| `why_it_matters` | string | Concrete explanation of the user-facing importance. |
| `confidence` | string | `confirmed`, `likely`, or `weak`. |
| `impact` | string | `low`, `medium`, or `high`. |
| `proof_points` | array | Non-empty list of evidence-backed points. |
| `source_refs` | object | Source links back to GitHub evidence. |

## Conditional fields

`how_to_try` is required when:

- `kind = "try_now"`, or
- `config_flags` is non-empty

`expected_effect` is required when `how_to_try` is present.

## Supporting fields

These fields are optional but expected when available:

- `config_flags`
- `caveats`
- `watch_state`

When `config_flags` are schema-backed feature toggles, prefer canonical user-facing config entries such as `features.plugins = true` rather than transient PR-local constants. Decodex should optimize these entries for what a user would actually add to `$CODEX_HOME/config.toml`.

## Source references

`source_refs` must contain:

- `repo`
- at least one commit or PR reference

`source_refs.items` may contain titled source entries for rendering. Each item must include:

- `kind` (`pull_request` or `commit`)
- `title`
- `url`
- optional `meta`

PR-first signals should include:

- `pr_url`

Commit-only signals should include:

- one or more `commit_urls`

## Publication rules

- `proof_points` must be evidence-backed and must not be empty.
- `why_it_matters` must describe user value, not internal implementation mechanics alone.
- `confidence = "weak"` is allowed only when the entry clearly signals uncertainty.
- `impact` and `confidence` must be rendered on the homepage card.

## Homepage inclusion rule

The signal collection may contain more entries than the homepage feed renders. The
homepage feed includes entries that meet at least one of these conditions:

- `impact` is `medium` or `high`
- `kind` is `try_now`
- `how_to_try` is present
- `config_flags` is non-empty
- the entry is a confirmed capability
- the entry describes deprecated, removed, legacy, rollback, disabled, or migration-
  relevant behavior

Other low-impact entries may remain checked in for release rollups, source trace, or
archive recovery, but they should not dominate the public feed.
