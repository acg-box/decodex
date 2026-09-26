ALTER TABLE chief_misalignment ADD COLUMN retired_voice INTEGER NOT NULL DEFAULT 0 CHECK(retired_voice IN (0,1));
-- Earlier records do not prove whether voice was retired. Require explicit acknowledgment.
UPDATE chief_misalignment SET retired_voice = 1;
