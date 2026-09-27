# Restore call-bound captions and local audio cleanup

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
The full inherited/current voice UI diff had lost caption history, call identity,
frameless transcript support and local cleanup after a failed service request.

## Reproduced defect

The old UI hid its single caption whenever saved text, role and a timestamp
matched. A GPUI regression with a newer ordinary history message and no voice
receipt failed: the unrelated text hid the current call's caption. The failure
was an assertion after successful compilation, not a setup error.

## Restored behavior

Restore the complete inherited `chief_voice.rs`. Its only adaptations from the
snapshot are the current Agent terminology and three new empty history DTO
fields in the production display and test fixture. Retain all five original
tests and the original reduced-motion follow behavior.

Keep both speakers and each turn until a saved, disposed voice receipt from the
same call replaces it. One receipt can replace only one caption, so repeated
phrases and another call cannot erase text. Completed and incomplete captions
have separate states. Late corrections update the matching speaker/turn.
Frameless input/output deltas and finals without IDs remain supported. Text is
bounded by UTF-8 bytes.

Before releasing media, drain at most the native mailbox's 128 queued events.
Keep the resulting captions for their task until history arrives. A failed
service request retires local media and enters the existing exact-session stop
path. A late result for another call cannot stop the current call. The native
media destructor retains its stop-and-destroy contract. This change does not
introduce a second voice execution or transcription service.

Restore receipt-level voice_session_id from the existing durable source identity.
Only voice_user/voice_assistant events with the exact transcript tuple supply it;
ordinary, malformed and overlong identities remain absent. Restore its original
wire test and runtime rejection cases. Existing fixture receipts use None.

Restore the surface's retained-caption collection, history reconciliation and
work-bound display in both native and local history layouts. Restore the visible
editable composer, attachments and task references during voice, with controls
above the editor. The inherited rendered test checks draft layout at 320/800px
and editing without ending the call. This supersedes the historical note that
Live hides the composer.

Local protocol revision advances from 2.94 to 2.95 because strict older readers
reject the restored field when present. Update exact-current assertions and JSON
goldens together. No database migration or native RPC schema change is required.

## Source and validation boundary

The fixed official upstream frameless parser reads input_transcript.added,
output_transcript.added and turn.done by role/text without requiring a turn ID.
Its transcript test uses the same event shapes. The local Swift media bridge
already forwards these events. Those source checks support the restored parser;
they do not prove a live subscription audio connection.

The complete GPUI suite passes: 532 tests, with five opt-in tests ignored. This
includes all five restored voice cases. Three runtime history-receipt tests pass. All
165 protocol tests pass after updating the version goldens. Strict stable Clippy
passes for protocol, runtime and GPUI, with all features and targets. The strict
run precedes only the three test-golden version literal corrections.

Close only the complete voice UI file row. Shared composer, surface, runtime and
protocol files still need their broader reconciliation. This restores a real
behavior gap; it does not qualify live ICE/media flow, physical microphone
quality, signed voice interaction or final application acceptance. A fresh signed
artifact is required. Automations remain paused.
