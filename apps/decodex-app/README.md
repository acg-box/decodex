# Decodex App

Purpose: Native macOS menu-bar client for daemon-owned accounts, quota windows,
and Reset Cards.

Read this when: You build, test, stage, or run the Decodex menu-bar app.

Not this document: Daemon installation, PostgreSQL administration, credential
import, or runtime scheduling.

## Scope

The app has one account authority: the Decodex daemon. Its in-process Rust
client connects to the daemon through the owner-only Unix transport. When the
user explicitly refreshes an expired login, the Rust bridge starts one finite
official Codex device-login child in an owner-private temporary home. It shows
Swift only the official URL, one-time code, and closed session state. The daemon
verifies and installs the exact account credential, and the bridge removes the
temporary home on success, failure, cancellation, or App exit. The app never
starts the Decodex CLI, helper, app-server, or legacy account process. Swift
does not read account credentials, auth-file paths, PostgreSQL, Keychain, or
provider-private Reset Card identifiers.

The primary panel reads the complete account skeleton, uses the returned
routing UUID order, and immediately renders every account. It then runs
independent Reset Card and account-profile value reads for every row. These
reads use only daemon-owned observations and all rows start concurrently. They
do not start provider work. A cold or failed observation affects only its
account row and does not block another row or account action.

Each row shows the exact current 300-minute and 10,080-minute quota
observations in a vertical stack, the percentage left and reset time when
known, and every complete public Reset Card expiry. Expired observations are
not shown as current data. The 300-minute row is absent when the provider does
not return a supported observation. Current quota bars and percentages stay
inside the panel theme: accent blue above 50% remaining, warning amber from 21%
to 50%, and destructive red at 20% or below. The numeric percentage and
accessibility value remain the primary status.

The app keeps one bounded wait open for the daemon's account-observation
generation. Each progressive daemon publication wakes that wait and causes one
coalesced reload of daemon-owned values. A 30-second daemon heartbeat bounds a
missed notification or daemon restart, and transport failures reconnect with a
250-millisecond to 8-second bounded backoff. The UI has no 15-second refresh
clock. Opening the panel and `Refresh all` can still request one cache reload.
Independently, the daemon starts one observation round immediately, repeats it
every 15 seconds, and wakes it after account and Reset Card changes. It starts
every independent account concurrently, keeps at most one active observation
per account, and coalesces a lifecycle or effect wake for that account as its
next round. A periodic tick does not hot-loop an already slow account. Within
one account, Reset Card observation settles before profile observation so a
credential rotation cannot race the profile read. A slow account does not
delay publication or later scheduling for another account.

The account plan appears beside the account identity. Each detail popover
contains only lifetime tokens, peak daily tokens, longest task, current and
longest streaks, and a 36-day usage chart. The compact panel keeps aggregate
total, peak, streak, and longest-task metrics with one daily chart across all
accounts. It does not expose profile coverage counters as account status.
Email is redacted by default. The eye control changes only the published
identity slot. Hiding email removes it from SwiftUI presentation state. A
revision-bound, process-only cache keeps later visibility changes immediate and
is never written to disk. Any missing email reads settle as one complete batch,
so identities do not change one row at a time. Cached or unavailable profile
data remains row-scoped and never hides Reset Cards.

The panel uses compact individual material cards with transparent gaps and
shows every account when they fit on the active display. On shorter displays,
the account list remains scrollable without a persistent scroll indicator.
The header and aggregate overview share one compact appearance-adaptive
surface. The overflow menu stores one panel-wide card material preference. It
defaults to `Thin`, or the operator can select the system `Liquid Glass` effect.
Each account row uses its own separate surface with no opaque custom fill or drawn
border.
Primary cards use one shared compact spacing rhythm and identical content insets.
Popovers and floating login recovery use one larger shared inset.
The host window follows the system Light or Dark appearance and does not draw
its own full-window shadow or backdrop. Login recovery uses a stronger floating
material inside that same transparent window, without a window-wide modal dimmer.
Transparent gaps remain visible; there is no background surface around the
complete panel.

The app never identifies an account from its alias or vector position. The
daemon derives a stable credential-negative one-word alias. The row displays
either that alias or the account email in the same identity slot without an
identity icon. The canonical account UUID is the only row identity. There is no
account rename surface.

Reset Card controls use a subtle neutral border that remains visible on each
material card. The armed confirmation uses the existing warning accent without
changing the control size. Reset Card use requires two clicks on the same
descriptor. The first click arms a five-second confirmation. The second click
ends the confirmation countdown, shows one busy indicator, writes one
credential-negative pending handle, and sends one native daemon request with
the same account revision, descriptor, and operation key. Restart recovery
reads durable status and retains that key. It never selects another card or
generates a replacement key for an unresolved request. While the request is
unresolved, the panel shows one compact status row and checks the saved request
automatically. It does not keep a disabled confirmation countdown, show a
separate transient message, or show a manual Resume control for the same
operation. If an inventory read advances during a skeleton read, the app queues
one newer skeleton read so the account row does not remain in its checking
state. The app keeps the last quota visible while it waits for the new account
revision, but it does not expose Reset Cards from that old revision. It retains
an advanced inventory until the matching account skeleton arrives, then applies
that inventory without a duplicate daemon read. This direct old-to-new update
lets the quota bar animate to the restored value. All callers share one
per-account inventory coordinator: same-revision reads coalesce, a use gate
waits for an older daemon read, and fresh reads wait until the use dispatch
ends. A terminal use result ends the button activity immediately and starts
bounded background reconciliation. Internal contention, provider
cleanup, and other retryable detail failures keep the last quota visible under
`Updating usage…`; they do not claim that the app disconnected. Only a typed
local daemon transport loss shows `Connecting to Decodex…`. A non-retryable
failure shows the unavailable state. Expiry times use compact bordered controls
so their click action remains visible without adding a second card container.

The bounded recovery journal is
`Application Support/Decodex/reset-card-pending-v1.json`. It uses an atomic
replacement, file and directory synchronization, exact readback, private file
modes, and one cross-process dispatch lock. A malformed or unsafe journal is
preserved and blocks new use.

The panel also exposes current daemon-owned account controls: enroll the
currently signed-in shared Codex login, enable or disable, log out, and select
fixed or balanced routing. An account with a provider-confirmed unauthorized
profile shows `Refresh login`; this official device-login flow is the app's only
interactive credential-replacement surface. The native app ABI has no direct
credential-refresh operation. The action presents the official device code
with fixed-size Copy, Open, and Cancel icon controls, then refreshes only that
account after the daemon completes the exact credential replacement. The app
then performs bounded short-interval daemon readback until the new background
observation replaces any old unauthorized value. Each
account row has one `Route` control. It first projects that exact daemon-owned login to shared
`~/.codex/auth.json` for future Codex launches, then selects the same account as
the fixed Decodex route. The underlying typed commands remain independently
fenced, and retrying the control completes whichever step is not current. The
Fast control updates only the current Codex `[features].fast_mode` preference
through the in-process native client.
The overflow menu contains only `Refresh all`, the material selector, and Quit.
`Refresh all` reloads all current daemon values; it does not contact a provider.
It remains available while a signal-driven read is in progress because the store
coalesces that request into the active reload. Account mutations and Reset Card
submissions still disable it.

Each account card reveals one compact trailing-edge reorder grip while the pointer
is over that card. The grip overlays the card padding instead of reserving a layout
column. Dragging uses the list's stable coordinate space, keeps the complete card
at its original size and horizontal position, and moves it only on the vertical
track. When it crosses an adjacent card's center, that card springs into the open
slot. On release, the dragged card springs into its final slot, then the app sends
one complete `set_account_order` command with the current routing revision. The
card order follows the daemon result. A rejected command restores the last
authoritative order. The grip also provides Move up and Move down accessibility
actions through the same command path. The app does not save a separate local
account order.

The app is intentionally menu-bar-only and uses the accessory activation
policy. It does not own daemon startup or credential persistence.

## Development

The app targets macOS 27 and uses Swift 6.4.

Run the focused Swift tests:

```sh
swift test --package-path apps/decodex-app
```

Build the SwiftPM app in release mode:

```sh
swift build --package-path apps/decodex-app -c release
```

Start the installed Decodex service before testing live reads or Reset Card use.
The app cannot bypass daemon admission or call Codex directly.

## Staging

Stage and sign without launching:

```sh
apps/decodex-app/script/build_and_run.sh stage
```

Build, stage, sign, and launch:

```sh
apps/decodex-app/script/build_and_run.sh
```

The bundle contains only:

- `Contents/MacOS/DecodexApp`
- `Contents/Frameworks/libdecodex_app_client_ffi.dylib`
- app and status-item icon resources

The script builds the Rust native client and Swift app in release mode, signs
the native library and app with the selected Apple Development identity,
enables hardened runtime, and verifies the staged bundle. Set
`DECODEX_APP_SIGN_IDENTITY` to select a signing identity and
`DECODEX_APP_STAGE_DIR` to select a staging directory.
If the active developer directory lacks the required SwiftUI macros, the script
uses `/Applications/Xcode-beta.app/Contents/Developer`.

The app bundle does not contain or start a daemon. Install and supervise
`decodexd` through the repository-owned macOS service installer.

Icon assets live under `assets/app-icon/` and `assets/tray-icon/`. Regenerate
them with:

```sh
scripts/assets/render_decodex_app_icons.swift
```
