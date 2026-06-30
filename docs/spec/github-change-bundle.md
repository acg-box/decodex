---
type: "Spec"
title: "GitHub Change Bundle"
description: "Define the normalized GitHub input bundle that feeds Decodex signal analysis."
status: active
authority: normative
owner: automation
tags: [spec, radar]
last_verified: 2026-06-25
---
# GitHub Change Bundle

Purpose: Define the normalized GitHub input bundle that feeds Decodex signal analysis.

Status: normative

Read this when:
- You are changing `radar bundle build` or bundle normalization behavior.
- You are deciding what data Codex should read before drafting a signal.
- You are validating whether a bundle contains enough context for GitHub-first analysis.

Not this document:
- The rendered site contract.
- The published signal-entry schema.
- The local or CI workflow orchestration.

Bundle generation remains deterministic whether it is run locally or on a
trusted automation runner. The bundle itself must not depend on Codex output or
other non-deterministic editorial state.

The Rust CLI owns deterministic bundle building and validation:

```sh
radar bundle build --repo openai/codex --pr 15222 --out .agent/automations/radar/cache/github/bundles/openai-codex-pr-15222.json
radar bundle validate .agent/automations/radar/cache/github/bundles/openai-codex-pr-15222.json
```

The Rust `radar bundle ...` surface is the single active deterministic bundle
command path.

Defines:
- The canonical `github_change_bundle/v1` shape.
- Required fields for PR-first analysis.
- The relationship between PR context and commit evidence.

## Bundle identity

The canonical schema identifier is:

- `github_change_bundle/v1`

## Required top-level fields

| Field | Type | Notes |
| --- | --- | --- |
| `schema` | string | Must be `github_change_bundle/v1`. |
| `repo` | string | Repository in `owner/name` format. |
| `analysis_mode` | string | `pr_first` or `commit_only`. |
| `default_branch` | string | The repository integration branch used to interpret merge context. |
| `commits` | array | Non-empty list of commit summaries tied to the change. |
| `files` | array | Non-empty list of changed files. |

## PR-first fields

When a PR is available, the bundle must also include:

| Field | Type | Notes |
| --- | --- | --- |
| `primary_pr.number` | number | Canonical PR number. |
| `primary_pr.title` | string | Primary semantic title. |
| `primary_pr.body` | string | Description text; may be empty but must be present. |
| `primary_pr.state` | string | Expected merged state for publishable changes. |
| `primary_pr.merged_at` | string or null | Merge timestamp when merged. |
| `primary_pr.labels` | array | Zero or more label names. |
| `primary_pr.url` | string | Canonical PR URL. |

## Supporting fields

These fields are recommended whenever present:

- `linked_issues`
- `extracted_flags`
- `docs_refs`
- `examples_refs`
- `notes`

## Required analysis rule

When `analysis_mode = "pr_first"`:

- `primary_pr.title`
- `primary_pr.body`
- `files`
- `commits`

form the primary semantic source for analysis.

Commit messages, patch excerpts, and file-level evidence support the analysis. They do not replace the PR as the primary narrative container.

## Commit summaries

Each item in `commits` must contain:

- `sha`
- `message`
- `url`

Each item may also contain:

- `author`
- `committed_at`

## File summaries

Each item in `files` must contain:

- `path`
- `status`
- `additions`
- `deletions`

Each item may contain:

- `patch_excerpt`

`patch_excerpt` is optional and may be truncated. It exists to provide evidence, not to reproduce the full diff.

## Minimal valid commit-only fallback

When no PR is available, the bundle remains valid if:

- `analysis_mode = "commit_only"`
- `commits` is non-empty
- `files` is non-empty

This fallback is lower context than PR-first mode and should normally lower downstream confidence unless corroborating evidence exists.
