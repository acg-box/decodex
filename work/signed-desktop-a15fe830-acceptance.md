# Signed desktop artifact at a15fe830

## Verified artifact

The repository stage script completed from clean signed commit
`a15fe8303e0350b9c668a62f783d00790a220462`. Its bundled helper reports that exact
commit and dirty=false. Bundle contracts, signing-team checks and deep strict
signature verification passed. This is local staging, not installation,
notarization or release. Two existing release-build warnings remain; this record
does not claim a warning-free build.

Artifact: `target/upstream-a15fe830-acceptance-stage/Decodex.app`.
Build log: `/tmp/decodex-upstream-a15fe830-stage.log`.
This artifact includes the later source/relationship and catalog repairs missing
from the older 0658 acceptance artifact.

## App-owned service observation

Launch the exact staged executable with a new private test home and valid local
profile. No account is enrolled. The app PID 94553 directly creates bundled
service PID 94556 with serve --parent-fd 6. A public status read from that private
root reports configuration, SQLite, protocol 2.95, server identity and vault ready.
Conversation authentication is unavailable, as expected for the empty fixture.

The UI tool cannot select the exact staged bundle: two getApp calls return
cgWindowNotFound. Inventory shows multiple Decodex entries but no usable exact
window identity. The documented optional launch_app API is unavailable on this
host and performs no action. Do not substitute another running Decodex instance.

After this failure, verify the fixture app's exact executable and send SIGTERM
only to that PID. Both app and its owned service are then absent. This observes
parent-exit service cleanup; it does not prove normal menu Quit, draft flush,
Dock reopening, an active native turn or graceful shutdown completion.

Fixture record:
`/Users/x/.decodex-a15-owned-cwilue1q/lifecycle-results.json`.

## Native conversation fixture

Compile the current runtime test artifact and run the existing isolated public
socket desktop fixture. It reaches desktop-ready with PID 95061, one native model
request and thread `01a0e274-a6dc-71f3-8f05-459761bd8a10`. The provider is synthetic
and local. This differs from the empty-profile case and tests whether initialized
conversation state makes the exact window accessible.

The UI tool again returns cgWindowNotFound for the staged bundle. No desktop
interaction is accepted. Stop the verified fixture app deliberately. Its normal
exit assertion then fails, and the test completes with exit 101. This is an
aborted acceptance fixture, not a passing test or a reproduced application crash.
Readback finds no remaining processes for the two fixture roots or staged GUI.

Evidence:

- `/Users/x/.decodex-a15fe830-desktop-xndpad0o/desktop-ready.json`
- `/Users/x/.decodex-a15fe830-desktop-xndpad0o/ui-tool-blocker.json`
- `/tmp/decodex-a15fe830-desktop-run.log`
- `/tmp/decodex-a15fe830-desktop-current.json`

## Remaining acceptance

The exact current artifact is built and verified. Current physical-window
interaction remains blocked by the UI tool's inability to locate it. The cause
is not established. Do not infer missing accessibility support, a blank app,
normal shutdown success or unchanged personal state from this tool error.

Prompt editing, recap, approval/media interaction, draft/restart behavior,
foreground/background, opt-out, Dock, conflicts, export and normal app-owned
shutdown still need their required signed desktop evidence or an explicit scope
decision. Existing rendered/native fixture results remain separate evidence.
No automation is enabled, and the overall manual update remains incomplete.
