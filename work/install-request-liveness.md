# Installation request liveness

A native installation suggestion can remain pending after its original local
turn is no longer active. Inspection and durable reservation previously required
the suggestion's turn to match the current running turn, so the UI could lose a
request that the native client still held open.

Use the exact native server-request guard for request liveness. Preserve durable
checks for the event, open disposition, work/thread relationship, process
generation, precaution state, plugin identity, and previous installation attempt.
The runtime obtains the guard before reservation and uses the guarded native
installation call. It rechecks ownership and request state around remote reads.

The new coordinator test retains a real in-memory server request, completes only
the local turn, and then exercises review, one installation, connector access,
receipt recovery and duplicate rejection. It first failed at inspection, then
exposed the matching reservation restriction. After both changes, all four local
installation scenarios pass. Three database tests retain foreign-owner,
resolved-request and no-replay checks. The native installation opt-in test was
not run for this batch; this is not proof of a live marketplace installation or
of a particular upstream tool-yield sequence.

The inherited inspection code used request liveness. The fixed upstream
`595cc91e8cbb1c2ca822d0311dcf12709410c582` plugin handler awaits its exact
elicitation response and verifies installation afterward. This change does not
authorize installation from an old stored event without a live native request.
