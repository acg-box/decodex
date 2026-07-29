# Decodex App

Purpose: Native macOS menu-bar client for daemon-owned accounts, quota windows,
and Reset Cards.

Read this when: You build, test, stage, or run the Decodex menu-bar app.

Not this document: Daemon installation, PostgreSQL administration, credential
import, or runtime scheduling.

## Scope

The app has one account authority: the Decodex daemon. It invokes the bundled
`decodex-cli`, which connects to the daemon through the owner-only Unix
transport. Swift does not read account credentials, PostgreSQL, Keychain, or
provider-private Reset Card identifiers.

The primary panel reads the complete account skeleton with `account list`, uses
the returned routing UUID order, and immediately renders every account. It then
runs independent `reset-card list` and `account profile` requests for each row.
A slow or failed provider request affects only its account row and does not
block the other read.

Each row shows the exact 300-minute and 10,080-minute quota observations in a
vertical stack, their current, stale, unknown, or error state, the percentage
left and reset time when known, and every complete public Reset Card time
window. The 300-minute row is absent only when the provider omits that window;
unknown observations and real errors remain visible.

The profile section restores the compact operator summary: lifetime tokens,
peak daily tokens, longest task, current and longest streaks, and a 36-day
usage chart. The panel also aggregates available profiles across all accounts.
Email is redacted by default. The eye control requests it explicitly and hiding
it immediately removes the value from retained presentation state. Cached or
unavailable profile data remains row-scoped and never hides Reset Cards.

The panel uses compact divider-separated rows and a bounded vertical viewport,
so all accounts remain present and scrollable instead of allowing one row to
expand across the window.

The app never identifies an account from its label or vector position. The
canonical account UUID is the only row identity.

Reset Card use requires two clicks on the same descriptor. The first click arms
a five-second confirmation. The second click writes one credential-negative
pending handle, then invokes `reset-card use` once with the same account
revision, descriptor, and operation key. Restart recovery reads durable status
and retains that key. It never selects another card or generates a replacement
key for an unresolved request.

The bounded recovery journal is
`Application Support/Decodex/reset-card-pending-v1.json`. It uses an atomic
replacement, file and directory synchronization, exact readback, private file
modes, and one cross-process dispatch lock. A malformed or unsafe journal is
preserved and blocks new use.

The panel also exposes current daemon-owned account controls: enroll the
currently signed-in shared Codex login, rename, enable or disable, refresh
credentials, log out, and select fixed or balanced routing. The Fast button
updates only the current Codex `[features].fast_mode` preference through the
bundled CLI.

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

For a development launch against a workspace CLI:

```sh
cargo build --release -p decodex-cli --bin decodex
DECODEX_APP_CLI="$(pwd)/target/release/decodex" \
  swift run --package-path apps/decodex-app -c release DecodexApp
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
- `Contents/Helpers/decodex-cli`
- app and status-item icon resources

The script builds both executable artifacts in release mode, signs the CLI and
the app with the selected Apple Development identity, enables hardened runtime,
and verifies the staged bundle. Set `DECODEX_APP_SIGN_IDENTITY` to select a
signing identity and `DECODEX_APP_STAGE_DIR` to select the staging directory.
If the active developer directory lacks the required SwiftUI macros, the script
uses `/Applications/Xcode-beta.app/Contents/Developer`.

The app bundle does not contain or start a daemon. Install and supervise
`decodexd` through the repository-owned macOS service installer.

Icon assets live under `assets/app-icon/` and `assets/tray-icon/`. Regenerate
them with:

```sh
scripts/assets/render_decodex_app_icons.swift
```
