import { useState, useEffect, useCallback, useId } from 'react';
import { VaultClient, VaultError, VaultErrorCode, UserStrategy } from '@neurowealth/vault-client';

const DECIMAL_PLACES = 7n;
const DECIMAL_PLACES_NUM = 7;

function formatUsdc(raw: bigint): string {
  const abs = raw < 0n ? -raw : raw;
  const whole = abs / 10n ** DECIMAL_PLACES;
  const frac = abs % 10n ** DECIMAL_PLACES;
  const fracStr = frac.toString().padStart(DECIMAL_PLACES_NUM, '0').slice(0, 2);
  return `${raw < 0n ? '-' : ''}${whole.toString()}.${fracStr}`;
}

const STRATEGIES: { value: UserStrategy; label: string; description: string }[] = [
  { value: 'conservative', label: 'Conservative', description: 'Low risk, steady returns' },
  { value: 'balanced', label: 'Balanced', description: 'Mix of safety and growth' },
  { value: 'growth', label: 'Growth', description: 'Higher risk, higher potential yield' },
];

type Mode = 'deposit' | 'withdraw';
type Status = 'idle' | 'loading' | 'success' | 'error';

export default function DepositWithdrawModal() {
  const [client] = useState(() => new VaultClient({ contractId: '' }));
  const [mode, setMode] = useState<Mode>('deposit');
  const [amount, setAmount] = useState('');
  const [strategy, setStrategy] = useState<UserStrategy>('balanced');
  const [previewShares, setPreviewShares] = useState<bigint | null>(null);
  const [previewBurn, setPreviewBurn] = useState<bigint | null>(null);
  const [balance, setBalance] = useState<bigint>(0n);
  const [totalAssets, setTotalAssets] = useState<bigint>(0n);
  const [isPaused, setIsPaused] = useState(false);
  const [status, setStatus] = useState<Status>('idle');
  const [txHash, setTxHash] = useState<string | null>(null);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [gasEstimate, setGasEstimate] = useState<string | null>(null);
  const [deployed, setDeployed] = useState<bigint>(0n);
  const amountId = useId();
  const amountHintId = useId();
  const amountErrorId = useId();
  const strategyId = useId();
  const headingId = useId();

  const publicKey = '';

  const refreshState = useCallback(async () => {
    try {
      const [bal, ta, paused, deploy, strat] = await Promise.all([
        client.get_balance(publicKey).catch(() => 0n),
        client.get_total_assets(publicKey),
        client.is_paused(publicKey),
        client.get_deployed_assets(publicKey),
        client.get_user_strategy(publicKey),
      ]);
      setBalance(bal);
      setTotalAssets(ta);
      setIsPaused(paused);
      setDeployed(deploy);
      if (strat === 'conservative' || strat === 'balanced' || strat === 'growth') {
        setStrategy(strat);
      }
    } catch {
      // ignore initial load errors when contract is unconfigured
    }
  }, [client, publicKey]);

  useEffect(() => {
    refreshState();
  }, [refreshState]);

  useEffect(() => {
    const raw = parseFloat(amount);
    if (!isFinite(raw) || raw <= 0) {
      setPreviewShares(null);
      setPreviewBurn(null);
      return;
    }
    const assetsRaw = BigInt(Math.floor(raw * 1e7));

    if (mode === 'deposit') {
      client
        .preview_deposit_to_shares(assetsRaw, publicKey)
        .then(shares => setPreviewShares(shares))
        .catch(() => setPreviewShares(null));
    } else {
      client
        .preview_withdraw(assetsRaw, publicKey)
        .then(burn => setPreviewBurn(burn))
        .catch(() => setPreviewBurn(null));
    }
  }, [amount, mode, client, publicKey]);

  const canSubmit =
    !isPaused &&
    (mode === 'deposit'
      ? amount.trim() !== '' && parseFloat(amount) > 0
      : balance > 0n && amount.trim() !== '' && parseFloat(amount) > 0);

  const handleSubmit = async () => {
    setStatus('loading');
    setErrorMsg(null);
    setTxHash(null);

    const assetsRaw = BigInt(Math.floor(parseFloat(amount) * 1e7));

    try {
      if (mode === 'deposit') {
        const result = await client.deposit(client as never, publicKey, assetsRaw);
        setTxHash(result.hash ?? null);
        setStatus('success');
      } else {
        const result = await client.withdraw(client as never, publicKey, assetsRaw);
        setTxHash(result.hash ?? null);
        setStatus('success');
      }
      await refreshState();
    } catch (err) {
      const code = err instanceof VaultError ? err.code : VaultErrorCode.ValidationError;
      let message = 'Transaction failed';
      if (code === VaultErrorCode.BelowMinimumDeposit) message = 'Amount below minimum deposit';
      else if (code === VaultErrorCode.MaximumDepositExceeded) message = 'Amount exceeds maximum deposit';
      else if (code === VaultErrorCode.PausedError) message = 'Vault is paused';
      else if (code === VaultErrorCode.InsufficientBalanceError) message = 'Insufficient balance to withdraw';
      else if (code === VaultErrorCode.InsufficientSharesForAmount) message = 'Insufficient shares for amount';
      setErrorMsg(message);
      setStatus('error');
    }
  };

  const simulateGas = useCallback(async () => {
    try {
      const sim = await client.simulate(mode === 'deposit' ? 'deposit' : 'withdraw', [
        publicKey,
        BigInt(Math.floor(parseFloat(amount || '0') * 1e7)),
      ], publicKey);
      setGasEstimate(sim.simulation?.minResourceFee ? `${sim.simulation.minResourceFee}` : '~100');
    } catch {
      setGasEstimate('~100');
    }
  }, [amount, client, mode, publicKey]);

  useEffect(() => {
    if (status === 'loading') {
      void simulateGas();
    }
  }, [status, simulateGas]);

  const showLiquidityWarning = deployed > 0n;
  const describedBy = [amountHintId, status === 'error' && errorMsg ? amountErrorId : null]
    .filter(Boolean)
    .join(' ');

  return (
    <section className="max-w-xl mx-auto bg-white rounded-xl shadow-sm border border-gray-200" aria-labelledby={headingId}>
      <form
        className="p-6"
        onSubmit={e => {
          e.preventDefault();
          if (canSubmit) void handleSubmit();
        }}
      >
        <div className="flex items-center justify-between mb-6 gap-4">
          <h2 id={headingId} className="text-2xl font-semibold text-gray-900">
            {mode === 'deposit' ? 'Deposit USDC' : 'Withdraw USDC'}
          </h2>
          <div className="flex rounded-md shadow-sm" role="group" aria-label="Transaction type">
            <button
              type="button"
              onClick={() => setMode('deposit')}
              aria-pressed={mode === 'deposit'}
              className={`px-4 py-2 text-sm font-medium rounded-l-lg border ${
                mode === 'deposit'
                  ? 'bg-primary-700 text-white border-primary-700'
                  : 'bg-white text-gray-700 border-gray-300 hover:bg-gray-50'
              }`}
            >
              Deposit
            </button>
            <button
              type="button"
              onClick={() => setMode('withdraw')}
              aria-pressed={mode === 'withdraw'}
              className={`px-4 py-2 text-sm font-medium rounded-r-lg border ${
                mode === 'withdraw'
                  ? 'bg-primary-700 text-white border-primary-700'
                  : 'bg-white text-gray-700 border-gray-300 hover:bg-gray-50'
              }`}
            >
              Withdraw
            </button>
          </div>
        </div>

        {isPaused && (
          <div className="mb-4 p-4 bg-red-50 border border-red-200 rounded-lg" role="alert">
            <p className="text-sm text-red-800">Vault is paused. Deposits and withdrawals are disabled.</p>
          </div>
        )}

        <label htmlFor={amountId} className="block text-sm font-medium text-gray-900 mb-1">
          Amount (USDC)
        </label>
        <input
          id={amountId}
          type="number"
          min="0"
          step="0.01"
          inputMode="decimal"
          value={amount}
          onChange={e => setAmount(e.target.value)}
          className="w-full rounded-lg border border-gray-300 px-3 py-2 text-gray-900 mb-1"
          placeholder="0.00"
          disabled={isPaused}
          aria-invalid={status === 'error'}
          aria-describedby={describedBy}
        />
        <p id={amountHintId} className="text-sm text-gray-700 mb-4">
          Enter the USDC amount to {mode}. Preview updates as you type.
        </p>

        {mode === 'deposit' && (
          <div className="mb-4">
            <label htmlFor={strategyId} className="block text-sm font-medium text-gray-900 mb-1">
              Strategy preference
            </label>
            <select
              id={strategyId}
              value={strategy}
              onChange={e => setStrategy(e.target.value as UserStrategy)}
              className="w-full rounded-lg border border-gray-300 px-3 py-2 text-gray-900"
              disabled={isPaused}
            >
              {STRATEGIES.map(s => (
                <option key={s.value} value={s.value}>
                  {s.label} — {s.description}
                </option>
              ))}
            </select>
          </div>
        )}

        {showLiquidityWarning && (
          <div className="mb-4 p-3 bg-yellow-50 border border-yellow-200 rounded-lg">
            <p className="text-sm text-yellow-900">
              Amount may vary based on pool liquidity.
            </p>
          </div>
        )}

        <div className="bg-gray-50 border border-gray-200 rounded-lg p-4 mb-4 text-sm text-gray-900 space-y-2">
          <div className="flex justify-between">
            <span>Preview shares to mint:</span>
            <span className="font-mono">
              {previewShares !== null ? formatUsdc(previewShares) : '—'}
            </span>
          </div>
          <div className="flex justify-between">
            <span>Exchange rate:</span>
            <span className="font-mono">
              {previewShares !== null && totalAssets > 0n
                ? `${(Number(totalAssets) / Number(previewShares)).toFixed(4)}`
                : '1.0000'}
            </span>
          </div>
          <div className="flex justify-between">
            <span>Preview shares to burn:</span>
            <span className="font-mono">
              {previewBurn !== null ? formatUsdc(previewBurn) : '—'}
            </span>
          </div>
          <div className="flex justify-between">
            <span>Your balance:</span>
            <span className="font-mono">{formatUsdc(balance)} USDC</span>
          </div>
          <div className="flex justify-between">
            <span>Vault TVL:</span>
            <span className="font-mono">{formatUsdc(totalAssets)} USDC</span>
          </div>
        </div>

        {gasEstimate && status === 'loading' && (
          <div className="mb-4 text-sm text-gray-700">Estimated gas: {gasEstimate}</div>
        )}

        <button
          type="submit"
          disabled={!canSubmit || status === 'loading'}
          className="w-full py-3 rounded-lg bg-primary-700 text-white font-semibold disabled:opacity-50 disabled:cursor-not-allowed hover:bg-primary-700"
        >
          {status === 'loading' ? 'Confirming...' : mode === 'deposit' ? 'Deposit' : 'Withdraw'}
        </button>

        {status === 'success' && txHash && (
          <div className="mt-4 p-3 bg-green-50 border border-green-200 rounded-lg text-sm" role="status" aria-live="polite">
            <p className="text-green-900">Transaction submitted!</p>
            <a
              href={`https://stellar.expert/explorer/testnet/tx/${txHash}`}
              target="_blank"
              rel="noreferrer"
              className="text-primary-700 underline"
            >
              View transaction
              <span className="sr-only"> (opens in a new tab)</span>
            </a>
          </div>
        )}

        {status === 'error' && errorMsg && (
          <div id={amountErrorId} className="mt-4 p-3 bg-red-50 border border-red-200 rounded-lg text-sm text-red-800" role="alert">
            {errorMsg}
          </div>
        )}
      </form>
    </section>
  );
}
