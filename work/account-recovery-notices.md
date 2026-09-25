# Account recovery notices and explicit actions

This is an optional account-management feature in the current full update. It is
not required merely to keep the core app-server protocol compatible. Keep it a
separate delivery batch for the user's later product review.

## Ownership

The account API decoder accepts recovery copy only when the outer account and user
identities match the authenticated source. The existing account observation service
owns freshness and invalidation. Missing, unsupported, current, and stale notices
remain distinct. A model-specific notice does not block an entire account.

The desktop binds queries and actions to the selected account revision. It receives
observation changes on an independent socket so a long wait cannot block retained
commands or events. Late results from a former selection cannot restore controls.
Stale notice text remains readable, but its actions cannot execute.

The service resolves known actions against fresh account context. Browser actions
validate destinations. Reset usage opens the existing picker and confirmation.
Notification actions require an explicit click, a durable account-command receipt,
and a freshly prepared source before one native request. Unknown outcomes remain
unknown across clients and restart; they are not automatically sent again. The UI
can read prior operation status and requires explicit acknowledgement before a new
attempt after an uncertain result.

The native process and its account authority remain with the existing attested
launch owner. No new credential store or provider authentication path is added.
The local wire contract is 2.74. Existing account-command storage holds receipts;
this batch adds no database migration.

## Reference and scope

Fixed upstream commit: 595cc91e8cbb1c2ca822d0311dcf12709410c582.
Relevant owners: tui/src/backend_banners.rs and backend_banners/actions.rs.
Installed codex-cli 0.155.0-alpha.16.4 was checked independently. Its generated
experimental schema exposes account/sendAddCreditsNudgeEmail with creditType
credits or usage_limit. Schema output: /tmp/decodex-account-recovery-schema.

This batch migrates inherited account recovery presentation and explicit actions.
Automatic model fallback, account analytics, and unrelated model-review changes
remain separate. No notification is sent merely because a banner is received.
Tests use synthetic identities and loopback servers; they do not prove a real
workspace owner received an email or that a purchase/reset occurred.

## Isolated native qualification

The native fixture must use a new private directory below the OS account home,
outside any `.codex` tree. Set both `HOME` and `DECODEX_TEST_ACCOUNT_HOME` to that
directory, create the explicit fixture marker, and use the installed binary through
`DECODEX_TEST_CODEX_BINARY`. A system temporary directory fails the production
working-directory policy. A `.codex` descendant fails the product-root policy.
Neither policy is relaxed for the test.

The isolated native test covers one notification for concurrent same-key commands,
source invalidation before send, same-UID command and event transport, status reads,
and receipt replay after the store reopens without another provider request.
The provider is a loopback fixture with synthetic credentials.

Final batch validation: protocol 133, adapter 182, database 116, runtime 559 and
GPUI 458 tests passed; 50 opt-in tests were skipped. The explicit installed-native
notification test passed separately. Strict all-target/all-feature Clippy passed
for all five affected packages. No real provider notification was sent.
