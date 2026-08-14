ALTER TABLE role_profiles RENAME TO role_profiles_v1;

CREATE TABLE role_profiles (
  role TEXT PRIMARY KEY CHECK (role = 'task'),
  revision INTEGER NOT NULL CHECK (revision > 0),
  model TEXT NOT NULL CHECK (length(CAST(model AS BLOB)) BETWEEN 1 AND 128),
  reasoning_effort TEXT NOT NULL CHECK (
    reasoning_effort IN ('none', 'minimal', 'low', 'medium', 'high', 'xhigh')
  ),
  service_tier TEXT NOT NULL DEFAULT 'default' CHECK (
    length(CAST(service_tier AS BLOB)) BETWEEN 1 AND 32
  ),
  instructions TEXT NOT NULL CHECK (
    length(CAST(instructions AS BLOB)) BETWEEN 1 AND 65536
  ),
  updated_at_micros INTEGER NOT NULL CHECK (updated_at_micros > 0)
) STRICT;

INSERT INTO role_profiles (
  role,
  revision,
  model,
  reasoning_effort,
  service_tier,
  instructions,
  updated_at_micros
)
SELECT
  role,
  revision + 1,
  model,
  reasoning_effort,
  service_tier,
  CASE
    WHEN length(CAST(instructions AS BLOB)) = 0
      THEN 'Follow the user request for this task.'
    ELSE instructions
  END,
  max(updated_at_micros, 2)
FROM role_profiles_v1;

DROP TABLE role_profiles_v1;
