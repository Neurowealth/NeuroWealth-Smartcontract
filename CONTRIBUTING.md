# Contributing to NeuroWealth

Thank you for your interest in contributing to NeuroWealth! We welcome contributions from everyone.

This guide will help you get started with our development process, issue labeling, and coding standards.

## Table of Contents
- [Good First Issues](#good-first-issues)
- [Reporting Issues](#reporting-issues)
- [Development Setup](#development-setup)
  - [Prerequisites](#prerequisites)
  - [Building the Contract](#building-the-contract)
  - [Running Tests](#running-tests)
  - [Running Individual Tests and Test Modules](#running-individual-tests-and-test-modules)
  - [Common Test Failures](#common-test-failures)
  - [Running Fuzz Tests](#running-fuzz-tests)
- [CI Requirements](#ci-requirements)
- [Coding Standards](#coding-standards)
- [Submitting a Pull Request](#submitting-a-pull-request)

## Good First Issues

If you're new to the project, a great place to start is our [good first issues](https://github.com/NeuroWealth/NeuroWealth-Smartcontract/issues?q=is%3Aopen+is%3Aissue+label%3A%22good+first+issue%22). These are typically smaller tasks that help you get familiar with the codebase.

## Reporting Issues

We use standardized issue templates to ensure that bug reports and feature requests contain all the necessary information for the team to respond effectively.

### Bug Reports

Use the [bug report template](/.github/ISSUE_TEMPLATE/bug_report.md) when:
- You've found a defect in the smart contract
- You've encountered unexpected behavior
- You have a reproducible test case

The bug report template will guide you to provide:
- Soroban network context (devnet, testnet, or mainnet)
- Contract ID and affected function
- Steps to reproduce the issue
- Expected vs. actual behavior
- Test command output and environment details
- Verification checklist to ensure completeness

### Feature Requests

Use the [feature request template](/.github/ISSUE_TEMPLATE/feature_request.md) when:
- You want to propose a new capability
- You want to suggest an enhancement
- You have an idea for improving the project

The feature request template will guide you to provide:
- Clear feature description
- Use case and motivation
- Proposed solution and alternative approaches
- Security and network-specific considerations
- Architecture alignment
- Related issues or pull requests

### Security Issues

**For security-related issues**, please follow the [Security Policy](SECURITY.md) and report via GitHub's security advisory system instead of creating a public issue. Do not disclose security vulnerabilities publicly.

### Issue Labels

We use the following labels to categorize issues:

- `bug`: Something isn't working as expected
- `enhancement`: New feature or request
- `documentation`: Improvements or additions to documentation
- `good first issue`: Good for newcomers
- `security`: Security-related issues or improvements
- `help wanted`: Extra attention needed

## Development Setup

### Prerequisites

To contribute to the smart contracts, you'll need the following installed:

- **Rust**: Latest stable version. [Install Rust](https://rustup.rs/)
- **WASM Target**: `rustup target add wasm32-unknown-unknown`
- **Stellar CLI**: The required version is pinned in [`.stellar-version`](.stellar-version) at the repo root. Install the exact pinned version to avoid build and deployment drift:
  ```bash
  STELLAR_VERSION=$(cat .stellar-version | tr -d '[:space:]')
  cargo install --locked stellar-cli --version "$STELLAR_VERSION" --features opt
  ```
  Using a different version than the pin is the most common cause of CI failures locally.
- **Node.js & npm**: For agent and frontend development (LTS version recommended).

### Building the Contract

Navigate to the contract directory and use the Stellar CLI to build:

```bash
cd neurowealth-vault
stellar contract build
```

### Running Tests

We prioritize high test coverage. Always run the test suite before submitting a PR:

```bash
cd neurowealth-vault
cargo test
```

For frontend or agent changes, run:
```bash
npm test
```

> **Touching share accounting?** If your change lands anywhere under
> `neurowealth-vault/contracts/vault/src/`, CI also runs the deposit/withdraw
> fuzz target. Reproduce it locally *before* pushing — see
> [Running Fuzz Tests](#running-fuzz-tests) for the one-line smoke run.

### Running Individual Tests and Test Modules

The full suite takes a while, so during local iteration it is usually faster to
run just the tests you care about. All commands below are run from
`neurowealth-vault/`.

#### How the test suite is laid out

Every vault test lives in a module under
`contracts/vault/src/tests/` and is pulled into the crate by
`contracts/vault/src/lib.rs`:

```rust
#[cfg(test)]
#[path = "tests/mod.rs"]
mod comprehensive_tests;
```

They are therefore **unit tests compiled into the library test binary**, not
separate integration-test binaries in a `tests/` directory. Two consequences:

- `cargo test --test test_deposit` does **not** work — there is no test target
  by that name, and cargo fails with
  ``error: no test target named `test_deposit` in default-run packages``.
- Every test's full path is prefixed with `comprehensive_tests::`, e.g.
  `comprehensive_tests::test_deposit::test_deposit_minimum_succeeds`.

You filter tests by **substring match on that full path**.

#### Run a single test

```bash
# Substring match — runs every test whose full path contains this string.
cargo test test_confirm_before_timelock_rejected

# Exact match only (useful when one name is a prefix of another).
cargo test comprehensive_tests::test_agent_timelock::test_cancel_clears_pending_agent -- --exact
```

#### Run a single test module

Filter on the module path. The trailing `::` keeps the match from spilling into
similarly named modules — a bare `test_upgrade` matches both
`test_upgrade_timelock` and `test_upgrade_compatibility`:

```bash
# All tests in contracts/vault/src/tests/test_deposit.rs
cargo test comprehensive_tests::test_deposit::

# Just the upgrade-timelock module, not test_upgrade_compatibility
cargo test comprehensive_tests::test_upgrade_timelock::
```

#### Useful flags

```bash
# List every test without running any — handy for finding the exact path.
cargo test -- --list

# Show println!/std::eprintln! output from passing tests (suppressed by default).
cargo test test_deposit -- --nocapture

# Run serially. Some stress modules are timing/ordering sensitive.
cargo test -- --test-threads=1
```

#### Run tests with features

Two optional features gate the production-interface pool tests. The modules are
declared `#![cfg(all(test, feature = "..."))]`, so **without the flag they
compile to nothing and are silently skipped** — no error, no skip message:

```bash
# Blend production-interface tests (contracts/vault/src/tests/test_blend_devnet.rs)
cargo test -p neurowealth-vault --features blend-devnet

# DEX production-interface tests (contracts/vault/src/tests/test_dex_devnet.rs)
cargo test -p neurowealth-vault --features dex-devnet

# Everything at once. Worth running before pushing, because the Clippy CI job
# uses --all-features and will lint feature-gated test code you never compiled.
cargo test -p neurowealth-vault --all-features
```

Feature flags combine with filters as expected:

```bash
cargo test -p neurowealth-vault --features dex-devnet comprehensive_tests::test_dex_devnet::
```

### Common Test Failures

| Symptom | Cause | Fix |
|---|---|---|
| ``error: no test target named `test_deposit` `` | Tests are library unit-test modules, not integration-test binaries. | Filter by module path instead: `cargo test comprehensive_tests::test_deposit::`. See [above](#run-a-single-test-module). |
| `test result: ok. 0 passed; ... N filtered out` | The filter string matched nothing (typo, or the module is feature-gated). | Run `cargo test -- --list` and copy the exact path; add `--features blend-devnet` / `dex-devnet` if the module is gated. |
| Feature-gated tests appear not to exist | Without the feature flag the whole file is `cfg`'d out — it does not even show up in `--list`. | Pass the matching `--features` flag. |
| `panicked ... expected "Error(Contract, #41)"` but a different code was returned | Contract errors surface as their `VaultError` discriminant, not the message text. A code shifted or a different guard fired first. | Look up the discriminant in the `VaultError` enum in `contracts/vault/src/lib.rs` and update the `#[should_panic(expected = ...)]` string, or fix the ordering of the guards. |
| A test in `test_budget.rs` fails on CPU/memory | A change pushed an operation past the recorded ledger-resource ceiling. | Check the baselines in [ARCHITECTURE.md](ARCHITECTURE.md) (*Ledger Resource Baselines*). Either reduce the cost or, if the increase is justified, raise the bound in the same PR and explain why. |
| Passes individually but fails in the full run | Ordering- or timing-sensitive stress test. | Reproduce with `cargo test -- --test-threads=1` before assuming flakiness. |
| `cargo fmt --all -- --check` fails in CI but the code looks fine | Formatting drift. | Run `cargo fmt --all` and commit the result. |
| Clippy fails in CI but passes locally | CI runs `cargo clippy --all-targets --all-features -- -D warnings`; a plain local run skips gated code and warnings are not denied. | Reproduce the exact CI command locally. |
| `stellar contract build` fails on a missing target or toolchain | The `wasm32-unknown-unknown` target is not installed, or the Stellar CLI does not match the pin. | `rustup target add wasm32-unknown-unknown`, then reinstall the version pinned in [`.stellar-version`](.stellar-version). Version drift is the most common cause of local-vs-CI differences. |
| `cargo fuzz` is not a recognised command, or the target refuses to build on stable | Fuzzing requires the nightly toolchain and `cargo-fuzz`. | Install both, then use `cargo +nightly fuzz run ...` — see [Running Fuzz Tests](#running-fuzz-tests). |

### Running Fuzz Tests

The vault's deposit/withdraw share-accounting logic is covered by a [libFuzzer](https://llvm.org/docs/LibFuzzer.html) harness located in `neurowealth-vault/fuzz/fuzz_targets/deposit_withdraw_sequence.rs`.

#### Prerequisites

Fuzz testing requires the Rust **nightly** toolchain and `cargo-fuzz`:

```bash
rustup toolchain install nightly
cargo +nightly install cargo-fuzz --locked
```

#### Running locally

```bash
cd neurowealth-vault

# Quick smoke-run (≈30 seconds, good for local iteration):
cargo +nightly fuzz run deposit_withdraw_sequence -- -runs=500 -max_total_time=30

# Longer run matching the CI PR bounds:
cargo +nightly fuzz run deposit_withdraw_sequence -- -runs=1000 -max_total_time=120

# Full weekly-schedule run:
cargo +nightly fuzz run deposit_withdraw_sequence -- -runs=5000 -max_total_time=300
```

If the fuzzer finds a crash, a minimised input is written to
`neurowealth-vault/fuzz/artifacts/deposit_withdraw_sequence/`.
Reproduce it with:

```bash
cargo +nightly fuzz run deposit_withdraw_sequence \
  fuzz/artifacts/deposit_withdraw_sequence/<crash-file>
```

#### CI behaviour

| Trigger | Bounds |
|---|---|
| PR touching `neurowealth-vault/contracts/vault/src/**` | `-runs=1000 -max_total_time=120` (2 min) |
| Weekly schedule (`0 3 * * 0`) | `-runs=5000 -max_total_time=300` (5 min) |

## CI Requirements

Our CI pipeline (defined in `.github/workflows/ci.yml`) runs on every push and pull request. For a PR to be merged, it must pass:

1. **Format Check**: `cargo fmt --all -- --check`
2. **Clippy Lint**: `cargo clippy --all-targets --all-features -- -D warnings`
3. **Tests**: `cargo test --verbose`
4. **Build WASM**: Successful build of the contract WASM.
5. **Dependency Audit**: `cargo-deny check` runs against the policy in [`deny.toml`](deny.toml), blocking dependencies with disallowed licenses or known security advisories.
6. **Fuzz Tests** *(conditional)*: PRs that modify files under `neurowealth-vault/contracts/vault/src/` automatically trigger the `deposit_withdraw_sequence` fuzz target with short bounds (`-runs=1000 -max_total_time=120`). The same target runs with extended bounds on the weekly schedule. See [Running Fuzz Tests](#running-fuzz-tests) for local reproduction steps.
7. **Kani proofs**: `cargo kani -p share-math` must pass. See [`docs/FORMAL_VERIFICATION.md`](docs/FORMAL_VERIFICATION.md) and `./scripts/run-kani-proofs.sh`.
8. **Vault UI tests**: `packages/vault-ui` `npm test` (axe-core WCAG 2.1 AA + notification unit tests).

### Public Function Auth Gate

The `pub-fn-auth-gate` CI job (`scripts/check-pub-fn-auth.sh`) runs on every push and pull request. It cross-references every **state-changing** public function listed in `contract-spec.json` against the Access Control Summary table in `SECURITY.md`. The build fails if any state-changing function is missing a row in that table.

This gate specifically catches new functions that were added to the contract but not yet classified — `check-access-control.sh` verifies that existing rows are *accurate*; this gate ensures no new function *slips through* without a documented access-control decision.

#### Adding a new state-changing function

When you add a new `pub fn` to the vault contract that modifies state, you must do **both** of the following before the PR can merge:

1. **Add a row to the Access Control Summary table in `SECURITY.md`** with the correct Owner / Agent / User / Anyone columns filled in.
2. **Update `contract-spec.json`** — add or update the function's entry with:
   - `"state_changing": true`
   - The correct `"access"` value (`"owner-only"`, `"agent-only"`, `"public"`, or `"pending-owner-only"`)
   - `"requires_auth": true/false` as appropriate

Both files must be updated in the **same PR** as the contract change so the gate and the accuracy check both pass together.

#### N/A escape hatch — genuinely non-auth functions

Occasionally a state-changing function is intentionally permissionless (for example, a storage-maintenance helper that anyone may call). In that case, mark it as N/A using **one** of these two methods:

- **In `contract-spec.json`**: add `"auth_gate_na": true` to the function's entry.
- **Via environment variable**: set `AUTH_GATE_NA_FUNCTIONS=fn_name` (colon-separated for multiple names) when invoking the script locally or in a custom workflow step.

Both methods cause the gate to skip that function. Reserve this escape hatch for functions that genuinely have no meaningful access-control boundary — all owner-only, agent-only, and user-scoped functions must have a SECURITY.md row.

### Dependency Audit & Advisory Exceptions

The dependency audit ([`EmbarkStudios/cargo-deny-action`](https://github.com/EmbarkStudios/cargo-deny-action)) enforces the license, advisory, and source policies defined in [`deny.toml`](deny.toml). To reproduce it locally:

```bash
cargo install --locked cargo-deny
cargo deny --manifest-path neurowealth-vault/Cargo.toml check --config deny.toml
```

Occasionally a published advisory cannot be resolved immediately (for example, the upstream fix has not yet been released). In that case you can request a **time-limited exception** rather than disabling the check:

1. Open a tracking issue describing the advisory and the remediation plan.
2. Open a PR that adds an `[[advisories.ignore]]` entry to [`deny.toml`](deny.toml). Each entry must include the RustSec `id`, the affected `crate`, and a `reason` that justifies the exception and links to the tracking issue. See the commented example and process notes in the *Exceptions* section of [`deny.toml`](deny.toml).
3. Exceptions are reviewed and re-evaluated every sprint, and should be removed as soon as an upstream fix is available.

License exceptions follow the same PR-based process but additionally require legal sign-off before the `[[licenses.exceptions]]` entry is added.

## Coding Standards

- **Error Messages**: All error messages must follow the [Error Message Style Guide](ERROR_STYLE_GUIDE.md).
- **Architecture**: Ensure changes align with the project [Architecture Documentation](ARCHITECTURE.md).
- **Events**: Every state change should emit a corresponding event as defined in [EVENTS.md](EVENTS.md).
- **Safety**: Always use `checked_*` arithmetic operations for financial calculations.

## Submitting a Pull Request

1. **Fork the repository** and create your branch from `develop`.
2. **Make your changes**, ensuring you add or update tests.
3. **Verify locally** that all tests pass and there are no linting errors.
4. **Commit your changes** with a clear and descriptive message.
5. **Push to your fork** and open a Pull Request against the `develop` branch.
6. **Provide a detailed description** in the PR of what you changed and why.

---

By contributing, you agree that your contributions will be licensed under the project's license.
