# Interactive MCP App UI

Classification: optional product capability. R05 was delivered in
[PR1521](https://github.com/acg-box/decodex/pull/1521), merged as
`9622cb749ebb3df062729ce411f54ac82c2211b9`. This document describes the current contract;
Git history retains the incremental implementation records.

## Delivered behavior in this branch

A native MCP timeline item with UI resource metadata has an **Open app** action.
The desktop loads its exact document in a separate native panel. An interactive
widget can request an app-visible tool from its originating server or connector.
The desktop displays the server, tool and arguments. **Allow this call** executes
one reviewed request; **Cancel** returns an error to the widget. The result is saved
before acknowledgement and then returned to the widget.

A lost reply never triggers automatic replay. The task provides saved-outcome and
unresolved-call controls even when the widget is closed. An unknown outcome requires
explicit acknowledgement before another separately reviewed call can proceed.

## Native and local ownership

Reference: fixed Codex commit `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Installed-native qualification uses Codex `0.158.0-alpha.2`; this is a separate
capability check, not a claim that installed Codex equals the fixed reference.

The adapter uses the retained native app-server connection and exact paginated
thread/turn/item history. Metadata preference is `mcpAppUi.resourceUri`, then legacy
`mcpAppResourceUri`, then `appContext.resourceUri`. Malformed preferred metadata is
not replaced with a fallback. The production Chief bridge admits the implemented
`mcpServer/resource/read`, `mcpServer/tool/call` and `app/read` methods.

The service owns task, account, process generation and credential identity. Native
history/settings/connection guards apply before reads and dispatch. Source changes
invalidate the document; the desktop also checks display identity every second.
Each callback receives a fresh service review. The exact descriptor, arguments,
source and host operation are bound to its review token.

For hosted apps, the enabled connector's `app/read` tools must match the live MCP
catalog. Raw tool names are used. Model-only tools and widgetAccessible=false tools
are refused. The effective link must match the original appContext: explicit
link_id arguments when required, otherwise descriptor `_meta.link_id`. Resource
reads must echo the originating call. A global discovery fallback is insufficient.

## Persistence and recovery

Local protocol 2.88 provides source-bound document, review, confirmation, receipt,
unknown-acknowledgement and pending-operation paths. Documents use 32 KiB chunks,
a 6 MiB bound and one complete fingerprint. Receipts also use chunked fingerprinted
readback and remain available without the native process.

Schema 45 uses existing `chief_inbox_events` and `chief_request_payloads`. There is
no second database, MCP connection or execution owner. Reservation atomically checks
ownership and rejects duplicate operation IDs, reused review tokens and unresolved
calls. Only a newly reserved invocation can dispatch.

| Saved state | Meaning | Consumer behavior |
| --- | --- | --- |
| completed | A native response was stored; it can contain isError. | Show and return that result. |
| unsent | Positive evidence proves dispatch did not occur. | Show refusal; require a fresh request. |
| unknown | Execution may have occurred. | Read back; require explicit acknowledgement; never replay. |
| reserved | Outcome remains unresolved. | Keep readback available and block another invocation. |

Only positive evidence that the original process died can turn an unfinished
reservation into unknown. A missing process record or uncertain death is insufficient.
Acknowledgement never turns unknown into success or reuses the old operation.
Closing the view retains submitted receipt readback; switching tasks rejects late
results belonging to the previous task.

## Native view and supported limits

The existing Swift library owns a nonpersistent WKWebView and versioned C ABI. HTML
runs in an opaque iframe with scripts allowed. The parent relay accepts only that
iframe; direct child-frame calls to native handlers are rejected. Tool callbacks use
a host UUID separate from the browser RPC ID. Duplicate browser IDs cannot replace
pending arguments. A second outstanding call is refused.

The view implements MCP Apps 2026-01-26 initialization, input/result notifications,
ping, mediated tools/call, display-mode changes and bounded resource teardown. It
advertises only implemented capabilities. Teardown stops admission immediately and
releases handlers after the exact acknowledgement or a 500 ms bound.

Inline and fullscreen modes intersect with the widget's declared modes. Fullscreen
fills the owning window's area; it does not create a macOS Space. Inline restores the
previous panel frame. Size and mode changes generate host-context notifications.

Declared HTTPS resource/connect origins and WSS connections are validated before CSP
construction. The following restrictions are intentional and must be included in the
optional-feature review:

- External nested frames remain blocked, including declared frameDomains. The
  [MCP Apps specification](https://github.com/modelcontextprotocol/ext-apps/blob/main/specification/2026-01-26/apps.mdx)
  permits a host to restrict declared domains further. This is not general browser
  compatibility; embedded videos and third-party frame flows can be unavailable.
- Camera, microphone, geolocation, clipboard writes, file panels, popups, script
  dialogs and external navigation are unavailable. The view does not reuse voice
  permissions or the trusted voice document.
- Picture-in-picture, ui/open-link, ui/message and browser resource callbacks are not
  advertised. No dedicated authenticated web origin or persistent browser session
  is provided. Widgets that require those capabilities are outside this consumer.

## Acceptance evidence and its scope

The adapter, database, protocol, runtime and desktop tests cover exact source and
catalog selection, changed arguments, stale ownership, large chunked results,
reservation replay, process death, cold discovery, lost replies and acknowledgement.
Real WebKit tests cover opaque isolation, exact browser reply identity, duplicate
requests, large responses, teardown and native panel dimensions.

The isolated account/runtime/socket fixture runs the installed Codex binary with a
local model backend and synthetic MCP counter. With DECODEX_TEST_APP_UI=1 it verifies
native history, public document read, review, confirmed call and persisted result.
DECODEX_TEST_APP_UI_GUI_BINARY adds the signed desktop capture executable while the
same real service stays live. The widget requests value42; the capture checks no call
occurred before confirmation, invokes the real desktop action handler, and requires
both the saved completed result and a browser-origin result acknowledgement.

The complete fixture passed with two model requests and three tool calls: native
origin value7, public-client confirmation value42 and desktop confirmation value42.
The desktop review screenshot was inspected. The capture services the macOS run loop
as well as GPUI's deterministic executor. Browser acknowledgement follows the DOM
update directly because a hidden window can defer animation frames.

This proves the action-handler round trip, not a mouse click or a painted widget
screenshot. Hosted matching/mismatched-link qualification uses synthetic account
metadata and local servers, not a real user's hosted connector. A signed capture
bundle is not a normal installed release. The normal signed package and packaging checks passed at clean
commit `c9abecb1709629090e68c027f8f359d5f538c7c4`. All 25 branch commits passed signature
verification; remote checks passed before normal merge. Current core suites passed
1,133 tests and the desktop suite passed 501. Two desktop leaky-process notices
passed isolated rechecks. Shared installed lifecycle acceptance remains in R07/R12.
