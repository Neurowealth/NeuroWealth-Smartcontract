import { expect, test, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import { startLocalRpcServer, type LocalRpcServer } from './rpcServer';

let rpc: LocalRpcServer;

test.beforeAll(async () => {
  rpc = await startLocalRpcServer();
});

test.afterAll(async () => {
  await rpc.close();
});

async function installFreighterAndRpcBoundary(page: Page, scenario: string): Promise<void> {
  await page.addInitScript(({ rpcUrl }) => {
    const scenario = () => new URL(window.location.href).searchParams.get('scenario') ?? 'journey';
    const call = async (method: string, params: Record<string, unknown> = {}) => {
      const response = await fetch(rpcUrl, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ method, params, scenario: scenario() }),
      });
      const data = await response.json() as { result?: unknown; error?: string };
      if (data.error) throw new Error(data.error);
      return data.result;
    };
    window.__NEUROWEALTH_E2E__ = {
      connectWallet: async () => await call('connectWallet') as string,
      getNetworkDetails: async () => await call('getNetworkDetails') as { networkPassphrase: string },
      signTransaction: async (xdr) => await call('signTransaction', { xdr }) as string,
      simulateContractCall: async (method) => call('simulateContractCall', { method }),
      prepareTransaction: async (input) => {
        const result = await call('simulateTransaction', input) as { xdr: string };
        return result.xdr;
      },
      sendTransaction: async (signedXdr) => await call('sendTransaction', { signedXdr }) as { status: string; hash: string },
      pollTransaction: async (hash) => await call('getTransaction', { hash }) as { status: string },
    };
  }, { rpcUrl: rpc.url });
  await page.goto(`/en?scenario=${scenario}`);
}

async function connectWallet(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'Connect Wallet' }).click();
  await expect(page.getByText('GABCDE...WXYZ')).toBeVisible();
  await expect(page.getByText('100.00', { exact: true })).toBeVisible();
}

test('starts disconnected and completes deposit and withdrawal against local RPC', async ({ page }) => {
  await installFreighterAndRpcBoundary(page, 'journey');

  await expect(page.getByRole('button', { name: 'Connect Wallet' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Deposit', exact: true })).toBeDisabled();
  await connectWallet(page);

  await page.getByRole('button', { name: 'Deposit', exact: true }).click();
  await expect(page.getByRole('dialog', { name: 'deposit USDC' })).toBeVisible();
  const accessibility = await new AxeBuilder({ page })
    .include('[role="dialog"]')
    .withRules(['aria-dialog-name', 'aria-hidden-focus', 'label'])
    .analyze();
  expect(accessibility.violations).toEqual([]);
  await page.getByLabel('Amount (USDC)').fill('25');
  await page.getByRole('button', { name: 'Confirm Deposit' }).click();
  await expect(page.getByRole('status')).toContainText('Transaction submitted; confirmation is still pending.');
  await expect(page.getByText('Deposit Successful!')).toBeVisible();

  await page.getByRole('button', { name: 'Done' }).click();
  await page.getByRole('button', { name: 'Withdraw', exact: true }).click();
  await expect(page.getByRole('dialog', { name: 'withdraw USDC' })).toBeVisible();
  await page.getByLabel('Amount (USDC)').fill('10');
  await page.getByRole('button', { name: 'Confirm Withdrawal' }).click();
  await expect(page.getByText('Withdrawal Successful!')).toBeVisible();

  const methods = rpc.requests.filter((entry) => entry.scenario === 'journey').map((entry) => entry.method);
  expect(methods).toContain('connectWallet');
  expect(methods).toContain('simulateContractCall');
  expect(methods.filter((method) => method === 'simulateTransaction')).toHaveLength(2);
  expect(methods.filter((method) => method === 'sendTransaction')).toHaveLength(2);
  expect(methods.filter((method) => method === 'getTransaction')).toHaveLength(2);
  expect(rpc.url).toMatch(/^http:\/\/127\.0\.0\.1:/);
});

test('keeps unresolved confirmation visible after closing and reopening the modal', async ({ page }) => {
  await installFreighterAndRpcBoundary(page, 'pending');
  await connectWallet(page);
  const depositButton = page.getByRole('button', { name: 'Deposit', exact: true });
  await depositButton.click();
  await page.getByLabel('Amount (USDC)').fill('5');
  await page.getByRole('button', { name: 'Confirm Deposit' }).click();
  await expect(page.getByRole('status')).toContainText('Transaction submitted; confirmation is still pending.');
  await expect(page.getByRole('button', { name: /confirming transaction/i })).toBeDisabled();

  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await depositButton.click();
  await expect(page.getByRole('status')).toContainText('local-test-tx-hash');
  await expect(page.getByRole('button', { name: /confirming transaction/i })).toBeDisabled();

  // "Check status later" affordance dismisses the pending submission and unlocks the form
  const checkStatusLaterBtn = page.getByRole('button', { name: /check status later/i });
  await expect(checkStatusLaterBtn).toBeVisible();
  await checkStatusLaterBtn.click();
  await expect(page.getByRole('status')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Confirm Deposit' })).toBeVisible();
});

test('handles rejected signatures, wrong networks, failed simulations, and modal cancellation', async ({ page }) => {
  await installFreighterAndRpcBoundary(page, 'rejected');
  await connectWallet(page);
  await page.getByRole('button', { name: 'Deposit', exact: true }).click();
  await page.getByLabel('Amount (USDC)').fill('5');
  await page.getByRole('button', { name: 'Confirm Deposit' }).click();
  await expect(page.getByRole('button', { name: 'Confirm Deposit' })).toBeEnabled();
  await expect(page.getByRole('alert')).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog')).toHaveCount(0);

  await installFreighterAndRpcBoundary(page, 'wrong-network');
  await connectWallet(page);
  await page.getByRole('button', { name: 'Deposit', exact: true }).click();
  await page.getByLabel('Amount (USDC)').fill('5');
  await page.getByRole('button', { name: 'Confirm Deposit' }).click();
  await expect(page.getByRole('alert')).toContainText('Switch Freighter to the required Stellar network');

  await installFreighterAndRpcBoundary(page, 'simulation-failed');
  await connectWallet(page);
  await page.getByRole('button', { name: 'Deposit', exact: true }).click();
  await page.getByLabel('Amount (USDC)').fill('5');
  await page.getByRole('button', { name: 'Confirm Deposit' }).click();
  await expect(page.getByRole('alert')).toContainText('Transaction failed. Please try again.');
  expect(rpc.requests.filter((entry) => entry.scenario === 'simulation-failed').map((entry) => entry.method)).not.toContain('sendTransaction');
});

test('covers the full deposit -> agent rebalance -> withdrawal lifecycle (#934)', async ({ page }) => {
  const scenario = 'lifecycle';
  await installFreighterAndRpcBoundary(page, scenario);
  await connectWallet(page);

  // 1. Deposit
  await page.getByRole('button', { name: 'Deposit', exact: true }).click();
  await page.getByLabel('Amount (USDC)').fill('25');
  await page.getByRole('button', { name: 'Confirm Deposit' }).click();
  await expect(page.getByText('Deposit Successful!')).toBeVisible();
  await page.getByRole('button', { name: 'Done' }).click();

  const currentStrategyBadge = page.locator('span.flex', { hasText: /^(Conservative|Balanced|Growth)$/ });
  await expect(currentStrategyBadge).toHaveText('Balanced');

  // 2. Agent rebalance: in production the agent submits this directly to
  // Soroban (never through the user's browser wallet), so the test drives
  // it the same way, straight against the sandbox RPC.
  await rpc.rebalance(scenario, { strategy: 'growth', totalAssets: 1_450_000_000, totalShares: 1_000_000_000 });

  // The dashboard only re-reads vault state on reconnect, mirroring the
  // user reopening the app after the agent has rebalanced their position.
  await page.reload();
  await connectWallet(page);
  await expect(currentStrategyBadge).toHaveText('Growth');

  // 3. Withdrawal, now against the rebalanced position
  await page.getByRole('button', { name: 'Withdraw', exact: true }).click();
  await expect(page.getByRole('dialog', { name: 'withdraw USDC' })).toBeVisible();
  await page.getByLabel('Amount (USDC)').fill('10');
  await page.getByRole('button', { name: 'Confirm Withdrawal' }).click();
  await expect(page.getByText('Withdrawal Successful!')).toBeVisible();

  const methods = rpc.requests.filter((entry) => entry.scenario === scenario).map((entry) => entry.method);
  const rebalanceIndex = methods.indexOf('rebalance');
  const withdrawIndex = methods.lastIndexOf('sendTransaction');
  expect(rebalanceIndex).toBeGreaterThan(-1);
  expect(rebalanceIndex).toBeLessThan(withdrawIndex);
  expect(methods.filter((method) => method === 'sendTransaction')).toHaveLength(2);
});