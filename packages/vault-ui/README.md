# @neurowealth/vault-ui

Reusable React components and notification logic for the NeuroWealth Vault.

> **This is a library, not the product app.** The canonical user-facing web app
> is [`frontend/`](../../frontend). See
> [docs/FRONTEND_PACKAGES.md](../../docs/FRONTEND_PACKAGES.md) for the decision
> and the dedup plan.

## Exports (`src/index.ts`)

- `DepositWithdrawModal`: deposit/withdraw with strategy picker and share previews
- `EarningsHistoryPage`: earnings history charts
- `NotificationSettings` plus the `notifications` module (Web Push, email fallback, batching)

Components must stay framework-agnostic: no Next.js, `next-intl` or Freighter
imports. Take those as props from the host app.

## Local development

```bash
npm install
npm run dev    # Vite playground (src/App.tsx), for local work only
npm test       # axe-core WCAG 2.1 AA + notification unit tests (CI: vault-ui-test)
```

See [docs/ACCESSIBILITY.md](../../docs/ACCESSIBILITY.md) and
[docs/NOTIFICATIONS.md](../../docs/NOTIFICATIONS.md).
