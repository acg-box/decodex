> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Reconcile policy and workspace-routing records

Read the complete preserved login-method, managed-provider and workspace-routing
notes. Restore their original text under explicit historical headings, with a
current classification before each record. Their old implementation status,
binary versions and migration numbering are not current acceptance evidence.

| Capability | Current source and classification | Remaining boundary |
| --- | --- | --- |
| Native login restrictions | Native `configRequirements/read` and native login enforcement are qualified. Restored fixtures verify running policy and cold restart. | `account_login.rs::run_login_session` still calls the independent browser/device-code login engine. No current production consumer of `allowedLoginMethods` was found in that path. Enrollment-policy applicability and authority remain unresolved; do not claim integration. |
| Managed-provider refusal | Core. Chief and ordinary conversations classify the exact native refusal and preserve known-unsent input through existing transactions. Prior injection or uncertain transport remains unknown and is not replayed. | Native managed policy delivery and signed desktop recovery are separate acceptance. The old migration 40/42 description maps to current migration 38; see [database reconciliation](database-owner-reconciliation.md). |
| Native workspace model routing | Core native compatibility. Native Codex owns discovery and model transport. Account APIs keep their account backend; model discovery does not redirect all account traffic. | Older discovery probes do not prove all production transport or desktop paths. |
| Automatic quota activation | Optional consumer with required correctness if retained. `account_launch/activation_policy.rs` obtains exact-account policy from an attested native child. `account_api/activation.rs` retains the credential revision and lock, applies origin/routing/residency policy and sends an empty-tools, `store:false` request. | No real quota activation or signed desktop flow is claimed. The native policy probe builds the routed request; it does not send content to that origin. |

The login note remains an explicit unimplemented integration question in the
[adoption register](upstream-adoption-review.md). Restoring and classifying that
document does not resolve the product decision or authorize a new global policy
owner. In particular, an arbitrary active task process cannot supply policy for
independent account enrollment.

## Enrollment and execution are separate policy boundaries

A further fixed-source review resolves which entry points the native policy owns.
At upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582`,
`app-server/src/request_processors/config_processor.rs` publishes the current
AuthManager allowed methods. `login/src/auth/manager.rs` computes them from
managed policy, forced login method and the effective workspace. The values are
API and ChatGPT authentication categories, not browser versus device-code choices.

In `app-server/src/request_processors/account_processor.rs`, both
login_chatgpt_common and login_chatgpt_auth_tokens_response reject disallowed
ChatGPT authentication. The latter also checks permitted workspaces. Decodex's
`account_launch/process.rs::CredentialProjection::authenticate_chatgpt` calls
that native account/login/start path and maps rejection to ProjectionRejected.
Independent enrollment therefore does not itself bypass native process admission.

The installed-native restriction test was run again against the unchanged binary
hash below. One test passed, with zero failures or skips. It checks API-only and
ChatGPT-only policies, running-policy stability after a file edit, rejection of
the prohibited authentication category, and zero model/provider requests on those
rejections. Log: `/tmp/decodex-final-login-policy-boundary.log`. This uses only
synthetic tokens and local disposable fixtures.

The separate `AccountLoginManager::run_login_session` creates a temporary login
home, runs decodex-account-login, and installs the result through AccountService.
It still has no native policy observation bound to an enrollment authority.
There is no current global enrollment-policy integration to claim. Adding one
requires an explicit owner and lifecycle for that authority, including managed
policy, restart and concurrent clients; reading an arbitrary task's policy is
incorrect. Keep that unimplemented integration visible for the user's scope
review. No local policy is relaxed, no live account is enrolled, and no new global
policy owner is introduced by this classification.

## Fresh routing evidence

Eight activation and policy unit tests pass with no failures. Two opt-in cases
are skipped in that run. The log is `/tmp/decodex-policy-routing-unit.log`.
They cover independent routing/residency headers, changed or incomplete policy,
unsafe origins, tool-free/non-persistent HTTP payloads, positive completion,
redirect refusal and no replay after ambiguous failure.

Run the installed-native policy case separately: it passes with no skips in
33.21 seconds on Codex `0.158.0-alpha.2.1`. The binary SHA-256 is
`3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a`.
The log is `/tmp/decodex-policy-routing-native.log`.
It uses production executable/schema attestation, an isolated home and synthetic
ephemeral authentication. It checks selected workspace, invalid-discovery refusal,
fresh cold lookup, expected origin/routing header and absence of `auth.json`.
It sends no model request and uses no real credential.

The current refusal cases are covered by the recent complete Chief suite; their
source and historical migration mapping were separately reconciled. This batch
does not reclassify old native managed-policy probes as fresh results.

Close the three document-reconciliation rows after verifying that each original
record is retained exactly after its new status notice. Product gaps and final
acceptance remain open in the adoption register. No production source, native
configuration or user database changes. Automations remain paused.
