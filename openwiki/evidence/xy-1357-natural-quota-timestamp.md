---
type: "Reference"
title: "XY-1357 natural quota timestamp evidence"
openwiki_generated: true
---

# XY-1357 natural quota timestamp evidence

Status: **pass for the XY-1304 timestamp-precision input only**.

Production routing remains structurally disabled. This evidence does not pass the XY-1304 aggregate gate.

## Authority and scope

The Manager authorized one passive `account/rateLimits/read` on base
`171317f3a4ae7d8c2275a145e334a990d7fc5961`.

The capture used the existing ambient app-server account as `ambient-account-1`.
It did not inspect an account pool or select another account.

The capture sent these protocol messages:

1. `initialize`
2. `initialized`
3. `account/rateLimits/read`

The third message occurred exactly once. The capture sent no turn, prompt, tool, login, account-configuration, or routing request.

## Exact build and method

| Item | Captured value |
| --- | --- |
| Codex CLI | `codex-cli 0.145.0-alpha.18` |
| App-server user agent | `Codex Desktop/0.145.0-alpha.18 (Mac OS 27.0.0; arm64) ghostty/1.3.2-main-_f3c9a2b72 (decodex-xy-1357-evidence; 1)` |
| Exact executable SHA-256 | `f0b214b476e04175bee104fe441caea874baeef3efc3828bfb79e972266156a9` |
| App-server build boundary | The app-server used the same exact executable. |
| Method | `account/rateLimits/read` |
| Transport | JSON-RPC 2.0 newline frames over app-server standard input and output |
| Capture interval | `2026-07-20T22:44:23.929416000Z` to `2026-07-20T22:44:24.911879000Z` |

The generated response schema SHA-256 was
`66be5e51929fa5a229ed2e243e3774d2c7f6a781500b8f0f5486f61e2e7ec58a`.
It declared `RateLimitWindow.resetsAt` as nullable JSON `int64`.

The raw aggregate schema SHA-256 was
`0f3e4ab3c86c4390ed3ce9468459ffcd990c59a77600dd6ade19896a5bcbf276`.
This per-generation raw hash is not a stable build identity.

## Capture and redaction boundary

The capture bounded one UTF-8 response frame to 1 MiB. It did not write the complete frame to disk.

The JSON decoder replaced each numeric parser result with its exact source lexeme.
This step occurred before integer, decimal, or floating-point conversion.

The redactor then constructed a strict allowlist. It retained only build data, method counts, opaque aliases, window duration, reset tokens, and conversion proof.

The redactor discarded raw bucket identifiers and all unrelated response fields.
It retained no account identity, email address, credential, prompt, thread, balance, plan, or usage value.

The checked-in redacted receipt is
[`fixtures/xy-1357-natural-quota-receipt.json`](fixtures/xy-1357-natural-quota-receipt.json).

## Natural representation and exact conversion

The one receipt contained three allowlisted reset observations. The legacy view duplicated one multi-bucket value.

| Source | Alias | Duration | Exact raw JSON token | Lexical precision | Exact UTC Unix microseconds | UTC |
| --- | --- | ---: | ---: | --- | ---: | --- |
| `rateLimits.primary.resetsAt` | `legacy-view` | 10080 minutes | `1785171262` | JSON integer, zero fractional digits | `1785171262000000` | `2026-07-27T16:54:22.000000Z` |
| `rateLimitsByLimitId.*.primary.resetsAt` | `bucket-1` | 10080 minutes | `1785171262` | JSON integer, zero fractional digits | `1785171262000000` | `2026-07-27T16:54:22.000000Z` |
| `rateLimitsByLimitId.*.primary.resetsAt` | `bucket-2` | 10080 minutes | `1785192264` | JSON integer, zero fractional digits | `1785192264000000` | `2026-07-27T22:44:24.000000Z` |

The exact integer arithmetic is:

```text
1785171262 × 1000000 / 1 = 1785171262000000, remainder 0
1785192264 × 1000000 / 1 = 1785192264000000, remainder 0
```

The conversion used no rounding or truncation. Each natural value had whole-second lexical precision.

The Unix-seconds interpretation matches the supported repository consumer and the seven-day window semantics.
Other common epoch units do not produce a future reset for this receipt.

## XY-1304 input

The timestamp-precision result is `exact_microseconds_compatible`.
Current exact-microsecond ingress can represent this observed natural form without an authority amendment.

This result does not authorize ingestion or routing. XY-1304 must retain all other gates without change.

## Limitations

- The receipt covers one ambient account, one app-server process, one build, and one instant.
- The receipt exposed only 10080-minute windows. It did not expose a 300-minute window.
- The receipt does not prove depletion, availability, freshness policy, exclusion, fallback, continuation, or wake behavior.
- The capture did not inspect secret-bearing auth or account files. It therefore did not hash their contents.
- The closed request sequence proves only that this capture issued no protected mutation request.
- The capture cannot exclude unrelated concurrent external changes outside its process authority.
- The complete app-server response was intentionally not retained. Review can verify only the allowlist code and redacted receipt.

## Acceptance disposition

1. The receipt retains every observed allowlisted reset token exactly.
2. Exact integer arithmetic proves UTC Unix microseconds with zero remainder.
3. The evidence records the build, method, scope, read count, redaction, and limitations.
4. The capture sent no mutation, turn, tool, account-management, or routing request.
5. XY-1304 receives a timestamp-only pass. The aggregate gate and production routing remain disabled.
