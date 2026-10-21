#!/usr/bin/env python3
"""
check-storage-layout.py — Storage-layout compatibility gate for NeuroWealth Vault.

Compares the `DataKey` enum / union definitions extracted from two WASM artifacts
(e.g., baseline/old WASM vs new candidate WASM) to ensure backward-compatible
storage decoding across contract upgrades.

Rules:
  1. Preserved Variants: Every variant in the old WASM must exist at the exact same
     discriminant index in the new WASM with identical name and type structure.
  2. Append-Only: New variants may only be added at indices >= len(old_variants).
  3. Reordering / Removal / Type-Mutation: Forbidden unless a valid migration
     document is explicitly provided via `--migration-doc <path>`.

Usage:
  python3 scripts/check-storage-layout.py <OLD_WASM> <NEW_WASM> [--migration-doc <PATH>] [--output <DIFF_FILE>]

Exit Codes:
  0 — Compatible layout (or breaking change explicitly permitted by migration doc)
  1 — Incompatible layout detected
"""

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple


def find_cli_binary() -> str:
    """Find stellar or soroban CLI in PATH."""
    candidates = []
    if os.environ.get("STELLAR_BIN"):
        candidates.append(os.environ["STELLAR_BIN"])
    candidates.extend(["stellar", "soroban"])
    for binary in candidates:
        try:
            result = subprocess.run(
                [binary, "--version"],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                check=False,
            )
            if result.returncode == 0:
                return binary
        except FileNotFoundError:
            continue
    raise RuntimeError("Neither 'stellar' nor 'soroban' CLI found in PATH.")


def extract_datakey_spec(wasm_path: Path, cli_bin: str) -> List[Dict[str, Any]]:
    """Extract DataKey cases from a Soroban contract WASM."""
    if not wasm_path.exists():
        raise FileNotFoundError(f"WASM file not found at: {wasm_path}")

    spec_entries: List[Dict[str, Any]] = []

    # Primary method (Stellar CLI v21.x): `contract inspect --output xdr-base64`
    # piped into `xdr decode --type ScSpecEntry --input stream-base64 --output json`
    inspect_cmd = [cli_bin, "contract", "inspect", "--wasm", str(wasm_path), "--output", "xdr-base64"]
    inspect_proc = subprocess.run(
        inspect_cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if inspect_proc.returncode == 0 and inspect_proc.stdout.strip():
        decode_cmd = [
            cli_bin,
            "xdr",
            "decode",
            "--type",
            "ScSpecEntry",
            "--input",
            "stream-base64",
            "--output",
            "json",
        ]
        decode_proc = subprocess.run(
            decode_cmd,
            input=inspect_proc.stdout,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
        if decode_proc.returncode == 0 and decode_proc.stdout.strip():
            for line in decode_proc.stdout.splitlines():
                line = line.strip()
                if line:
                    spec_entries.append(json.loads(line))

    # Fallback for newer CLI versions with `contract info interface`
    if not spec_entries:
        cmd = [cli_bin, "contract", "info", "interface", "--wasm", str(wasm_path), "--output", "json"]
        try:
            proc = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=True)
            raw_output = proc.stdout.strip()
        except subprocess.CalledProcessError as e:
            err_msg = inspect_proc.stderr.strip() or e.stderr.strip()
            raise RuntimeError(f"Failed to inspect WASM '{wasm_path}' with {cli_bin}: {err_msg}")

        try:
            parsed = json.loads(raw_output)
            if isinstance(parsed, list):
                spec_entries = parsed
            elif isinstance(parsed, dict):
                spec_entries = [parsed]
        except json.JSONDecodeError as e:
            raise RuntimeError(f"Could not parse JSON output from {cli_bin} for '{wasm_path}': {e}")

    datakey_union = None
    for entry in spec_entries:
        if isinstance(entry, dict) and "udt_union_v0" in entry:
            union_def = entry["udt_union_v0"]
            if union_def.get("name") == "DataKey":
                datakey_union = union_def
                break

    if datakey_union is None:
        raise ValueError(f"No 'DataKey' enum/union found in contract specification of '{wasm_path}'.")

    cases = datakey_union.get("cases", [])
    parsed_cases = []
    for idx, case in enumerate(cases):
        if not isinstance(case, dict) or not case:
            continue
        case_kind = list(case.keys())[0]
        case_data = case[case_kind]
        case_name = case_data.get("name", f"Unknown_{idx}")
        case_type = case_data.get("type_", [])
        parsed_cases.append({
            "index": idx,
            "kind": case_kind,
            "name": case_name,
            "types": case_type,
            "raw": case,
        })

    return parsed_cases


def compare_storage_layouts(
    old_cases: List[Dict[str, Any]],
    new_cases: List[Dict[str, Any]],
) -> Tuple[bool, List[str], List[str], List[str]]:
    """
    Compare old vs new DataKey layouts.
    Returns: (is_compatible, incompatible_keys, issues, report_lines)
    """
    is_compatible = True
    incompatible_keys: List[str] = []
    issues: List[str] = []
    report_lines: List[str] = []

    report_lines.append("Storage Layout Comparison Report (DataKey Enum)")
    report_lines.append("=" * 60)
    report_lines.append(f"Old WASM variants count: {len(old_cases)}")
    report_lines.append(f"New WASM variants count: {len(new_cases)}")
    report_lines.append("-" * 60)

    # Check mapping by index (discriminant)
    min_len = min(len(old_cases), len(new_cases))
    for i in range(min_len):
        old_v = old_cases[i]
        new_v = new_cases[i]

        old_desc = f"{old_v['name']} ({old_v['kind']}{': ' + str(old_v['types']) if old_v['types'] else ''})"
        new_desc = f"{new_v['name']} ({new_v['kind']}{': ' + str(new_v['types']) if new_v['types'] else ''})"

        if old_v["name"] != new_v["name"]:
            is_compatible = False
            incompatible_keys.append(old_v["name"])
            incompatible_keys.append(new_v["name"])
            msg = f"Variant name mismatch at discriminant index {i}: old '{old_v['name']}' vs new '{new_v['name']}'"
            issues.append(msg)
            report_lines.append(f"[FAIL] Index {i:02d}: {msg}")
        elif old_v["kind"] != new_v["kind"] or old_v["types"] != new_v["types"]:
            is_compatible = False
            incompatible_keys.append(old_v["name"])
            msg = f"Variant type signature mismatch at discriminant index {i} ('{old_v['name']}'): old {old_desc} vs new {new_desc}"
            issues.append(msg)
            report_lines.append(f"[FAIL] Index {i:02d}: {msg}")
        else:
            report_lines.append(f"[ OK ] Index {i:02d}: {old_desc}")

    # Check for removed variants if new is shorter than old
    if len(new_cases) < len(old_cases):
        is_compatible = False
        for i in range(len(new_cases), len(old_cases)):
            old_v = old_cases[i]
            incompatible_keys.append(old_v["name"])
            msg = f"Variant '{old_v['name']}' at discriminant index {i} was REMOVED in new WASM"
            issues.append(msg)
            report_lines.append(f"[REMOVED] Index {i:02d}: {msg}")

    # Check for newly added variants (valid append-only)
    if len(new_cases) > len(old_cases):
        for i in range(len(old_cases), len(new_cases)):
            new_v = new_cases[i]
            new_desc = f"{new_v['name']} ({new_v['kind']}{': ' + str(new_v['types']) if new_v['types'] else ''})"
            report_lines.append(f"[APPENDED] Index {i:02d}: {new_desc} (Backward-compatible)")

    report_lines.append("=" * 60)

    # Deduplicate incompatible keys
    unique_incompatible = list(dict.fromkeys(incompatible_keys))
    return is_compatible, unique_incompatible, issues, report_lines


def main():
    parser = argparse.ArgumentParser(
        description="Verify DataKey storage layout compatibility between two Soroban WASM binaries."
    )
    parser.add_argument("old_wasm", help="Path to old/baseline WASM binary")
    parser.add_argument("new_wasm", help="Path to new/candidate WASM binary")
    parser.add_argument(
        "--migration-doc",
        help="Path to referenced migration documentation authorizing breaking changes (e.g. docs/UPGRADE_MIGRATION.md)",
        default=os.environ.get("MIGRATION_DOC", None),
    )
    parser.add_argument(
        "--output",
        help="Path to write the diff report artifact (e.g. scripts/e2e-artifacts/storage_layout_diff.txt)",
        default=None,
    )

    args = parser.parse_args()

    old_wasm_path = Path(args.old_wasm)
    new_wasm_path = Path(args.new_wasm)

    print("=" * 70)
    print("NeuroWealth Vault — Storage Layout Compatibility Gate")
    print("=" * 70)
    print(f"Old WASM: {old_wasm_path}")
    print(f"New WASM: {new_wasm_path}")
    if args.migration_doc:
        print(f"Referenced Migration Doc: {args.migration_doc}")
    print("")

    try:
        cli_bin = find_cli_binary()
        old_cases = extract_datakey_spec(old_wasm_path, cli_bin)
        new_cases = extract_datakey_spec(new_wasm_path, cli_bin)
    except Exception as e:
        print(f"❌ ERROR: Failed to extract storage key metadata: {e}", file=sys.stderr)
        sys.exit(1)

    is_compatible, incompatible_keys, issues, report_lines = compare_storage_layouts(old_cases, new_cases)

    report_text = "\n".join(report_lines)
    print(report_text)
    print("")

    if args.output:
        out_path = Path(args.output)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(report_text + "\n")
        print(f"Artifact saved: {out_path}")

    if is_compatible:
        print("✅ STORAGE LAYOUT CHECK PASSED: Storage layouts are 100% compatible.")
        sys.exit(0)

    # Incompatible layout detected
    print(f"❌ INCOMPATIBLE STORAGE LAYOUT DETECTED: {len(issues)} issue(s) found.")
    print("Incompatible keys identified:")
    for key in incompatible_keys:
        print(f"  - {key}")
    print("")

    # Check if a valid migration doc was provided
    if args.migration_doc:
        doc_path = Path(args.migration_doc)
        if doc_path.exists() and doc_path.stat().st_size > 0:
            print(f"⚠️  Breaking changes permitted: Migration document verified at '{args.migration_doc}'.")
            print("   Proceeding with upgrade under documented migration plan.")
            sys.exit(0)
        else:
            print(f"❌ ERROR: Referenced migration document '{args.migration_doc}' was not found or is empty.")
            sys.exit(1)

    print("❌ UPGRADE BLOCKED:")
    print("   DataKey enum variants have been reordered, removed, or modified, which will")
    print("   corrupt contract storage decoding upon upgrade.")
    print("   To resolve:")
    print("     1. Preserve all existing DataKey variants in their original positions and types, or")
    print("     2. Provide a migration document reference via `--migration-doc <PATH>` if a")
    print("        storage migration entrypoint is implemented and documented.")
    sys.exit(1)


if __name__ == "__main__":
    main()
