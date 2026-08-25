---
type: "Reference"
title: "XY-1368 retained-title validation"
openwiki_generated: true
---

# XY-1368 retained-title validation

> **Superseded historical provenance.** This document records the former V1-V32
> migration-based validation boundary. Its commands and results are non-executable
> historical evidence. They are not current command or gate authority and must not be
> run for vNext acceptance. Current authority is
> [Commands and validation](commands-and-validation.md) and the
> [vNext gate manifest](../specs/vnext-gates.md).

This document formerly owned the bounded validation commands for the V14-V22
retained-title core.

## Historical preparation command

The former boundary instructed operators to run this command once after source freeze:

```console
cargo make check-vnext-retained-title-preparation
```

The command used one private former server store 18 cluster and reported three complete stages:

1. It checked the full V1-V32 migration syntax, applied and canonically provisioned a
   fresh full ledger, and separately verified the closed migration-only authority delta
   from V24 through V32. V28, V29, V30, V31, and the V32 exact-release repair did not
   derive or grant a runtime principal. The one configured post-migration provisioner
   owned the final runtime ACL.
2. It parsed and prepared all 30 changed V22/V27/V28 embedded former server store statements in
   one Rust process.
3. It verified the generated schema and configured-authority digests, including the exact 28
   migration-owned function sources and 201 final function contracts in `authority.rs`.

The command did not execute the prepared statements.

## Historical semantic boundary command

The former boundary instructed operators to stage the complete source candidate before
running this command:

```console
cargo make test-vnext-retained-title-core
```

The command required an index-only candidate. It bound the base commit and staged tree.
It reused the accepted former server store continuation contract and the V22 production-inert check.

The receipt recorded these accepted pinned protocol facts for `codex-cli 0.145.0-alpha.18`:

- A `thread/start` result can contain a null thread name.
- `thread/name/set` is a separate title mutation.

The versioned primary source was the OpenAI Codex tag `rust-v0.145.0-alpha.18`.
The relevant source was `codex-rs/app-server-protocol/src/protocol/v2`.

The historical boundary did not start an app-server thread or use Desktop.

## Historical command scope

These commands were partial-boundary commands. They did not replace the trusted full
check and did not publish `decodex/local-full-check`.

XY-1304 owned aggregate validation, production enablement, full-check publication, and
landing for that historical boundary.
