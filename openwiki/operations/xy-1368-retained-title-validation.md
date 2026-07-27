# XY-1368 retained-title validation

This document owns the bounded validation commands for the V14-V22 retained-title core.

## Preparation command

Run this command once after the source boundary freezes:

```console
cargo make check-vnext-retained-title-preparation
```

The command uses one private PostgreSQL 18 cluster. It reports three complete stages:

1. It checks the full V1-V27 migration syntax, applies a fresh full ledger, and verifies a V24-to-V27 closed-authority upgrade.
2. It parses and prepares all 27 changed V22/V27 embedded PostgreSQL statements in one Rust process.
3. It verifies the generated schema and configured-authority digests, including the exact 23 migration-owned function sources and 196 final function contracts in `authority.rs`.

The command does not execute the prepared statements.

## Semantic boundary command

Stage the complete source candidate before you run this command:

```console
cargo make test-vnext-retained-title-core
```

The command requires an index-only candidate. It binds the base commit and staged tree.
It reuses the accepted PostgreSQL continuation contract and the V22 production-inert check.

The receipt records these accepted pinned protocol facts for `codex-cli 0.145.0-alpha.18`:

- A `thread/start` result can contain a null thread name.
- `thread/name/set` is a separate title mutation.

The versioned primary source is the OpenAI Codex tag `rust-v0.145.0-alpha.18`.
The relevant source is `codex-rs/app-server-protocol/src/protocol/v2`.

Do not start an app-server thread for this boundary. Do not use Desktop for this boundary.

## Command authority

These commands are partial-boundary commands. They do not replace the trusted full check.
Do not publish `decodex/local-full-check` from either command.

XY-1304 owns aggregate validation, production enablement, full-check publication, and landing.
