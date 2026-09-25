ALTER TABLE chief_live_output ADD COLUMN kind TEXT NOT NULL DEFAULT 'agentMessage'
 CHECK(kind IN ('agentMessage','plan'));
ALTER TABLE chief_live_output ADD COLUMN completed INTEGER NOT NULL DEFAULT 0
 CHECK(completed IN (0,1));
