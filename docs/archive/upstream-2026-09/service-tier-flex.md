> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Explicit Flex service tier

## Current qualification on 2026-09-26

Installed Codex 0.158.0-alpha.2 now passes the configured-Flex case. The earlier
ordinary fixture expected a null tier and failed on a fresh run because native
thread startup returned `flex`. Replace that stale expectation with the fixed
upstream contract and check the outbound requests, not only the settings receipt.

The fixture sets `fast_mode=false` explicitly. A catalog that does not advertise
Flex still preserves configured Flex at startup, on the first Responses request,
and after native process restart and one explicit continuation. The advertised
per-turn Flex case also passes. Cold settings/history reads do not infer or replay;
each scenario produces exactly two requested turns. The existing four reasoning
inheritance cases pass with the same fixture. Resume uses the current complete
native-settings inheritance contract.

Fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582` defines this behavior in
`protocol/src/openai_models.rs::supports_service_tier` and
`core/src/session/mod.rs::get_service_tier`. Its
`service_tier_for_request_preserves_flex_without_catalog_support` test explicitly
covers an empty service-tier catalog. These sources were read, not executed.
The installed fixture verifies the corresponding native request behavior.

This supersedes the earlier configured-Flex failure below. It does not prove the
old automatic-model-recovery setter path: that path used
`ThreadModelRecoveryUpdate`, which is absent from current delivery. The current
model-only selector intentionally rejects `serviceTier`. Keep that separate
inherited adapter/service scope open; do not widen it merely to make a fixture run.
The original `chief_process_native_flex_tests.rs` row therefore remains open.
Native Bedrock filtering and signed desktop acceptance remain unqualified here.

Classification: native-owned tier semantics, with local preservation of explicit
choices. This batch changes a stale test and evidence, not application policy or
user configuration. All three ordinary native test functions pass on the current
binary. Automations remain paused.

## Preserved alpha.16 review

The following record describes the older tested binary and historical recovery
implementation. Its failure is not a claim about the current configured path.

Upstream `7abf2a3b5cbe08ca875d677dcd027528f9556152` preserves explicit Flex in
session settings and Responses requests even when fast mode is disabled or the
catalog omits Flex. TUI resolution follows this rule. Native Bedrock providers
omit the service tier, including with a custom catalog. Read startup, settings,
outbound request, review, connected-thread and TUI submission tests. The cutoff
retains these branches; later reasoning-effort handling does not replace them.

Decodex already retains Flex in model recovery and forwards explicit message
tiers in `chief.rs::apply_message_options`. It does not construct the native
Responses body. Keep native provider filtering as the authority.

The new installed-runtime qualification uses isolated homes and deterministic
loopback Responses requests with `fast_mode=false`. It separately tests startup
configuration and a native thread-settings update, then cold resume without a
client tier override. Both scenarios fail on `0.155.0-alpha.16.3`: the
settings publication accepts Flex, but the first outbound request omits
`service_tier`. An earlier check also found null in cold-resume settings.
Evidence: `/tmp/decodex-1449-native-flex-matrix.log` records both current failures;
`/tmp/decodex-1449-native-flex-final.log` records the earlier resume failure.
The final matrix stops at first-request failure and does not reach cold resume.
Full runtime Clippy passes in `/tmp/decodex-1449-runtime-lint-final.log`.
Do not treat settings acceptance as
successful request application. The strict qualification remains ignored by
default because it requires an explicit installed binary; its failure is an open
compatibility gap, not a passing regression test.

Re-run both qualification cases against the supported delivery runtime. Native
Bedrock and signed desktop acceptance are not covered by this local fixture.
