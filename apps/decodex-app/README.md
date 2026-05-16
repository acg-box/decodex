# Decodex App

Purpose: Native macOS app for the local Decodex account pool.

Read this when: You are building or running the first Decodex desktop UI surface.

Not this document: Runtime scheduling, retained-lane orchestration, public site behavior,
or the full operator dashboard.

## Scope

The first Decodex App release only manages the shared Codex account pool:

- list accounts from `decodex account list --json`
- pin future runs to one account with `decodex account select`
- return future runs to balanced selection with `decodex account clear`
- run isolated Codex device login with `decodex account login`
- remove a stored account with `decodex account logout`

The app does not schedule Decodex runs, own project registration, or replace
`decodex serve`. It is a native UI over the CLI-owned account-management surface.

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

By default, the app invokes `decodex` from `PATH`. During development, point it at a
workspace build:

```sh
cargo build -p decodex
DECODEX_CLI="$(pwd)/target/debug/decodex" apps/decodex-app/script/build_and_run.sh
```

The staging script follows the local Rsnap-style signing path: it writes
`target/decodex-app/Decodex App.app`, signs the bundle with an Apple Development
identity, enables hardened runtime, and verifies the signature before launch. Override
the signing identity with `DECODEX_APP_SIGN_IDENTITY`; override the staging directory
with `DECODEX_APP_STAGE_DIR`. This is local development signing, not a notarized
distribution build.

App icon assets live under `assets/app-icon/` with `source/`, `composer/`, and
`generated/` lanes. Menu bar icon assets live under `assets/tray-icon/` with matching
`source/` and `generated/` lanes. Regenerate the full icon set with:

```sh
scripts/assets/render_decodex_app_icons.swift
```

The staging script copies `app-icon.icns` and the template status item image into the
app bundle resources.
