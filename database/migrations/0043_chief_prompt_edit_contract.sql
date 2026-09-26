-- Require readers that honor unresolved prompt-edit receipts before admitting input.
-- The receipts use chief_inbox_events. No table or existing product data changes.
-- The migration ledger and user_version prevent an older service from ignoring
-- an unresolved history mutation after a downgrade.
SELECT 1;
