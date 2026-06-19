---
name: repo-memory
description: Use when bootstrapping, evaluating, or curating source-backed repo-memory OKF/LLM Wiki knowledge for a code repository, including create repo knowledge, organize repo docs, make this repo an LLM wiki, assess owner coverage, repair orphans, fix weak owners, or curate graph links.
---

# Repo Memory

Build and maintain source-backed repository memory as an OKF/LLM Wiki bundle. Use
`../../references/okf-layer.md` first, then run the smallest mode that matches the
task: `write`, `evaluate`, or `curate`.

## Modes

- `write`: bootstrap or improve concepts after reading source evidence such as
  README, `AGENTS.md`, manifests, CI, task runners, entrypoints, config, tests, docs
  indexes, and recent command output.
- `evaluate`: judge usefulness with `okf check`, `okf graph`, representative task
  questions, expected owners, navigation paths, and precise `okf find` probes.
- `curate`: repair weak owners, missing links, unclear indexes, orphans, duplicates,
  stale claims, and graph decay after real usage or evaluation exposes misses.

## Rules

- Write only claims backed by checked files, command output, external sources, or
  explicit user statements.
- Keep one owner concept per durable claim; improve metadata and links before adding
  new concepts.
- Treat `okf check` as shape validation only; prove usefulness with graph/find
  evidence and owner-navigation review.
- Record material navigation repairs in the bundle log or final answer.

## Validation

Use the matching profile and report check result, graph counts, owner-navigation
review result, field/find probes when relevant, and remaining orphan or unclear-owner
risk.
