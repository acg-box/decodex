CREATE TABLE chief_live_output_next (
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 work_id TEXT NOT NULL REFERENCES chief_work_items(id),
 turn_id TEXT NOT NULL,
 item_id TEXT NOT NULL,
 text TEXT NOT NULL DEFAULT '',
 truncated INTEGER NOT NULL DEFAULT 0 CHECK(truncated IN (0,1)),
 kind TEXT NOT NULL DEFAULT 'agentMessage' CHECK(kind IN ('agentMessage','plan','reasoningSummary')),
 completed INTEGER NOT NULL DEFAULT 0 CHECK(completed IN (0,1)),
 summary_parts TEXT,
 UNIQUE(work_id,turn_id,item_id),
 CHECK(length(CAST(text AS BLOB)) <= 65536),
 CHECK(summary_parts IS NULL OR (kind='reasoningSummary' AND length(CAST(summary_parts AS BLOB)) <= 524288))
) STRICT;
INSERT INTO chief_live_output_next(id,work_id,turn_id,item_id,text,truncated,kind,completed)
 SELECT id,work_id,turn_id,item_id,text,truncated,kind,completed FROM chief_live_output;
DROP TABLE chief_live_output;
ALTER TABLE chief_live_output_next RENAME TO chief_live_output;
