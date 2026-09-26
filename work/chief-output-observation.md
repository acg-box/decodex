# Chief output observation ownership

Classification: core correctness of the existing live-output consumer. This
change is needed independently of the optional task recap feature.

The visible Chief conversation owns one output observation connection. Previously,
its ongoing Tokio block_on ran inside the shared GPUI background executor. During
installed-native automatic recap qualification, a thread sample showed the visual
scheduler blocked in ChiefClient::observe_output. A production background worker
would likewise remain occupied for that connection's lifetime; this evidence does
not claim that a production UI freeze was reproduced.

The same observation now runs in a named chief-output-io thread. The existing
latest-value watch channel still coalesces updates. Dropping the view's receiver
cancels the protocol client's closed() branch and releases the connection. Normal
failure and thread-start failure use the existing two-second retry interval.
There is no second subscription, replay queue, transcript store or protocol change.

The foreground owner still applies exact work/current-turn checks and the existing
8-ms update coalescing. Threading does not promote an observation into execution
authority. The protocol regression checks connection reuse and cancellation while
waiting; installed-native desktop qualification checks that other UI work can
complete while observation remains active.

The native qualification used a disposable same-UID service, Codex
0.158.0-alpha.2, synthetic credentials/provider responses and a GPUI offscreen
window. No installed user profile or production preference was modified.
