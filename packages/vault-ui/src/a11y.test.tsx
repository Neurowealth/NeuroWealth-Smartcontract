import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { axe } from 'vitest-axe';
import App from './App';

vi.mock('@neurowealth/vault-client', () => {
  class VaultClient {
    get_balance() {
      return Promise.resolve(0n);
    }
    get_total_assets() {
      return Promise.resolve(0n);
    }
    is_paused() {
      return Promise.resolve(false);
    }
    get_deployed_assets() {
      return Promise.resolve(0n);
    }
    get_user_strategy() {
      return Promise.resolve('balanced');
    }
    preview_deposit_to_shares() {
      return Promise.resolve(0n);
    }
    preview_withdraw() {
      return Promise.resolve(0n);
    }
    simulate() {
      return Promise.resolve({ simulation: { minResourceFee: '100' } });
    }
    deposit() {
      return Promise.resolve({ hash: 'abc' });
    }
    withdraw() {
      return Promise.resolve({ hash: 'abc' });
    }
  }
  return {
    VaultClient,
    VaultError: class VaultError extends Error {
      code = 100;
    },
    VaultErrorCode: {
      ValidationError: 100,
      BelowMinimumDeposit: 38,
      MaximumDepositExceeded: 39,
      PausedError: 101,
      InsufficientBalanceError: 104,
      InsufficientSharesForAmount: 11,
    },
  };
});

vi.mock('recharts', () => ({
  ResponsiveContainer: ({ children }: { children: unknown }) => children,
  LineChart: () => null,
  Line: () => null,
  BarChart: () => null,
  Bar: () => null,
  XAxis: () => null,
  YAxis: () => null,
  CartesianGrid: () => null,
  Tooltip: () => null,
}));

describe('WCAG 2.1 AA accessibility', () => {
  it('exposes a skip link and labelled primary navigation', () => {
    render(<App />);
    const skip = screen.getByRole('link', { name: /skip to main content/i });
    expect(skip).toHaveAttribute('href', '#main-content');
    expect(screen.getByRole('navigation', { name: 'Primary' })).toBeInTheDocument();
    expect(screen.getByRole('main')).toHaveAttribute('id', 'main-content');
  });

  it('has accessible form labels on the deposit view', () => {
    render(<App />);
    expect(screen.getByLabelText(/amount \(usdc\)/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/strategy preference/i)).toBeInTheDocument();
  });

  it('has no axe violations on the deposit view', async () => {
    const { container } = render(<App />);
    const results = await axe(container, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa'] } });
    expect(results).toHaveNoViolations();
  });

  it('has no axe violations on earnings and notification views', async () => {
    const { container } = render(<App />);
    screen.getByRole('button', { name: 'Earnings History' }).click();
    expect(await axe(container, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa'] } })).toHaveNoViolations();

    screen.getByRole('button', { name: 'Notifications' }).click();
    expect(screen.getByRole('heading', { name: 'Notifications' })).toBeInTheDocument();
    expect(await axe(container, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa'] } })).toHaveNoViolations();
  });
});
