# Refresh open history after conversation recovery

Classification: preserve the existing optional ordinary History consumer.

The service can reconcile an uncertain ordinary turn during an explicit refresh.
`execute_control_conversation` then publishes `ConversationChanged` with the
recovered summary. The client had stopped reloading history for that event, so
an already open page could retain its old contents after durable recovery.

Restore `ConversationChanged` alongside `ConversationTurnFinished` in the
existing history reload branch. Keep the added `ConversationHistoryChanged`
branch for native status and warning updates. All three use `reload_if_open`;
an event for a different conversation does not replace or reload the selected
page. The normal event validation runs before this routing.

The lifecycle fixture supplies a valid snapshot and then a changed-conversation
or history-change event through the transport. It checks selected and foreign
conversation identities. Before the fix, the selected changed-conversation case
failed because the view generation did not advance. The fix restores that case
while retaining the other cases. No command, native turn, navigation control or
new history owner is added.

The complete inherited `client_lifecycle.rs` differs only by retaining the extra
explicit history-change event in a match. All original reload cases are present.
This file reconciliation does not establish signed desktop or live-provider
acceptance.
