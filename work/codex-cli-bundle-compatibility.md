# Signed Codex CLI bundle compatibility

Classification: core compatibility for the existing native process owner.

The installed Codex changed from a standalone binary to
`ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex`.
The observed version is `0.158.0-alpha.2`. The fixed upstream review endpoint
remains `595cc91e8cbb1c2ca822d0311dcf12709410c582`; this change repairs admission
of the installed executable and does not expand the feature scan.

## Failure and repair

A bare copy of this binary fails strict signature validation because its signed
Info.plist is absent. The original app bundle passes the same codesign check.
The former native profile failed with `ExecutableUnavailable` before any recap
request or model inference.

The existing executable snapshot owner now retains the surrounding CLI bundle
context when the executable is in an app's `Contents/MacOS` directory. Other
executables keep the standalone snapshot path. The main image still uses the
existing bounded copy, digest and immutable-file mechanism. Bundle context has
separate limits of 16 MiB, 1,024 entries and depth 32. Links must be relative and
resolve inside the source bundle. Special files and incomplete copies fail.

Security.framework reports the enclosing bundle as its code path. Static and
dynamic checks now use `kSecCodeInfoMainExecutable`, which the installed Apple
SDK documents as the main executable URL for both standalone files and bundles.
The URL must resolve to the exact expected executable. This preserves the path
check instead of accepting any executable under a matching bundle directory.
Strict signature validation, all-architecture identity comparison, exact CDHash
requirements and suspended-child verification remain active. The original
canonical executable remains the execution target. The snapshot is reference
material; no installed file is modified or re-signed.

## Verification

Synthetic signed-bundle tests verify snapshot admission, independent retained
metadata, rejection after source Info.plist modification, and rejection of an
external resource link. An explicit installed-binary test verifies the exact
snapshot identity of the observed Codex app. Existing standalone process and
macOS suspended-spawn tests remain part of the runtime suite.

The runtime suite passed 637 tests with 52 skipped. Strict runtime Clippy passed
for all targets and features. The installed snapshot identity test passed on
0.158.0-alpha.2. The separate public recap fixture passed native profile admission
and then reached the existing account quota gate; full recap acceptance belongs
to its own test batch.
