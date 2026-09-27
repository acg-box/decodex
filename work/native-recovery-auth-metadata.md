# Read native authentication metadata for recovery

Restore the complete inherited `app_server_client/recovery_auth.rs` file and its
module export. The restored bytes and both inherited tests are unchanged. This
is a prerequisite for the still-missing automatic model recovery policy; it does
not enable automatic model selection or add a second mutation owner.

The read asks `getAuthStatus` for metadata with `includeToken: false` and
`refreshToken: false`. It uses the existing exact-process history guard and
rejects a stale response. It distinguishes a ChatGPT-authenticated provider from
custom providers, signed-out state, non-ChatGPT authentication and malformed or
unknown metadata. An unexpected exported token makes the projection unavailable.
It never infers ChatGPT authentication from a provider name. Account identity,
thread ownership and recovery-banner freshness remain caller obligations.

## Source and installed binary

Read fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582`:
`app-server-protocol/src/protocol/v1.rs` defines the metadata request/response;
`app-server/src/request_processors/account_processor.rs` implements the flags
and provider-dependent result. The existing retained Decodex bridge already
admits this method.

The installed executable has changed since the earlier qualification:

- Version: `codex-cli 0.158.0-alpha.2.1`.
- SHA-256: `3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a`.
- Path: `/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex`.

Generate its schema into `/tmp/decodex-recovery-auth-schema-20260926`. The default
schema output omits the legacy method, so it does not prove method support.
An actual isolated native process with a synthetic custom provider accepts the
request and returns `authMethod: null`, `authToken: null` and
`requiresOpenaiAuth: false`. The temporary HOME contains no account credentials;
no inference backend is contacted. The process exits after the probe. Result:
`/tmp/decodex-recovery-auth-native-result.json`.

## Validation and limits

Both restored adapter tests pass, including exact request flags and no follow-up
request. Strict adapter Clippy passes with all features and targets. Logs:
`/tmp/decodex-recovery-auth-tests.log` and `/tmp/decodex-recovery-auth-clippy.log`.

The native probe proves the custom-provider metadata path, not real ChatGPT login
or the whole automatic recovery flow. Close only this restored helper's file row.
The shared client file, recovery producer, durable selection integration and
signed desktop acceptance remain open. No installed binary, preference or
automation is changed by this batch.
