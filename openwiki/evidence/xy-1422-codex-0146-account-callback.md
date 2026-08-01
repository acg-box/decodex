# Codex 0.146 account-callback receipt

Date: 2026-07-29

This receipt binds the 2026-07-29 macOS account-launch profile to one exact Codex image and its
generated app-server schema. It does not authorize another Codex build.

| Fact | Accepted value |
| --- | --- |
| Platform | macOS arm64 |
| Codex version | `codex-cli 0.146.0-alpha.3.1` |
| Executable SHA-256 | `fa0cb7c5f80e6a192563fcb1d9f98857f4a808a28cb29289400ed7110291bce4` |
| Release archive SHA-256 | `147297da351dc408e4f1e7f9d9c4d96873f4da70c13af6d5416d3c5e1cef4cd4` |
| Code-signing team | `2DC432GLL2` |
| `ClientRequest.json` | `ee9fcbf5c0b3af8526dea54d3c1c7a6ca480f0847b049b9b7d4cde00ddd82735` |
| `ServerNotification.json` | `189dc3b9bf8e96a115cf1102e60c379d8e34382ddca2868d1b2b46847d122166` |
| Aggregate v2 schema | `2ad5e818b870a6a26387678bbe276e4c67b3b078f6ac03143fba623b0969605d` |
| Account refresh method | `account/chatgptAuthTokens/refresh` |
| Refresh request schema | root `ChatgptAuthTokensRefreshParams.json` |
| Refresh response schema | root `ChatgptAuthTokensRefreshResponse.json` |
| Callback profile SHA-256 | `918c879482dd6b6732335b05a2b208b518467b42ae827fd08d01a45f2c907587` |

The operator generated the schema with:

```sh
codex app-server generate-json-schema --experimental --out ABSOLUTE_PRIVATE_DIRECTORY
```

The generated directory contained 347 regular schema files. `GeneratedSchemaEvidence::load`
reproduced the recorded schema digests and callback profile for this exact replacement image.
The final local-service installation must pass the daemon callback preflight and report the
credential vault ready.

The accepted executable is the signed `codex-aarch64-apple-darwin` binary from the
OpenAI Codex [`rust-v0.146.0-alpha.3.1` release](https://github.com/openai/codex/releases/tag/rust-v0.146.0-alpha.3.1).
