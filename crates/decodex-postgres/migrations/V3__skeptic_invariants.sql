CREATE OR REPLACE FUNCTION decodex.has_credential_material(value text)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
STRICT
AS $$
	SELECT regexp_replace(value, '[^[:graph:]]+', ' ', 'g') ~* '(^|[[:space:][:punct:]])(bearer[[:space:]]+[[:alnum:]_.~+/-]{8,}|basic[[:space:]]+[[:alnum:]+/]{8,}={0,2})|(^|[^[:alnum:]])(sk-[[:alnum:]_-]{8,}|(sk|pk|rk)_(live|test|proj)?[[:alnum:]_-]{8,}|xox[baprs]-[[:alnum:]-]{8,}|glpat-[[:alnum:]_-]{8,}|npm_[[:alnum:]]{8,})|gh[pousr]_[[:alnum:]]{20,}|eyj[[:alnum:]_-]{8,}\.[[:alnum:]_-]{8,}\.[[:alnum:]_-]{8,}|-----begin[^-]*private[[:space:]]+key-----|(password|passphrase|secret|token|authorization)[[:space:]]*[:=][[:space:]]*[^[:space:]]{4,}|[a-z][a-z0-9+.-]*://[^/:[:space:]]+:[^@[:space:]]+@|akia[0-9a-z]{16}'
$$;

CREATE FUNCTION decodex.is_meaningful_evidence(document jsonb)
RETURNS boolean
LANGUAGE plpgsql
IMMUTABLE
STRICT
AS $$
DECLARE
	entry jsonb;
BEGIN
	CASE jsonb_typeof(document)
		WHEN 'object' THEN
			FOR entry IN SELECT value FROM jsonb_each(document)
			LOOP
				IF decodex.is_meaningful_evidence(entry) THEN
					RETURN true;
				END IF;
			END LOOP;
		WHEN 'array' THEN
			FOR entry IN SELECT value FROM jsonb_array_elements(document)
			LOOP
				IF decodex.is_meaningful_evidence(entry) THEN
					RETURN true;
				END IF;
			END LOOP;
		WHEN 'string' THEN
			RETURN length(regexp_replace(document #>> '{}', '[[:space:]]', '', 'g')) > 0;
		WHEN 'number', 'boolean' THEN
			RETURN true;
		ELSE
			NULL;
	END CASE;

	RETURN false;
END
$$;

ALTER TABLE decodex.outbox
	ADD CONSTRAINT outbox_meaningful_receipt_state CHECK (
		(effect_state = 'receipt_recorded')
			= COALESCE(decodex.is_meaningful_evidence(receipt), false)
	),
	ADD CONSTRAINT outbox_delivered_evidence CHECK (
		state <> 'delivered'
		OR (
			effect_state = 'receipt_recorded'
			AND COALESCE(decodex.is_meaningful_evidence(receipt), false)
			AND COALESCE(decodex.is_meaningful_evidence(reconciliation), false)
			AND NOT decodex.has_credential_material(receipt)
			AND NOT decodex.has_credential_material(reconciliation)
		)
	);
