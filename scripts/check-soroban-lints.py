#!/usr/bin/env python3
"""
check-soroban-lints.py — catch common Soroban pitfalls in the vault contract.

A lightweight, dependency-free stand-in for custom clippy lints (dylint would
need a pinned nightly toolchain and a separate lint crate). The script parses
the production contract sources (test modules excluded) and enforces:

  unwrap-storage-get
      `env.storage().<tier>().get(..).unwrap()` / `.expect(..)`. A missing key
      aborts with an opaque host panic instead of a typed `VaultError`. Use
      `.unwrap_or(..)`, `.unwrap_or_else(|| panic_with_error!(..))`, or a
      helper that maps `None` to a contract error.

  missing-require-initialized
      A `pub fn` in the `#[contractimpl]` block that never reaches
      `require_initialized` (directly or through a helper such as
      `require_is_owner`). Uninitialised vaults must fail with
      `VaultError::NotInitialized`, not with a storage unwrap.

  missing-require-auth
      A `pub fn` in the `#[contractimpl]` block that mutates state (storage
      set/remove or a token transfer, directly or via helpers) without
      reaching `require_auth` / `require_auth_for_args`.

Call reachability is resolved transitively across every fn defined in the
scanned files, so guards implemented in helpers are recognised.

Suppressions
------------
1. Inline, with a reason, on the offending line or the line above it (for fn
   rules: anywhere in the doc/attribute block directly above `pub fn`):
       // soroban-lint: allow(missing-require-auth) permissionless TTL bump
2. The allowlist file (default: scripts/soroban-lints-allow.json):
     - "missing-require-initialized" / "missing-require-auth":
           {"fn_name": "reason", ...}
     - "unwrap-storage-get": {"fn_name": max_count, ...}
       A ratchet baseline for pre-existing call sites: the check fails if a
       function gains new storage unwraps, and asks you to lower the number
       once some are removed. Regenerate with --update-baseline.

Usage:
    python3 scripts/check-soroban-lints.py [--src DIR] [--allow FILE]
                                           [--update-baseline]

Exit codes: 0 clean, 1 violations found, 2 usage / parse error.
"""
import argparse
import json
import os
import re
import sys

RULES = (
    "unwrap-storage-get",
    "missing-require-initialized",
    "missing-require-auth",
)

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_SRC = os.path.join(REPO_ROOT, "neurowealth-vault", "contracts", "vault", "src")
DEFAULT_ALLOW = os.path.join(REPO_ROOT, "scripts", "soroban-lints-allow.json")

# Public entry points that legitimately run before initialisation.
INIT_EXEMPT = {"initialize"}

SUPPRESS_RE = re.compile(r"soroban-lint:\s*allow\(([\w\-,\s]+)\)")
STORAGE_GET_RE = re.compile(
    r"storage\(\)\s*\.\s*(?:instance|persistent|temporary)\(\)\s*\.\s*get\s*(?:::\s*<[^;{}]*?>\s*)?\("
)
STORAGE_WRITE_RE = re.compile(
    r"storage\(\)\s*\.\s*(?:instance|persistent|temporary)\(\)\s*\.\s*(?:set|remove)\s*(?:::\s*<[^;{}]*?>\s*)?\("
)
TOKEN_WRITE_RE = re.compile(r"\.\s*(?:transfer|transfer_from|mint|burn|burn_from|approve)\s*\(")
AUTH_RE = re.compile(r"\.\s*require_auth(?:_for_args)?\s*\(")
FN_RE = re.compile(r"\b(pub(?:\([^)]*\))?\s+)?fn\s+(\w+)")
CALL_RE = re.compile(r"\b(\w+)\s*(?:::\s*<[^;{}()]*?>\s*)?\(")


# ── Source preprocessing ────────────────────────────────────────────────────

def strip_code(src):
    """Blank out comments, string and char literals, preserving offsets and
    newlines so that positions still map to the original line numbers."""
    out = list(src)
    i, n = 0, len(src)

    def blank(a, b):
        for k in range(a, b):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        nxt = src[i + 1] if i + 1 < n else ""
        if c == "/" and nxt == "/":
            j = src.find("\n", i)
            j = n if j == -1 else j
            blank(i, j)
            i = j
        elif c == "/" and nxt == "*":
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            blank(i, j)
            i = j
        elif c == "r" and re.match(r'r#*"', src[i:i + 10]) and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            hashes = re.match(r'r(#*)"', src[i:]).group(1)
            end = src.find('"' + hashes, i + 2 + len(hashes))
            end = n if end == -1 else end + 1 + len(hashes)
            blank(i + 1, end)
            i = end
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            blank(i + 1, j)
            i = j + 1
        elif c == "'":
            # Char literal ('a', '\n', '\u{..}') vs lifetime ('a).
            m = re.match(r"'(?:\\[^']*|[^\\'])'", src[i:i + 12])
            if m:
                blank(i + 1, i + len(m.group(0)) - 1)
                i += len(m.group(0))
            else:
                i += 1
        else:
            i += 1
    return "".join(out)


def match_delim(code, open_idx, open_ch="{", close_ch="}"):
    depth = 0
    for k in range(open_idx, len(code)):
        if code[k] == open_ch:
            depth += 1
        elif code[k] == close_ch:
            depth -= 1
            if depth == 0:
                return k
    return -1


def strip_cfg_test(code):
    """Blank out `#[cfg(test)]` items (mods, fns, impls)."""
    out = list(code)
    for m in re.finditer(r"#\[cfg\(test\)\]", code):
        k = m.end()
        # Skip to the item body or a `;` (e.g. `mod tests;`).
        while k < len(code) and code[k] not in "{;":
            k += 1
        if k >= len(code):
            continue
        end = k if code[k] == ";" else match_delim(code, k)
        for j in range(m.start(), end + 1):
            if out[j] != "\n":
                out[j] = " "
    return "".join(out)


def line_of(code, idx):
    return code.count("\n", 0, idx) + 1


# ── Function extraction ─────────────────────────────────────────────────────

class Fn:
    def __init__(self, name, path, line, sig_start, body_start, body_end, is_pub, in_contract):
        self.name = name
        self.path = path
        self.line = line
        self.sig_start = sig_start
        self.body_start = body_start
        self.body_end = body_end
        self.is_pub = is_pub
        self.in_contract = in_contract


def contract_impl_ranges(code):
    ranges = []
    for m in re.finditer(r"#\[contractimpl\]", code):
        k = code.find("{", m.end())
        if k == -1:
            continue
        end = match_delim(code, k)
        if end != -1:
            ranges.append((k, end))
    return ranges


def extract_fns(path, code):
    impls = contract_impl_ranges(code)
    fns = []
    for m in FN_RE.finditer(code):
        # Find the body `{` after the signature's closing paren.
        p = code.find("(", m.end())
        if p == -1:
            continue
        close = match_delim(code, p, "(", ")")
        if close == -1:
            continue
        k = close + 1
        while k < len(code) and code[k] not in "{;":
            k += 1
        if k >= len(code) or code[k] == ";":
            continue  # trait method declaration / extern fn
        end = match_delim(code, k)
        if end == -1:
            continue
        in_contract = any(a < m.start() < b for a, b in impls)
        # Only methods directly inside the impl (depth 1) are entry points.
        if in_contract:
            a = next(a for a, b in impls if a < m.start() < b)
            depth = code[a:m.start()].count("{") - code[a:m.start()].count("}")
            in_contract = depth == 1
        fns.append(Fn(m.group(2), path, line_of(code, m.start()), m.start(), k, end,
                      bool(m.group(1)), in_contract))
    return fns


def own_body(code, fn, all_fns):
    """The fn body with nested fn items blanked out."""
    body = list(code[fn.body_start:fn.body_end + 1])
    for other in all_fns:
        if other is fn or other.path != fn.path:
            continue
        if fn.body_start < other.sig_start and other.body_end <= fn.body_end:
            for j in range(other.sig_start - fn.body_start, other.body_end - fn.body_start + 1):
                if body[j] != "\n":
                    body[j] = " "
    return "".join(body)


# ── Suppressions ────────────────────────────────────────────────────────────

def suppressions_by_line(raw):
    sup = {}
    for i, text in enumerate(raw.splitlines(), start=1):
        for m in SUPPRESS_RE.finditer(text):
            rules = {r.strip() for r in m.group(1).split(",") if r.strip()}
            sup.setdefault(i, set()).update(rules)
    return sup


def line_suppressed(sup, line, rule):
    return rule in sup.get(line, set()) or rule in sup.get(line - 1, set())


def fn_suppressed(raw_lines, sup, fn_line, rule):
    """Look through the doc-comment / attribute block directly above the fn."""
    if rule in sup.get(fn_line, set()):
        return True
    i = fn_line - 1
    while i >= 1:
        text = raw_lines[i - 1].strip()
        if not (text.startswith("//") or text.startswith("#[") or text.startswith("*")
                or text.startswith("/*") or text == ""):
            break
        if rule in sup.get(i, set()):
            return True
        if text == "":
            break
        i -= 1
    return False


# ── Analysis ────────────────────────────────────────────────────────────────

def closure(fns_by_name, calls, seed):
    """Names of fns that (transitively) reach a fn in `seed`."""
    reach = set(seed)
    changed = True
    while changed:
        changed = False
        for name in fns_by_name:
            if name not in reach and calls[name] & reach:
                reach.add(name)
                changed = True
    return reach


def find_storage_unwraps(code, body_offset, body):
    hits = []
    for m in STORAGE_GET_RE.finditer(body):
        close = match_delim(body, m.end() - 1, "(", ")")
        if close == -1:
            continue
        tail = body[close + 1:close + 80]
        if re.match(r"\s*\.\s*(?:unwrap|expect)\s*\(", tail):
            hits.append(line_of(code, body_offset + m.start()))
    return hits


def collect_sources(src_dir):
    files = []
    for root, dirs, names in os.walk(src_dir):
        dirs[:] = [d for d in dirs if d != "tests" and "fuzz" not in d]
        for name in names:
            if not name.endswith(".rs") or "test" in name or "fuzz" in name:
                continue
            files.append(os.path.join(root, name))
    return sorted(files)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--src", default=DEFAULT_SRC, help="contract src directory")
    ap.add_argument("--allow", default=DEFAULT_ALLOW, help="allowlist / baseline JSON")
    ap.add_argument("--update-baseline", action="store_true",
                    help="rewrite the unwrap-storage-get baseline to current counts")
    args = ap.parse_args()

    if not os.path.isdir(args.src):
        print(f"ERROR: source directory not found: {args.src}", file=sys.stderr)
        return 2

    allow = {r: {} for r in RULES}
    if os.path.exists(args.allow):
        with open(args.allow, encoding="utf-8") as f:
            loaded = json.load(f)
        unknown = set(loaded) - set(RULES) - {"_comment"}
        if unknown:
            print(f"ERROR: unknown rule(s) in {args.allow}: {sorted(unknown)}", file=sys.stderr)
            return 2
        for r in RULES:
            allow[r] = loaded.get(r, {})

    files = {}
    all_fns = []
    for path in collect_sources(args.src):
        with open(path, encoding="utf-8") as f:
            raw = f.read()
        code = strip_cfg_test(strip_code(raw))
        files[path] = (raw, code, suppressions_by_line(raw))
        all_fns.extend(extract_fns(path, code))

    entry_points = [fn for fn in all_fns if fn.in_contract and fn.is_pub]
    if not entry_points:
        print(f"ERROR: no #[contractimpl] pub fns found under {args.src}", file=sys.stderr)
        return 2

    # Per-name facts (names are merged across files; helpers are unique here).
    names = {fn.name for fn in all_fns}
    calls, auth_seed, write_seed = {n: set() for n in names}, set(), set()
    bodies = {}
    for fn in all_fns:
        code = files[fn.path][1]
        body = own_body(code, fn, all_fns)
        bodies[id(fn)] = body
        calls[fn.name] |= {c for c in CALL_RE.findall(body) if c in names and c != fn.name}
        if AUTH_RE.search(body):
            auth_seed.add(fn.name)
        if STORAGE_WRITE_RE.search(body) or TOKEN_WRITE_RE.search(body):
            write_seed.add(fn.name)

    reaches_init = closure(names, calls, {"require_initialized"} & names)
    reaches_auth = closure(names, calls, auth_seed)
    reaches_write = closure(names, calls, write_seed)

    violations = []

    def rel(path):
        r = os.path.relpath(path, REPO_ROOT)
        return path if r.startswith("..") else r

    # Rule: unwrap-storage-get (ratcheted per fn).
    unwrap_counts = {}
    for fn in all_fns:
        raw, code, sup = files[fn.path]
        hits = [ln for ln in find_storage_unwraps(code, fn.body_start, bodies[id(fn)])
                if not line_suppressed(sup, ln, "unwrap-storage-get")]
        if hits:
            unwrap_counts.setdefault(fn.name, []).extend((fn.path, ln) for ln in hits)

    if args.update_baseline:
        data = {"_comment": loaded.get("_comment") if os.path.exists(args.allow) else None}
        data = {k: v for k, v in data.items() if v}
        for r in RULES[1:]:
            data[r] = allow[r]
        data["unwrap-storage-get"] = {n: len(h) for n, h in sorted(unwrap_counts.items())}
        with open(args.allow, "w", encoding="utf-8") as f:
            json.dump(data, f, indent=2, ensure_ascii=False)
            f.write("\n")
        print(f"Wrote baseline for {len(unwrap_counts)} fn(s) to {rel(args.allow)}")
        return 0

    baseline = allow["unwrap-storage-get"]
    stale = []
    for name, hits in sorted(unwrap_counts.items()):
        limit = baseline.get(name, 0)
        if len(hits) > limit:
            for path, ln in hits:
                violations.append((path, ln, "unwrap-storage-get",
                                   f"`{name}`: storage get().unwrap()/expect() "
                                   f"({len(hits)} in fn, baseline {limit})"))
        elif len(hits) < limit:
            stale.append(f"{name}: baseline {limit}, now {len(hits)}")
    for name, limit in sorted(baseline.items()):
        if name not in unwrap_counts and limit:
            stale.append(f"{name}: baseline {limit}, now 0")

    # Rules on contract entry points.
    for fn in entry_points:
        raw, code, sup = files[fn.path]
        raw_lines = raw.splitlines()

        if (fn.name not in INIT_EXEMPT and fn.name not in reaches_init
                and fn.name not in allow["missing-require-initialized"]
                and not fn_suppressed(raw_lines, sup, fn.line, "missing-require-initialized")):
            violations.append((fn.path, fn.line, "missing-require-initialized",
                               f"`{fn.name}` never reaches require_initialized()"))

        if (fn.name in reaches_write and fn.name not in reaches_auth
                and fn.name not in allow["missing-require-auth"]
                and not fn_suppressed(raw_lines, sup, fn.line, "missing-require-auth")):
            violations.append((fn.path, fn.line, "missing-require-auth",
                               f"`{fn.name}` mutates state but never reaches require_auth()"))

    # Allowlist entries that no longer correspond to a contract fn are stale.
    entry_names = {fn.name for fn in entry_points}
    for r in RULES[1:]:
        for name in allow[r]:
            if name not in entry_names:
                stale.append(f"{r}: `{name}` is not a contract entry point")

    print(f"Scanned {len(files)} file(s), {len(entry_points)} contract entry point(s).")
    if stale:
        print("\nStale allowlist / baseline entries (tighten scripts/soroban-lints-allow.json):")
        for s in stale:
            print(f"  - {s}")

    if violations:
        print("")
        for path, ln, rule, msg in sorted(violations):
            print(f"{rel(path)}:{ln}: [{rule}] {msg}")
            if os.environ.get("GITHUB_ACTIONS") == "true":
                print(f"::error file={rel(path)},line={ln},title={rule}::{msg}")
        print("\n❌ SOROBAN LINTS FAILED")
        print("   Fix the code, or add `// soroban-lint: allow(<rule>) <reason>` /")
        print("   an allowlist entry with a justification. See the script header.")
        return 1

    if stale:
        print("\n❌ SOROBAN LINTS FAILED (stale allowlist entries)")
        return 1

    print("✅ SOROBAN LINTS PASSED")
    return 0


if __name__ == "__main__":
    sys.exit(main())
