-- XY-1402 enum expansion committed before the transactional V26 authority cutover.
--
-- PostgreSQL cannot use a value added to an existing enum until the transaction that adds the
-- value commits. Refinery runs each migration in its own transaction. V25 therefore changes only
-- enum vocabulary. V26 performs the coordinated cutover and is the only new relation/function
-- authority writer.

ALTER TYPE decodex.managed_run_wait_reason ADD VALUE IF NOT EXISTS 'reconciliation';
ALTER TYPE decodex.managed_run_wait_reason ADD VALUE IF NOT EXISTS 'reviewer_ambiguous';

ALTER TYPE decodex.routing_blocker ADD VALUE IF NOT EXISTS 'authentication_required';
ALTER TYPE decodex.routing_blocker ADD VALUE IF NOT EXISTS 'plugin_unready';
ALTER TYPE decodex.routing_blocker ADD VALUE IF NOT EXISTS 'dependency_blocked';
ALTER TYPE decodex.routing_blocker ADD VALUE IF NOT EXISTS 'approval_required';
ALTER TYPE decodex.routing_blocker ADD VALUE IF NOT EXISTS 'user_required';
ALTER TYPE decodex.routing_blocker ADD VALUE IF NOT EXISTS 'external_blocked';
ALTER TYPE decodex.routing_blocker ADD VALUE IF NOT EXISTS 'usage_unproven';
ALTER TYPE decodex.routing_blocker ADD VALUE IF NOT EXISTS 'reconciliation_unproven';
ALTER TYPE decodex.routing_blocker ADD VALUE IF NOT EXISTS 'reviewer_unavailable';
ALTER TYPE decodex.routing_blocker ADD VALUE IF NOT EXISTS 'reviewer_failed';
ALTER TYPE decodex.routing_blocker ADD VALUE IF NOT EXISTS 'reviewer_ambiguous';
ALTER TYPE decodex.routing_blocker ADD VALUE IF NOT EXISTS 'process_generation_unresolved';
ALTER TYPE decodex.routing_blocker ADD VALUE IF NOT EXISTS 'process_generation_unavailable';
ALTER TYPE decodex.routing_blocker ADD VALUE IF NOT EXISTS 'provider_attempt_unresolved';
ALTER TYPE decodex.routing_blocker ADD VALUE IF NOT EXISTS 'provider_attempt_completed';
ALTER TYPE decodex.routing_decision_kind ADD VALUE IF NOT EXISTS 'waiting_reconciliation';
