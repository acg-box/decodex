CREATE TABLE chief_managers (
 work_id TEXT PRIMARY KEY REFERENCES chief_work_items(id)
) STRICT;
CREATE TRIGGER chief_manager_goal BEFORE INSERT ON chief_managers BEGIN
 SELECT RAISE(ABORT,'manager must be a goal') WHERE NOT EXISTS(SELECT 1 FROM chief_work_items WHERE id=NEW.work_id AND kind='goal');
END;
CREATE TABLE chief_workspaces (
 chief_id TEXT PRIMARY KEY REFERENCES chief_managers(work_id),
 name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 256),
 directory TEXT NOT NULL CHECK(length(directory) BETWEEN 1 AND 4096)
) STRICT;
