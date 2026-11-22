#!/usr/bin/env python3
"""
Validate that the contract specification matches the actual implementation.

Checks performed:
  1. Every public function in the contract is documented in the spec.
  2. Every event struct defined in the contract is in the spec.
  3. Event field names in the spec match the struct fields in source.
  4. VaultError numeric codes in the spec match the enum assignments in source.
  5. Spec has the required top-level fields.

Usage (local):
    python3 scripts/validate-spec.py

Exit codes:
    0: Spec is valid and current
    1: Drift detected — spec must be updated
"""

import io
import json
import re
from pathlib import Path
from typing import Dict, List, Set
import sys

if sys.platform == "win32":
    try:
        sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
        sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding="utf-8", errors="replace")
    except Exception:
        pass


class ContractValidator:
    """Validate contract spec against actual implementation."""

    def __init__(self, contract_path: str, spec_path: str):
        self.contract_path = Path(contract_path)
        self.spec_path = Path(spec_path)

        if not self.contract_path.exists():
            raise FileNotFoundError(f"Contract file not found: {contract_path}")
        if not self.spec_path.exists():
            raise FileNotFoundError(f"Spec file not found: {spec_path}")

        self.source = self.contract_path.read_text()
        with open(self.spec_path) as f:
            self.spec = json.load(f)

        self.errors: List[str] = []
        self.warnings: List[str] = []
        self.info: List[str] = []

    # ------------------------------------------------------------------
    # Public entry point
    # ------------------------------------------------------------------

    def validate(self) -> bool:
        """Run all validation checks. Returns True when no errors."""
        print("=" * 70)
        print("NeuroWealth Vault — Contract Specification Validator")
        print("=" * 70)

        self._validate_functions()
        self._validate_access_control()
        self._validate_events()
        self._validate_event_fields()
        self._validate_error_codes()
        self._validate_spec_structure()

        return self._report_results()

    def _validate_access_control(self):
        """
        Verify that function access levels in the spec match the authentication
        enforced in the contract source code (#919).
        """
        print("\n🔒 Validating Access Control...")
        # Derive access map from contract source using check-auth-matrix rules
        parts = self.source.split("pub fn ")[1:]
        access_map = {}

        for part in parts:
            match = re.match(r"^([a-zA-Z0-9_]+)\s*\(", part)
            if not match:
                continue
            func_name = match.group(1)

            if func_name == "initialize" or func_name.startswith(("get_", "preview_", "convert_")) or func_name == "is_paused":
                continue

            brace_count = 0
            in_body = False
            body_end = 0
            for i, char in enumerate(part):
                if char == '{':
                    in_body = True
                    brace_count += 1
                elif char == '}':
                    brace_count -= 1
                    if in_body and brace_count == 0:
                        body_end = i
                        break

            body = part[:body_end + 1] if body_end > 0 else part
            access = "anyone"

            auth_match = re.search(r"([a-zA-Z0-9_]+)\.require_auth\(\)", body)
            if auth_match:
                var = auth_match.group(1)
                if var == "owner":
                    access = "owner"
                elif var == "agent":
                    access = "agent"
                elif var == "user":
                    access = "user"
                elif var in ("new_owner", "pending_owner"):
                    access = "pending-owner"
            elif "require_auth" in body:
                if "owner" in body.lower():
                    access = "owner"
                elif "user" in body.lower():
                    access = "user"
                elif "agent" in body.lower():
                    access = "agent"
            else:
                if (
                    "Self::require_is_owner" in body
                    or "OnlyOwner" in body
                    or "VaultError::CallerIsNotOwner" in body
                    or "Self::set_rate_limit(" in body
                    or "Self::set_max_batch_size(" in body
                ):
                    access = "owner"
                elif (
                    "Self::require_is_agent" in body
                    or "OnlyAgent" in body
                    or "VaultError::OnlyAgentCanUpdateTotalAssets" in body
                    or "VaultError::UnauthorizedAgent" in body
                    or "Self::rebalance(" in body
                    or "Self::harvest(" in body
                ):
                    access = "agent"
                elif "Self::require_is_pending_owner" in body or "CallerIsNotPendingOwner" in body:
                    access = "pending-owner"
                elif "emergency_harvest" in func_name and "owner" in body.lower():
                    access = "owner"

            access_map[func_name] = access

        spec_funcs = {f["name"]: f for f in self.spec.get("functions", [])}
        mismatches = 0

        for fn_name, expected_access in access_map.items():
            spec_fn = spec_funcs.get(fn_name)
            if not spec_fn:
                continue

            raw_access = spec_fn.get("access", "public")
            requires_auth = spec_fn.get("requires_auth", False)

            if raw_access in ("owner-only", "owner"):
                actual = "owner"
            elif raw_access in ("agent-only", "agent"):
                actual = "agent"
            elif raw_access in ("pending-owner-only", "pending-owner"):
                actual = "pending-owner"
            elif raw_access in ("user-only", "user"):
                actual = "user"
            elif raw_access == "public":
                actual = "user" if requires_auth else "anyone"
            elif raw_access in ("anyone", "permissionless"):
                actual = "anyone"
            else:
                actual = raw_access

            if actual != expected_access:
                self.errors.append(
                    f"Function '{fn_name}': spec access is '{raw_access}' (auth={requires_auth}) "
                    f"but source code requires '{expected_access}'"
                )
                mismatches += 1

        if mismatches == 0:
            self.info.append("✓ All function access levels in spec match contract source code")

    # ------------------------------------------------------------------
    # Function validation
    # ------------------------------------------------------------------

    def _validate_functions(self):
        print("\n📋 Validating Functions...")
        contract_fns = self._extract_contract_functions()
        spec_fns = {f["name"] for f in self.spec.get("functions", [])}

        print(f"   Contract has {len(contract_fns)} public functions")
        print(f"   Spec documents {len(spec_fns)} functions")

        for fn in sorted(contract_fns - spec_fns):
            self.errors.append(f"Function '{fn}' exists in contract but is absent from spec")

        for fn in sorted(spec_fns - contract_fns):
            self.warnings.append(
                f"Function '{fn}' is in spec but not in contract (deprecated?)"
            )

        self._validate_function_parameters(contract_fns)

        if not (contract_fns - spec_fns) and not (spec_fns - contract_fns):
            self.info.append("✓ All contract functions are documented in spec")

    def _extract_contract_functions(self) -> Set[str]:
        pattern = r"pub\s+(?:async\s+)?fn\s+(\w+)\s*\("
        return set(re.findall(pattern, self.source))

    def _validate_function_parameters(self, contract_fns: Set[str]):
        for fn_name in contract_fns:
            pattern = rf"pub\s+(?:async\s+)?fn\s+{fn_name}\s*\(([^)]*)\)"
            match = re.search(pattern, self.source)
            if not match:
                continue
            params = match.group(1)
            spec_fn = next(
                (f for f in self.spec.get("functions", []) if f["name"] == fn_name),
                None,
            )
            if not spec_fn:
                continue
            if "env" in params and not any(
                p.get("name") == "env" for p in spec_fn.get("parameters", [])
            ):
                self.warnings.append(
                    f"Function '{fn_name}': 'env' parameter not documented in spec"
                )

    # ------------------------------------------------------------------
    # Event type-name validation
    # ------------------------------------------------------------------

    def _validate_events(self):
        print("\n📢 Validating Event Types...")
        pattern = r"pub struct (\w*Event)\s*\{"
        contract_events = set(re.findall(pattern, self.source))
        spec_events = {e["name"] for e in self.spec.get("events", [])}

        print(f"   Contract defines {len(contract_events)} event structs")
        print(f"   Spec documents {len(spec_events)} event types")

        for ev in sorted(contract_events - spec_events):
            self.warnings.append(f"Event '{ev}' defined in contract but absent from spec")

        for ev in sorted(spec_events - contract_events):
            self.warnings.append(f"Event '{ev}' in spec but not found in contract")

        if contract_events == spec_events:
            self.info.append("✓ All contract events are documented in spec")

    # ------------------------------------------------------------------
    # Event field validation (#251)
    # ------------------------------------------------------------------

    def _validate_event_fields(self):
        """
        For every event documented in the spec, verify that its listed field
        names match the public fields of the corresponding Rust struct.

        Catches renames, additions, and removals of struct fields that would
        cause event payloads to silently drift from the spec.
        """
        print("\n🔎 Validating Event Field Names...")
        drift_found = False

        for spec_event in self.spec.get("events", []):
            event_name = spec_event.get("name", "")
            if not event_name:
                continue

            struct_pattern = (
                rf"pub\s+struct\s+{re.escape(event_name)}\s*\{{([^}}]*)\}}"
            )
            match = re.search(struct_pattern, self.source, re.DOTALL)
            if not match:
                continue  # already flagged by _validate_events

            struct_body = match.group(1)
            field_pattern = r"pub\s+(\w+)\s*:"
            contract_fields = set(re.findall(field_pattern, struct_body))

            spec_fields = {
                f["name"] for f in spec_event.get("fields", []) if "name" in f
            }

            for f in sorted(contract_fields - spec_fields):
                self.errors.append(
                    f"Event '{event_name}': field '{f}' exists in contract struct "
                    f"but is absent from spec — update spec to match"
                )
                drift_found = True

            for f in sorted(spec_fields - contract_fields):
                self.errors.append(
                    f"Event '{event_name}': field '{f}' is in spec but not in "
                    f"contract struct — remove from spec or add to struct"
                )
                drift_found = True

        if not drift_found:
            self.info.append("✓ All event field names match contract structs")

    # ------------------------------------------------------------------
    # Error code validation (#251)
    # ------------------------------------------------------------------

    def _validate_error_codes(self):
        """
        Extract VaultError enum variants and their numeric codes from source,
        then verify the spec's error entries match.

        Detects: renumbered codes, removed variants, or spec entries that no
        longer correspond to a real error variant.
        """
        print("\n❗ Validating Error Codes...")

        enum_match = re.search(
            r"pub\s+enum\s+VaultError\s*\{([^}]*)\}", self.source, re.DOTALL
        )
        if not enum_match:
            self.warnings.append(
                "Could not locate VaultError enum in source — skipping error code validation"
            )
            return

        contract_errors: Dict[int, str] = {}
        for variant, code_str in re.findall(r"(\w+)\s*=\s*(\d+)\s*,", enum_match.group(1)):
            contract_errors[int(code_str)] = variant

        print(f"   Contract defines {len(contract_errors)} VaultError variants")

        spec_vault_errors: Dict[int, str] = {}
        for key, val in self.spec.get("errors", {}).items():
            if not key.startswith("VaultError::"):
                continue
            variant_name = key[len("VaultError::"):]
            code = val.get("code") if isinstance(val, dict) else None
            if code is None:
                self.warnings.append(f"Spec error entry '{key}' has no 'code' field")
                continue
            spec_vault_errors[int(code)] = variant_name

        print(f"   Spec documents {len(spec_vault_errors)} VaultError entries")

        drift_found = False

        for code, variant in sorted(contract_errors.items()):
            if code not in spec_vault_errors:
                self.errors.append(
                    f"VaultError::{variant} (code {code}) exists in contract "
                    f"but is absent from spec errors"
                )
                drift_found = True
            elif spec_vault_errors[code] != variant:
                self.errors.append(
                    f"Error code {code}: contract has '{variant}' but spec has "
                    f"'{spec_vault_errors[code]}' — name drift detected"
                )
                drift_found = True

        for code, variant in sorted(spec_vault_errors.items()):
            if code not in contract_errors:
                self.errors.append(
                    f"Spec error VaultError::{variant} (code {code}) has no "
                    f"matching variant in the contract enum"
                )
                drift_found = True

        if not drift_found:
            self.info.append("✓ All VaultError codes match between contract and spec")

    # ------------------------------------------------------------------
    # Spec structure validation
    # ------------------------------------------------------------------

    def _validate_spec_structure(self):
        print("\n🔍 Validating Spec Structure...")
        required = ["version", "contract", "network", "functions", "events", "errors"]
        missing = [f for f in required if f not in self.spec]
        if missing:
            for f in missing:
                self.errors.append(f"Spec is missing required top-level field: '{f}'")
        else:
            self.info.append("✓ Spec has all required top-level fields")

        for fn in self.spec.get("functions", []):
            if "name" not in fn:
                self.errors.append("A function spec entry is missing the 'name' field")
            if "description" not in fn:
                self.warnings.append(f"Function '{fn.get('name')}' is missing 'description'")
            if "parameters" not in fn:
                self.warnings.append(f"Function '{fn.get('name')}' is missing 'parameters'")

        for ev in self.spec.get("events", []):
            if "name" not in ev:
                self.errors.append("An event spec entry is missing the 'name' field")
            if "description" not in ev:
                self.warnings.append(f"Event '{ev.get('name')}' is missing 'description'")

    # ------------------------------------------------------------------
    # Report
    # ------------------------------------------------------------------

    def _report_results(self) -> bool:
        print("\n" + "=" * 70)
        print("Validation Results")
        print("=" * 70)

        if self.info:
            print("\n✅ Passed Checks:")
            for msg in self.info:
                print(f"   {msg}")

        if self.warnings:
            print("\n⚠️  Warnings:")
            for msg in self.warnings:
                print(f"   ⚠️  {msg}")

        if self.errors:
            print("\n❌ Errors (spec must be updated):")
            for msg in self.errors:
                print(f"   ❌ {msg}")
            print(
                "\n💡 To fix: update contract-spec.json so every function, event field,\n"
                "   and error code matches the contract source, then re-run:\n"
                "   python3 scripts/validate-spec.py"
            )
            return False

        print("\n✅ All validation checks passed — spec is current.")
        return True


def main():
    contract_path = "neurowealth-vault/contracts/vault/src/lib.rs"
    spec_path = "contract-spec.json"

    try:
        validator = ContractValidator(contract_path, spec_path)
        success = validator.validate()
        sys.exit(0 if success else 1)

    except FileNotFoundError as e:
        print(f"❌ {e}", file=sys.stderr)
        sys.exit(1)
    except json.JSONDecodeError as e:
        print(f"❌ Invalid JSON in spec: {e}", file=sys.stderr)
        sys.exit(1)
    except Exception as e:
        print(f"❌ Validation failed: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
