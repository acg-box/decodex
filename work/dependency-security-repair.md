# Repair inherited dependency advisories

Scope: transitive Cargo security fixes from the preserved manual catch-up snapshot.
This batch does not add a product capability or enable account routing cookies.
The manual upstream endpoint remains 595cc91e8cbb1c2ca822d0311dcf12709410c582.
Automation remains paused, including after manual completion.

## Resolved changes

| Package | Baseline | Selected |
| --- | --- | --- |
| anyhow | 1.0.102 | 1.0.103 |
| event-listener | 5.4.1 | 5.4.2 |
| rustls | 0.23.40 | 0.23.45 |
| rustls-webpki | 0.103.13 | 0.103.15 |
| aws-lc-rs | 1.17.0 | 1.18.1 |
| aws-lc-sys | 0.41.0 | 0.45.0 |

Cargo generated the lock using precise updates for the first three packages.
No direct declaration changed. rustls 0.23.45 requires aws-lc-rs 1.18 and
rustls-webpki 0.103.14 or later. All six selected versions match the preserved
snapshot. No new package name entered the graph. aws-lc-sys now uses the existing
pkg-config package; event-listener no longer uses concurrent-queue directly.

## Evidence at 2026-09-25

Baseline source: b98a71e334e756b55f4220fbc1e6a10ed92050d4.
Baseline and final cargo-audit used RustSec database commit
 e2111519ba6d14a5da59a7b2e5c8083ae8a37c01 (1,271 advisories).
Registry, source and artifact checks were observed at 21:09 UTC.

| Check | Status | Evidence |
| --- | --- | --- |
| Baseline advisories | finding | RUSTSEC-2026-0285; unsound RUSTSEC-2026-0190 and RUSTSEC-2026-0221 |
| Final changed-package advisories | checked-no-record | These three findings are absent after the roll |
| Known malicious package records | checked-no-record | Baseline and final Cargo audit against RustSec, including malicious-package advisories |
| Registry identity and artifact integrity | checked-no-record | All six cached crate SHA256 values match Cargo.lock and live crates.io version metadata; none is yanked |
| Source identity | checked-no-record | Embedded VCS commits resolve in dtolnay/anyhow, smol-rs/event-listener, rustls/rustls, rustls/webpki and aws/aws-lc-rs |
| Build behavior | checked-no-record | Inspected changed manifests and build owners before building; details below |
| Artifact signatures | unsupported | The selected Cargo registry workflow verifies checksums; no signed provenance verification is claimed |
| Remaining graph advisories | finding | Four pre-existing unmaintained packages and one pre-existing yanked version remain |

Remaining records: paste 1.0.15 (RUSTSEC-2024-0436), proc-macro-error2 2.0.1
(RUSTSEC-2026-0173), rustybuzz 0.20.1 (RUSTSEC-2026-0206), ttf-parser 0.25.1
(RUSTSEC-2026-0192), and yanked chacha20 0.10.1. This batch does not select or
approve these historical residuals. No Cargo Dependabot PR was open at the check;
the existing site npm PR is outside this scope.

The anyhow, rustls and aws-lc-rs build scripts are unchanged. event-listener and
rustls-webpki have no build script. aws-lc-sys adds system-library discovery through
pkg-config and environment controls, with a bundled-source fallback. This is a
build behavior change. The local stable build selected CC, the aws_lc_0_45_0
symbol prefix and the generated static crypto archive. No host library or toolchain
was installed or reconfigured. Source-commit existence and checksums do not prove
that package behavior is benign.

risk_coverage: complete for the stated scoped public checks and six identities.
risk_delta: improved; three findings removed, historical residuals unchanged.
decision: proceed with the scoped repair; no new or worsened finding was observed.

## Full workspace compatibility

The first full workspace build exposed an existing CLI exhaustiveness error from
Chief request pagination. The high-level client already assembles and verifies the
pages. The CLI now handles an unexpected transport page as a non-success response
in human and JSON output, without printing a partial request as complete.
Available and unavailable result behavior stays the same.

The repository test gate passed with stable Rust: 2,454 tests passed, 64 skipped
(`cargo nextest run --locked --workspace --all-targets --all-features`).
Repository-owned strict Clippy passed for all 12 workspace packages, all targets
and features (`python3 scripts/lint_rust_workspace.py`, stable Rust). This record
does not establish signed desktop, audio or production-provider acceptance.
