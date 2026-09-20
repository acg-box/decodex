CREATE TABLE reset_card_operations (
    idempotency_key TEXT PRIMARY KEY NOT NULL,
    account_id TEXT NOT NULL,
    account_revision INTEGER NOT NULL CHECK (account_revision > 0),
    granted_at INTEGER NOT NULL CHECK (granted_at >= 0),
    expires_at INTEGER NOT NULL CHECK (expires_at > granted_at),
    exact_credit_id TEXT,
    state TEXT NOT NULL CHECK (state IN ('prepared', 'sending', 'completed', 'failed')),
    outcome TEXT CHECK (outcome IN ('reset', 'nothing_to_reset', 'no_credit', 'already_redeemed')),
    failure TEXT CHECK (failure IN ('account_changed', 'inventory_changed', 'provider_unavailable')),
    CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    CHECK (exact_credit_id IS NULL OR length(exact_credit_id) BETWEEN 1 AND 1024),
    CHECK ((state IN ('prepared', 'sending') AND exact_credit_id IS NOT NULL AND failure IS NULL AND (state = 'sending' OR outcome IS NULL))
        OR (state = 'completed' AND exact_credit_id IS NULL AND outcome IS NOT NULL AND failure IS NULL)
        OR (state = 'failed' AND exact_credit_id IS NULL AND outcome IS NULL AND failure IS NOT NULL))
) STRICT;
CREATE UNIQUE INDEX reset_card_active_account ON reset_card_operations(account_id)
    WHERE state IN ('prepared', 'sending');
