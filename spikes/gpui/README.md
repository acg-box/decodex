# GPUI feasibility spike

This isolated workspace is the XY-1263 evidence harness, not the production Decodex UI.
It pins GPUI and `gpui_platform` to Zed commit
`aeeacf5439b2d30d01e38d65d767e6f31b255ecc` and Rust 1.97.0. The isolation keeps the
pre-1.0 dependency graph out of the root runtime lockfile. The candidate is not accepted:
the live macOS content-accessibility probe triggered the gate's downstream stop condition.

Run the deterministic and native probes from the repository root:

```sh
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer \
  cargo +1.97.0 test --manifest-path spikes/gpui/Cargo.toml --release
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer \
  cargo +1.97.0 test --manifest-path spikes/gpui/Cargo.toml --release --lib \
  workspace_headless_frame_benchmark -- --ignored --nocapture --test-threads=1
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer \
  cargo +1.97.0 run --manifest-path spikes/gpui/Cargo.toml --release \
  --features visual-probe --bin render_probe -- /tmp/decodex-gpui-spike.png
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer \
  scripts/macos/stage_gpui_spike.sh
open -n "target/gpui-spike/Decodex GPUI Spike.app"
swift scripts/macos/inspect_gpui_spike_accessibility.swift # expected exit 2 at this pin
```

The native GPUI renderer requires Xcode's Metal toolchain. Install the matching Xcode
component once if `metal` is unavailable:

```sh
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer \
  xcodebuild -downloadComponent MetalToolchain
```

See [the gate evidence](../../openwiki/evidence/gpui-feasibility.md) for measurements,
license/provenance findings, rejected candidate assumptions, and the triggered falsifier.
