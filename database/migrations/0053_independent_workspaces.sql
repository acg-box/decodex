-- Workspaces own directories, never executable agents.
CREATE TABLE workspaces (
 id TEXT PRIMARY KEY,
 name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 256),
 directory TEXT NOT NULL UNIQUE CHECK(length(directory) BETWEEN 1 AND 4096)
) STRICT;
CREATE TABLE work_workspace (
 work_id TEXT PRIMARY KEY REFERENCES agent_work_items(id) ON DELETE CASCADE,
 workspace_id TEXT NOT NULL REFERENCES workspaces(id)
) STRICT;
CREATE INDEX work_workspace_scope ON work_workspace(workspace_id);
INSERT INTO workspaces(id,name,directory)
 SELECT 'workspace-' || agent_id,name,directory FROM agent_workspaces
 GROUP BY directory HAVING agent_id=MIN(agent_id);
-- Resolve the closest legacy scope, including nested scopes, before removing it.
WITH RECURSIVE ancestors(work_id,ancestor,depth) AS (
 SELECT id,id,0 FROM agent_work_items
 UNION ALL
 SELECT a.work_id,w.parent_goal_id,a.depth+1 FROM ancestors a
 JOIN agent_work_items w ON w.id=a.ancestor WHERE w.parent_goal_id IS NOT NULL
), scopes AS (
 SELECT a.work_id,n.id,a.depth,
 ROW_NUMBER() OVER(PARTITION BY a.work_id ORDER BY a.depth) AS position
 FROM ancestors a JOIN agent_workspaces old ON old.agent_id=a.ancestor
 JOIN workspaces n ON n.directory=old.directory
)
INSERT INTO work_workspace SELECT work_id,id FROM scopes WHERE position=1;
-- Preserve the existing visible title without retaining a workspace-name override.
UPDATE agent_work_items SET title=(SELECT name FROM agent_workspaces WHERE agent_id=id)
 WHERE id IN (SELECT agent_id FROM agent_workspaces) AND title=id;
DROP TABLE agent_workspaces;
