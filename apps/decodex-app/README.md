# Decodex App

Purpose: Native macOS app for the local Decodex account pool.

Read this when: You are building or running the first Decodex desktop UI surface.

Not this document: Runtime scheduling, retained-lane orchestration, public site behavior,
or the full operator dashboard.

## Scope

The first Decodex App release only manages the shared Codex account pool through the
bundled `decodex-app-helper`, which links the Rust account service directly:

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

The staging script builds both the Swift app and the Rust `decodex-app-helper`, then
copies the helper into `Contents/Helpers/`. Direct SwiftPM launches are development-only;
when needed, point them at a workspace-built helper:

```sh
cargo build -p decodex --bin decodex-app-helper
DECODEX_APP_HELPER="$(pwd)/target/debug/decodex-app-helper" swift run --package-path apps/decodex-app DecodexApp
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
signed `decodex-app-helper` into the app bundle.
