---
type: Integration
title: "Elephant–Goldfish component"
description: "Install, update and remove Google's original EGM plugin through native Codex, then use its design and independent review skills in a fresh conversation."
tags: ["components", "egm", "plugins", "review"]
sources:
  - id: openwiki-source-59ed67d26d1c310707a365e5
    resource: repo://apps/decodex-gpui/src/component_plugins.rs
  - id: openwiki-source-0813b2e35e2133631c0aa4db
    resource: repo://apps/decodex-gpui/src/settings_components.rs
  - id: openwiki-source-fac913afc0144f630680095f
    resource: repo://crates/decodex-components/src/lib.rs
  - id: openwiki-source-e379b8a714d71f87371ae328
    resource: repo://crates/decodex-components/src/plugins.rs
  - id: openwiki-source-9976259de97569ef11093ba6
    resource: repo://crates/decodex-runtime/src/account_launch/process_native_component_tests.rs
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-acd129fd3a88516b785fc929
    resource: repo://crates/decodex-runtime/src/agent/instructions.md
generated: { by: "codex", at: "2026-10-28T05:00:14.298Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-28T05:00:14.298Z
---

# Elephant–Goldfish component

EGM is an optional [Google plugin](https://github.com/google/elephant-goldfish-model). Its skills guide design work and coordinate fresh-context Goldfish reviewers with the persistent Elephant conversation. Decodex installs the original package through Codex. It does not replace the skills with local prompts or add a separate review engine.

## Install and use

1. Open **Settings > Components**. The **This Mac** section manages local components, including when the selected service profile is remote.
2. Let Codex finish preparation if its required runtime is missing. Choose **Install** for **Elephant–Goldfish (EGM)** after its native state loads.
3. Start a new conversation. Select an enabled EGM skill from the native skill inventory for the intended task. Use the discovered name and absolute path. Native names can include a plugin prefix, such as `egm:egm-review-loop`; do not invent a cache path.

Use the original brainstorm, drafting, review or resolution skill that fits the task. The plugin's [Codex integration guide](https://github.com/google/elephant-goldfish-model/blob/main/docs/codex.md) defines resource lookup, fresh reviewers and human decision gates. Installing a component does not start a review, assign a Project Lead, create an Aide role or schedule recurring work. Existing conversation context remains under native Codex ownership. Start a new conversation after installation or update to load the current plugin.

## Lead selection

New Lead conversations receive concise selection principles through the existing Agent instructions. Use EGM when independent review can resolve important design assumptions, competing approaches or cross-component contracts. The Lead chooses the smallest useful review; simple explanations and local fixes need no EGM phase. User-selected skills retain the existing native skill input path.

The Lead reads the original skill and necessary resources, gives Goldfish the current artifact and bounded materials in fresh context, waits for actual findings, and preserves the original human decision gates. A skill read or reviewer launch is not a completed review. The guidance respects current native invocation policy and does not install or modify an unavailable plugin. Existing Main conversations keep their native context; this change does not hot-replace their instructions or migrate their history.

## Lifecycle and owners

| Control | Effect |
| --- | --- |
| Refresh | Read native installed version and enabled state. |
| Install / Enable | Add the original Google marketplace and install or enable the whole `egm@egm` plugin. |
| Update | Refresh its Git marketplace, then reinstall the plugin. A marketplace refresh alone does not update the installed payload. |
| Uninstall | Remove the plugin. Remove its marketplace when no other plugin from that marketplace is installed. Preserve user work and reports. |
| Clean up | When Codex no longer lists EGM, run the same exact native uninstall to remove files left after an external marketplace removal. Repeated cleanup is safe. |

Codex owns marketplace configuration, package materialization, the plugin cache and enabled state. `NativePlugins` is a narrow CLI adapter. `PluginOperations` runs an action outside the UI thread, coalesces repeated input while busy and reads native state after failure as well as success. An action error remains visible beside the observed state. Decodex adds no second installed-version database or plugin download cache.

The displayed version and enabled flag report native configuration. They do not prove that all installed files are intact. A missing marketplace can leave cache and configuration behind; an empty catalog is not proof of cleanup. Removal therefore calls native uninstall even when the marketplace is absent, retaining unrelated plugins and user work.

The Codex runtime has a separate required lifecycle. It follows the reviewed runtime requirement embedded in this app build. **Verify** checks or repairs that requirement; the active app offers no runtime uninstall. Sparkle application updates remain in **Settings > Updates**. See [runtime architecture](../architecture/runtime-architecture.md#components-and-app-updates).

General plugin and connection setup remains native. The generic Agent app-server bridge still rejects plugin-management methods. The explicit Component controls use the native CLI and do not reopen that bridge.

## Isolated qualification

The focused fixtures cover these different boundaries:

- An empty component store prepares the qualified runtime, then the owned service starts. Failure and retry preserve the connection retry budget.
- The native plugin adapter tests installation, same-version payload updates, failed-update preservation and removal. It also covers incomplete orphan installations, repeated cleanup and preservation of unrelated files and another installed plugin in a shared marketplace.
- The optional-component worker exercises the original Google Git install, update and uninstall path in a private native home.
- The opt-in native EGM fixture discovers the original skill suite, compares installed skills, personas, standards and documentation with the marketplace source, and uses the existing app-server client for real model turns. It checks a fresh-context probe and one original review round, native child completion and successful source reads, the report artifact and the unchanged target. It stops at the resolution decision gate.

The live fixture reads the existing native subscription through the protected credential reader. It projects access only into the test process, uses no refresh token and asserts that the private home contains no persisted auth file. Component setup needs no new API key. Live review qualification consumes subscription model quota.

This fixture does not qualify every EGM workflow, nested synthesizer dispatch, user-driven repair loops, long-running quality or an installed Decodex application. Keep these boundaries separate. Run the [focused commands](../operations/commands-and-validation.md#qualify-components-and-original-egm) with the qualified runtime and an explicit private evidence directory. The pinned Codex source and generated protocol remain separate from the installed desktop binary.

See [tools and integrations](tools-plugins-and-apps.md) for native skill selection and [runtime architecture](../architecture/runtime-architecture.md) for execution ownership.
