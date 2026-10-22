# Shared Linting Baseline (#904)

This document outlines the shared ESLint and code style baseline across all TypeScript/JavaScript Node packages in the NeuroWealth repository:

- `frontend/` (Next.js 15 App Router web application)
- `agent/` (Off-chain AI autonomous agent service)
- `whatsapp/` (WhatsApp notification and custodial wallet handler)
- `packages/vault-ui/` (Reusable React component and notification library)
- `packages/vault-client/` (Contract client bindings)

---

## Shared Rule Baseline

To maintain consistency and avoid CI drift across services, all Node packages follow these common linting rules:

1. **Unused Variables**:
   - Variables and arguments prefixed with an underscore (`_`) are intentionally ignored.
   - Core rule: `@typescript-eslint/no-unused-vars` configured with `argsIgnorePattern: "^_"` and `varsIgnorePattern: "^_"`.
   - Base `no-unused-vars` is turned off in favor of the TypeScript-aware rule.

2. **Any Type Usage**:
   - Event payloads, RPC simulations, and external API integrations use progressive typing.
   - `@typescript-eslint/no-explicit-any` is set to `warn` (or scoped in legacy handlers) rather than blocking builds on external third-party SDK untyped responses.

3. **Import Ordering & Syntax**:
   - All `import` statements must be placed at the top of the file before any executable code or declarations.
   - Clean up unused imports before committing.

4. **Console Output**:
   - Direct `console.log` is discouraged in production services.
   - Structured loggers (`pino`) are used in backend services (`agent`, `whatsapp`).
   - `console.warn` and `console.error` are permitted for non-fatal runtime warnings in client UI applications.

---

## Package Configuration Summary

| Package | ESLint Version / Format | Configuration File | Key Extends |
|---|---|---|---|
| `frontend/` | ESLint 8 / legacy JSON | `.eslintrc.json` | `next/core-web-vitals` + shared TS rules |
| `agent/` | ESLint 9 / Flat Config | `eslint.config.mjs` | `@eslint/js`, `typescript-eslint` |
| `whatsapp/` | ESLint 8 / legacy JSON | `.eslintrc.json` | `eslint:recommended`, `@typescript-eslint/recommended` |
| `packages/vault-ui/` | ESLint 8 / legacy JSON | `.eslintrc.json` (inherits) | `eslint:recommended`, `plugin:react/recommended` |

---

## Local Verification Commands

Run linting in each package root:

```bash
# Frontend
cd frontend && npm run lint

# Agent
cd agent && npm run lint

# WhatsApp
cd whatsapp && npm run lint

# Vault UI
cd packages/vault-ui && npm run lint
```
