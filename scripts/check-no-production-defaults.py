#!/usr/bin/env python3
"""Fail CI when production-sensitive env vars have source-level defaults."""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CHECKS = {
    "whatsapp/src/cryptoUtils.ts": ("ENCRYPTION_KEY", "PHONE_HASH_SALT"),
    "whatsapp/src/keyring.ts": ("ENCRYPTION_KEY", "ENCRYPTION_KEY_VERSION", "ENCRYPTION_KEY_RETIRED"),
    "whatsapp/src/db.ts": ("DATABASE_URL",),
    "whatsapp/src/index.ts": ("TWILIO_AUTH_TOKEN",),
    "whatsapp/src/twilioSignature.ts": ("TWILIO_AUTH_TOKEN",),
    "frontend/src/lib/stellar.ts": ("NEXT_PUBLIC_VAULT_CONTRACT_ID",),
    "whatsapp/src/vaultRouter.ts": ("VAULT_CONTRACT_ID",),
}

# Entry points that must run the shared boot-time env validation (#945). Each
# listed file has to import the guard before anything reads configuration.
REQUIRED_GUARDS = {
    "whatsapp/src/index.ts": "./bootstrapEnv",
    "whatsapp/src/rotateKeys.ts": "./bootstrapEnv",
}

FALLBACK_TEMPLATE = r"process\.env\.{name}\s*(?:\|\||\?\?)\s*(['\"`])([^'\"`]+)\1"


def main() -> int:
    failures: list[str] = []

    for relative, names in CHECKS.items():
        path = ROOT / relative
        if not path.exists():
            failures.append(f"missing expected file: {relative}")
            continue

        text = path.read_text(encoding="utf-8")
        for name in names:
            pattern = re.compile(FALLBACK_TEMPLATE.format(name=re.escape(name)))
            match = pattern.search(text)
            if match:
                failures.append(
                    f"{relative}: {name} has a hardcoded fallback value; use fail-closed env validation instead"
                )

    for relative, marker in REQUIRED_GUARDS.items():
        path = ROOT / relative
        if not path.exists():
            failures.append(f"missing expected file: {relative}")
            continue
        text = path.read_text(encoding="utf-8")
        if f"'{marker}'" not in text and f'"{marker}"' not in text:
            failures.append(f"{relative}: must import {marker} so insecure env defaults are rejected on boot")

    if failures:
        print("Production default configuration check failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1

    print("No production-sensitive hardcoded env defaults found.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())