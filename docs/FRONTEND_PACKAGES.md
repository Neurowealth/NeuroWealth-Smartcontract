# Canonical UI Package: `frontend/` vs `packages/vault-ui`

> **Status:** Accepted (Issue #768)
> **Decision:** `frontend/` is the canonical user-facing web app.
> `packages/vault-ui` is a **component library** (`@neurowealth/vault-ui`),
> not a second app, and is not deployed on its own.

## Context

Two directories were building the same product surface:

| | `frontend/` | `packages/vault-ui` |
|---|---|---|
| Framework | Next.js 15 (App Router) | Vite + React |
| Wallet | Freighter (`src/lib/freighter.ts`) | none (uses `@neurowealth/vault-client` with an empty contract id) |
| Contract access | `src/lib/stellar.ts` (hand-rolled getters) | `@neurowealth/vault-client` (typed, generated from `contract-spec.json`) |
| i18n | `next-intl`, 6 locales ([TRANSLATIONS.md](TRANSLATIONS.md)) | none |
| Realtime data | Supabase (`useRealtimePortfolio`) | none |
| Theming / onboarding | dark mode, `OnboardingTutorial` | none |
| Tests in CI | none | `vault-ui-test` job: axe-core WCAG 2.1 AA + notification unit tests |
| PWA / Web Push | none | `vite-plugin-pwa`, `public/sw.js`, notifications module |
| Referenced by | README project table, [AUDIT_PREP.md](AUDIT_PREP.md) | [ACCESSIBILITY.md](ACCESSIBILITY.md), [NOTIFICATIONS.md](NOTIFICATIONS.md) |

Keeping both as apps meant every UI change (deposit/withdraw flow, earnings
chart, strategy picker) had to be made twice, and they had already drifted
(e.g. different `@stellar/stellar-sdk` majors, different amount formatting).

## Decision

1. **`frontend/` is the single deployable web app.** New pages, routes,
   wallet flows and translations go there.
2. **`packages/vault-ui` is a library.** It exports reusable, framework-agnostic
   React components and the notification module from `src/index.ts`
   (`DepositWithdrawModal`, `EarningsHistoryPage`, `NotificationSettings`, and
   everything in `src/notifications`). Its Vite `App.tsx` / `index.html` remain
   only as a local playground and as the render target for the axe-core a11y
   test. It must not be deployed as a separate site.
3. **`@neurowealth/vault-client` is the contract access layer** for both.
   `frontend/src/lib/stellar.ts` is to be replaced by `vault-client` calls.

**Why `frontend/`:** it already has everything a production app needs that
`vault-ui` lacks (wallet signing, i18n, realtime data, theming, onboarding), and
it is what the README and audit scope point to. What `vault-ui` does better
(typed client, a11y testing, Web Push) is easy to move into or consume from a
library, whereas porting Next.js routing, i18n and Freighter into Vite is not.

## Overlap and dedup plan

| Concern | `frontend/` | `packages/vault-ui` | Resolution |
|---|---|---|---|
| Deposit / withdraw | `components/ActionModal.tsx` | `components/DepositWithdrawModal.tsx` | Keep `DepositWithdrawModal` (strategy picker, share previews, typed errors); `frontend` imports it and wraps it with Freighter signing, then `ActionModal` is removed. |
| Earnings chart | `components/PortfolioChart.tsx`, `EarningsCard.tsx` | `components/EarningsHistoryPage.tsx` | Keep `PortfolioChart` / `EarningsCard` for the dashboard summary; use `EarningsHistoryPage` for the full history view. Chart code must not be copied again. |
| Notifications / PWA | none | `notifications/*`, `NotificationSettings`, `public/sw.js` | Library-only; `frontend` imports from `@neurowealth/vault-ui`. |
| Contract reads | `lib/stellar.ts` | via `@neurowealth/vault-client` | Migrate `frontend` to `vault-client`, then delete `lib/stellar.ts`. |
| USDC formatting (7 decimals) | inline in components | `formatUsdc` in `DepositWithdrawModal` | Move to one helper exported by `vault-ui` (or `vault-client`). |
| i18n, theming, wallet, onboarding | yes | none | `frontend` only. |

### Migration steps

1. Add `"@neurowealth/vault-ui": "file:../packages/vault-ui"` and
   `"@neurowealth/vault-client": "file:../packages/vault-client"` to
   `frontend/package.json`, and add both to `transpilePackages` in
   `frontend/next.config.js`.
2. Make `DepositWithdrawModal` and `EarningsHistoryPage` take a `VaultClient`
   and a signing callback as props, instead of building a client with an empty
   `contractId`, so `frontend` can inject its Freighter signer.
3. Swap `ActionModal` for `DepositWithdrawModal`, then delete `ActionModal.tsx`.
4. Replace `frontend/src/lib/stellar.ts` with `vault-client`.
5. Move the axe-core test pattern into `frontend` so the deployed app is
   covered by the WCAG gate, not just the library playground.

## Rules for contributors

- **User-facing features** (pages, routes, copy, translations, wallet UX) go
  in `frontend/`.
- **Reusable, app-agnostic components** go in `packages/vault-ui` and must be
  exported from `packages/vault-ui/src/index.ts`. They must not depend on
  Next.js, `next-intl` or Freighter directly; take those as props.
- Do not add a component to one package when an equivalent already exists in
  the other. Extend the existing one and follow the table above.
- `packages/vault-ui/docs/DEPLOYMENT.md` describes the PWA build for local and
  preview use only. Production deploys `frontend/`.
