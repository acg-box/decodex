## OpenWiki

Maintain project documentation only in `openwiki/` through the OpenWiki generation lifecycle. Do not create a separate `docs/` tree or copy old documents into the wiki one for one. Consolidate by current system responsibilities and reader tasks. Git history retains retired plans and revision-specific records; link an exact historical revision only when it is necessary evidence. Keep root entrypoints concise.

Start here:
- [OpenWiki quickstart](openwiki/quickstart.md)

OpenWiki includes repository overview, architecture notes, workflows, domain concepts, operations, integrations, testing guidance, and source maps.

When working in this repository, read the OpenWiki quickstart first, then follow its links to the relevant architecture, workflow, domain, operation, and testing notes.

## Rust toolchain

- Use the `stable` Rust compiler channel for every active Rust workspace.
- Keep each active `rust-toolchain.toml` channel set to `"stable"`.
- Do not replace `stable` with a numbered, beta, or nightly compiler channel unless the
  user explicitly authorizes that change.
- Build and test commands must not override this policy with a numbered toolchain.

## GitHub labels

- Do not create or apply GitHub labels in this repository.

## UI copy

- Use explicit labels for values and complete sentences for explanations.
- Put counts, status qualifiers, and keyboard shortcuts in parentheses when they
  qualify a label. Use separate rows for independent details when needed.
- Do not use centered dots as decorative text separators. Preserve user and
  upstream text, mathematical operators, and actual list markers.

## UI design language

- Follow [Interface geometry and motion](openwiki/architecture/design-language.md)
  for desktop layout, spacing, type, color, and motion.
- Every activity island must use the shared sizing, allocation, transition, and
  material-surface components. Keep feature content separate from geometry.
- Change the shared contract and its focused checks when a new requirement
  needs another variant. Do not add a feature-specific island animation or size
  system.
- Use `ui_motion::HoverIntent` and its shared delay tokens for hover-triggered
  expansion. Keep pointer dwell, departure grace, cancellation and explicit
  click behavior in this shared contract; do not add feature-specific timers.

<!-- OPENWIKI:START -->

## OpenWiki

This repository has a generated `openwiki/` evidence index. It is optional just-in-time context, not required startup reading.

- Do not enumerate, preload, or search wikis at task start. Use retrieval when the user asks for it, when unfamiliar architecture or dependency behavior materially affects the task, or when source inspection leaves an important uncertainty. Stop once the question is grounded.
- When those conditions apply and OpenWiki retrieval tools are available, use `openwiki_search` for just-in-time context and `openwiki_read` for the relevant complete sections. If search returns `workspace_required`, ask which listed workspace to use and retry with its ID.
- Use `openwiki_list_workspaces` or `openwiki_list_wikis` when workspace membership itself needs to be discovered.
- If the retrieval tools are unavailable, read `openwiki/quickstart.md` and follow its links to the relevant pages.
- Treat source code and tests as authoritative. A brief's unknowns and review items are verification gaps, not automatic requirements.
- Prefer the narrowest quiet validation that proves the changed behavior. Preserve complete failure output.

Use the repository's documented maintenance process to refresh generated pages after source changes. Check its configuration and completed runs before relying on automatic updates. Do not hand-edit generated OpenWiki pages unless explicitly asked; prefer updating source code/docs and letting OpenWiki regenerate.

<!-- OPENWIKI:END -->

### Current Wiki scheduling status

The generated scheduling sentence above does not prove an active automation.
This repository currently has no checked-in OpenWiki Actions workflow or Wiki role
in `automations/portfolio.toml`. Use the OpenWiki page-job update lifecycle and
verify actual workflow registration before claiming scheduled refresh. See
[Wiki maintenance](openwiki/operations/wiki-maintenance.md).

## Codex upstream reference

- For Codex-backed behavior, inspect the relevant official `openai/codex` source,
  protocol schema, and tests before implementing a local replacement.
- Record the upstream commit and distinguish upstream main from capabilities in
  the installed Codex binary. Generate schemas from that binary to verify support.
- Reuse native conversation, usage, context, recovery, and agent capabilities when
  supported. Keep Decodex-specific presentation and coordination in this repository.
- Keep reference checkouts out of tracked application sources. Do not execute
  upstream setup scripts merely to read source or compare protocols.

## Rust channel policy

Use the unversioned `stable` channel for Rust builds and tests. Use the
unversioned `nightly` channel for Rust formatting. Do not select numbered
compiler versions or dated nightly versions in local tasks, CI, or container
builds. Rust container images must follow the stable release with a floating
distribution tag.
