# Remaining native execution ownership

Fixed source: openai/codex `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
This is an ownership disposition, not a claim that every upstream test passes in
the installed binary. The current installed binary SHA-256 was checked again:
`3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a`.

| Area | Fixed upstream owner and evidence | Decodex boundary |
| --- | --- | --- |
| Guardian review context and checkpoints | `codex-rs/core/src/guardian/review_session.rs`, ReviewerSession implementation: commit_snapshot captures guardian_fork_history and commits the admitted response sequence under the review state. | Preserve native review evidence and exact approval identity. Do not add a local review cache, context builder or checkpoint migration owner. |
| Explicit continuation after a denial | Native method `thread/approveGuardianDeniedAction`; the local `crates/decodex-runtime/src/chief/guardian.rs` validates digest, conflict, task/thread and latest turn, then records the request claim. | Remote rejection, success and unknown transport outcomes remain distinct. No local action retry or new turn is sent by this method. |
| Proxy connection shutdown | `codex-rs/network-proxy/src/connection_lifecycle/service.rs`: CancelOnShutdown selects executor cancellation before the wrapped connection future. `lifecycle_tests.rs` covers shutdown, dropping the handle and cancelling the wait. | Native networking owns proxy cancellation. The local process supervisor and its shutdown evidence are different owners; do not infer proxy-race coverage from local EOF or signal tests. |
| Thread unload and closing admission | `codex-rs/app-server/src/request_processors/thread_lifecycle.rs` owns pending_thread_unloads and its lifecycle. | The local closing recovery owner preserves exact thread identity and retries only qualified closing refusals. It does not implement native unload scheduling. |
| Native tool execution and Code Mode | See [native tools reconciliation](native-tools-record-reconciliation.md). | Keep output, source and turn projections. Do not add a second tool runtime. |
| OS-specific execution | The supported Decodex product here is macOS. Windows provisioning and Linux proxy/kernel-specific paths remain native/platform owners. | A source disposition does not establish a Windows/Linux Decodex port or cross-platform runtime acceptance. |

The upstream source and named test structure were inspected. Upstream tests were
not executed by this review. Installed Guardian compaction/restart and image/action
fixtures retain the limits in [Guardian qualification](native-guardian-evidence-qualification.md).
Incompatible checkpoint hashes, pending-review migration, late cached-allow races,
and Linux proxy cancellation have no new installed-runtime acceptance here.
These are explicit qualification limits, not reasons to implement replacements.

The local shutdown and closing tests separately passed 6 and 10 cases, and the CLI
signal fixture passed 3 cases. They cover prepared/no-send work, in-flight drain,
process ownership, exact closing refusal and stale socket recovery. Their logs are
`/tmp/decodex-final-shutdown-tests.log`, `/tmp/decodex-final-closing-tests.log` and
`/tmp/decodex-final-signal-tests.log`. They do not prove an active native shutdown
race or signed GUI behavior during active work.

This closes the ownership question for these R10 areas. It leaves the stated
native qualification limits visible and does not close shared desktop acceptance.
Ordinary native-composer interaction is deferred to the final acceptance step by
the user's instruction. Maintenance remains paused.
