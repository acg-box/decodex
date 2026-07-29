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
runs one independent `reset-card list` request for each row. A slow or failed
provider request affects only its account row. Each row displays the exact
300-minute and 10,080-minute quota observations, their current, stale, unknown,
or error state, the percentage used and reset time when known, and every public
Reset Card descriptor. The panel uses compact divider-separated rows so the
normal six-account pool fits in one scan. A provider-unsupported quota duration
is a muted row-local fact; it does not present the account or Reset Card service
as failed.

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

The app is intentionally menu-bar-only and uses the accessory activation
policy. It does not own daemon startup or account import.

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
