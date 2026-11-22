#!/usr/bin/env python3
"""Secret scan for env templates and key material (#766).

Complements gitleaks (.gitleaks.toml) with repo-specific policy that a
generic ruleset cannot express:

1. Real env files (.env, .env.mainnet, packages/x/.env.local, ...) must never
   be committed — only *.template / *.example / *.sample variants.
2. Private-key files (*.pem, *.key, *.p12, id_rsa, ...) must never be committed.
3. Env templates may only carry placeholders for secret-looking variables
   (*_SECRET*, *_KEY, *TOKEN*, *PASSWORD*, *SALT*, *SEED*, *MNEMONIC*, webhooks).
4. No template line (comments included) may contain key material: Stellar
   secret seeds, PEM private keys, raw 32-byte hex keys, AWS/OpenAI/Slack
   tokens, or credentials embedded in URLs.

Usage:
  python3 scripts/check-env-templates.py            # scan tracked files (CI)
  python3 scripts/check-env-templates.py --staged   # scan the index (pre-commit)

Exit status is 1 when any violation is found. Documented in
docs/SECRETS_HYGIENE.md.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import PurePosixPath

TEMPLATE_SUFFIXES = (".template", ".example", ".sample")

# Any env file: ".env", ".env.<anything>", "<name>.env".
ENV_FILE = re.compile(r"(^|/)(\.env(\.[^/]+)?|[^/]+\.env)$")

KEY_FILE = re.compile(
    r"(^|/)(id_(rsa|dsa|ecdsa|ed25519)|[^/]+\.(pem|key|p12|pfx|jks|keystore|secret))$",
    re.IGNORECASE,
)

SECRET_VAR = re.compile(
    r"(SECRET|PRIVATE|PASSWORD|PASSWD|TOKEN|API_?KEY|_KEY$|^KEY$|SALT|SEED|MNEMONIC|WEBHOOK|CREDENTIAL)",
    re.IGNORECASE,
)
# Public identifiers that merely contain a secret-looking word
# (e.g. MAINNET_USDC_TOKEN_ADDRESS, STELLAR_USDC_TOKEN_ID).
PUBLIC_VAR = re.compile(r"(_ADDRESS|_CONTRACT_ID|_TOKEN_ID|_ID)$", re.IGNORECASE)

PLACEHOLDER = re.compile(
    r"""^(
        |                             # empty
        [A-Z0-9]{0,2}\.\.\.           # "...", "S...", "0x..."
        |<[^>]*>                      # <your-secret>
        |\$\{[^}]*\}|\$[A-Z_]+        # ${VAR} / $VAR
        |your[-_].*|.*[-_]here        # your-anon-key / put-key-here
        |changeme|change[-_]me|placeholder.*|example.*|dummy.*|test|none|null
        |x{3,}|X{3,}|\*{3,}
        |true|false|\d+
    )$""",
    re.IGNORECASE | re.VERBOSE,
)

CONTENT_RULES = [
    (
        "Stellar secret seed",
        re.compile(r"\bS[A-Z2-7]{55}\b"),
        # "SXXXX…" style documentation placeholders.
        lambda m: re.fullmatch(r"S(X+|\.+)", m.group(0)) is not None,
    ),
    ("PEM private key", re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----"), None),
    ("raw 32-byte hex key", re.compile(r"\b0x[0-9a-fA-F]{64}\b"), None),
    ("AWS access key id", re.compile(r"\b(AKIA|ASIA)[0-9A-Z]{16}\b"), None),
    ("OpenAI API key", re.compile(r"\bsk-(proj-)?[A-Za-z0-9_-]{20,}\b"), None),
    ("Slack token", re.compile(r"\bxox[abprs]-[A-Za-z0-9-]{10,}\b"), None),
    ("Twilio auth token/SID", re.compile(r"\b(AC|SK)[0-9a-f]{32}\b"), None),
    (
        "credentials embedded in URL",
        re.compile(r"[a-z][a-z0-9+.-]*://([^/\s:@]+):([^@\s/]+)@", re.IGNORECASE),
        lambda m: PLACEHOLDER.match(m.group(2)) is not None
        or m.group(2).lower() in {"password", "pass", "secret"},
    ),
]


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], check=True, capture_output=True, text=True
    ).stdout


def list_files(staged: bool) -> list[str]:
    if staged:
        out = git("diff", "--cached", "--name-only", "--diff-filter=ACMR", "-z")
    else:
        out = git("ls-files", "-z")
    return [p for p in out.split("\0") if p and "node_modules/" not in p]


def read(path: str, staged: bool) -> str:
    if staged:
        return git("show", f":{path}")
    with open(path, encoding="utf-8", errors="replace") as fh:
        return fh.read()


def is_template(path: str) -> bool:
    return PurePosixPath(path).name.lower().endswith(TEMPLATE_SUFFIXES)


def parse_assignment(line: str) -> tuple[str, str] | None:
    m = re.match(r"^\s*(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)$", line)
    if not m:
        return None
    value = m.group(2).strip()
    # Drop trailing inline comment on unquoted values.
    if not value.startswith(("'", '"')):
        value = value.split(" #", 1)[0].strip()
    return m.group(1), value.strip("'\"")


def scan_template(path: str, text: str) -> list[str]:
    problems = []
    for lineno, line in enumerate(text.splitlines(), 1):
        where = f"{path}:{lineno}"
        for name, pattern, allowed in CONTENT_RULES:
            for match in pattern.finditer(line):
                if allowed is None or not allowed(match):
                    problems.append(f"{where}: {name} in env template")
        if line.lstrip().startswith("#"):
            continue
        assignment = parse_assignment(line)
        if assignment is None:
            continue
        var, value = assignment
        if SECRET_VAR.search(var) and not PUBLIC_VAR.search(var):
            if not PLACEHOLDER.match(value):
                problems.append(
                    f"{where}: {var} has a non-placeholder value; "
                    "templates must leave secrets empty or use a placeholder like <your-value>"
                )
    return problems


def main() -> int:
    staged = "--staged" in sys.argv[1:]
    problems: list[str] = []

    for path in list_files(staged):
        if KEY_FILE.search(path):
            problems.append(f"{path}: private-key file must not be committed")
            continue
        if not ENV_FILE.search(path):
            continue
        if not is_template(path):
            problems.append(
                f"{path}: real env file must not be committed "
                f"(commit a {'/'.join(TEMPLATE_SUFFIXES)} variant with placeholders)"
            )
            continue
        problems.extend(scan_template(path, read(path, staged)))

    if problems:
        print("Env template / key-material scan failed (#766):", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        print(
            "See docs/SECRETS_HYGIENE.md. If a key was ever committed, treat it as "
            "compromised and rotate it — removing it from history is not enough.",
            file=sys.stderr,
        )
        return 1

    print("Env template / key-material scan passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
