import { fireEvent, render, screen } from '@testing-library/react';
import React from 'react';
import { describe, expect, it, vi } from 'vitest';
import { BalanceCard } from './BalanceCard';
import { EarningsCard } from './EarningsCard';
import { PortfolioChart } from './PortfolioChart';
import { StrategyBadge } from './StrategyBadge';
import { TransactionHistory } from './TransactionHistory';

vi.mock('recharts', () => ({
  ResponsiveContainer: ({ children }: { children: React.ReactNode }) => <div data-testid="responsive-container">{children}</div>,
  AreaChart: ({ data }: { data: unknown[] }) => <div data-testid="area-chart" data-points={data.length} />,
  Area: ({ name }: { name: string }) => <div data-testid="area">{name}</div>,
  XAxis: () => <div data-testid="x-axis" />,
  YAxis: () => <div data-testid="y-axis" />,
  Tooltip: () => <div data-testid="tooltip" />,
  CartesianGrid: () => <div data-testid="grid" />,
}));

describe('portfolio display components', () => {
  it('renders connected balance values and deposit/withdraw actions', () => {
    const onOpenDeposit = vi.fn();
    const onOpenWithdraw = vi.fn();
    const { container } = render(
      <BalanceCard
        balance={1234.56}
        usdEquivalent={1234.56}
        exchangeRate={1.042}
        onOpenDeposit={onOpenDeposit}
        onOpenWithdraw={onOpenWithdraw}
        isConnected
      />,
    );

    expect(screen.getByText('1,234.56')).toBeInTheDocument();
    expect(screen.getByText(/1,234\.56 USD/)).toBeInTheDocument();
    expect(container.querySelector('.grid.grid-cols-2')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /deposit/i }));
    fireEvent.click(screen.getByRole('button', { name: /withdraw/i }));

    expect(onOpenDeposit).toHaveBeenCalledOnce();
    expect(onOpenWithdraw).toHaveBeenCalledOnce();
  });

  it('disables portfolio actions when disconnected', () => {
    render(
      <BalanceCard
        balance={1234.56}
        usdEquivalent={1234.56}
        exchangeRate={1.042}
        onOpenDeposit={vi.fn()}
        onOpenWithdraw={vi.fn()}
        isConnected={false}
      />,
    );

    expect(screen.getByRole('button', { name: /deposit/i })).toBeDisabled();
    expect(screen.getByRole('button', { name: /withdraw/i })).toBeDisabled();
  });

  it('switches earnings periods', () => {
    render(<EarningsCard earnings={{ today: 2.5, week: 15, month: 60 }} isConnected />);

    expect(screen.getAllByText('+$2.50').length).toBeGreaterThan(0);

    fireEvent.click(screen.getByRole('button', { name: /week/i }));

    expect(screen.getAllByText('+$15.00')).toHaveLength(2);
  });

  it('renders strategy options and selection callback', () => {
    const onSelectStrategy = vi.fn();

    render(<StrategyBadge strategy="Balanced" apy={8.4} onSelectStrategy={onSelectStrategy} />);

    fireEvent.click(screen.getByRole('button', { name: /growth/i }));

    expect(screen.getByText('8.4%')).toBeInTheDocument();
    expect(onSelectStrategy).toHaveBeenCalledWith('Growth');
  });

  it('renders portfolio chart data', () => {
    render(
      <PortfolioChart
        data={[
          { date: 'Jul 21', value: 1000, yield: 0 },
          { date: 'Jul 22', value: 1010, yield: 3 },
        ]}
      />,
    );

    expect(screen.getByTestId('area-chart')).toHaveAttribute('data-points', '2');
    expect(screen.getByText('Portfolio Growth & Yield')).toBeInTheDocument();
    expect(screen.getByText('Portfolio Value (USDC)')).toBeInTheDocument();
    expect(screen.getByText('Accrued Yield')).toBeInTheDocument();
  });

  it('renders empty transaction history state', () => {
    render(<TransactionHistory transactions={[]} />);

    expect(screen.getByText(/no transaction history found/i)).toBeInTheDocument();
  });
});
