# Repo Layout

Purpose: Define the canonical repository layout for the GitHub-first Decodex MVP.

Status: normative

Read this when:
- You are adding, moving, or deleting top-level directories.
- You are scaffolding the static site or GitHub analysis tooling.
- You need to know which parts of the repo are authoritative versus transitional.

Not this document:
- The Decodex page or route contract.
- The GitHub bundle or signal-entry schema.
- The local analysis or deployment workflow.

Defines:
- Canonical top-level roots and their responsibilities.
- The transitional status of the root Rust template surface.
- Reserved locations for site code, signal content, and GitHub tooling.

## Canonical top-level roots

The repository has these canonical roots:

| Path | Role |
| --- | --- |
| `docs/` | Authoritative documentation, including specs, guides, and saved plans. |
| `site/` | Static-site application code and site-owned content. |
| `tools/` | Deterministic automation used to collect, normalize, render, and validate Decodex content. |
| `skills/` | Repo-local AI workflow entrypoints that point at Decodex-specific procedures and tooling. |
| `src/` | Legacy Rust template surface retained only until the template cleanup is explicitly executed. |

## Root layout invariants

- All new user-facing site code must live under `site/`.
- All new GitHub collection, normalization, render, and validation scripts must live under `tools/`.
- All repo-local Decodex skills must live under `skills/`.
- All normative contracts must live under `docs/spec/`.
- All procedural runbooks must live under `docs/guide/`.
- The root Rust template surface is not the target location for new Decodex product features.
- Removing or repurposing the root Rust template surface requires an explicit plan update or follow-on plan.

## Reserved site-owned paths

These paths are reserved for the static site implementation:

- `site/src/`
- `site/public/`
- `site/src/content/`
- `site/src/content/signals/`

Site build outputs must remain under site-owned ignored paths such as framework-specific cache or build directories. They must not write generated artifacts into the repo root.

## Reserved tooling paths

These paths are reserved for GitHub-first automation:

- `tools/github/`
- `tools/github/bundles/` for normalized intermediate artifacts if persisted
- `tools/github/templates/` for script-owned static templates when needed

## Transitional rule for the Rust template

The files `Cargo.toml`, `Cargo.lock`, `build.rs`, and `src/` remain part of the checked-in template history during this transition. They are treated as legacy template surfaces, not as the canonical home for the Decodex product.

Do not attach new Decodex site work to:

- `src/`
- `build.rs`
- Rust-specific root binaries

unless a later plan explicitly repurposes those surfaces.

## Example target shape

```text
docs/
  spec/
  guide/
  plans/
site/
  src/
  public/
  src/content/signals/
tools/
  github/
skills/
src/                # legacy template surface
Cargo.toml          # legacy template surface
```
