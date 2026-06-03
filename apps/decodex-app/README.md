# Decodex App

Purpose: Native macOS app for the local Decodex account pool.

Read this when: You are building or running the first Decodex desktop UI surface.

Not this document: Runtime scheduling, retained-lane orchestration, public site behavior,
or the full operator dashboard.

## Scope

The first Decodex App release manages the shared Codex account pool through the
bundled Rust app helper so account UI stays on the same CLI-owned files even when a
long-running local `decodex serve` is older than the app bundle. On launch the app also
connects to an existing `decodex serve` on the default local endpoint when one is
available; otherwise it starts the bundled Decodex binary as a normal scheduler with
`decodex serve --allow-unverified-codex --listen-address 127.0.0.1:8912`. The override
only downgrades unverified Codex app-server identity to a warning; other preflight
blockers remain fail-closed. The CLI owns the default scheduler interval, currently
15 seconds. App-started servers load the enabled project registry and own the same
operator listener as a manually started `decodex serve`. The helper owns account
operations and interactive login flows that need streamed command output:

- list accounts without printing token material
- pin future Decodex runs to one account
- return future Decodex runs to balanced selection
- force Codex itself to use a stored account
- run isolated Codex device login, then import the resulting auth file
- remove a stored account from the local pool

The app does not schedule Decodex runs itself, own project registration, or replace the
Rust control plane. It is a native UI over the shared Rust account-management service
and uses the bundled `decodex` server only when no compatible local server is already
running.

The app and operator dashboard share account-pool state through the Rust account API:
stored accounts come from `~/.codex/decodex/accounts.jsonl`, run routing and account
display-name offsets come from `~/.codex/decodex/config.toml`, and Codex CLI auth
switching writes `auth.json`. Presentation-only choices such as local privacy
visibility remain client-local. Usage probes update the bounded seven-day local
estimate file at `~/.codex/decodex/account-usage-history.jsonl`; it stores daily
percentage snapshots for account-pool display and does not contain token material.

## Development

Build the SwiftPM app:

```sh
swift build --package-path apps/decodex-app
```

Run it as a local `.app` bundle:

```sh
apps/decodex-app/script/build_and_run.sh
```

Stage a signed bundle without launching it:

```sh
apps/decodex-app/script/build_and_run.sh stage
cargo make test-decodex-app-stage
```

The staging script builds the Swift app, the Rust `decodex` server binary, and
`decodex-app-helper`, then copies both Rust executables into `Contents/Helpers/`.
Direct SwiftPM launches are development-only; when needed, point them at workspace-built
executables:

```sh
cargo build -p decodex --bin decodex-app-helper
cargo build -p decodex --bin decodex
DECODEX_APP_DECODEX="$(pwd)/target/debug/decodex" \
DECODEX_APP_HELPER="$(pwd)/target/debug/decodex-app-helper" \
swift run --package-path apps/decodex-app DecodexApp
```

Use hidden `decodex serve --dev` only when manually testing local account APIs, the app
snapshot API, or dashboard routes while deliberately avoiding scheduler activity. The
normal app fallback is `decodex serve --allow-unverified-codex --listen-address
127.0.0.1:8912` so development Codex app-server builds can still run through the local
operator surface. Do not use `--dev` to validate project registration, Linear polling,
queue intake, or retained-lane execution; use ordinary `decodex serve` for those paths.

The staging script follows the local Rsnap-style signing path: it writes
`target/decodex-app/Decodex App.app`, signs the bundle with an Apple Development
identity, enables hardened runtime, and verifies the signature before launch. Override
the signing identity with `DECODEX_APP_SIGN_IDENTITY`; override the staging directory
with `DECODEX_APP_STAGE_DIR`. Override the Rust profile with
`DECODEX_APP_RUST_PROFILE`; release CI uses `final-release`.

Release tags package the app through `.github/workflows/release.yml`. The workflow
imports `APPLE_CERTIFICATE_P12_BASE64`, `APPLE_CERTIFICATE_PASSWORD`, and
`APPLE_SIGNING_IDENTITY`, builds the Swift app in release mode, bundles the
`final-release` Rust `decodex` and `decodex-app-helper` executables, then publishes
`decodex-app-aarch64-apple-darwin.zip` beside the CLI archives. If
`APPLE_NOTARY_KEY_ID` and `APPLE_NOTARY_KEY_P8` are set, the workflow notarizes and
staples the staged app before packaging; `APPLE_NOTARY_ISSUER` is used when present.

The "Use in Codex" action overwrites Codex's `auth.json` from one stored
`~/.codex/decodex/accounts.jsonl` entry. The destination is `$CODEX_HOME/auth.json`
when `CODEX_HOME` is set, otherwise `~/.codex/auth.json`.

App icon assets live under `assets/app-icon/` with `source/`, `composer/`, and
`generated/` lanes. Menu bar icon assets live under `assets/tray-icon/` with matching
`source/` and `generated/` lanes. Regenerate the full icon set with:

```sh
scripts/assets/render_decodex_app_icons.swift
```

The staging script copies `app-icon.icns`, the template status item image, and the
signed `decodex` / `decodex-app-helper` executables into the app bundle.
