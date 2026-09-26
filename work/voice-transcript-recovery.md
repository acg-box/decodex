# Voice transcript persistence after disconnect

Classification: core recovery for the existing optional live voice feature.
Fixed upstream reference: `595cc91e8cbb1c2ca822d0311dcf12709410c582`,
`app-server-protocol/src/protocol/v2/realtime.rs`. Native transcript deltas and
final transcript parts are distinct notifications.

A transport close previously marked voice failed without saving its received
text. Final and normal-close handlers also cleared text and advanced the local
sequence before the store accepted the write.

The runtime now saves received tails on transport close. It keeps the call open
until native closure or process-death reconciliation establishes its lifetime.
Text and sequence advance only after a successful store write. A failed final
write retains both the corrected text and its finality for a later save. A new
transcript cannot overwrite an unsaved final part. Saving never sends native
input, requests approval or restarts voice.

The existing 32 KiB per-record and delta-buffer limits remain. The buffer keeps
the most recent suffix on UTF-8 boundaries when more text arrives. A final native
transcript above that bound retains its suffix with `complete: false`, so storage
does not reject the entire part or label truncated text complete. This does not
claim complete capture of transcripts above those bounds. No schema or
native execution owner changes. Received partial text remains marked partial;
it is not promoted to a completed native transcript.

A real store fixture with an admitted process and bound work reproduces the
missing record before the fix. Tests cover disconnect, repeated disconnect,
injected insert failure for partial and final text, later successful closure,
exact text/finality, sequence retention, call lifetime and no native replay.
These are service recovery tests, not microphone or WebRTC acceptance.

A Unicode overflow fixture checks both delta accumulation and final text. It
requires a nearly full bounded suffix, the exact latest correction, valid UTF-8
and partial provenance, without a native replay.

A provider precaution first retires microphone authority and requests native stop.
It then saves received text before clearing the session. A storage failure keeps
the pending text and session available for the subsequent native closure event;
it does not prevent the stop request. Fixtures cover successful stop and an
injected insert failure followed by successful closure.
