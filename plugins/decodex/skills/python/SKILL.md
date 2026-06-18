---
name: python
description: Use when changing Python code, scripts, `pyproject.toml`, Poetry/bootstrap, virtualenv setup, or Python lint/type-check/test tooling and you need the touched project's checked-in Python authority.
---

# Python

## Policy

Use this skill to keep Python changes aligned with the touched project's checked-in
tooling and bootstrap path instead of personal defaults.

## Scope

- These rules apply to Python services, libraries, and tooling in this repository when Python code or Python tooling is present.
- Do not apply them to non-Python projects.

## When to use

- You are about to run, modify, or add Python code or Python tooling (Poetry, scripts, CI helpers).
- You need to decide which environment, packaging, format, lint, type-check, or test commands are authoritative for Python work.

## Language-specific rules

- Follow the touched project's checked-in bootstrap and runtime selection rules first.
- Reuse a shared root `.venv` only when the repo or project already documents that layout.
- Allow documented isolated runtimes when required, such as a skill-local private environment.
- Activate or select the intended runtime before running project Python commands.
- If the touched project is Poetry-managed, use the Poetry workflow it documents; do not assume one sync command is universal.

## Quick reference

- Runtime choice: follow the documented project or skill bootstrap first.
- Shared env: use repo/root `.venv` only when the repo already standardizes it.
- Isolated envs: allowed when the project or skill explicitly requires them.

## Common mistakes

- Assuming every Python task should use the repo root `.venv` or one Poetry sync command.
- Ignoring a documented isolated runtime such as a skill-local private environment.
- Running Python or Poetry commands against the wrong runtime, leading to confusing tool resolution and caches.

## Outputs

Return evidence for:

- The environment source and activation approach used.
- Packaging/bootstrap steps executed, including any documented runtime selection.
