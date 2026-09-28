> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Native Flex restart qualification

Restore both inherited tests in `chief_process_native_flex_tests.rs`. Keep every
assertion and adapt only the shared Responses fixture helper. This is a native
compatibility check for an explicitly selected service tier, not a new automatic
product policy.

The installed executable is `codex-cli 0.158.0-alpha.2.1`, SHA-256
`3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a`.
With `fast_mode=false`, the actual native tests produce these results:

| Source of Flex | Live request | Cold resume | Result |
| --- | --- | --- | --- |
| `service_tier="flex"` in the isolated config | Flex | Flex; second request remains Flex | Pass |
| Explicit `thread/settings/update` | Publication and first request use Flex | `serviceTier` is null instead of Flex | Fail |

The second row fails before the second explicit turn. Do not infer its outbound
tier after restart. The fixture closes the retained bridge, kills and waits for
the native process, then starts a new native process. This proves the specified
restart case; it does not establish graceful-shutdown behavior or the exact cause.
Both tests use disposable homes and loopback inference fixtures without account
credentials. Native children are cleaned up on assertion failure.

The fixed upstream commit is
`595cc91e8cbb1c2ca822d0311dcf12709410c582`. Its
`protocol/src/openai_models.rs` explicitly preserves Flex without catalog support.
Its `app-server/src/request_processors/persisted_resume_settings.rs` restores
permission facts, and `thread_processor.rs::merge_persisted_resume_metadata`
restores model/provider/effort. Neither of those helpers restores a tier. This
source evidence is consistent with the observed limit; it does not prove the
cause in the installed binary or a promise that the inherited expectation holds.

The native run reports one pass and one failure. Preserve the failure log at
`/tmp/decodex-native-flex-restored.log`. Strict runtime Clippy passes with all
features and targets; log `/tmp/decodex-native-flex-clippy.log`. The tests retain
their explicit-binary opt-in requirement. Ordinary CI does not qualify them.

The file's source comparison is now closed in [the preservation review](source-preservation-native-limits.md). Native qualification remains failed. The passing standard-tier fallback
fixture does not qualify explicit Flex across restart. Do not replay a settings
write or change global config to hide this native limit. No production code,
installed binary or maintenance automation changes in this batch.
