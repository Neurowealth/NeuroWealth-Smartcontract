#!/usr/bin/env python3
"""
Detect drift between Supabase migrations (source of truth) and db/types.ts.

Checks performed:
  1. Every table created across supabase/migrations/*.sql exists as an exported
     interface and Database table in db/types.ts.
  2. Every column defined in migrations exists with matching types in db/types.ts.
  3. db/types.ts matches the canonical deterministic output of scripts/generate-db-types.py.

Usage (local):
    python3 scripts/check-schema-drift.py

Exit codes:
    0: No drift detected
    1: Drift detected — db/types.ts must be updated
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GENERATE_SCRIPT = ROOT / "scripts" / "generate-db-types.py"

spec = importlib.util.spec_from_file_location("generate_db_types", GENERATE_SCRIPT)
if not spec or not spec.loader:
    print(f"Error: could not load {GENERATE_SCRIPT}", file=sys.stderr)
    sys.exit(1)
gen_mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gen_mod)

MIGRATIONS_DIR = gen_mod.MIGRATIONS_DIR
TYPES_FILE = gen_mod.TYPES_FILE
generate_typescript_code = gen_mod.generate_typescript_code
parse_migrations = gen_mod.parse_migrations
to_record_interface = gen_mod.to_record_interface


def main() -> int:
    failures: list[str] = []

    if not MIGRATIONS_DIR.is_dir():
        print(f"missing migrations directory: {MIGRATIONS_DIR}", file=sys.stderr)
        return 1
    if not TYPES_FILE.exists():
        print(f"missing types file: {TYPES_FILE}", file=sys.stderr)
        return 1

    tables = parse_migrations(MIGRATIONS_DIR)
    expected_code = generate_typescript_code(tables)
    current_code = TYPES_FILE.read_text(encoding="utf-8")

    # Check for table interfaces
    for table_name in sorted(tables.keys()):
        expected_iface = to_record_interface(table_name)
        if f"export interface {expected_iface} " not in current_code:
            failures.append(
                f"db/types.ts is missing `{expected_iface}` for table `{table_name}`"
            )

    # Check for overall file drift
    if current_code.strip() != expected_code.strip():
        failures.append("db/types.ts contents diverge from generated migration schema")

    if failures:
        print("Schema drift check failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        print(
            "\nRun `python3 scripts/generate-db-types.py` to regenerate db/types.ts.",
            file=sys.stderr,
        )
        return 1

    print("No schema drift detected: supabase/migrations/ and db/types.ts agree.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
