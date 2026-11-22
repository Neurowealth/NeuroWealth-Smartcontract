#!/usr/bin/env python3
"""
Generate TypeScript database types from Supabase migrations.

Parses table definitions and column types from supabase/migrations/*.sql
and outputs deterministic TypeScript interfaces in db/types.ts (both
per-table Record interfaces and the canonical Supabase Database schema).

Usage:
    python3 scripts/generate-db-types.py
    python3 scripts/generate-db-types.py --check
"""

from __future__ import annotations

import argparse
import glob
import re
import sys
from pathlib import Path
from typing import Any, Dict, List

ROOT = Path(__file__).resolve().parents[1]
MIGRATIONS_DIR = ROOT / "supabase" / "migrations"
TYPES_FILE = ROOT / "db" / "types.ts"


def to_record_interface(table_name: str) -> str:
    """`yield_snapshots` -> `YieldSnapshotRecord`, `whatsapp_message_deliveries` -> `WhatsappMessageDeliveryRecord`."""
    if table_name.endswith("ies"):
        singular = table_name[:-3] + "y"
    elif table_name.endswith("s"):
        singular = table_name[:-1]
    else:
        singular = table_name
    pascal = "".join(part.capitalize() for part in singular.split("_"))
    return f"{pascal}Record"


def parse_migrations(migrations_dir: Path) -> Dict[str, List[Dict[str, Any]]]:
    tables: Dict[str, List[Dict[str, Any]]] = {}
    for migration_file in sorted(migrations_dir.glob("*.sql")):
        content = migration_file.read_text(encoding="utf-8")
        for match in re.finditer(
            r"CREATE TABLE(?:\s+IF NOT EXISTS)?\s+(\w+)\s*\((.*?)\);",
            content,
            re.DOTALL | re.IGNORECASE,
        ):
            table_name = match.group(1).lower()
            body = match.group(2)
            # Remove single-line SQL comments
            body = re.sub(r"--.*", "", body)

            # Split columns by comma, respecting nested parentheses
            parts: List[str] = []
            current: List[str] = []
            depth = 0
            for char in body:
                if char == "(":
                    depth += 1
                elif char == ")":
                    depth -= 1
                elif char == "," and depth == 0:
                    parts.append("".join(current).strip())
                    current = []
                    continue
                current.append(char)
            if current:
                parts.append("".join(current).strip())

            cols: List[Dict[str, Any]] = []
            for part in parts:
                part = part.strip()
                if not part:
                    continue
                first_word = part.split()[0].upper()
                if first_word in (
                    "CONSTRAINT",
                    "PRIMARY",
                    "FOREIGN",
                    "CHECK",
                    "UNIQUE",
                ):
                    continue

                tokens = part.split(None, 2)
                col_name = tokens[0]
                col_type = tokens[1] if len(tokens) > 1 else "TEXT"
                rest = tokens[2] if len(tokens) > 2 else ""

                is_primary = "PRIMARY KEY" in part.upper()
                is_nullable = "NOT NULL" not in rest.upper() and not is_primary
                has_default = "DEFAULT" in rest.upper() or is_primary

                # Check constraint with IN ('a', 'b')
                ts_type = "string"
                m_in = re.search(
                    r"CHECK\s*\(\s*(?:\w+\s+IN\s+)?\((.*?)\)\s*\)",
                    part,
                    re.IGNORECASE,
                )
                if not m_in:
                    m_in = re.search(
                        r"CHECK\s*\(\s*\w+\s+IN\s+\((.*?)\)\s*\)",
                        part,
                        re.IGNORECASE,
                    )
                if m_in:
                    opts = [opt.strip() for opt in m_in.group(1).split(",")]
                    ts_type = " | ".join(opts)
                elif any(
                    t in col_type.upper()
                    for t in [
                        "INT",
                        "NUMERIC",
                        "DECIMAL",
                        "REAL",
                        "DOUBLE",
                        "FLOAT",
                        "SERIAL",
                    ]
                ):
                    ts_type = "number"
                elif any(
                    t in col_type.upper()
                    for t in [
                        "TEXT",
                        "VARCHAR",
                        "CHAR",
                        "UUID",
                        "TIMESTAMPTZ",
                        "TIMESTAMP",
                        "DATE",
                        "TIME",
                    ]
                ):
                    ts_type = "string"
                elif "BOOL" in col_type.upper():
                    ts_type = "boolean"
                elif "JSON" in col_type.upper():
                    ts_type = "Record<string, any>"

                cols.append(
                    {
                        "name": col_name,
                        "ts_type": ts_type,
                        "is_nullable": is_nullable,
                        "has_default": has_default,
                    }
                )

            tables[table_name] = cols
    return tables


def generate_typescript_code(tables: Dict[str, List[Dict[str, Any]]]) -> str:
    lines: List[str] = [
        "/**",
        " * This file is auto-generated from supabase/migrations/*.sql.",
        " * Do not edit manually. Run `python3 scripts/generate-db-types.py` to regenerate.",
        " */",
        "",
    ]

    for table_name in sorted(tables.keys()):
        cols = tables[table_name]
        interface_name = to_record_interface(table_name)
        lines.append(f"export interface {interface_name} {{")
        for col in cols:
            name = col["name"]
            ts_type = col["ts_type"]
            if col["is_nullable"]:
                lines.append(f"  {name}?: {ts_type} | null;")
            else:
                lines.append(f"  {name}: {ts_type};")
        lines.append("}")
        lines.append("")

    # Canonical Supabase Database interface
    lines.append("export interface Database {")
    lines.append("  public: {")
    lines.append("    Tables: {")
    for table_name in sorted(tables.keys()):
        cols = tables[table_name]
        interface_name = to_record_interface(table_name)
        lines.append(f"      {table_name}: {{")
        lines.append(f"        Row: {interface_name};")

        # Insert type
        lines.append("        Insert: {")
        for col in cols:
            name = col["name"]
            ts_type = col["ts_type"]
            optional = "?" if (col["is_nullable"] or col["has_default"]) else ""
            nullable_suffix = " | null" if col["is_nullable"] else ""
            lines.append(f"          {name}{optional}: {ts_type}{nullable_suffix};")
        lines.append("        };")

        # Update type
        lines.append("        Update: {")
        for col in cols:
            name = col["name"]
            ts_type = col["ts_type"]
            nullable_suffix = " | null" if col["is_nullable"] else ""
            lines.append(f"          {name}?: {ts_type}{nullable_suffix};")
        lines.append("        };")
        lines.append("      };")
    lines.append("    };")
    lines.append("    Views: Record<string, never>;")
    lines.append("    Functions: Record<string, never>;")
    lines.append("  };")
    lines.append("}")
    lines.append("")

    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Generate TypeScript types from Supabase migrations."
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="Check if db/types.ts is in sync without writing.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=TYPES_FILE,
        help="Path to output types file (default: db/types.ts).",
    )
    args = parser.parse_args()

    if not MIGRATIONS_DIR.is_dir():
        print(f"Error: missing migrations directory at {MIGRATIONS_DIR}", file=sys.stderr)
        return 1

    tables = parse_migrations(MIGRATIONS_DIR)
    code = generate_typescript_code(tables)

    if args.check:
        if not args.output.exists():
            print(f"Error: {args.output} does not exist.", file=sys.stderr)
            return 1
        current_code = args.output.read_text(encoding="utf-8")
        if current_code.strip() != code.strip():
            print(
                f"Error: {args.output} is out of date with supabase/migrations/.",
                file=sys.stderr,
            )
            print(
                "Run `python3 scripts/generate-db-types.py` to regenerate.",
                file=sys.stderr,
            )
            return 1
        print(f"OK: {args.output} is up to date with supabase/migrations/.")
        return 0

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(code, encoding="utf-8")
    print(f"Generated database types in {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
