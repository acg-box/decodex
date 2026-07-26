# Decodex App

Purpose: Native macOS app for the local Decodex account pool.

Read this when: You are building or running the first Decodex desktop UI surface.

Not this document: Runtime scheduling, retained-lane orchestration, or public site
behavior.

## Scope

The Decodex App is a native client over Rust-owned services. Its vNext reset-card
panel invokes the bundled `decodex-cli` with fixed arguments and decodes the stable
`decodex/reset-card-cli/1` JSON response. `decodexd` is the only reset-card
credential-vault reader, Codex app-server child owner, opaque credit-ID resolver,
mutation coordinator, and durable recovery owner.

The panel discovers only configured vNext account UUIDs in `available` or `depleted`
state. It displays complete public card descriptors and sends the selected account
revision, grant timestamp, expiry timestamp, and one idempotency key. The UI never
receives the provider credit ID. It does not stage credentials, create a temporary
Codex home, launch app-server, call the provider method, or own retry state.
Account discovery records the validated profile name and stable server UUID returned by
the CLI. Every later list, use, and status invocation supplies both values through
`--profile` and `--expected-server-id`. Changing the active profile cannot redirect a
pending operation. Reset-card remote profiles fail closed until authenticated remote
transport exists.

The app does not schedule Decodex runs, own project registration, or replace the Rust
control plane. Presentation-only choices and the five-second second-click reset-card
confirmation remain client-local. Existing unrelated account-pool controls continue to
use the bundled legacy `decodex` and `decodex-app-helper` executables. They do not own
the vNext reset-card path.

## Development

The app targets macOS 27 and uses the Swift 6.4 toolchain. Older macOS releases are
not supported.

Build the SwiftPM app in release mode:

```sh
swift build --package-path apps/decodex-app -c release
```

Run it as a local release `.app` bundle:

```sh
apps/decodex-app/script/build_and_run.sh
```

Stage a signed bundle without launching it:

```sh
scripts/macos/test_decodex_app_stage.sh
```

The stage test script builds the Swift app, legacy `decodex` and
`decodex-app-helper` executables for existing account UI, the active Rust `decodexd`
daemon, and `decodex-cli`. It copies all four Rust executables into
`Contents/Helpers/`, then
verifies the staged bundle layout and signature. The app staging script always builds
Swift and Rust artifacts in release mode. If the active developer directory lacks
macOS SwiftUI macro support, the script uses
`/Applications/Xcode-beta.app/Contents/Developer`.
Direct SwiftPM launches are development-only. Point the reset-card client at a
workspace-built CLI when needed:

```sh
cargo build --release -p decodex-cli --bin decodex
cargo build --release -p decodexd --bin decodexd
DECODEX_APP_CLI="$(pwd)/target/release/decodex" \
swift run --package-path apps/decodex-app -c release DecodexApp
```

Start `decodexd` with the same vNext configuration before you test reset-card reads
or use. The CLI and app cannot bypass daemon admission or recover an operation by
calling Codex directly. The app bundle includes `decodexd` as a distribution artifact,
but Swift does not start it or inject its database and vault credentials.

For a source installation on macOS, install the local user service after the new
CLI and daemon binaries are in `~/.local/bin`:

```sh
python3 scripts/macos/install_decodex_local_service.py --replace-config
```

The installer requires PostgreSQL 18. It creates one checksummed cluster below
`~/.decodex/postgres`, disables TCP, uses a private Unix-socket directory, creates
separate migration and runtime roles, applies the embedded migrations and exact
runtime grants, and installs the `space.decodex.local-service` user LaunchAgent.
The LaunchAgent runs `decodexd supervise-local`. The Rust supervisor starts the
daemon only after PostgreSQL is ready. A PostgreSQL process or socket generation
change stops the daemon and makes the supervisor exit; launchd starts the next
coherent generation.

The current local cutover uses an explicit, bounded bridge for accounts that are
already in `~/.codex/decodex/accounts.jsonl`. The installer generates independent
vNext account UUIDs and stores only fixed slot references and SHA-256 provider-ID
selectors. The supervisor reads the account file under its existing lock and passes
current values only in the child process environment. After an atomic account-file
replacement, it restarts the daemon only when that credential projection changes.
It does not copy credentials to the Decodex
config, LaunchAgent, mapping file, logs, Infisical, or Keychain. A changed account
set fails closed and requires an operator-managed enrollment migration before the
installer can run again. This bridge is a local migration exception. It does not
make the legacy pool a vNext product-state authority or fallback.

The app retries startup-only Reset Card reads for a bounded period while the local
service becomes ready. It never retries a consume request or changes a retained
idempotency key.

The staging script follows the local Rsnap-style signing path: it writes
`target/decodex-app/Decodex.app`, signs the bundle with an Apple Development
identity, enables hardened runtime, and verifies the signature before launch. Override
the signing identity with `DECODEX_APP_SIGN_IDENTITY`; override the staging directory
with `DECODEX_APP_STAGE_DIR`.

Release packaging is owned by external Codex automation. That automation supplies
`APPLE_CERTIFICATE_P12_BASE64`, `APPLE_CERTIFICATE_PASSWORD`, and
`APPLE_SIGNING_IDENTITY`, builds the Swift app in release mode, bundles the release
legacy `decodex` and `decodex-app-helper` executables plus active `decodexd` and
`decodex-cli`, then publishes
`decodex-app-aarch64-apple-darwin.zip` beside the CLI archives. If
`APPLE_NOTARY_KEY_ID` and `APPLE_NOTARY_KEY_P8` are set, the automation notarizes and
staples the staged app before packaging; `APPLE_NOTARY_ISSUER` is used when present.

The reset-card action requires two clicks on the same card. The first click is a
local-only state change that immediately shows `Confirm Use` with a five-second
countdown. The confirmation cancels when the countdown expires or the panel closes.
A second click during the countdown runs `decodex-cli reset-card use` once. The CLI
polls the daemon's durable operation status and never resends the consume command.
If the operation remains unresolved, a UI retry keeps the same key and the daemon
returns the same durable operation.
The app persists only this credential-negative pending request handle. It reads durable
status after restart and offers `Resume` without receiving provider state.
The CLI response repeats the key and reports whether dispatch was definitely absent,
potential, durably accepted, or rejected before acceptance. The app keeps the pending
handle for both nonterminal dispatch cases and removes it only after a terminal state or
rejection before acceptance.
The journal is a bounded schema-versioned file in
`Application Support/Decodex/reset-card-pending-v1.json`. The app writes it through an
atomic replacement, synchronizes the file and parent directory, verifies an exact
readback, and enforces private file modes. Intent-based insert and remove operations use
a stable cross-process lock. One journal dispatch lock covers the final handle check,
the CLI call, result classification, and terminal removal. A malformed, oversized, or
wrong-schema journal is preserved for recovery and blocks new use. Valid recorded
attempts remain status-readable; a blocked journal never turns `NotFound` into a new
consume call.
The app has no automatic journal repair or export command. Preserve a blocked file for
manual inspection. Do not replace it until an operator has resolved every recoverable
key and has established that replacement cannot lose an unresolved operation.
`decodexd` persists the exact opaque provider credit ID and that same key before the
effect. Restart recovery never rematches another card or generates another key.
Ambiguous or incomplete inventories fail closed.

App icon assets live under `assets/app-icon/` with `source/`, `composer/`, and
`generated/` lanes. Menu bar icon assets live under `assets/tray-icon/` with matching
`source/` and `generated/` lanes. Regenerate the full icon set with:

```sh
scripts/assets/render_decodex_app_icons.swift
```

The staging script copies `app-icon.icns`, the template status item image, and the
signed legacy `decodex` and `decodex-app-helper`, active `decodexd`, and
`decodex-cli` executables into the app bundle.
The isolated `apps/decodex/standalone/Cargo.toml` packaging manifest builds the two
frozen legacy executables without adding frozen v0.2 to the active vNext workspace.
