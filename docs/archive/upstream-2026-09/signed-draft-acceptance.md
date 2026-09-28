> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Signed desktop draft and recap-return acceptance

## Artifact and scope

The tested local signed bundle is
`target/upstream-publication-final-stage/Decodex.app`, built from clean commit
`cc0895c3a440a544d6c414028456abd0b469cc77`. Its production source matches main
`b66ab719b6a11ffbfb8e4a98cddf1c1f626dda88`; the intervening changes are documentation.
This is local signed desktop acceptance, not installation, notarization or release.
Each fixture uses a private home, an app-owned service, the installed native Codex
binary and a local synthetic provider. No real account or user draft store is used.
The installed application was closed normally before each final fixture and restored
afterwards. Only one Decodex GUI instance remained after testing.

## Ordinary Agent composer input and restart

Fixture: `/Users/x/.decodex-active-gui-05w_1yy5`.
Native thread: `01a0e3b6-93ea-7b53-8d15-3e6f4e567bda`.
Evidence: `ordinary-draft-acceptance.json`, `draft-before-restart.json` and
`restored-draft.png` in that fixture directory.

Computer Use selected the native Composer window and its Agent message field.
The exact mixed-language text recorded in the evidence was visible in that field
and present in the private draft store. Nothing was sent. Normal menu Quit returned
exit 0. A relaunch visibly restored the complete text in the ordinary composer.
A second normal menu Quit returned exit 0 and retained the same text. The provider
request count stayed at one through both launches and exits: the original fixture
turn was not replayed and the draft was not submitted.

The clipboard operation reported a timeout, but fresh accessibility and persisted
state confirmed delivery. The test did not retry or infer failure from that timeout.
This case covers the ordinary Agent composer, not the separate, unexposed ordinary
History workbench and not the history-edit field.

Earlier attempts did not select the child window. Their conclusion that manual
user takeover was required was premature. Direct coordinate actions still do not
select the child consistently. One successful selection followed a click in the
left attachment region; this is an observed sequence, not a proven deterministic
focus fix. No production change was required for this acceptance result.

## Shared-store conflict, cancelled Quit and export

Fixture: `/Users/x/.decodex-active-gui-3a04xwww`.
Native thread: `01a0e3bb-4d1c-7353-93c7-95c71f1486ab`.
Evidence: `conflict-export-acceptance.json` and `decodex-draft-copy.json` in that
directory. The two exact fixture strings are retained in those records.

Open Review earlier input without confirming or sending an edit. A private helper
uses the existing ClientDraftStore and DesktopDraftDocument owners to publish a
competing saved composer value at revision 4. It requires a private fixture path,
one profile and no pending or uncertain dispatch. It does not corrupt the file.
Then change the review field through Computer Use. The desktop displays a draft
conflict while retaining the local edit.

Normal menu Quit is cancelled: the same GUI process remains alive, and the local
edit and conflict are still visible. Select Keep both draft copies. The desktop
reports that changes were saved and nothing was sent; one recovery copy is available.
Select Export full draft and save through the native dialog into the private home.
The exported JSON contains the complete competing copy with file mode 0600.
The canonical draft store retains both the competing composer text and local review
edit. Keyboard Command-Q then returns exit 0. The provider request count stays at one.

This case verifies shared draft-storage conflict handling through the history-edit
consumer. It does not claim a second ordinary-composer interaction result. No
history edit was confirmed and no input was submitted.

## Recap response after Hide and result on return

Fixture: `/Users/x/.decodex-active-gui-ztcuu0ad`.
Native thread: `01a0e3c1-0fd2-7932-aede-a50ded36bec3`.
Evidence: `recap-held.json`, `hidden-release.json`,
`background-display-acceptance.json` and `background-return.png` in that directory.

Request one manual recap from the signed desktop. The synthetic provider holds
its response and records the second request. Select Hide Decodex, then release
the held response. Return to the application and select its main window through
the Window menu. The expected summary is visible. Keyboard Quit returns exit 0,
and the request count remains two: the original fixture turn and the one recap.

The first return observation still displayed Cancel recap. Thus this case proves
response delivery after the Hide action and eventual display on return; it does
not prove that processing completed entirely before the first return observation.
It does not test automatic eligibility or the 30-minute timer. The older a15fe830
record separately observed the real 1,817-second automatic trigger and service
result. These two cases must not be presented as one complete automatic test.

## Remaining acceptance

Basic ordinary composer restart, shared-store conflict cancellation, keep-both
recovery, explicit export and keyboard Quit now have signed interaction evidence.
Dock reopening, combined automatic recap interaction, physical voice/media and the native
limitations in the adoption inventory remain separate. Broader recovery scenarios
must retain their own evidence boundaries. Automations remain paused.
