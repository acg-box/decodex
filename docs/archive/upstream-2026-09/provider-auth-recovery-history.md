> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Retired provider authentication recovery history

The user retired O24 on 2026-09-27. Decodex no longer consumes provider recovery
notifications or writes new provider recovery receipts. The dedicated database
writer and its ownership-only tests are removed. Native authentication, credential
refresh, account routing and ordinary sign-in diagnostics are unchanged.

Existing receipts remain readable in saved history and the timeline. A focused
regression verifies that new notifications create no receipts or outgoing work,
while a pre-retirement receipt stays readable after the store is reopened. No
user data or schema is removed. Do not restore this optional consumer during
upstream maintenance without a new user decision.

## Historical implementation record

The following describes the implementation before retirement, not current scope.


## Applicability and removal choice

This is an optional historical notice surface. At fixed upstream commit
`595cc91e8cbb1c2ca822d0311dcf12709410c582`, the provider trait defaults to no
recovery messages. The only production override is Amazon Bedrock, and it emits
these messages only when AWS authentication recovery is enabled. This is not a
general ChatGPT sign-in or account-reset implementation. If Bedrock is outside
Decodex's intended use, this surface is a removal candidate for the user's final
review.

Native Codex retains authentication, credential refresh and retry authority.
The optional local pieces are the two notification cases, the resolved history
receipt writer/export, and the history label/rendering. New observation and writes
can be removed while retaining native authentication and account routing. Keep
read-only rendering for saved receipts unless the user also retires that data.
There is no schema migration or configuration toggle in this restoration.

## Behavior

Record only the exact running thread/turn under its current ready process owner.
Retain the provider, message, connection and generation/account association as
historical facts. Multiple notifications have no native attempt ID, so append
separate receipts rather than claim that each receipt is a distinct attempt.
Ignore foreign, stale, unready or dead owners and malformed bounded fields.

A completed notification means Codex reported recovery at that time. The visible
text explicitly says that the saved event does not confirm current sign-in
status. Receipts do not complete work, wake it, retry input or create a login
command. A start followed by a disconnect remains a start after reopen.

## Provenance and validation

The three source/test files already existed in pre-scan baseline
`2ffa385c3b49efe6a4109de0fd7353fb64abd2c5` and retained PR1378 commit
`4e370c07464ea3528ed1334fd6ce75fcc5ca595a`, with identical bytes between those
commits. They are outside the 360-row changed-file snapshot. Restore the database
implementation and ownership test exactly; adapt the runtime test to the existing
history renderer and read through the public transcript store path. This is
baseline recovery, not another newly adopted feature from the 1,569-commit scan.

The regression first found zero receipts for three native notifications. After
restoration, receipt/history readback survives reopen unchanged and produces no
outgoing request, pending work or wake. The database test rejects unready,
foreign, missing-generation and dead owners across rotation and restart, while
retaining the exact account/generation association. The installed Codex
0.158.0-alpha.2 schema contains both notifications. These are synthetic transport
and storage tests. A GPUI render regression verifies that the saved and native
conversation views both show a recorded provider notice. Provider text stays
plain text, including link-shaped content. These checks do not prove a live AWS
credential refresh or signed desktop acceptance.

Upstream owners: `model-provider/src/provider.rs`,
`model-provider/src/amazon_bedrock/mod.rs`, `core/src/client.rs`, and
`app-server/src/bespoke_event_handling.rs` under `codex-rs/`.
