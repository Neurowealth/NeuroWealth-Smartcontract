#!/usr/bin/env python3
"""
Detect drift between the Supabase migrations (source of truth), the
standalone db/schema.sql copy, and the db/types.ts TypeScript interfaces.

Checks performed:
  1. Every table created across supabase/migrations/*.sql exists in
     db/schema.sql.
  2. Every table in db/schema.sql has a corresponding exported interface in
     db/types.ts (matched by convention: `foo_bar` -> `FooBarRecord`).

Usage (local):
    python3 scripts/check-schema-drift.py

Exit codes:
    0: No drift detected
    1: Drift detected — db/schema.sql and/or db/types.ts must be updated
"""

from __future__ import annotations

import re
import sys
from pathlib import Path
from typing import Set

ROOT = Path(__file__).resolve().parents[1]
MIGRATIONS_DIR = ROOT / "supabase" / "migrations"
SCHEMA_COPY = ROOT / "db" / "schema.sql"
TYPES_FILE = ROOT / "db" / "types.ts"

TABLE_RE = re.compile(
    r"CREATE TABLE(?:\s+IF NOT EXISTS)?\s+(\w+)", re.IGNORECASE
)
INTERFACE_RE = re.compile(r"export interface (\w+)")


def tables_in(sql_text: str) -> Set[str]:
    return {match.group(1).lower() for match in TABLE_RE.finditer(sql_text)}


def to_record_interface(table_name: str) -> str:
    """`yield_snapshots` -> `YieldSnapshotRecord` (naive singularization: drop a trailing 's')."""
    singular = table_name[:-1] if table_name.endswith("s") else table_name
    pascal = "".join(part.capitalize() for part in singular.split("_"))
    return f"{pascal}Record"


def main() -> int:
    failures: list[str] = []

    if not MIGRATIONS_DIR.is_dir():
        print(f"missing migrations directory: {MIGRATIONS_DIR}", file=sys.stderr)
        return 1
    if not SCHEMA_COPY.exists():
        print(f"missing schema copy: {SCHEMA_COPY}", file=sys.stderr)
        return 1
    if not TYPES_FILE.exists():
        print(f"missing types file: {TYPES_FILE}", file=sys.stderr)
        return 1

    migration_tables: Set[str] = set()
    for migration in sorted(MIGRATIONS_DIR.glob("*.sql")):
        migration_tables |= tables_in(migration.read_text(encoding="utf-8"))

    schema_copy_tables = tables_in(SCHEMA_COPY.read_text(encoding="utf-8"))
    declared_interfaces = set(
        INTERFACE_RE.findall(TYPES_FILE.read_text(encoding="utf-8"))
    )

    missing_from_schema_copy = migration_tables - schema_copy_tables
    if missing_from_schema_copy:
        failures.append(
            "db/schema.sql is missing tables defined in supabase/migrations/: "
            + ", ".join(sorted(missing_from_schema_copy))
        )

    stale_in_schema_copy = schema_copy_tables - migration_tables
    if stale_in_schema_copy:
        failures.append(
            "db/schema.sql has tables no longer present in supabase/migrations/: "
            + ", ".join(sorted(stale_in_schema_copy))
        )

    for table in sorted(migration_tables):
        expected_interface = to_record_interface(table)
        if expected_interface not in declared_interfaces:
            failures.append(
                f"db/types.ts is missing `{expected_interface}` for table `{table}`"
            )

    if failures:
        print("Schema drift check failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        print(
            "\nUpdate db/schema.sql and db/types.ts to match supabase/migrations/.",
            file=sys.stderr,
        )
        return 1

    print("No schema drift detected: migrations, db/schema.sql, and db/types.ts agree.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
