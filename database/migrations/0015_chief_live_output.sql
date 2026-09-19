CREATE TABLE chief_live_output (
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 work_id TEXT NOT NULL REFERENCES chief_work_items(id),
 turn_id TEXT NOT NULL,
 item_id TEXT NOT NULL,
 text TEXT NOT NULL DEFAULT '',
 truncated INTEGER NOT NULL DEFAULT 0 CHECK(truncated IN (0,1)),
 UNIQUE(work_id,turn_id,item_id),
 CHECK(length(CAST(text AS BLOB)) <= 65536)
) STRICT;
