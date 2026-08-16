CREATE TABLE program_domain_pack_bindings (
  program_id TEXT PRIMARY KEY REFERENCES programs(program_id),
  pack_id TEXT NOT NULL CHECK (
    length(CAST(pack_id AS BLOB)) BETWEEN 3 AND 128 AND
    pack_id NOT GLOB '*[^a-z0-9.-]*' AND
    instr(pack_id, '.') > 0
  ),
  pack_version TEXT NOT NULL CHECK (
    length(CAST(pack_version AS BLOB)) BETWEEN 5 AND 32 AND
    pack_version NOT GLOB '*[^0-9.]*'
  ),
  pack_digest TEXT NOT NULL CHECK (
    length(pack_digest) = 64 AND
    pack_digest NOT GLOB '*[^0-9a-f]*'
  ),
  bound_at_micros INTEGER NOT NULL CHECK (bound_at_micros > 0)
) STRICT;

CREATE TRIGGER program_domain_pack_binding_is_immutable_update
BEFORE UPDATE ON program_domain_pack_bindings
BEGIN
  SELECT RAISE(ABORT, 'Program Domain Pack binding is immutable');
END;

CREATE TRIGGER program_domain_pack_binding_is_immutable_delete
BEFORE DELETE ON program_domain_pack_bindings
BEGIN
  SELECT RAISE(ABORT, 'Program Domain Pack binding is immutable');
END;
