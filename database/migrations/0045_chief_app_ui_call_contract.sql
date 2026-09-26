-- Require readers that retain App UI call uncertainty across restart.
-- Attempts and results use immutable inbox events and complete event payloads.
-- No table or existing data changes. The version prevents an older service from
-- silently opening a database with tool-call receipts it does not understand.
SELECT 1;
