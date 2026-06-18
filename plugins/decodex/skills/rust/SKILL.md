---
name: rust
description: Use when changing Rust code, `Cargo.toml`, module layout, error handling, logging, time/TLS dependencies, or Rust tooling and you need the checked-in Rust policy for the touched crate.
---

# Rust

## Policy

Use this skill to resolve Rust implementation and review choices. If the crate you are
touching has a tighter checked-in rule, follow the tighter local rule.

## Scope

- These rules apply to Rust crates, binaries, and tooling in this repository.
- They do not apply to non-Rust projects.

## When to use

- You are about to implement, refactor, or review Rust code in this repo.
- You are about to change error handling, logging, time/TLS deps, or module layout.

## Language-specific rules

- The Rust toolchain is pinned. Do not modify `rust-toolchain.toml` or `.cargo/config.toml`.
- Do not install, update, or override toolchains.
- Do not invoke system package managers.

- Do not use `unwrap()` in non-test code.
- `expect()` requires a clear, user-actionable message.
- Use the `time` crate for all date and time types. Do not add `chrono`.
- Use rustls for TLS. Use native-tls only when rustls is not supported.
- Use `color_eyre::eyre::Result` for fallible APIs. Do not introduce `anyhow`.
- Use `#[error(transparent)]` only for thin wrappers where this crate adds no context and the upstream message is already sufficient for developers.
- Use `ok_or_else` to convert `Option` to `Result` with context.

## Logging

- Always use structured fields for dynamic values such as identifiers, names, counts, and errors.
- Use short, action-oriented messages as complete sentences.

## Borrowing and ownership

- Use borrowing with `&` over `.as_*()` conversions when both are applicable.
- Avoid `.clone()` unless it is required by ownership or lifetimes, or it clearly improves clarity.
- Use `into_iter()` when intentionally consuming collections.
- Do not use scope blocks solely to end a borrow.
- When an early release is required, use an explicit `drop`.
- When the value is a reference and you need to end a borrow without a drop warning, use `let _ = value;`.

## Quick reference

- Error type: `color_eyre::eyre::Result` (do not add `anyhow`).
- Time: `time` crate (do not add `chrono`).
- TLS: rustls (native-tls only if rustls is unsupported).

## Common mistakes

- Adding `chrono`/`anyhow` out of habit (violates repo conventions).
- Using `unwrap()` in non-test code.

## Outputs

Return evidence for:

- Time/TLS, error handling, and logging choices aligned with this policy.
- Borrowing/ownership choices where they affect API boundaries and mutability.
