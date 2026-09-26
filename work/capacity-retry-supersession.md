# Capacity retry supersession

Classification: correctness of the existing Decodex capacity retry coordinator.
This does not add automatic retries or change the native execution owner.

If a user changes the native model or effort during capacity backoff, a later
settings read can detect the change before a notification reaches the local
coordinator. The retry validator cancels the old retry before it claims a dispatch
or sends a turn. The inherited implementation classified this as a superseded
retry. Current main had replaced that classification with a generic rejection,
which made the due-work loop return an error for a successful cancellation.

Restore the specific `CapacityRetrySuperseded` result. The due-work loop consumes
only this expected result and continues. Other errors still return normally.
The validator still compares the saved failed-turn execution with current native
settings and cancels the original retry. No dispatch claim or native turn is
created for a superseded retry.

Fixed upstream commit `595cc91e8cbb1c2ca822d0311dcf12709410c582` defines native
model and effort changes in `ThreadSettingsUpdateParams`, in
`app-server-protocol/src/protocol/v2/thread.rs`. The local test transport now
applies that settings update to its existing settings map without starting a
turn or emitting a notification. This lets the regression exercise readback-based
supersession directly.

The restored regression failed before the fix with the generic rejection. It
checks successful due-work processing after a native selection change, no turn
submission, no remaining capacity retry, an idle dispatch state and no retry on
the next due-work check. This is local coordinator evidence, not live-provider
capacity exhaustion or signed desktop acceptance.
