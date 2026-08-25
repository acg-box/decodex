## OpenWiki

This repository uses `openwiki/` as its project knowledge entrypoint.

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

<!-- OPENWIKI:START -->

## OpenWiki

This repository has a generated `openwiki/` evidence index. It is optional just-in-time context, not required startup reading.

- Treat source code and tests as authoritative. A brief's unknowns and review items are verification gaps, not automatic requirements.
- Prefer the narrowest quiet validation that proves the changed behavior. Preserve complete failure output.

The scheduled OpenWiki GitHub Actions workflow refreshes the repository wiki. Do not hand-edit generated OpenWiki pages unless explicitly asked; prefer updating source code/docs and letting OpenWiki regenerate.

<!-- OPENWIKI:END -->
