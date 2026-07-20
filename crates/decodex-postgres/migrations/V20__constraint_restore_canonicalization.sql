-- Forward-only canonicalization for CHECK definitions whose leading BETWEEN form is
-- rewritten by PostgreSQL dump/restore. The observer remains exact; these equivalent
-- explicit comparisons make the authoritative definition a restore fixed point.

ALTER TABLE decodex.repository_admissions
	DROP CONSTRAINT repository_admissions_identity_bounded;
ALTER TABLE decodex.repository_admissions
	ADD CONSTRAINT repository_admissions_identity_bounded CHECK (
		pg_catalog.octet_length(admitted_identity) >= 1
		AND pg_catalog.octet_length(admitted_identity) <= 256
		AND admitted_identity = pg_catalog.btrim(admitted_identity)
		AND admitted_identity COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
	);

ALTER TABLE decodex.repository_admissions
	DROP CONSTRAINT repository_admissions_base_bounded;
ALTER TABLE decodex.repository_admissions
	ADD CONSTRAINT repository_admissions_base_bounded CHECK (
		pg_catalog.octet_length(admitted_base) >= 1
		AND pg_catalog.octet_length(admitted_base) <= 256
		AND admitted_base = pg_catalog.btrim(admitted_base)
		AND admitted_base COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
	);

ALTER TABLE decodex.repository_admissions
	DROP CONSTRAINT repository_admissions_path_bounded;
ALTER TABLE decodex.repository_admissions
	ADD CONSTRAINT repository_admissions_path_bounded CHECK (
		pg_catalog.octet_length(repository_absolute_path) >= 2
		AND pg_catalog.octet_length(repository_absolute_path) <= 4096
		AND pg_catalog.left(repository_absolute_path, 1) = '/'
		AND repository_absolute_path COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND repository_absolute_path COLLATE pg_catalog."C" !~ '(^|/)\.{1,2}(/|$)'
		AND repository_absolute_path NOT LIKE '%//%'
	);

ALTER TABLE decodex.repository_operations
	DROP CONSTRAINT repository_operations_descriptor_bounded;
ALTER TABLE decodex.repository_operations
	ADD CONSTRAINT repository_operations_descriptor_bounded CHECK (
		pg_catalog.octet_length(descriptor::text) >= 2
		AND pg_catalog.octet_length(descriptor::text) <= 1048576
		AND pg_catalog.octet_length(payload::text) >= 2
		AND pg_catalog.octet_length(payload::text) <= 262144
	);

ALTER TABLE decodex.repository_authority_transitions
	DROP CONSTRAINT repository_authority_transitions_head_bounded;
ALTER TABLE decodex.repository_authority_transitions
	ADD CONSTRAINT repository_authority_transitions_head_bounded CHECK (
		pg_catalog.octet_length(head) >= 1
		AND pg_catalog.octet_length(head) <= 256
		AND head = pg_catalog.btrim(head)
		AND head COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
	);

ALTER TABLE decodex.managed_repositories
	DROP CONSTRAINT managed_repositories_worktree_path_bounded;
ALTER TABLE decodex.managed_repositories
	ADD CONSTRAINT managed_repositories_worktree_path_bounded CHECK (
		pg_catalog.octet_length(worktree_absolute_path) >= 2
		AND pg_catalog.octet_length(worktree_absolute_path) <= 4096
		AND pg_catalog.left(worktree_absolute_path, 1) = '/'
		AND worktree_absolute_path COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
		AND worktree_absolute_path COLLATE pg_catalog."C" !~ '(^|/)\.{1,2}(/|$)'
		AND worktree_absolute_path NOT LIKE '%//%'
	);

ALTER TABLE decodex.managed_repositories
	DROP CONSTRAINT managed_repositories_head_bounded;
ALTER TABLE decodex.managed_repositories
	ADD CONSTRAINT managed_repositories_head_bounded CHECK (
		pg_catalog.octet_length(head) >= 1
		AND pg_catalog.octet_length(head) <= 256
		AND head = pg_catalog.btrim(head)
		AND head COLLATE pg_catalog."C" !~ '[[:cntrl:]]'
	);

ALTER TABLE decodex.routing_policy_revisions
	DROP CONSTRAINT routing_policy_revisions_build;
ALTER TABLE decodex.routing_policy_revisions
	ADD CONSTRAINT routing_policy_revisions_build CHECK (
		pg_catalog.octet_length(required_build_id) >= 1
		AND pg_catalog.octet_length(required_build_id) <= 256
		AND required_build_id COLLATE pg_catalog."C" ~ '^sha256:[0-9a-f]{64}$'
	);

ALTER TABLE decodex.routing_decision_exclusions
	DROP CONSTRAINT routing_decision_exclusion_range;
ALTER TABLE decodex.routing_decision_exclusions
	ADD CONSTRAINT routing_decision_exclusion_range CHECK (
		observed_at_micros >= 0
		AND observed_at_micros <= 253402300799999999
		AND resets_at_micros >= observed_at_micros + 1
		AND resets_at_micros <= 253402300799999999
		AND raw_observed_at = observed_at_micros::text
		AND raw_resets_at = resets_at_micros::text
	);
