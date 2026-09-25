# Large native approval decisions and reader

Classification: core compatibility for existing explicit approvals.
Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
This continues the storage and complete page transport in PR1490 and PR1491.

## Explicit decisions

Protocol 2.79 adds RespondWithRequestedDecision. It carries the work ID, immutable
request event ID and one selected decision. The service reconstructs the response
from the complete original request. It supports exactly the requested permissions
for the current turn or an offered command/network policy amendment at its exact
array position. It cannot select an unoffered amendment or widen turn scope.

The desktop uses this reference only when it reconstructs the exact explicit
response and the normal response exceeds the existing history text bound.
Other responses keep that bound. Oversized free-form answers produce feedback
and retain their editable input. The existing question timer is unchanged.

All pending native replies now require the saved connection identity and the
native request guard for the original ID, method and params. The native transport
checks and consumes that guard before it writes. The coordinator then clears the
pending map and records the receipt. A stale or consumed request cannot reply
again. Existing installation checks and native-child ownership checks remain.
No automatic approval, permission expansion or resend is added.

## Complete content navigation

The reader displays complete selected JSON in bounded UTF-8 sections with previous
and next controls. Mouse and keyboard navigation are supported. Large duplicate
summaries point to the reader so that they cannot expand the entire conversation.
Native execution environment labels remain source-derived.

The section label describes position. It does not claim that the user has read the
content. Existing explicit approval and decline controls remain available. This
adapts the inherited reader without its mandatory page-visit confirmation flow.
A request refresh changes the reader revision; controls captured from the old
request cannot navigate or approve the new request. Service changes also retain
the existing surface generation check.

## Evidence boundaries

Protocol fixtures check exact permissions, scope, offered-array positions and a
compact command body. Runtime fixtures deliver large requests through the native
transport adapter, compare complete reply bytes, reject replay and reused IDs,
and preserve nested native ownership. A former synthetic direct-event fixture
was changed to use the transport so that it establishes a real request guard.

Rendered desktop fixtures cover mouse and keyboard section navigation, stale
reader controls, a large permission choice reaching dispatch, and oversized
free-form answer retention. The dispatch fixture has no configured service;
it does not prove a provider accepted the action. Native transport fixtures use
a local fake provider. No new installed Codex or signed desktop run is claimed.

Pending native file-change evidence before history publication and signed desktop
acceptance remain open. Other inherited scopes are tracked separately. Scheduled
automation stays paused, including after manual delivery.

Validation results: protocol136 unit and six integration tests passed; final
runtime575 passed with41 opt-in skips; final GPUI465 passed with five opt-in
skips. The dedicated reader/grant tests also passed separately. Strict protocol,
runtime and GPUI Clippy passed for all targets and features.
