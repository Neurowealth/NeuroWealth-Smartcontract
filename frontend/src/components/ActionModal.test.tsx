import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ActionModal } from './ActionModal';
import { signWithFreighter, WalletSigningError } from '@/lib/freighter';

vi.mock('@/lib/freighter', async () => {
  const actual = await vi.importActual<typeof import('@/lib/freighter')>('@/lib/freighter');
  return {
    ...actual,
    signWithFreighter: vi.fn(),
  };
});

const { getAccountMock, prepareTransactionMock, sendTransactionMock, pollTransactionMock } = vi.hoisted(() => ({
  getAccountMock: vi.fn(),
  prepareTransactionMock: vi.fn(),
  sendTransactionMock: vi.fn(),
  pollTransactionMock: vi.fn(),
}));

vi.mock('@stellar/stellar-sdk', () => {
  const preparedTx = { toXDR: () => 'AAAAAgAAAAD...SorobanVaultTx...' };

  class MockTransactionBuilder {
    addOperation() {
      return this;
    }
    setTimeout() {
      return this;
    }
    build() {
      return preparedTx;
    }
    static fromXDR(xdr: string) {
      return { __signedXdr: xdr };
    }
  }

  return {
    rpc: {
      Server: vi.fn().mockImplementation(() => ({
        getAccount: getAccountMock,
        prepareTransaction: prepareTransactionMock,
        sendTransaction: sendTransactionMock,
        pollTransaction: pollTransactionMock,
      })),
    },
    Contract: vi.fn().mockImplementation(() => ({ call: vi.fn() })),
    Address: vi.fn().mockImplementation(() => ({ toScVal: vi.fn() })),
    nativeToScVal: vi.fn(),
    TransactionBuilder: MockTransactionBuilder,
    BASE_FEE: '100',
  };
});

const renderModal = (type: 'deposit' | 'withdraw' = 'deposit') => render(
  <ActionModal
    isOpen
    onClose={vi.fn()}
    type={type}
    userPublicKey="GUSER"
    balance={250}
    exchangeRate={1.25}
  />,
);

const submit = (amount: string, label = /confirm deposit/i) => {
  fireEvent.change(screen.getByPlaceholderText('0.00'), { target: { value: amount } });
  fireEvent.submit(screen.getByRole('button', { name: label }).closest('form')!);
};

describe('ActionModal', () => {
  beforeEach(() => {
    vi.mocked(signWithFreighter).mockResolvedValue('signed-xdr');
    getAccountMock.mockResolvedValue({ accountId: () => 'GUSER' });
    prepareTransactionMock.mockResolvedValue({ toXDR: () => 'AAAAAgAAAAD...SorobanVaultTx...' });
    sendTransactionMock.mockResolvedValue({ status: 'PENDING', hash: 'tx-hash-abc123' });
    pollTransactionMock.mockResolvedValue({ status: 'SUCCESS' });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it('submits a deposit and renders the success state', async () => {
    renderModal('deposit');

    submit('100');

    expect(screen.getByText(/signing with freighter/i)).toBeInTheDocument();

    expect(await screen.findByText(/deposit successful/i, {}, { timeout: 3000 })).toBeInTheDocument();
    expect(signWithFreighter).toHaveBeenCalledWith('AAAAAgAAAAD...SorobanVaultTx...', expect.any(String));
    expect(screen.getByText(/tx-hash-ab/i)).toBeInTheDocument();
  });

  it('fills max amount for withdrawals', async () => {
    renderModal('withdraw');

    fireEvent.click(screen.getByRole('button', { name: /max/i }));

    expect(screen.getByPlaceholderText('0.00')).toHaveValue(250);
    expect(screen.getByText('200.0000 NV-SHARES')).toBeInTheDocument();
  });

  it('shows a generic transaction error when an unclassified failure occurs', async () => {
    vi.mocked(signWithFreighter).mockRejectedValue(new Error('boom'));
    vi.spyOn(console, 'error').mockImplementation(() => undefined);

    renderModal('deposit');
    submit('25');

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Transaction failed. Please try again.');
    });
  });

  it('quietly resets without an error or a transaction hash when the user cancels the signature', async () => {
    vi.mocked(signWithFreighter).mockRejectedValue(
      new WalletSigningError('user_rejected', 'User declined access')
    );

    renderModal('deposit');
    submit('25');

    await waitFor(() => {
      expect(screen.queryByText(/signing with freighter/i)).not.toBeInTheDocument();
    });

    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(screen.queryByText(/deposit successful/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/tx-hash/i)).not.toBeInTheDocument();
    expect(sendTransactionMock).not.toHaveBeenCalled();
  });

  it('shows network-switch guidance for a wrong-network wallet error', async () => {
    vi.mocked(signWithFreighter).mockRejectedValue(
      new WalletSigningError('wrong_network', 'Freighter is connected to the wrong network.')
    );

    renderModal('deposit');
    submit('25');

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent(/switch freighter to the required/i);
    });
    expect(screen.queryByText(/tx-hash/i)).not.toBeInTheDocument();
  });

  it('shows reconnect guidance for a disconnected-wallet error', async () => {
    vi.mocked(signWithFreighter).mockRejectedValue(
      new WalletSigningError('wallet_disconnected', 'Freighter is not allowed to access this account')
    );

    renderModal('deposit');
    submit('25');

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent(/reconnect freighter/i);
    });
    expect(screen.queryByText(/tx-hash/i)).not.toBeInTheDocument();
  });

  it('preserves the transaction hash instead of relabeling a submitted transaction as failed', async () => {
    sendTransactionMock.mockResolvedValue({ status: 'PENDING', hash: 'tx-hash-still-pending' });
    pollTransactionMock.mockResolvedValue({ status: 'NOT_FOUND' });

    renderModal('deposit');
    submit('25');

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Transaction failed. Please try again.');
    });

    // The hash was genuinely observed by the network - it must still be shown
    // even though on-chain confirmation didn't resolve within the polling budget.
    expect(screen.getByRole('alert')).toHaveTextContent(/tx-hash-st/i);
  });

  it('does not show a transaction hash when the network rejects the submission outright', async () => {
    sendTransactionMock.mockResolvedValue({ status: 'ERROR' });

    renderModal('deposit');
    submit('25');

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Transaction failed. Please try again.');
    });
    expect(screen.queryByText(/tx-hash/i)).not.toBeInTheDocument();
    expect(pollTransactionMock).not.toHaveBeenCalled();
  });
});
