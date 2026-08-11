# GPUI feasibility spike

This isolated workspace is the XY-1263 evidence harness, not the production Decodex UI.
It pins GPUI and `gpui_platform` to Zed commit
`aeeacf5439b2d30d01e38d65d767e6f31b255ecc` and uses the stable Rust channel. The isolation
keeps the pre-1.0 dependency graph out of the root runtime lockfile. The exact-pin replacement
remains a candidate pending review and repository acceptance. A later 38/40 invalidated the initial
10/10 stability claim, and a subsequent ad-hoc 40/40 lacked exact launch provenance. The
current PID-bound harness then passed a fresh normalized current-main direct 40/40 with
complete provenance and literal-zero outer receipts. All earlier results remain provenance
in the gate evidence.

Run the deterministic and native probes from the repository root:

```sh
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer \
  cargo +stable test --manifest-path spikes/gpui/Cargo.toml --release
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer \
  cargo +stable test --manifest-path spikes/gpui/Cargo.toml --release --lib \
  workspace_headless_frame_benchmark -- --ignored --nocapture --test-threads=1
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer \
  cargo +stable run --manifest-path spikes/gpui/Cargo.toml --release \
  --features visual-probe --bin render_probe -- /tmp/decodex-gpui-spike.png
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer \
  scripts/macos/stage_gpui_spike.sh
swift scripts/macos/run_gpui_spike_accessibility_gate.swift --runs 40
```

The app creates its native window hidden and unfocused, then activates it only after GPUI
has installed AccessKit's adapter. The bound harness refuses preexisting candidate processes,
uses the normal activating `NSWorkspace` launch contract, waits for the exact returned PID
to become active, passes that PID to the live probe, verifies candidate and window identity
plus staged executable/probe hashes, captures each probe exit status, and cleans up only the
process it launched. The probe additionally requires the application and exact window to be
active before its focus work begins. Per-run artifacts and `summary.json` are written under
`target/gpui-spike/evidence/`. The probe uses bounded retries and a 20 ms Unicode
keydown-to-keyup interval, but neither is established as the cause of successful runs.

The native GPUI renderer requires Xcode's Metal toolchain. Install the matching Xcode
component once if `metal` is unavailable:

```sh
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer \
  xcodebuild -downloadComponent MetalToolchain
```

See [the gate evidence](../../openwiki/evidence/gpui-feasibility.md) for measurements,
license/provenance findings, the preserved rejected configuration, and remaining
falsifiers.
