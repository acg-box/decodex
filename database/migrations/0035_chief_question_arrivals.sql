-- History recovery must not produce a new-question notification.
ALTER TABLE chief_async_questions ADD COLUMN arrived_live INTEGER NOT NULL DEFAULT 0 CHECK(arrived_live IN (0, 1));
