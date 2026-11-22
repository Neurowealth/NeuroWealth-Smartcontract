/**
 * Mock mode is the only way the frontend serves non-chain data (the in-browser
 * test harness). It is off unless explicitly enabled at build time:
 *
 * - `NEXT_PUBLIC_USE_MOCKS=true` enables it.
 * - `NEXT_PUBLIC_E2E_TESTS=true` is kept as an alias for the Playwright suite.
 *
 * Every other configuration reads real Soroban RPC / Supabase data.
 */
export function isMockModeEnabled(): boolean {
  return process.env.NEXT_PUBLIC_USE_MOCKS === 'true' || process.env.NEXT_PUBLIC_E2E_TESTS === 'true';
}
