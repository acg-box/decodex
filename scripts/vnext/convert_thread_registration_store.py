#!/usr/bin/env python3
"""Make a schema 62 copy of a retained schema 61 Decodex database."""

import argparse
from contextlib import closing
import os
from pathlib import Path
import sqlite3


SCHEMA = Path(__file__).resolve().parents[2] / "database" / "schema.sql"
NEW_OBJECTS = (
    ("table", "agent_managed_conversations", "agent_managed_conversations"),
    ("index", "managed_conversations_by_agent", "agent_managed_conversations"),
)


def inventory(db: sqlite3.Connection) -> dict[tuple[str, str, str], str]:
    rows = db.execute(
        "SELECT type,name,tbl_name,sql FROM sqlite_schema "
        "WHERE name NOT LIKE 'sqlite_%' AND sql IS NOT NULL"
    )
    return {(kind, name, table): sql for kind, name, table, sql in rows}


def convert(source: Path, destination: Path) -> None:
    with closing(sqlite3.connect(":memory:")) as reference:
        reference.executescript(SCHEMA.read_text())
        expected = inventory(reference)
    prior = {key: sql for key, sql in expected.items() if key not in NEW_OBJECTS}
    uri = source.resolve().as_uri() + "?mode=ro"
    # Reserve a private output path. The input and existing output files are never replaced.
    with closing(sqlite3.connect(uri, uri=True)) as original:
        os.close(os.open(destination, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600))
        try:
            with closing(sqlite3.connect(destination)) as copy:
                original.backup(copy)
                if (copy.execute("PRAGMA application_id").fetchone() != (0x44435831,)
                        or copy.execute("PRAGMA user_version").fetchone() != (61,)
                        or inventory(copy) != prior):
                    raise ValueError("the input must match the Decodex schema 61 exactly")
                copy.execute("PRAGMA foreign_keys=ON")
                with copy:
                    copy.execute("BEGIN IMMEDIATE")
                    for key in NEW_OBJECTS:
                        copy.execute(expected[key])
                    copy.execute("PRAGMA user_version=62")
                    if (inventory(copy) != expected
                            or copy.execute("PRAGMA quick_check").fetchall() != [("ok",)]
                            or copy.execute("PRAGMA foreign_key_check").fetchone()):
                        raise ValueError("the converted database failed verification")
        except BaseException:
            destination.unlink()
            raise


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path, help="a new owner-private output file")
    args = parser.parse_args()
    convert(args.source, args.destination)
