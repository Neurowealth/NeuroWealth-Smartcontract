#!/usr/bin/env python3
"""
Cross-checks the access-control documentation in SECURITY.md against the
actual `require_auth` / access-guard logic implemented in the Rust vault
contract (lib.rs). Fails (exit code 1) if either:
  - a function documented in SECURITY.md can't be found as a state-changing
    function in lib.rs, or
  - the access level documented in SECURITY.md doesn't match what the
    Rust code actually enforces.

Intended to run in CI so that the auth-matrix docs can't silently drift
out of sync with the contract's real permission checks.
"""
import sys
import re
import os


def parse_security_md(filepath):
    """
    Parse the markdown table in SECURITY.md that documents, per function,
    which roles (owner / agent / user / anyone / pending-owner) are allowed
    to call it. Returns a dict of {function_name: access_level}.
    """
    with open(filepath, "r", encoding="utf-8") as f:
        content = f.read()

    # Scan line by line for the table that starts with a "| Function" header
    # row, collecting the raw table rows until the table ends (a blank line
    # or non-table content).
    in_table = False
    table_lines = []
    for line in content.splitlines():
        stripped = line.strip()
        if stripped.startswith("| Function"):
            # Found the header row — start capturing rows after this point.
            in_table = True
            continue
        if in_table:
            # Skip the markdown separator row, e.g. "|---|---|---|"
            if stripped.startswith("|---") or stripped.startswith("| ---"):
                continue
            if stripped.startswith("|") and not stripped.startswith("| Function"):
                table_lines.append(stripped)
            else:
                # Non-table line encountered — the table has ended.
                break

    functions = {}
    for line in table_lines:
        # Split on "|" and strip whitespace from each cell, discarding the
        # empty strings produced by leading/trailing pipe characters.
        cells = [c.strip() for c in line.split("|")]
        cells = [c for c in cells if c]
        if len(cells) < 5:
            # Malformed/incomplete row — not enough columns to parse safely.
            continue

        func_name = cells[0].strip()
        # Columns 2-4 are boolean-ish "yes"/"✅" checkmarks for each role.
        owner = cells[1].strip() in ("yes", "✅")
        agent = cells[2].strip() in ("yes", "✅")
        user = cells[3].strip() in ("yes", "✅")
        # Column 5 ("Anyone") can instead hold special string values that
        # override the simple owner/agent/user flags above.
        anyone_raw = cells[4].strip()

        if anyone_raw == "pending owner":
            access = "pending-owner"
        elif anyone_raw == "anyone":
            access = "anyone"
        elif owner:
            access = "owner"
        elif agent:
            access = "agent"
        elif user:
            access = "user"
        else:
            # No role flag set and no special "anyone" value — can't
            # determine the intended access level from the table row.
            access = "unknown"

        functions[func_name] = access
    return functions


def parse_lib_rs(filepath):
    """
    Parse the Rust contract source to infer, per `pub fn`, which role is
    actually required to call it, based on `require_auth()` calls and a
    set of known guard-function / error-variant heuristics. Returns a dict
    of {function_name: access_level}.
    """
    with open(filepath, "r", encoding="utf-8") as f:
        content = f.read()

    # Split the file on "pub fn " so each `part` starts right after that
    # keyword, at the function name. Skip index 0 (the content before the
    # first "pub fn").
    parts = content.split("pub fn ")[1:]
    functions = {}

    for part in parts:
        # The function name is whatever identifier immediately precedes
        # the opening parenthesis of the argument list.
        match = re.match(r"^([a-zA-Z0-9_]+)\s*\(", part)
        if not match:
            continue
        func_name = match.group(1)

        # Find the end of this function's body by counting braces from the
        # first "{" until they balance back out to 0. This is a simple
        # (non-syntax-aware) brace matcher — good enough for well-formatted
        # Rust source, but can be thrown off by braces inside strings or
        # comments.
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

        # If we found a matching closing brace, slice out just this
        # function's body; otherwise fall back to scanning the whole
        # remaining text (covers edge cases where brace matching failed).
        body = part[:body_end + 1] if body_end > 0 else part

        # Default assumption: no auth check found => open to anyone.
        access = "anyone"

        # First, look for an explicit `<var>.require_auth()` call and use
        # the variable name to decide which role is being authenticated.
        auth_match = re.search(r"([a-zA-Z0-9_]+)\.require_auth\(\)", body)
        if auth_match:
            var = auth_match.group(1)
            if var == "owner":
                access = "owner"
            elif var == "agent":
                access = "agent"
            elif var == "user":
                access = "user"
            elif var == "new_owner":
                access = "pending-owner"
        elif "require_auth" in body:
            # require_auth() is called, but not in the simple "<var>.require_auth()"
            # form matched above — fall back to keyword-sniffing the body text
            # to guess which role it's checking.
            if "owner" in body.lower():
                access = "owner"
            elif "user" in body.lower():
                access = "user"
            elif "agent" in body.lower():
                access = "agent"
        else:
            # No require_auth() call at all — check for known helper
            # functions / custom error variants that imply a role-gated
            # permission check via a different mechanism.
            if "Self::require_is_owner" in body or "OnlyOwner" in body or "VaultError::CallerIsNotOwner" in body:
                access = "owner"
            elif "Self::require_is_agent" in body or "OnlyAgent" in body or "VaultError::OnlyAgentCanUpdateTotalAssets" in body:
                access = "agent"
            elif "Self::require_is_pending_owner" in body or "CallerIsNotPendingOwner" in body:
                access = "pending-owner"
            elif "emergency_harvest" in func_name and "owner" in body.lower():
                # Special-cased fallback for emergency_harvest, which may
                # guard ownership a different way than the patterns above.
                access = "owner"

        # `initialize` is a one-time constructor-like function, not a
        # regular state-changing function subject to the auth matrix —
        # exclude it from the comparison entirely.
        if func_name == "initialize":
            continue

        # Read-only / view-style functions (getters, previews, converters,
        # pause-status check) aren't part of the state-changing auth
        # matrix either, so skip them too.
        if func_name.startswith("get_") or func_name.startswith("preview_") or func_name.startswith("convert_") or func_name == "is_paused":
            continue

        functions[func_name] = access

    return functions


def main():
    # Resolve paths relative to the repo root (this script is assumed to
    # live one directory below the repo root, e.g. in a `scripts/` folder).
    repo_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    security_md_path = os.path.join(repo_root, "SECURITY.md")
    lib_rs_path = os.path.join(repo_root, "neurowealth-vault", "contracts", "vault", "src", "lib.rs")

    sec_funcs = parse_security_md(security_md_path)
    lib_funcs = parse_lib_rs(lib_rs_path)

    errors = False

    # For every function documented in SECURITY.md, confirm it exists in
    # the contract source and that the documented access level matches
    # what the code actually enforces.
    for func, sec_access in sec_funcs.items():
        if func not in lib_funcs:
            print(f"FAIL: Function {func} is in SECURITY.md but not found as a state-changing function in lib.rs.")
            errors = True
        else:
            lib_access = lib_funcs[func]
            if lib_access != sec_access:
                print(f"FAIL: Mismatch for {func}: SECURITY.md says '{sec_access}', lib.rs code says '{lib_access}'")
                errors = True

    if not errors:
        print("PASS: Auth matrix matches between SECURITY.md and lib.rs")
        sys.exit(0)
    else:
        sys.exit(1)


if __name__ == "__main__":
    main()