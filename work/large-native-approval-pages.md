# Complete selected approval pages

Classification: core compatibility for existing native approvals.
Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
This continues the immutable request storage delivered in PR1490.

## Read path

Local protocol 2.78 adds GetChiefRequestPage and a Page result. Small selected
requests retain the Available result. Larger selected JSON is split at UTF-8
boundaries into 8 KiB pages. Each page binds event, work, native method, digest,
offset and total length. A changed digest or invalid offset yields Unavailable.
Private provider envelope fields stay outside the selected request projection.

The typed client assembles pages within the existing native 8 MiB limit and a
60-second total deadline. Individual queries allow ten seconds for the native
file-detail read budget. The client checks every page identity and offset and
recomputes the complete SHA-256 digest before exposing Available to callers.
Expiration returns Unavailable; no partial action is published to the desktop.
ChiefRequestText carries the complete locally assembled content. HistoryText and
other local protocol bounds remain unchanged.

## Native liveness

The transport assigns an opaque connection identity that is shared by its clones.
The coordinator persists that identity with a native approval envelope. Service
queries require both the same retained connection and the exact native request
guard for ID, method and original params. They recheck after awaited work.
A reconnect with identical RPC ID and params cannot revive the saved request.
Persisted requests from before this connection binding remain readable as history;
they do not become live approval authority after upgrade or reconnect.

An owned native child's request can outlive its parent turn. Read projection uses
the coordinator's saved owner identity and the current native guard. A live
background request need not match the parent's current turn. Reply handling still
uses the existing pending-event and native-ancestry owners; this batch does not
add automatic decisions or retry effects.

## Validation scope and remaining work

Protocol fixtures cover lossless Unicode assembly, exact identities, changed
offsets/digests, expiration and modified content with an unchanged digest label.
Runtime fixtures cover selected-field privacy, complete command suffixes, resolved
requests, live background/child scope and reused RPC IDs after reconnect.
Desktop changes in this batch adapt request text types; they do not implement a
new reader layout or claim visual acceptance.

[The decision and reader batch](large-native-approval-decisions.md) adds large
explicit decisions and complete-content navigation. [Live file evidence](live-native-file-approvals.md)
adds pending diffs before native history and expands the composed envelope bound.
Signed desktop acceptance remains. The complete manual catch-up remains open. Automation
stays paused, including after delivery.

Validation results: protocol135 unit and six integration tests passed; adapter188
passed with seven opt-in skips; GPUI463 passed with five opt-in skips. The full
runtime suite passed574 with41 skips before the final missing-host guard cleanup.
The final runtime request selection passed29 with three opt-in skips. Strict
adapter, protocol, runtime and GPUI Clippy passed for all targets and features.
The final changes did not include a new installed-native or signed desktop run.
