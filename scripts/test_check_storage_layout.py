#!/usr/bin/env python3
"""
Unit tests for check-storage-layout.py:
- Identical layouts pass
- Appended variants pass
- Reordered variants are detected as incompatible
- Removed variants are detected as incompatible
- Mutated types are detected as incompatible
- Migration doc validation checks section presence and all incompatible keys
"""

import importlib.util
import tempfile
import unittest
from pathlib import Path

# Load check-storage-layout.py dynamically
SCRIPT_PATH = Path(__file__).parent / "check-storage-layout.py"
spec = importlib.util.spec_from_file_location("check_storage_layout", SCRIPT_PATH)
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

compare_storage_layouts = mod.compare_storage_layouts
validate_migration_doc = mod.validate_migration_doc


class TestStorageLayoutComparison(unittest.TestCase):
    def setUp(self):
        self.base_cases = [
            {"index": 0, "kind": "tuple", "name": "Initialized", "types": [], "raw": {}},
            {"index": 1, "kind": "tuple", "name": "Owner", "types": ["address"], "raw": {}},
            {"index": 2, "kind": "tuple", "name": "Agent", "types": ["address"], "raw": {}},
            {"index": 3, "kind": "tuple", "name": "TotalAssets", "types": ["i128"], "raw": {}},
        ]

    def test_identical_layouts_are_compatible(self):
        new_cases = [dict(c) for c in self.base_cases]
        is_compat, incomp_keys, issues, _ = compare_storage_layouts(self.base_cases, new_cases)
        self.assertTrue(is_compat)
        self.assertEqual(len(incomp_keys), 0)
        self.assertEqual(len(issues), 0)

    def test_appended_variants_are_compatible(self):
        new_cases = [dict(c) for c in self.base_cases]
        new_cases.append({"index": 4, "kind": "tuple", "name": "NewSetting", "types": ["u32"], "raw": {}})
        is_compat, incomp_keys, issues, _ = compare_storage_layouts(self.base_cases, new_cases)
        self.assertTrue(is_compat)
        self.assertEqual(len(incomp_keys), 0)
        self.assertEqual(len(issues), 0)

    def test_reordered_variants_are_incompatible(self):
        # Swap Owner and Agent
        new_cases = [
            self.base_cases[0],
            self.base_cases[2],  # Agent at index 1
            self.base_cases[1],  # Owner at index 2
            self.base_cases[3],
        ]
        is_compat, incomp_keys, issues, _ = compare_storage_layouts(self.base_cases, new_cases)
        self.assertFalse(is_compat)
        self.assertIn("Owner", incomp_keys)
        self.assertIn("Agent", incomp_keys)
        self.assertTrue(any("mismatch at discriminant index 1" in issue for issue in issues))

    def test_removed_variants_are_incompatible(self):
        # Remove TotalAssets at index 3
        new_cases = self.base_cases[:3]
        is_compat, incomp_keys, issues, _ = compare_storage_layouts(self.base_cases, new_cases)
        self.assertFalse(is_compat)
        self.assertIn("TotalAssets", incomp_keys)
        self.assertTrue(any("REMOVED in new WASM" in issue for issue in issues))

    def test_type_mutation_is_incompatible(self):
        # Change TotalAssets from i128 to i64
        new_cases = [dict(c) for c in self.base_cases]
        new_cases[3] = {"index": 3, "kind": "tuple", "name": "TotalAssets", "types": ["i64"], "raw": {}}
        is_compat, incomp_keys, issues, _ = compare_storage_layouts(self.base_cases, new_cases)
        self.assertFalse(is_compat)
        self.assertIn("TotalAssets", incomp_keys)
        self.assertTrue(any("type signature mismatch" in issue for issue in issues))


class TestMigrationDocValidation(unittest.TestCase):
    def test_missing_doc_fails(self):
        valid, errors = validate_migration_doc(Path("non_existent_doc.md"), ["TotalAssets"])
        self.assertFalse(valid)
        self.assertTrue(any("not found" in e for e in errors))

    def test_empty_doc_fails(self):
        with tempfile.NamedTemporaryFile("w+", delete=False, suffix=".md") as f:
            f.write("")
            path = Path(f.name)
        valid, errors = validate_migration_doc(path, ["TotalAssets"])
        self.assertFalse(valid)
        self.assertTrue(any("empty" in e for e in errors))

    def test_doc_without_storage_section_fails(self):
        with tempfile.NamedTemporaryFile("w+", delete=False, suffix=".md") as f:
            f.write("# General Changelog\n\nSome changes were made to TotalAssets.")
            path = Path(f.name)
        valid, errors = validate_migration_doc(path, ["TotalAssets"])
        self.assertFalse(valid)
        self.assertTrue(any("missing required section" in e for e in errors))

    def test_doc_without_mentioning_incompatible_key_fails(self):
        with tempfile.NamedTemporaryFile("w+", delete=False, suffix=".md") as f:
            f.write("# Contract Storage Migration Guide\n\nWe migrated OldKey to NewKey.")
            path = Path(f.name)
        valid, errors = validate_migration_doc(path, ["TotalAssets"])
        self.assertFalse(valid)
        self.assertTrue(any("fails to mention incompatible DataKey variant(s): TotalAssets" in e for e in errors))

    def test_valid_doc_with_section_and_keys_passes(self):
        with tempfile.NamedTemporaryFile("w+", delete=False, suffix=".md") as f:
            f.write("# Storage Migration Plan\n\nBreaking change: TotalAssets was migrated.")
            path = Path(f.name)
        valid, errors = validate_migration_doc(path, ["TotalAssets"])
        self.assertTrue(valid)
        self.assertEqual(len(errors), 0)


if __name__ == "__main__":
    unittest.main()
