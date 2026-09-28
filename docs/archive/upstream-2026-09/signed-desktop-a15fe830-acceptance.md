> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

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

## Initial app-owned service observation

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

## Initial native conversation fixture

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

## Restored window access and normal exits

After the user reported restored access, the UI tool selected the exact staged
bundle. The previous cgWindowNotFound observations above are historical.

In the empty-account fixture `/Users/x/.decodex-a15-owned-kka2yj3e`, the UI menu
Quit exited GUI PID 8251 with status 0. Its recorded bundled service children,
PIDs 8256 and 8269, were absent after exit. The fixture's lifecycle-results.json
records these observations. This qualifies normal GUI exit and owned-service
cleanup for an empty profile. It does not qualify shutdown during active work.

The initialized native fixture at
`/Users/x/.decodex-a15fe830-desktop-i7b9p91b` completed two normal GUI exits and one
relaunch. A recap action increased the local synthetic provider request count
from one to two. The recap text was not visibly confirmed, so this is request and
exit evidence, not complete recap acceptance. The fixture test passed. Its
service remained owned by the test across GUI launches.

## History-edit draft persistence

The next fixture at `/Users/x/.decodex-a15fe830-desktop-ckykp4ty` provides a stronger
restart result. Select Review earlier input in the connected native conversation.
The canonical input appears in the review field. Set its unconfirmed edit text to
`Unsent history edit ckykp4ty`; do not confirm the history edit or send input.

After normal menu Quit, the private draft file retains the text. Relaunch the
same fixture: the Edit draft control and reopened review field both show the
exact text. The original prompt and Saved native answer remain visible. A second
normal menu Quit completes the test. The fixture records two launches, two
successful exits and one model request throughout: restart did not replay input.
The private draft-acceptance.json records the exact native thread and observations.
The isolated native fixture test passes without ignored or failed cases.

This proves persistence of an unconfirmed history-edit draft. It does not prove
ordinary-composer draft recovery, conflict resolution or export.

## Deferred composer interaction

The main window can be raised with its exposed accessibility action. Its native
history and review controls are then available. The ordinary composer is rendered
in a separate native child window owned by chief_native_composer.rs; it is absent
from the observed main-window accessibility tree. Coordinate and keyboard focus
attempts did not establish input in that child. The Window menu exposed only the
main Decodex entry. A later isolated fixture exposed a Decodex Composer window
with a settable Chief message field. This supplies a possible interaction route;
it does not establish a passing input test. The user deferred this work until
the other independent acceptance steps finish.

A separate probe at `/Users/x/.decodex-a15fe830-desktop-stl6a0vr` ended with one
normal GUI exit and one provider request. Its test passed, but no composer input
was verified. These fixtures use private homes and a local synthetic provider;
they do not establish physical audio or real-provider behavior.

## Automatic recap opt-out

In `/Users/x/.decodex-a15fe830-desktop-_89two_4`, use the signed Settings window to
change Automatically recap tasks from off to on and then off. Read-only database
observations confirm auto_recap=1 at revision 2 and auto_recap=0 at revision 3.
After normal Quit and relaunch, the Settings control remains off. A second normal
Quit completes the fixture with two launches, two successful exits and exactly
one model request. The test passes in 120.08 seconds.

Evidence: `recap-optout-acceptance.json` in that fixture home. This verifies the
setting and persistence of the disabled state. It does not verify the real
30-minute background trigger or cancellation of an active recap.

## Close the main window

In `/Users/x/.decodex-a15fe830-desktop-_se9357r`, open Settings, select the main
Decodex window, then use File > Close Window. GUI PID 20071 remains alive and
Settings remains visible. The fixture still has zero exits and one model
request. Normal Quit from Settings then produces one successful exit with the
same request count. See `window-close-acceptance.json` in the fixture home.

This verifies that closing the main window can retain the application and its
Settings window. The Window menu does not list the hidden main window in this
observation. Dock reopening remains unverified.

## Real background recap timing

The isolated signed application at
`/Users/x/.decodex-background-recap-jvyedqde` starts with three completed native
turns and three provider requests. Enable automatic recap through Settings, select
the main window, then use Hide Decodex. The post-action observation timestamp is
2026-09-27 12:43:29.884155 UTC. The production clock and 30-minute delay are unchanged.
Read-only observations retain three requests before the check point. At 13:13:47
UTC, 1817 seconds after the recorded hiding observation, the count is four.

The existing public ChiefClient recap query returns Ready for the exact original
thread, with a valid request identity and the complete expected synthetic summary:
The requested fix was tested; installation is still pending. The readback itself
does not issue an inference command, and the provider count remains four. The
public task snapshot is Idle with no active turn. This verifies real background
generation and the service result without a controlled-clock substitution.

Evidence in the fixture home: `background-observation.json`,
`background-trigger-observed.json`, `background-after-snapshot.json`,
`background-recap-service-readback.txt` and `desktop-ready.json`. The read-only
probe uses the existing compiled protocol library through ChiefClient; it does
not introduce a production command or replace the recap owner.

After this result, the window tool returns cgWindowNotFound for the exact staged
bundle and its existing handle. The visible result and normal menu Quit remain
unverified in this run. Before the fixture deadline, terminate only the verified
isolated GUI PID 60154. Its normal-exit assertion fails as expected; the enclosing
fixture ends with exit 101 after 2373.07 seconds. This is an intentional acceptance
abort caused by unavailable window access, not an observed product crash or a
passing interactive test. Readback confirms that the test and its three observed
descendants are absent. Restore auto_recap=0 only in the stopped private fixture
database; this cleanup is not Settings UI acceptance. See acceptance-aborted.json
and aborted-run.log in the same home. The successful service readback above
remains a separate result.

## Shutdown during an active native request

The installed Codex binary is 0.158.0-alpha.2.1, SHA-256
`3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a`.
An isolated local provider holds a real /responses HTTP connection open after
SSE headers. Before shutdown, the probe observes the exact native turn/started,
an active process and no turn/completed. Closing native stdin produces exit 0
in 0.018 seconds; the provider observes disconnect and exactly one request.
Evidence: `/private/tmp/decodex-active-shutdown-3it104xy/result.json` and the
adjacent native-messages.json and stderr.log.

The Decodex service fixture adds a separate boundary. Its opt-in
DECODEX_TEST_ACTIVE_SERVICE_SHUTDOWN mode requires a public Running work snapshot
with an active native turn and the provider-started witness. ProtocolServer
shutdown succeeds, the provider observes disconnect, and the request count
remains one. The final-source test passes in 12.60 seconds. Evidence:
`/Users/x/.decodex-active-service-5h5w3f2v/active-shutdown-before.json`,
`active-shutdown-result.json` and the provider witness files in the same home.
The ordinary socket regression passes in 16.65 seconds; strict runtime Clippy
passes with all features and targets.

These observations verify native EOF and service shutdown with an active model
request. They do not verify normal GUI Quit during active work, tool-child
cleanup or every Guardian retry and cached-allow race.

## Native unload and resume

The isolated installed-native probe uses thread_unload_delay_secs=2, a supported
test configuration in the fixed upstream source. After one completed turn,
unsubscribe leaves the thread loaded. A warm resume cancels the pending unload;
the thread remains loaded after 2.3 seconds. A second unsubscribe produces
thread/closed after 2.02 seconds and removes the thread from the loaded list.
Cold resume preserves the thread identity and Saved native answer. The provider
request count remains one, and native EOF produces exit 0.

Evidence: `/private/tmp/decodex-unload-resume-b34cfziw/result.json` and its adjacent
native message records. This verifies actual idle unload, warm cancellation and
cold history recovery without replay. Pending-unload/revert replacement races
remain outside this probe.

## Restored desktop access and Finder reopen

After the user restored desktop access, Finder menu actions and the exact signed
Decodex window became available again. The empty-profile fixture
`/Users/x/.decodex-a15-owned-y1pmfg8a` completed normal menu Quit with exit 0;
GUI PID 67113 and its observed service children 67118 and 67132 were absent.

In a separate fixture, `/Users/x/.decodex-a15-owned-johk7ex5`, select File > Close
Window, then use Finder to open the exact staged Decodex.app. The main window is
visible again in a screenshot. GUI PID 83291 remains the sole process for that
exact executable, and service PID 83293 remains its child. Normal menu Quit exits
with status 0 and both processes are absent. See `reopen-acceptance.json` in that
fixture home. This qualifies Finder reopen without a second GUI process. The Dock
tool returns timeoutReached, so a Dock click is not qualified by this result.

## Active work in an app-owned service

The signed app can start its own service from a private fixture root. A valid
standalone service fixture needs full cache settings and its execution-authority
file; the earlier in-process socket fixture did not supply those disk inputs.
Prepare synthetic accounts and fresh quota observations without creating a task
or a native process. The opt-in DECODEX_TEST_ACCOUNT_SEED_ONLY path records
prepared_only=true and zero model requests. That mode prepares data only; a
successful preparation is not recap or desktop acceptance.

With this empty-task seed and the existing offline initialization owner, the
public service reports conversation readiness and accepts Chief start. However,
the native process does not reach a running model request. In
`/Users/x/.decodex-active-gui-fzhz_5ku`, the saved events report ProcessUnavailable
followed by a Chief process authority conflict. The provider observes no request.
The service-child timeline retains the same child PID 85176 during the attempt;
no service restart was observed. A standalone signed-service probe reproduces the same failure without a GUI.
A temporary debug helper narrows the error to account/read returning -32603.
The external fixture returned an empty object for every GET request. It omitted
workspace routing from accounts/check. Upstream commit
595cc91e8cbb1c2ca822d0311dcf12709410c582 reads this routing during account/read;
the existing native fixture already supplies it. Restore the equivalent synthetic
routing response in the external fixture. No production source change is needed.
The temporary diagnostic source was restored before further acceptance.

The unchanged signed helper then reaches Running with a native active turn and
one held provider request in `/Users/x/.decodex-active-gui-kb_f6kgh`.
Controlled service stop exits 0, disconnects the provider and removes all observed
children. This is the standalone comparison, not GUI acceptance.

The app-owned fixture `/Users/x/.decodex-active-gui-ofj0aivx` then reaches Running
with native thread 01a0e36c-50a1-7c62-a173-11e4888caf79 and active turn
01a0e36c-50df-7343-a602-4542982a60af. The exact signed window exposes
Agent is working. Select Decodex > Quit Decodex through the desktop tool.
The GUI exits 0, the held provider connection closes, and observed descendants
88020, 88057, 88058, 88059 and 88060 are absent. The model request count remains
one. The runner completes successfully. See result.json and active-before.json
in that private fixture. This qualifies normal GUI Quit during active work,
owned-service cleanup and absence of input replay for this synthetic case.

Earlier aborted fixtures retain their original failure evidence. They are not
retroactively marked passed. The corrected fixture establishes that their
initialization error came from missing synthetic routing metadata.

## Manual foreground recap

In the app-owned fixture `/Users/x/.decodex-active-gui-jt611px0`, the signed GUI
shows the completed native prompt and Saved native answer. Open Task recap and
select Generate recap. The first attempt is interrupted by a transient GUI
connection-unavailable state; the provider count remains one and the service
process remains alive. After the interface recovers, reopen the panel and select
Generate recap. The provider count becomes two, with a structured recap response.
The screenshot displays the full expected summary:
The requested fix was tested; installation is still pending.

Normal menu Quit exits 0. See foreground-recap-acceptance.json and
provider-observations.json in that fixture. This qualifies foreground summary
presentation and exit for the successful attempt. It does not erase the observed
transient connection failure or establish complete background interaction.

## Ordinary input focus boundary

The app-owned fixture `/Users/x/.decodex-active-gui-no36sav5` reaches a completed
native conversation. The ordinary composer is visible in the screenshot. A
coordinate click followed by text input leaves focus on the main operational
shell; the test text is not displayed. The Window menu lists only the main
Decodex window, and the app inventory supplies no child-window selector.
The input and restart check needs a successful focus action before it can pass.
Do not substitute history-edit persistence for ordinary composer acceptance.

## Outstanding acceptance

Ordinary composer input and restart, complete background recap interaction, live
voice, Dock reopening and draft conflicts/export
remain unverified to their required scope. Real background generation and its
Ready service result are verified; the complete interactive fixture was aborted
and is not a passing result. Existing approval/media and
rendered/native fixture results remain separate evidence. Main-window access is
restored; the composer interaction work is deferred by user instruction.
Maintenance automation remains paused, and the overall manual update remains
incomplete.
