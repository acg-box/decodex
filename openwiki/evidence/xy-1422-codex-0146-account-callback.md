---
type: "Reference"
title: "Codex 0.146.0-alpha.9.2 account-callback receipt"
openwiki_generated: true
---

# Codex 0.146.0-alpha.9.2 account-callback receipt

Date: 2026-08-02

This receipt binds the current macOS account-launch profile to one exact installed Codex image and
its generated app-server schema. It does not authorize another Codex build. The upstream source
observation and the release artifact are separate facts; this receipt does not claim that the
observed upstream head is byte-identical to the release.

## Source and release

| Fact | Accepted value |
| --- | --- |
| Upstream repository | `openai/codex` |
| Observed upstream head | [`5157493c23713ac12034cf250ffb0a8ce0670277`](https://github.com/openai/codex/commit/5157493c23713ac12034cf250ffb0a8ce0670277) |
| Release | [`rust-v0.146.0-alpha.9.2`](https://github.com/openai/codex/releases/tag/rust-v0.146.0-alpha.9.2) |
| Release publication time | `2026-07-29T23:53:10Z` |
| Platform | macOS arm64 |
| Installed path | `/Applications/ChatGPT.app/Contents/Resources/codex` |
| Codex version | `codex-cli 0.146.0-alpha.9.2` |
| Installed executable SHA-256 | `d96ae1ca1ff6fc8587842fa04c92d3ee4d31651a811c2f89b65fcfd9c28473e2` |
| Release archive SHA-256 | `dc578db9698a8f76be3d576fa6f5fa7008e1fd329582c44fde2c82db4d5d27b5` |
| Extracted release executable SHA-256 | `a2795588f2492f8839bc03c3f6ffc0d4ac2950812ae8c66800122db584a8af04` |
| Mach-O UUID | `6B87DCFB-91F9-3249-8B01-56255808E271` |
| Unsigned payload size | `268490400` bytes |
| Unsigned payload SHA-256 | `eca7b04fcad0d9102eae75dc3cb74974c096cc394df65a1bab9a68343ccecfeb` |
| Code-signing team | `2DC432GLL2` |
| Installed signing timestamp | 2026-07-31 |

The reviewed range from `9949245d1d2b4a39a6f1841922322f767fa146ad` through the observed head contains
one protocol change. It adds an optional `onboardingEntrypoint` field to the
`account/login/completed` notification. Decodex does not consume that notification, and the field
is not present in the generated schema of the installed image accepted by this receipt. No current
runtime behavior changes. A future installed image with the upstream field will change the pinned
`ServerNotification.json` and aggregate schema digests, so the exact-image gate will reject that
image until a separate adaptation accepts it.

The GitHub release executable and the installed executable have different full-file SHA-256
values. Both have the same Mach-O UUID. After signatures were removed from private copies, the
two copies were byte-identical at the size and SHA-256 recorded above. This evidence supports an
OpenAI re-signing explanation for the full-file difference. It proves matching unsigned payloads;
it does not claim that the signed files are identical.

## Generated schema

The operator generated the schema from the installed executable with:

```sh
codex app-server generate-json-schema --experimental --out ABSOLUTE_PRIVATE_DIRECTORY
```

The owner-private directory had mode `0700`, contained 349 regular files with no symlinks, and
used 4,328 KiB. The file count remains below the fixed 512-file safety limit.

| Schema file | Canonical SHA-256 |
| --- | --- |
| `ClientRequest.json` | `6ffc593d603d21a051840539a4dbfad95cad2e7fec315e252b6722bd71bf37b4` |
| `ServerNotification.json` | `abbb54060ea6a6005e63267bc6996eacd70cbb7954a7e0d61f50ea02af4acf02` |
| `codex_app_server_protocol.v2.schemas.json` | `e554a74bd59d38d16acb1744750b2999156ee3d65d0fe906b22ab52edf17fbbc` |
| `ServerRequest.json` | `6455b23a65fa3d9c7749ecd2ecbc4b829c9039f6cd8f9adc44d86ad4522e37ec` |
| `v2/LoginAccountParams.json` | `3bec7003eb85aabbeaf0ba8a22ec54b68ec26d2657d6878a31ca0d01dfe642e0` |
| `ChatgptAuthTokensRefreshParams.json` | `74d490082dab616ac01c94d388c9a836304c96092db37290cfdd10a46b0f3ef9` |
| `ChatgptAuthTokensRefreshResponse.json` | `ff76f5cc58bff40216f9d5f3c5be921268059f6d66d6c034970cddf0e08f0ced` |
| `v2/ThreadReadResponse.json` | `94689cd705b4936a5c361deaa51fed69101eaba0629899ef8a39b600180de9b3` |
| `v2/ThreadStartParams.json` | `001c07a58981df5d860335bf8cee4d336df2165db6dc9c645cefed0467ccebbe` |
| Callback profile SHA-256 | `64a98c3328d1eba74aaf18a3995523e07fd2f1395bc6fb4a121b74338c404a29` |

`GeneratedSchemaEvidence::load` independently reproduced the callback profile from these files.
The exact account refresh method remains `account/chatgptAuthTokens/refresh`, with root
`ChatgptAuthTokensRefreshParams.json` and `ChatgptAuthTokensRefreshResponse.json` request and
response schemas.

## Failure and repair

Before this adaptation, the exact ignored live read-only probe failed with `SchemaMissing` for the
three checked-in marker digests. After the digest update, an unfiltered `thread/list` response
included the new `preview` field and JSON escapes. The zero-scratch safety gate rejected that frame
before typed decoding. A direct structural check showed that bounded `thread/list` with the fixed
nonmatching search term returned zero rows in a 71-byte response with no backslash bytes.

The exact schema advertises `thread/search`, but a direct bounded check received no response within
20 seconds. The live foundation probe therefore does not invoke that method. It records
`Capability::ThreadSearch` as `Unavailable(NotProbed)` unless separate live evidence exists. This
change does not weaken frame validation or a timeout.

The first acceptance run after this repair failed closed during cold preflight with
`Supervision(PreflightFailed)`. The same unmodified command then passed in 35.28 seconds. It
negotiated the installed image and used only schema generation, version inspection, and read-only
RPCs; it did not dispatch a turn. A final repeat run with all nine file digests pinned also passed
in 34.67 seconds:

```sh
cargo test --locked -p decodex-runtime account_launch::process::tests::live_read_only_probe_negotiates_without_dispatch -- --ignored --exact --nocapture
```

The V32 migration and current source profile remove support for the prior `alpha.3.1` image.
