-- One admitted native process can host several independent user conversations.
-- Keep each immutable Agent binding and all existing process/account fences.
DROP TRIGGER agent_process_binding_authority;
DROP TRIGGER agent_process_binding_immutable;
CREATE TABLE agent_process_bindings_shared (
 operation_key TEXT PRIMARY KEY CHECK(length(CAST(operation_key AS BLOB)) BETWEEN 1 AND 256),
 root_id TEXT NOT NULL REFERENCES agent_work_items(id),
 account_id TEXT NOT NULL REFERENCES accounts(account_id),
 generation_id TEXT NOT NULL REFERENCES process_generations(generation_id),
 request_sha256 TEXT NOT NULL CHECK(length(request_sha256)=64),
 created_at_micros INTEGER NOT NULL CHECK(created_at_micros>0),
 UNIQUE(root_id,generation_id)
) STRICT;
INSERT INTO agent_process_bindings_shared(rowid,operation_key,root_id,account_id,generation_id,request_sha256,created_at_micros)
 SELECT rowid,operation_key,root_id,account_id,generation_id,request_sha256,created_at_micros FROM agent_process_bindings;
DROP TABLE agent_process_bindings;
ALTER TABLE agent_process_bindings_shared RENAME TO agent_process_bindings;
CREATE INDEX agent_process_root ON agent_process_bindings(root_id,created_at_micros);
CREATE INDEX agent_process_generation ON agent_process_bindings(generation_id);
CREATE TRIGGER agent_process_binding_immutable BEFORE UPDATE ON agent_process_bindings
BEGIN
 SELECT RAISE(ABORT,'Agent process admission is immutable');
END;
CREATE TRIGGER agent_process_binding_authority BEFORE INSERT ON agent_process_bindings
BEGIN
 SELECT RAISE(ABORT,'Agent root must be a root goal') WHERE NOT EXISTS(
  SELECT 1 FROM agent_work_items WHERE id=NEW.root_id AND kind='goal' AND parent_goal_id IS NULL
 );
 SELECT RAISE(ABORT,'Previous Agent process must be dead before account rotation') WHERE EXISTS(
  SELECT 1 FROM agent_process_bindings b JOIN process_generations g ON g.generation_id=b.generation_id
  WHERE b.root_id=NEW.root_id AND b.account_id<>NEW.account_id AND g.state<>'dead'
 );
 SELECT RAISE(ABORT,'Agent work must be idle before account rotation') WHERE
  coalesce((SELECT account_id<>NEW.account_id FROM agent_process_bindings WHERE root_id=NEW.root_id ORDER BY created_at_micros DESC,rowid DESC LIMIT 1),0)
  AND EXISTS(
   WITH RECURSIVE family(id) AS (
    SELECT NEW.root_id UNION SELECT w.id FROM agent_work_items w JOIN family f ON w.parent_goal_id=f.id
   )
   SELECT 1 FROM agent_work_items WHERE id IN(SELECT id FROM family) AND dispatch_state<>'idle'
  );
 SELECT RAISE(ABORT,'Agent generation ownership differs') WHERE NOT EXISTS(
  SELECT 1 FROM process_generations WHERE generation_id=NEW.generation_id
   AND account_id=NEW.account_id AND runtime_session_id IS NULL AND quick_task_admission_key IS NULL
 );
END;
