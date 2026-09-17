# Chief connection recovery

## Failure and decision

The retained Chief process generation was `death_unknown` after a supervisor
restart. The old leader PID was no longer present, but the lost kqueue witness
could never report its exit to the new supervisor. The unique account-generation
fence then rejected every new connection. Backoff alone could not recover it.

Add one narrowly defined macOS recovery evidence kind. For a persisted session
leader on the same boot, require the kernel to return ESRCH for the positive PID
and its negative process-group ID. Repeat the leader check and verify the boot
again. Signal zero sends no signal. Success, EPERM, lookup errors, PID reuse, a
surviving group, missing bound identity, and boot uncertainty do not qualify.
The new check runs only after local ownership and in-flight supervision checks.
Linux recovery remains unchanged. This explicitly extends the earlier policy
that required an attached exit witness for every same-boot restoration.

This uses the OS's defined existence result, not a missing ps/proc listing, an
expired lease, or an elapsed timeout. Apple documents these signal-zero and ESRCH
semantics in [kill(2)](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/kill.2.html).
The existing isolation boundary is the process group, as for owned-child cleanup;
this does not add claims about descendants that deliberately escape that boundary.

## Ownership and upgrade

`ProcessGenerationControl` remains the only runtime writer of process evidence.
`database/` remains the versioned-first SQLite owner. Migration 19 extends the
closed evidence-kind constraint through an atomic table rebuild that copies all
existing columns. Earlier migrations remain unchanged. The service applies the
migration through its established startup transaction to the existing local
Decodex store. No direct repair SQL, credential changes, or deletion of work and
conversation history is used. Old binaries must not run against the newer schema.

Process retirement does not authorize replay of a turn. Existing dispatch claims,
unknown outcomes, thread identities, and parent/child work relationships remain
under the existing Chief recovery rules. A restored connection clears its saved
connection error through the existing recovery receipt.

## Verification

A real macOS subprocess regression checks a live leader, a departed leader with a
surviving child group, final group retirement, and a mismatched boot. The migration
regression verifies retained evidence field values and rejects unsupported evidence
kinds. Full database reconstruction and upgrade checks cover the schema graph.

## Live readback and external writer conflict

The upgraded service applied schema 19, recorded `macos_kernel_confirmed_gone`
for generation cfcb9e33-aa1a-481a-97fa-5caa2825b356, restored the original Chief
account connection, and cleared its connection alert. All four work identities and
provider thread IDs remained unchanged.

A single verification input was saved with command key
`connection-recovery-verification-20260916-1`. Before any turn dispatch, the provider
rejected thread resume with code -32600. Read-only lsof identified the native Codex
app-server as the holder of this exact thread's writer-lock file and rollout file.
It is a live external writer, not an orphan that Decodex may terminate or override.

Classify that provider refusal as `ThreadOwnedElsewhere`. Keep the saved input
unclaimed until its original thread can resume. Show a precise actionable message.
Record delivery alerts without duplicate accumulation; clear only those alerts
when delivery processing recovers. Later failures create new diagnostic records.
Never clear user requests or work acceptance as part of diagnostic recovery.
A regression verifies one eventual turn on the original thread after a writer
conflict, with no new thread and no duplicate delivery.

The user was asked whether the exact Chief task may be temporarily archived and
immediately restored in Codex to release the external writer. Do not perform that
operation until the user answers. No writer-lock files are deleted, and no native
Codex processes are terminated.

The final signed preview is open as a single process. Its live readback now shows
one precise external-writer diagnostic instead of the old generic wake/follow-up
failures. The original verification input remains unclaimed. The native Codex
writer is unchanged while user permission is pending.

Final validation includes the macOS leader/group regression, all 300 runtime unit
tests, the database suite plus the corrected diagnostic-lifecycle regression,
strict Clippy, formatting, and signed bundle verification. The connection-level
repair is verified on the user's retained store. A successful provider reply is
still unverified because the external writer has not been released.

## External writer release and visible ownership status

The user authorized a temporary archive and immediate restore of provider thread
`01a0ab10-9668-7cf0-bda4-39d9769a5537`. The native Codex archive API confirmed both
operations. Decodex then delivered the existing queued input exactly once in the
same thread. Turn `01a0ab65-e105-7c62-83d0-3bc960e5d4bc` completed with the requested
connection confirmation. Service readback showed zero pending events and an idle
Chief. No lock file was removed and no Codex process was terminated.

An attested external writer now produces a dedicated
`thread_in_use_needs_attention` service event. The GUI shows `In use elsewhere`
in the status center and a waiting notice beside the conversation composer.
History remains readable and the existing durable input queue remains active.
Successful delivery clears the ownership notice automatically. Repeated identical
notices are deduplicated. A later different delivery failure supersedes the old
notice without modifying its saved payload.

The upstream reference is `openai/codex` commit
`fd346b8dbaa24573a0244bc917811849d27c4cf4`, specifically the active writer classifier
in `codex-rs/tui/src/app_server_session.rs` and resume conflict tests. Decodex still
uses the installed provider's `thread/resume` response as authority. It does not
infer an owner from a visible window or automatically archive a user's conversation.

Validation includes the database delivery recovery test, the native status
presentation test, the runtime external-writer retry regression, strict Clippy,
and `target/visual-tests/chief-in-use.png` (test-only presentation fixture).
