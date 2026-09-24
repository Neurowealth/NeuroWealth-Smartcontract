import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { fromMock } = vi.hoisted(() => ({ fromMock: vi.fn() }));

vi.mock('./supabase', () => ({
  supabase: { from: fromMock },
}));

import { getEarningsSummary, getPortfolioValueHistory, getRecentTransactions } from './database';

/** Builds a thenable chainable query mock: any method returns itself, `single()`
 * and awaiting the chain both resolve to `result`. */
function chainable(result: { data: unknown }) {
  const chain: any = {
    select: vi.fn(() => chain),
    eq: vi.fn(() => chain),
    gte: vi.fn(() => chain),
    order: vi.fn(() => chain),
    limit: vi.fn(() => chain),
    single: vi.fn(() => Promise.resolve(result)),
    then: (resolve: (value: unknown) => unknown) => resolve(result),
  };
  return chain;
}

const USER_ADDRESS = 'GABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890';

describe('getEarningsSummary', () => {
  beforeEach(() => {
    vi.stubEnv('NEXT_PUBLIC_SUPABASE_URL', 'https://example.supabase.co');
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    fromMock.mockReset();
  });

  it('returns zeros when no user address is given', async () => {
    await expect(getEarningsSummary(undefined)).resolves.toEqual({ today: 0, week: 0, month: 0 });
    expect(fromMock).not.toHaveBeenCalled();
  });

  it('returns zeros when supabase is not configured', async () => {
    vi.unstubAllEnvs();
    await expect(getEarningsSummary(USER_ADDRESS)).resolves.toEqual({ today: 0, week: 0, month: 0 });
    expect(fromMock).not.toHaveBeenCalled();
  });

  it('returns zeros when the user is not found', async () => {
    fromMock.mockReturnValueOnce(chainable({ data: null }));

    await expect(getEarningsSummary(USER_ADDRESS)).resolves.toEqual({ today: 0, week: 0, month: 0 });
  });

  it('returns zeros when there is no earnings history', async () => {
    fromMock
      .mockReturnValueOnce(chainable({ data: { id: 'user-1' } }))
      .mockReturnValueOnce(chainable({ data: [] }));

    await expect(getEarningsSummary(USER_ADDRESS)).resolves.toEqual({ today: 0, week: 0, month: 0 });
  });

  it('aggregates today/week/month totals from earnings history', async () => {
    const now = new Date();
    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    const earlierThisMonth = new Date(now.getFullYear(), now.getMonth(), 1);

    fromMock
      .mockReturnValueOnce(chainable({ data: { id: 'user-1' } }))
      .mockReturnValueOnce(
        chainable({
          data: [
            { daily_earnings: '10.5', date: today.toISOString() },
            { daily_earnings: '5.25', date: earlierThisMonth.toISOString() },
          ],
        }),
      );

    const result = await getEarningsSummary(USER_ADDRESS);

    expect(result.month).toBe(15.75);
    expect(result.today).toBeGreaterThanOrEqual(10.5);
  });
});

describe('getPortfolioValueHistory', () => {
  beforeEach(() => {
    vi.stubEnv('NEXT_PUBLIC_SUPABASE_URL', 'https://example.supabase.co');
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    fromMock.mockReset();
  });

  it('returns an empty array when no user address is given', async () => {
    await expect(getPortfolioValueHistory(undefined)).resolves.toEqual([]);
  });

  it('returns an empty array when the user is not found', async () => {
    fromMock.mockReturnValueOnce(chainable({ data: null }));

    await expect(getPortfolioValueHistory(USER_ADDRESS)).resolves.toEqual([]);
  });

  it('returns an empty array when there are no snapshots', async () => {
    fromMock
      .mockReturnValueOnce(chainable({ data: { id: 'user-1' } }))
      .mockReturnValueOnce(chainable({ data: [] }));

    await expect(getPortfolioValueHistory(USER_ADDRESS)).resolves.toEqual([]);
  });

  it('maps snapshots into chart points with a computed yield delta', async () => {
    fromMock.mockReturnValueOnce(chainable({ data: { id: 'user-1' } })).mockReturnValueOnce(
      chainable({
        data: [
          { total_assets: '100', timestamp: '2026-01-01T00:00:00Z' },
          { total_assets: '110.5', timestamp: '2026-01-02T00:00:00Z' },
        ],
      }),
    );

    const result = await getPortfolioValueHistory(USER_ADDRESS);

    expect(result).toEqual([
      { date: 'Jan 1', value: 100, yield: 100 },
      { date: 'Jan 2', value: 110.5, yield: 10.5 },
    ]);
  });
});

describe('getRecentTransactions', () => {
  beforeEach(() => {
    vi.stubEnv('NEXT_PUBLIC_SUPABASE_URL', 'https://example.supabase.co');
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    fromMock.mockReset();
  });

  it('returns an empty array when no user address is given', async () => {
    await expect(getRecentTransactions(undefined)).resolves.toEqual([]);
  });

  it('returns an empty array when the user is not found', async () => {
    fromMock.mockReturnValueOnce(chainable({ data: null }));

    await expect(getRecentTransactions(USER_ADDRESS)).resolves.toEqual([]);
  });

  it('merges deposits and withdrawals sorted by timestamp descending, capped at 10', async () => {
    fromMock
      .mockReturnValueOnce(chainable({ data: { id: 'user-1' } }))
      .mockReturnValueOnce(
        chainable({
          data: [
            { id: 'd1', amount: '50', tx_hash: 'txd1', timestamp: '2026-01-01T00:00:00Z' },
          ],
        }),
      )
      .mockReturnValueOnce(
        chainable({
          data: [
            { id: 'w1', amount: '20', tx_hash: 'txw1', timestamp: '2026-01-02T00:00:00Z' },
          ],
        }),
      );

    const result = await getRecentTransactions(USER_ADDRESS);

    expect(result).toHaveLength(2);
    expect(result[0]).toMatchObject({ id: 'w1', type: 'withdrawal', amount: 20, status: 'confirmed' });
    expect(result[1]).toMatchObject({ id: 'd1', type: 'deposit', amount: 50, status: 'confirmed' });
  });
});
