# Voice media readiness and capture identity

This batch delivers the inherited Swift media-host changes. It does not change
native Codex authentication or the thread/realtime protocol.

Each microphone authorization and native dictation callback belongs to one capture
identity. Stop, replacement, and close retire that identity. Finish retains it
until the final PCM and ended event arrive. Late callbacks cannot publish data or
errors into a newer capture.

The WebKit media document reports connected only when both the peer connection
and its locally created ordered data channel are ready. Channel failure stops
capture. Device lookup, audio initialization, offer creation, playback callbacks,
and caption callbacks check the current generation after asynchronous work.
Errors identify the failed stage without exposing native error details.

Tests execute the production document in JavaScriptCore and WKWebView. They cover
readiness order, channel loss, delayed authorization, stop/replacement, final PCM,
caption ordering, playback before captions, and startup failures. An opt-in synthetic native WebRTC loopback test attempts mute/unmute without
a physical microphone or a provider call. It did not pass on this host: one run
reported connection loss, and the isolated repeat did not reach connected.
The repeat ended with zero local/remote candidates in the diagnostic snapshot;
that post-failure snapshot does not identify the cause. Logs are retained at
/tmp/decodex-inherited-voice-full.log and decodex-inherited-voice-loopback.log. These checks do not establish signed desktop, Bluetooth-device,
audible playback, or live subscription acceptance.
