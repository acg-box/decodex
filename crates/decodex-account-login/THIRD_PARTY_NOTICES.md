# Third-party notices

## OpenAI Codex login source

The bounded account-login engine in `src/lib.rs` is derived from the
Apache-2.0-licensed OpenAI Codex repository:

- repository: `https://github.com/openai/codex`
- annotated tag: `rust-v0.148.0-alpha.9`
- peeled source commit: `9392c3fa5bcda342b5b96a1a04d67b2f781617c2`
- reviewed source files and functions:
  - `codex-rs/login/src/pkce.rs`: `generate_pkce`
  - `codex-rs/login/src/server.rs`: `build_authorize_url`,
    `exchange_code_for_tokens`, `persist_tokens_async`
  - `codex-rs/login/src/device_code_auth.rs`: `request_device_code`,
    `complete_device_code_login`
  - `codex-rs/login/src/auth/storage.rs`: `FileAuthStorage::save`

Decodex keeps the protocol constants and bounded behavior needed for browser and device-code
login. It replaces Codex terminal/process integration with a closed in-process adapter, adds
strict size and lifecycle bounds, distinguishes bounded structured pending device polls from
terminal authorization rejection, uses the daemon-compatible four-field auth document, and does
not copy the general Codex login framework.

The upstream Apache License 2.0 text is retained at
`third_party/openai-codex-LICENSE-APACHE`.
