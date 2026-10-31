#!/usr/bin/env python3
"""i18n key-parity check for the frontend message catalogs (#932).

The frontend ships one catalog per locale under frontend/messages/<locale>.json.
`en.json` is the reference catalog; every other locale must define exactly the
same set of keys (nested objects flattened to dotted paths), with the same
shape: a key that is a string in `en` must be a string everywhere, and a key
that is a namespace object in `en` must be an object everywhere.

The check also verifies the locale list in frontend/src/i18n/request.ts and
frontend/src/middleware.ts agrees with the catalogs on disk, so adding a locale
without a catalog (or a catalog without routing) fails too.

Usage:
  python3 scripts/check-i18n-parity.py                 # check the repo (CI)
  python3 scripts/check-i18n-parity.py --messages DIR  # check another catalog dir

Exit status is 1 when any locale has missing or extra keys, a shape mismatch,
or the locale configuration disagrees with the catalog files.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
FRONTEND = REPO_ROOT / "frontend"
DEFAULT_MESSAGES_DIR = FRONTEND / "messages"
REFERENCE_LOCALE = "en"
LOCALE_CONFIG_FILES = (
    FRONTEND / "src" / "i18n" / "request.ts",
    FRONTEND / "src" / "middleware.ts",
)
LOCALES_ARRAY = re.compile(r"locales\s*[:=]\s*\[([^\]]*)\]")
LOCALE_LITERAL = re.compile(r"""['"]([A-Za-z-]+)['"]""")


def flatten(node: object, prefix: str = "") -> dict[str, str]:
    """Maps every dotted key path to its kind: 'object' or 'string' (or another JSON type)."""
    shapes: dict[str, str] = {}
    if not isinstance(node, dict):
        return shapes
    for key, value in node.items():
        path = f"{prefix}.{key}" if prefix else key
        if isinstance(value, dict):
            shapes[path] = "object"
            shapes.update(flatten(value, path))
        else:
            shapes[path] = "string" if isinstance(value, str) else type(value).__name__
    return shapes


def load_catalog(path: Path) -> dict[str, str] | None:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        print(f"::error file={path}::cannot parse {path.name}: {exc}")
        return None
    if not isinstance(data, dict):
        print(f"::error file={path}::{path.name} must contain a JSON object")
        return None
    return flatten(data)


def configured_locales(config_file: Path) -> set[str] | None:
    if not config_file.exists():
        return None
    match = LOCALES_ARRAY.search(config_file.read_text(encoding="utf-8"))
    if not match:
        return None
    return set(LOCALE_LITERAL.findall(match.group(1)))


def check(messages_dir: Path, check_config: bool) -> list[str]:
    errors: list[str] = []
    catalogs = {path.stem: path for path in sorted(messages_dir.glob("*.json"))}

    if REFERENCE_LOCALE not in catalogs:
        return [f"reference catalog {REFERENCE_LOCALE}.json not found in {messages_dir}"]

    reference = load_catalog(catalogs[REFERENCE_LOCALE])
    if reference is None:
        return [f"reference catalog {REFERENCE_LOCALE}.json is invalid"]

    for locale, path in catalogs.items():
        if locale == REFERENCE_LOCALE:
            continue
        shapes = load_catalog(path)
        if shapes is None:
            errors.append(f"{locale}: catalog is invalid")
            continue
        missing = sorted(set(reference) - set(shapes))
        extra = sorted(set(shapes) - set(reference))
        mismatched = sorted(
            key for key in set(reference) & set(shapes) if reference[key] != shapes[key]
        )
        for key in missing:
            errors.append(f"{locale}: missing key '{key}' (present in {REFERENCE_LOCALE})")
        for key in extra:
            errors.append(f"{locale}: extra key '{key}' (not in {REFERENCE_LOCALE})")
        for key in mismatched:
            errors.append(
                f"{locale}: key '{key}' is a {shapes[key]} but is a {reference[key]} in {REFERENCE_LOCALE}"
            )

    if check_config:
        on_disk = set(catalogs)
        for config_file in LOCALE_CONFIG_FILES:
            locales = configured_locales(config_file)
            rel = config_file.relative_to(REPO_ROOT)
            if locales is None:
                errors.append(f"{rel}: could not find a `locales` array")
                continue
            for locale in sorted(locales - on_disk):
                errors.append(f"{rel}: locale '{locale}' has no messages/{locale}.json catalog")
            for locale in sorted(on_disk - locales):
                errors.append(f"{rel}: catalog messages/{locale}.json is not a configured locale")

    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--messages",
        type=Path,
        default=None,
        help="catalog directory to check (default: frontend/messages, also checks locale config)",
    )
    args = parser.parse_args()

    messages_dir = args.messages or DEFAULT_MESSAGES_DIR
    errors = check(messages_dir, check_config=args.messages is None)

    if errors:
        print(f"i18n key parity check FAILED ({len(errors)} problem(s)):")
        for error in errors:
            print(f"  - {error}")
        return 1

    count = len(list(messages_dir.glob("*.json")))
    print(f"i18n key parity OK: {count} locale catalogs match {REFERENCE_LOCALE}.json")
    return 0


if __name__ == "__main__":
    sys.exit(main())
