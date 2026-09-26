# Closing refusal recovery

This is core compatibility for the retained native connection. The typed
`AppServerClient::thread_resume` helper retries only a native invalid-request
response with code -32600 and the exact requested thread's closing prefix.
There are at most five attempts, with 1, 2, 4 and 8 second delays. Every attempt
uses the same connection and parameters. A changed history revision or closed
connection stops retry. Other refusals, missing threads and lost replies do not
permit a retry.

The helper submits no input. Native queued work or an active goal can continue
when the native owner resumes a thread, so resume is not described as a read-only
operation. Raw transport requests remain non-replayed.

The Chief deferred recovery owner still uses `request_with_history` directly.
It schedules its own source-bound attempts and cancels on changed work, archive,
delete or revert. The ordinary-conversation retry owner remains separate and
unchanged. This restoration does not stack retries on either existing loop.

Fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582` checks
`pending_thread_unloads` in `request_processors/thread_processor.rs` before
resume admission and after history selection. The native error explicitly asks
the caller to retry after closure. `thread_lifecycle.rs` also returns that
refusal before attaching a closing thread to a connection.

The inherited success test fails with the previous direct-request helper and
passes with the restored helper. Four transport tests verify exact parameters,
peer request progress, no replay after other errors or lost replies, a five-attempt
limit and cancellation after revert. The success case includes a `thread/closed`
notification: this invalidates native settings facts, but does not change history
like a revert. Nine current runtime closing/recovery tests retain the existing
scheduler and ordinary-conversation behavior.

This batch qualifies the explicit response-handling contract with local transport
fixtures. It does not reproduce an installed-native shutdown race or close R08's
remaining uncertain-dispatch and signed application acceptance work. No protocol,
database schema, configuration or automation setting changes.
