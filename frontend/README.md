# NeuroWealth Frontend Dashboard

> **Canonical web app.** This is the only deployable NeuroWealth UI. Reusable
> components live in [`packages/vault-ui`](../packages/vault-ui) (a library, not
> a second app). See [docs/FRONTEND_PACKAGES.md](../docs/FRONTEND_PACKAGES.md).

Next.js 15 Web Application with Tailwind CSS, `@stellar/stellar-sdk`, `@stellar/freighter-api`, and Recharts portfolio analytics.

## Features
- **Next.js 15 App Router**: High-performance React 18 server and client components.
- **Tailwind CSS Design System**: Vibrant dark mode UI with glassmorphism panels, custom color tokens, and micro-interactions.
- **Freighter Wallet Integration**: Non-custodial wallet connection & transaction signing.
- **Vault Contract Integration**: Connects to Soroban vault smart contract getters (`get_balance`, `get_exchange_rate`, `get_user_strategy`).
- **Portfolio Analytics**: Interactive Recharts area chart for tracking portfolio growth and accrued yield.
- **Deposit & Withdraw Modal**: Previews share minting/burning rates with real-time exchange rates.
- **Transaction History**: Displays recent on-chain events with Stellar Explorer links.

## Setup & Running
```bash
npm install
npm run dev
```

Open [http://localhost:3000](http://localhost:3000) to view the application.

## Mock mode

The dashboard reads real data only: vault state from Soroban RPC, history from
Supabase (or Soroban RPC contract events when Supabase is not configured).
There are no silent fallbacks to hard-coded balances, APYs or transactions.

Mock data is served only when mock mode is explicitly enabled at build time:

| Variable | Effect |
| --- | --- |
| `NEXT_PUBLIC_USE_MOCKS=true` | Enables the in-browser test harness (`window.__NEUROWEALTH_E2E__`) instead of Freighter / Soroban RPC. |
| `NEXT_PUBLIC_E2E_TESTS=true` | Alias used by the Playwright suite (`playwright.config.ts`). |

Leave both unset (the default) for any real deployment.
