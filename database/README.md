# Product database

SQLite owns local Decodex product state. The service is the only writer.

- `schema.sql` defines the current schema and initial settings.
- `src/schema.rs` initializes an empty database and verifies its exact structure.
- `src/` contains the domain storage owners.

The app and service use the same current schema. Older schemas are not upgraded at
startup. Change the schema directly during private development and convert the
single local development database before installing the matching app and service.
See [local database operations](../openwiki/operations/local-database.md).
