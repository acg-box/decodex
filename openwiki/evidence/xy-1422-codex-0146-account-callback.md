# Codex 0.146 account-callback receipt

Date: 2026-07-28

This receipt binds the current macOS account-launch profile to one exact Codex image and its
generated app-server schema. It does not authorize another Codex build.

| Fact | Accepted value |
| --- | --- |
| Platform | macOS arm64 |
| Codex version | `codex-cli 0.146.0-alpha.3.1` |
| Executable SHA-256 | `6d8be49e49751554df16572369e636cbe02c84b208cad3dc35528c846eeca223` |
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

The generated directory contained 347 regular schema files. Canonical JSON hashing reproduced
all three checked-in digests. `cargo test -p decodex-codex --all-targets --all-features` passed
47 tests, and the macOS attested-spawn test group passed 8 tests. The final local-service
installation must still pass the daemon callback preflight and report the credential vault ready
before account import.

The upstream source for this profile is the OpenAI Codex tag
[`rust-v0.146.0-alpha.3.1`](https://github.com/openai/codex/tree/rust-v0.146.0-alpha.3.1).
