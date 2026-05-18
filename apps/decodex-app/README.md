# Decodex App

Purpose: Native macOS app for the local Decodex account pool.

Read this when: You are building or running the first Decodex desktop UI surface.

Not this document: Runtime scheduling, retained-lane orchestration, public site behavior,
or the full operator dashboard.

## Scope

The first Decodex App release manages the shared Codex account pool through the local
Decodex server. On launch the app connects to an existing `decodex serve` on the
default local endpoint when one is available; otherwise it starts the bundled
`decodex serve --api-only` binary and talks to that server. App-started servers do not
poll registered projects or dispatch Linear work. The helper remains available for
interactive login flows that need streamed command output:

- list accounts without printing token material
- pin future Decodex runs to one account
- return future Decodex runs to balanced selection
- force Codex itself to use a stored account
- run isolated Codex device login, then import the resulting auth file
- remove a stored account from the local pool

The app does not schedule Decodex runs, own project registration, or replace
`decodex serve`. It is a native UI over the shared Rust account-management service,
not a wrapper around the `decodex` CLI binary.

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

The staging script follows the local Rsnap-style signing path: it writes
`target/decodex-app/Decodex App.app`, signs the bundle with an Apple Development
identity, enables hardened runtime, and verifies the signature before launch. Override
the signing identity with `DECODEX_APP_SIGN_IDENTITY`; override the staging directory
with `DECODEX_APP_STAGE_DIR`. This is local development signing, not a notarized
distribution build.

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
