import { render, screen } from '@testing-library/react';
import React from 'react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { WalletConnect } from './WalletConnect';

vi.mock('@/lib/stellar', () => ({
  shortenAddress: (address: string) => `${address.slice(0, 6)}...${address.slice(-4)}`,
}));

describe('WalletConnect', () => {
  it('calls onConnect from the disconnected state', async () => {
    const onConnect = vi.fn();

    render(
      <WalletConnect
        publicKey={null}
        onConnect={onConnect}
        onDisconnect={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByRole('button', { name: /connect wallet/i }));

    expect(onConnect).toHaveBeenCalledOnce();
  });

  it('shows loading state while connecting', () => {
    render(
      <WalletConnect
        publicKey={null}
        onConnect={vi.fn()}
        onDisconnect={vi.fn()}
        isConnecting
      />,
    );

    expect(screen.getByRole('button', { name: /connecting/i })).toBeDisabled();
  });

  it('shows the connected address and disconnects', async () => {
    const onDisconnect = vi.fn();

    render(
      <WalletConnect
        publicKey="GABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890"
        onConnect={vi.fn()}
        onDisconnect={onDisconnect}
      />,
    );

    expect(screen.getByText('GABCDE...7890')).toBeInTheDocument();

    await userEvent.click(screen.getByTitle('Disconnect Wallet'));

    expect(onDisconnect).toHaveBeenCalledOnce();
  });
});
