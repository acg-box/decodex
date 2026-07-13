CREATE INDEX outbox_claim_idx
	ON decodex.outbox (available_at, id)
	WHERE state IN ('pending', 'in_flight');

CREATE INDEX activity_timeline_idx
	ON decodex.activity (aggregate_kind, aggregate_id, sequence DESC);

CREATE INDEX quota_windows_observation_idx
	ON decodex.quota_windows (account_id, observed_at DESC);
