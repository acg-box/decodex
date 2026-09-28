> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Reconcile runtime evidence owners

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Compare all changes in these five preserved files and verify their snapshot
SHA-256 values. This batch changes review records only.

| Preserved runtime source | Current owner and disposition |
| --- | --- |
| `account_api.rs` | Extract the HTTP client without changing timeout, redirect, retry or routing-cookie policy. Account health remains independent of Codex. Optional quota activation requires the native policy profile, the existing account lock and the same credential revision. Map the two added authentication lifecycle errors into the existing closed error type. |
| `chief/file_changes.rs` | Bind pending evidence to connection, thread, turn and item. Repeated reads retain uncommitted evidence. Remove it after durable approval insertion or native lifecycle completion. Reuse the immutable saved envelope for an exact duplicate request. Include identity bytes in cache accounting. Tests retain foreign-identity and lifecycle assertions and add connection and commit checks. |
| `chief/reasoning.rs` | Only widen the shared voice-handoff classifier to crate visibility. The existing timeline, recap and prompt-edit owners use this classifier; no duplicate parser is added. |
| `chief/tests/drain_rejection.rs` | Adapt error matches to `InputNotSent(ChiefDispatchRefusal)`. Retain the original assertions for server drain, provider change, prior effects and retry behavior. |
| `account_launch/chief_process_native_file_approval_tests.rs` | Adapt the shared fixture helper and keep the original native assertions. Preserve a diff larger than 90 KB, read it after reopening the database, reject once and reject a later response. Native history must not contain the required diff suffix, so history cannot mask lost live evidence. |

## Current validation

At source `4f9d61b986558e69b59ee44fdbbb907f1a6c1ba4`, the runtime library
suite passes 703 tests with no failures. The suite skips 93 opt-in tests.
The log is `/tmp/decodex-runtime-owner-audit-tests.log`.

Run the file-approval opt-in test separately against installed Codex
`0.158.0-alpha.2.1`, SHA-256
`3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a`.
The test passes with no skips. It uses a temporary home, a loopback Responses
fixture and a disposable database. It requires no real account credential.
The log is `/tmp/decodex-native-file-approval-current.log`.
Database reopen is tested; this is not signed desktop cold-start acceptance.

## Remaining boundaries

Close only these five source rows. Keep the shared bridge, native fixture
registry, model-settings tests and Chief test registry open. The audit found
deleted native coverage for context, detail, continuation and usage from the
pre-snapshot base. Map those assertions to current owners or restore coverage
before closing that boundary. Also reconcile the former inline native account,
login-policy, voice and Daybreak tests.

Keep explicit Flex cold-resume and child MCP qualification open under their
existing version-specific reports. A passing ordinary suite does not resolve
those native failures. Fresh signed desktop acceptance remains open.

Approval evidence and dispatch correctness are core. Quota activation and voice
or recap presentation remain optional consumers for the user's removal review.
Automations remain paused, including after completion.
