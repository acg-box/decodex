-- Direct business-model migration. Native conversation, input and execution identities remain intact.
ALTER TABLE workspaces RENAME TO projects;
ALTER TABLE work_workspace RENAME TO project_agents;
ALTER TABLE project_agents RENAME COLUMN work_id TO agent_id;
ALTER TABLE project_agents RENAME COLUMN workspace_id TO project_id;
ALTER TABLE projects ADD COLUMN native_project_id TEXT;
ALTER TABLE projects ADD COLUMN lead_agent_id TEXT REFERENCES agent_work_items(id);
CREATE UNIQUE INDEX project_native_identity ON projects(native_project_id) WHERE native_project_id IS NOT NULL;

-- Legacy execution rows remain readable for exact native recovery and history grants.
-- Only persistent conversations are Agents. A passive outcome is never promoted to an executor.
CREATE TABLE agents (
 id TEXT PRIMARY KEY REFERENCES agent_work_items(id),
 assistant INTEGER NOT NULL DEFAULT 0 CHECK(assistant IN(0,1))
) STRICT;
INSERT INTO agents(id,assistant)
 SELECT id,id=(SELECT id FROM agent_work_items WHERE parent_goal_id IS NULL ORDER BY created_at_micros,id LIMIT 1) FROM agent_work_items
 WHERE parent_goal_id IS NULL OR id IN(SELECT work_id FROM agent_managers);
CREATE UNIQUE INDEX one_assistant ON agents(assistant) WHERE assistant=1;
UPDATE agent_work_items SET title='Assistant' WHERE parent_goal_id IS NULL AND title IN('Main','Agent');

CREATE TABLE works (
 id TEXT PRIMARY KEY CHECK(length(id) BETWEEN 1 AND 512),
 project_id TEXT REFERENCES projects(id),
 owner_agent_id TEXT NOT NULL REFERENCES agents(id),
 title TEXT NOT NULL CHECK(length(title) BETWEEN 1 AND 120),
 requirements_id TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN('open','blocked','completed','cancelled')),
 reason TEXT,
 resume_condition TEXT,
 user_hold INTEGER NOT NULL DEFAULT 0 CHECK(user_hold IN(0,1)),
 assessment_json TEXT CHECK(assessment_json IS NULL OR json_valid(assessment_json)),
 CHECK(state<>'blocked' OR (length(reason)>0 AND length(resume_condition)>0)),
 CHECK(state<>'completed' OR assessment_json IS NOT NULL),
 FOREIGN KEY(id,requirements_id) REFERENCES work_requirements(work_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE INDEX work_owner ON works(owner_agent_id,state);
CREATE INDEX work_project ON works(project_id,state);
CREATE TABLE work_requirements (
 work_id TEXT NOT NULL REFERENCES works(id),
 id TEXT NOT NULL,
 goal TEXT NOT NULL,
 criteria TEXT NOT NULL,
 source TEXT NOT NULL,
 PRIMARY KEY(work_id,id)
) STRICT;
CREATE TABLE work_materials (
 id TEXT NOT NULL,
 work_id TEXT NOT NULL REFERENCES works(id),
 record_json TEXT NOT NULL CHECK(json_valid(record_json)),
 PRIMARY KEY(work_id,id)
) STRICT;
CREATE TABLE collaboration_requests (
 id TEXT PRIMARY KEY,
 work_id TEXT NOT NULL REFERENCES works(id),
 recipient_agent_id TEXT NOT NULL REFERENCES agents(id),
 instruction_id TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN('pending','accepted','returned','closed')),
 required INTEGER NOT NULL CHECK(required IN(0,1)),
 outcome TEXT CHECK(outcome IN('used','not_used','declined','cancelled','failed')),
 note TEXT,
 CHECK((state='closed')=(outcome IS NOT NULL)),
 FOREIGN KEY(id,instruction_id) REFERENCES collaboration_instructions(request_id,id) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE INDEX collaboration_recipient ON collaboration_requests(recipient_agent_id,state);
CREATE TABLE collaboration_instructions (
 request_id TEXT NOT NULL REFERENCES collaboration_requests(id),
 id TEXT NOT NULL UNIQUE,
 requirements_id TEXT NOT NULL,
 text TEXT NOT NULL,
 material_ids_json TEXT NOT NULL CHECK(json_valid(material_ids_json)),
 event_id INTEGER NOT NULL UNIQUE REFERENCES agent_inbox_events(id),
 PRIMARY KEY(request_id,id)
) STRICT;
CREATE TABLE work_results (
 id TEXT PRIMARY KEY,
 work_id TEXT NOT NULL,
 requirements_id TEXT NOT NULL,
 instruction_id TEXT REFERENCES collaboration_instructions(id),
 agent_id TEXT NOT NULL REFERENCES agents(id),
 record_json TEXT NOT NULL CHECK(json_valid(record_json)),
 FOREIGN KEY(work_id,requirements_id) REFERENCES work_requirements(work_id,id)
) STRICT;
CREATE INDEX result_work ON work_results(work_id);
CREATE TABLE work_result_reuse (
 work_id TEXT NOT NULL,
 requirements_id TEXT NOT NULL,
 result_id TEXT NOT NULL REFERENCES work_results(id),
 assessment TEXT NOT NULL,
 PRIMARY KEY(work_id,requirements_id,result_id),
 FOREIGN KEY(work_id,requirements_id) REFERENCES work_requirements(work_id,id)
) STRICT;
CREATE TABLE work_commands (
 id TEXT PRIMARY KEY,
 agent_id TEXT NOT NULL REFERENCES agents(id),
 command_json TEXT NOT NULL CHECK(json_valid(command_json))
) STRICT;

-- Passive legacy outcomes survive with qualified limits; their prior native receipts stay available.
INSERT INTO works(id,project_id,owner_agent_id,title,requirements_id,state,reason,resume_condition,assessment_json)
 WITH RECURSIVE ancestors(work_id,ancestor,depth) AS (
  SELECT id,parent_goal_id,0 FROM agent_work_items WHERE parent_goal_id IS NOT NULL
  UNION ALL SELECT a.work_id,w.parent_goal_id,a.depth+1 FROM ancestors a JOIN agent_work_items w ON w.id=a.ancestor WHERE w.parent_goal_id IS NOT NULL
 ), owners AS (
  SELECT a.work_id,a.ancestor,ROW_NUMBER() OVER(PARTITION BY a.work_id ORDER BY a.depth) AS position
  FROM ancestors a JOIN agents p ON p.id=a.ancestor
 )
 SELECT w.id,p.project_id,o.ancestor,substr(w.title,1,120),'legacy-'||w.id,
 CASE WHEN w.status='resolved' THEN 'completed' WHEN w.status IN('wait','user_decision') THEN 'blocked' ELSE 'open' END,
 CASE WHEN w.status IN('wait','user_decision') THEN 'Retained legacy wait; inspect original evidence' END,
 CASE WHEN w.status IN('wait','user_decision') THEN 'Owner reconciles the retained decision or dependency' END,
 CASE WHEN w.status='resolved' THEN json_object('requirements_id','legacy-'||w.id,'result_ids',json('[]'),'summary','Retained historical owner disposition','limits','Migrated legacy completion; no new Result qualification. Original native receipts remain available.') END
 FROM agent_work_items w JOIN owners o ON o.work_id=w.id AND o.position=1 LEFT JOIN project_agents p ON p.agent_id=w.id
 WHERE w.id NOT IN(SELECT id FROM agents);
INSERT INTO work_requirements(work_id,id,goal,criteria,source)
 SELECT w.id,w.requirements_id,coalesce((SELECT json_extract(e.payload,'$.text') FROM agent_inbox_events e WHERE e.work_item_id=w.id AND e.event_kind='work_instruction' AND json_valid(e.payload) ORDER BY e.id DESC LIMIT 1),old.instructions),
 'Reconcile original completion criteria and retained evidence before a new assessment',
 coalesce((SELECT 'legacy-instruction:'||e.id FROM agent_inbox_events e WHERE e.work_item_id=w.id AND e.event_kind='work_instruction' ORDER BY e.id DESC LIMIT 1),'legacy-agent-record:'||w.id)
 FROM works w JOIN agent_work_items old ON old.id=w.id;

CREATE TABLE guardian_settings (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 reviewer TEXT NOT NULL CHECK(reviewer IN('user','auto_review')),
 review_details INTEGER NOT NULL CHECK(review_details IN(0,1)),
 revision INTEGER NOT NULL CHECK(revision>0)
) STRICT;
INSERT INTO guardian_settings VALUES(1,'user',0,1);
CREATE TABLE guardian_sources (
 source_id TEXT NOT NULL,
 agent_id TEXT PRIMARY KEY REFERENCES agent_work_items(id),
 thread_id TEXT NOT NULL,
 revision INTEGER NOT NULL,
 outcome TEXT NOT NULL CHECK(outcome IN('pending','applied','overridden','unsupported','rejected','unknown')),
 effective_reviewer TEXT,
 detail TEXT NOT NULL
) STRICT;
CREATE TABLE guardian_review_details (
 id INTEGER PRIMARY KEY,
 parent_thread_id TEXT NOT NULL,
 reviewer_thread_id TEXT NOT NULL,
 reviewer_turn_id TEXT NOT NULL,
 item_id TEXT NOT NULL,
 review_id TEXT,
 record_json TEXT NOT NULL CHECK(length(record_json)<=65536 AND json_valid(record_json)),
 UNIQUE(reviewer_thread_id,reviewer_turn_id,item_id)
) STRICT;

CREATE TABLE agent_tool_migrations (
 agent_id TEXT PRIMARY KEY REFERENCES agents(id),
 old_thread_id TEXT NOT NULL,
 new_thread_id TEXT,
 parameters_json TEXT NOT NULL CHECK(json_valid(parameters_json)),
 outcome TEXT NOT NULL CHECK(outcome IN('reserved','created','retiring','unknown','applied')),
 CHECK(outcome NOT IN('created','retiring','applied') OR new_thread_id IS NOT NULL)
) STRICT;

CREATE TABLE native_work_contributions (
 id TEXT PRIMARY KEY,
 work_id TEXT NOT NULL,
 requirements_id TEXT NOT NULL,
 instruction TEXT NOT NULL,
 child_thread_id TEXT,
 parent_turn_id TEXT,
 parent_item_id TEXT,
 FOREIGN KEY(work_id,requirements_id) REFERENCES work_requirements(work_id,id),
 CHECK((child_thread_id IS NULL)=(parent_turn_id IS NULL)),
 CHECK((child_thread_id IS NULL)=(parent_item_id IS NULL)),
 UNIQUE(parent_turn_id,parent_item_id)
) STRICT;

-- V2 public child activity omits the assignment text. Retain only an exact matched native call
-- identity, not raw Responses frames or another executor/mailbox.
CREATE TABLE native_work_dispatches (
 contribution_id TEXT PRIMARY KEY REFERENCES native_work_contributions(id),
 thread_id TEXT NOT NULL,
 turn_id TEXT NOT NULL,
 call_id TEXT NOT NULL,
 UNIQUE(thread_id,turn_id,call_id)
) STRICT;
