# Reconcile optional prompt editing and voice records

Compare the complete inherited prompt-editing and voice-input records with the
current documents. Keep the fixed upstream cutoff
`595cc91e8cbb1c2ca822d0311dcf12709410c582`. Both product interfaces are optional;
native history invalidation, transport ownership and recovery remain core.

## Prompt editing

The original 55-line record described a missing local editor and required native
in-place revert, complete canonical input, exact ownership and uncertain-outcome
recovery. The current record maps these requirements to the native reader,
runtime coordinator, durable inbox journal, public protocol and desktop draft
owners. The implementation and its documentation are present in main. Remove
the obsolete top-level pending-merge statement without claiming release.

Current desktop owners are chief_prompt_edit.rs, chief_prompt_confirm.rs,
chief_prompt_handback.rs and the canonical draft send/recovery paths. The installed
native socket fixture retains an opt-in revert and complete 70 KB resend path.
Its assertions check the exact retained history prefix, preserved native settings,
no inference during staging, one inference on explicit send and no replay during
receipt readback. This source review does not rerun that opt-in fixture.

Nine current GPUI prompt tests pass with no skips in 0.20 seconds. They include
durable confirmation before dispatch, history/draft handback before acknowledgement,
lost-send-reply readback, source invalidation and local-media validation. Log:
`/tmp/decodex-prompt-record-tests.log`. These tests do not prove complete signed
desktop interaction, cross-client atomicity or physical media accessibility.
Native revert has no expected-latest compare-and-swap. Retain that explicit limit.

## Voice

Restore the original removed source-review section without changing its bytes.
Mark it as historical. Retain its applicability decisions: TUI flags do not gate
the Decodex app-server control; Windows resources/TLS are not macOS work; WebKit
owns speaker negotiation instead of CPAL; playback does not wait for captions;
the local meter represents microphone activity. Bluetooth switching remains an
acceptance case, not a reason to copy another platform's implementation.

Current chief_voice.rs retains both speakers, frameless input, late/empty finals,
UTF-8 bounds, exact call receipts and bounded draining of 128 queued events before
media retirement. Five current GPUI voice tests pass with no skips in 0.01 seconds,
including rendered editable draft behavior and call-bound receipt reconciliation.
Log: `/tmp/decodex-voice-record-tests.log`. The first attempted test filter matched
zero tests; only this corrected five-test run supplies evidence.

The already queued correction case is covered. A correction received remotely
only after local closure still lacks proven exact cross-stream identity. Do not
replace exact receipt matching with same-role or prefix matching. The historical
subscription successes and later unsuccessful local WebRTC probes are distinct
observations. Neither establishes usable live audio in the current signed app.
Physical microphone, audible output, interruption and Bluetooth acceptance remain
open. See voice-caption-recovery.md and voice-media-readiness.md.

Close these two document comparisons only. Shared source comparisons, native
capability limits and final signed desktop acceptance remain open. This batch adds
no optional feature. Automations remain paused, including after completion.
