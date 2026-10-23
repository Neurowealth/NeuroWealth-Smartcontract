'use client';

import React, { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { X, ArrowDownLeft, ArrowUpRight, Loader2, CheckCircle2, ShieldAlert, ExternalLink } from 'lucide-react';
import { ErrorBoundary } from './ErrorBoundary';
import { signWithFreighter, WalletSigningError, type WalletErrorKind } from '@/lib/freighter';
import { getBrowserTestHarness } from '@/lib/browserTestHarness';
import { useTranslations } from 'next-intl';
import {
  rpc,
  Contract,
  Address,
  nativeToScVal,
  TransactionBuilder,
  BASE_FEE,
} from '@stellar/stellar-sdk';

const RPC_URL = process.env.NEXT_PUBLIC_SOROBAN_RPC_URL || 'https://soroban-testnet.stellar.org';
const NETWORK_PASSPHRASE = process.env.NEXT_PUBLIC_SOROBAN_NETWORK_PASSPHRASE || 'Test SDF Network ; September 2015';
const VAULT_CONTRACT_ID = process.env.NEXT_PUBLIC_VAULT_CONTRACT_ID || 'CDLZFC3SYJYD7M6LJEFAPCHRLHAFKP6WYTHRF3EGO5CYD3EP4GZGM37T';

type TxErrorKind = Exclude<WalletErrorKind, 'user_rejected'> | 'submission_failed';

interface TxError {
  kind: TxErrorKind;
  message: string;
}

type ActionType = 'deposit' | 'withdraw';
type SubmissionPhase = 'idle' | 'preparing' | 'signing' | 'confirming';

interface ActionSubmission {
  phase: SubmissionPhase;
  requestId: string | null;
}

type ActionSubmissions = Record<ActionType, ActionSubmission>;

interface ActionModalProps {
  isOpen: boolean;
  onClose: () => void;
  type: ActionType;
  userPublicKey: string | null;
  balance: number;
  exchangeRate: number;
  onOptimisticBalanceChange?: (delta: number) => void;
  onTransactionConfirmed?: () => void;
}

export const ActionModal: React.FC<ActionModalProps> = ({
  isOpen,
  onClose,
  type,
  userPublicKey,
  balance,
  exchangeRate,
  onOptimisticBalanceChange,
  onTransactionConfirmed,
}) => {
  const t = useTranslations('ActionModal');
  const [amount, setAmount] = useState<string>('');
  const activeSubmissions = useRef<Record<ActionType, string | null>>({ deposit: null, withdraw: null });
  const [submissions, setSubmissions] = useState<ActionSubmissions>({
    deposit: { phase: 'idle', requestId: null },
    withdraw: { phase: 'idle', requestId: null },
  });
  const [txSuccess, setTxSuccess] = useState<boolean>(false);
  const [txHash, setTxHash] = useState<string>('');
  const [txError, setTxError] = useState<TxError | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const amountRef = useRef<HTMLInputElement>(null);
  const [portalRoot, setPortalRoot] = useState<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!isOpen || typeof document === 'undefined') return;

    const previouslyFocused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const portal = document.createElement('div');
    portal.dataset.transactionModalPortal = 'true';
    document.body.appendChild(portal);
    setPortalRoot(portal);

    const background = Array.from(document.body.children)
      .filter((element): element is HTMLElement => element instanceof HTMLElement && element !== portal)
      .map((element) => ({ element, inert: element.inert, ariaHidden: element.getAttribute('aria-hidden') }));
    background.forEach(({ element }) => {
      element.inert = true;
      element.setAttribute('aria-hidden', 'true');
    });

    return () => {
      background.forEach(({ element, inert, ariaHidden }) => {
        element.inert = inert;
        if (ariaHidden === null) element.removeAttribute('aria-hidden');
        else element.setAttribute('aria-hidden', ariaHidden);
      });
      portal.remove();
      setPortalRoot(null);
      previouslyFocused?.focus();
    };
  }, [isOpen]);

  useEffect(() => {
    if (isOpen && portalRoot) amountRef.current?.focus();
  }, [isOpen, portalRoot]);

  if (!isOpen) return null;

  const submission = submissions[type];
  const loading = submission.phase !== 'idle';
  const numAmount = parseFloat(amount) || 0;
  const estimatedShares = (numAmount / exchangeRate).toFixed(4);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!userPublicKey || numAmount <= 0 || activeSubmissions.current[type]) return;

    const actionType = type;
    const requestId = crypto.randomUUID();
    activeSubmissions.current[actionType] = requestId;
    setSubmissions((current) => ({
      ...current,
      [actionType]: { phase: 'preparing', requestId },
    }));

    setTxSuccess(false);
    setTxError(null);
    let canRetry = false;
    let submissionStarted = false;
    let optimisticBalanceDelta = 0;

    try {
      const amountInBaseUnits = BigInt(Math.round(numAmount * 1e7));
      const testHarness = getBrowserTestHarness();
      let server: rpc.Server | null = null;
      let xdr: string;
      if (testHarness) {
        xdr = await testHarness.prepareTransaction({
          method: actionType,
          publicKey: userPublicKey,
          amount: amountInBaseUnits.toString(),
        });
      } else {
        server = new rpc.Server(RPC_URL);
        const account = await server.getAccount(userPublicKey);
        const contract = new Contract(VAULT_CONTRACT_ID);
        const txBuilder = new TransactionBuilder(account, {
          fee: BASE_FEE,
          networkPassphrase: NETWORK_PASSPHRASE,
        });
        const operation = contract.call(
          actionType,
          new Address(userPublicKey).toScVal(),
          nativeToScVal(amountInBaseUnits, { type: 'i128' }),
        );
        const transaction = txBuilder.addOperation(operation).setTimeout(300).build();
        xdr = (await server.prepareTransaction(transaction)).toXDR();
      }

      setSubmissions((current) => ({
        ...current,
        [actionType]: { phase: 'signing', requestId },
      }));

      let signedXdr: string;
      try {
        signedXdr = await signWithFreighter(xdr, NETWORK_PASSPHRASE);
      } catch (err) {
        if (err instanceof WalletSigningError && err.kind === 'user_rejected') {
          // A cancelled signature is a normal, recoverable state: reset quietly
          // and never fabricate a failure transaction hash for it.
          canRetry = true;
          return;
        }
        const kind = err instanceof WalletSigningError && err.kind !== 'user_rejected' ? err.kind : 'unknown';
        const errorMessages: Record<TxErrorKind, string> = {
          wrong_network: t('wrongNetwork'),
          wallet_disconnected: t('walletDisconnected'),
          submission_failed: t('submissionFailed'),
          unknown: t('transactionFailed'),
        };
        setTxError({ kind, message: errorMessages[kind] });
        canRetry = true;
        return;
      }

      setSubmissions((current) => ({
        ...current,
        [actionType]: { phase: 'confirming', requestId },
      }));
      submissionStarted = true;
      const sendResult = testHarness
        ? await testHarness.sendTransaction(signedXdr)
        : await server!.sendTransaction(TransactionBuilder.fromXDR(signedXdr, NETWORK_PASSPHRASE));

      if (sendResult.status === 'ERROR' || sendResult.status === 'TRY_AGAIN_LATER') {
        setTxError({ kind: 'submission_failed', message: t('submissionFailed') });
        canRetry = true;
        return;
      }

      // The network has now observed this transaction - the hash is real and must
      // be preserved even while confirmation is still pending, so a later polling
      // step can't relabel a genuine submission as a failure.
      setTxHash(sendResult.hash);
      optimisticBalanceDelta = actionType === 'deposit' ? numAmount : -numAmount;
      onOptimisticBalanceChange?.(optimisticBalanceDelta);

      const confirmation = testHarness
        ? await testHarness.pollTransaction(sendResult.hash)
        : await server!.pollTransaction(sendResult.hash, { attempts: 10 });

      if (confirmation.status === 'SUCCESS') {
        setTxSuccess(true);
        canRetry = true;
        onTransactionConfirmed?.();
      } else if (confirmation.status === 'FAILED') {
        onOptimisticBalanceChange?.(-optimisticBalanceDelta);
        setTxError({ kind: 'submission_failed', message: t('submissionFailed') });
        canRetry = true;
      }
    } catch (err) {
      console.error('Transaction execution failed:', err);
      setTxError({ kind: 'unknown', message: t('transactionFailed') });
      canRetry = !submissionStarted;
    } finally {
      if (canRetry && activeSubmissions.current[actionType] === requestId) {
        activeSubmissions.current[actionType] = null;
        setSubmissions((current) => ({
          ...current,
          [actionType]: { phase: 'idle', requestId: null },
        }));
      }
    }
  };

  const handleDismissPending = () => {
    activeSubmissions.current[type] = null;
    setSubmissions((current) => ({
      ...current,
      [type]: { phase: 'idle', requestId: null },
    }));
    setAmount('');
    setTxHash('');
    setTxSuccess(false);
    setTxError(null);
  };

  const handleResetAndClose = () => {
    if (!loading) {
      setAmount('');
      setTxSuccess(false);
      setTxHash('');
      setTxError(null);
    }

    onClose();
  };

  const modal = (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-slate-950/80 backdrop-blur-md">
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="transaction-modal-title"
        aria-describedby="transaction-modal-description"
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.preventDefault();
            handleResetAndClose();
            return;
          }
          if (event.key !== 'Tab' || !dialogRef.current) return;
          const focusable = Array.from(dialogRef.current.querySelectorAll<HTMLElement>(
            'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])',
          ));
          const first = focusable[0];
          const last = focusable[focusable.length - 1];
          if (event.shiftKey && (document.activeElement === first || !dialogRef.current.contains(document.activeElement))) {
            event.preventDefault();
            last?.focus();
          } else if (!event.shiftKey && (document.activeElement === last || !dialogRef.current.contains(document.activeElement))) {
            event.preventDefault();
            first?.focus();
          }
        }}
        className="glass-panel w-full max-w-md rounded-2xl p-6 relative border border-slate-200 dark:border-slate-700 shadow-2xl animate-in fade-in zoom-in duration-200"
      >
        <button
          onClick={handleResetAndClose}
          aria-label="Close transaction dialog"
          className="absolute top-4 right-4 text-slate-400 hover:text-slate-800 dark:hover:text-white p-1 rounded-lg hover:bg-slate-100 dark:hover:bg-slate-800"
        >
          <X size={20} />
        </button>

        <ErrorBoundary surfaceName="transaction-modal" onReset={() => setTxError(null)}>
        {txSuccess ? (
          <div className="text-center py-6">
            <div className="h-16 w-16 bg-emerald-500/20 text-emerald-400 rounded-full flex items-center justify-center mx-auto mb-4 border border-emerald-500/30">
              <CheckCircle2 size={36} />
            </div>
            <h3 id="transaction-modal-title" className="text-xl font-bold text-slate-900 dark:text-white mb-2">
              {type === 'deposit' ? t('depositSuccessful') : t('withdrawalSuccessful')}
            </h3>
            <p className="sr-only" role="status" aria-live="polite" aria-atomic="true">{t('transactionConfirmed')}</p>
            <p id="transaction-modal-description" className="text-sm text-slate-600 dark:text-slate-400 mb-4 font-mono">
              {t('transactionHash')} {txHash.substring(0, 12)}...{txHash.substring(txHash.length - 8)}
            </p>
            <p className="text-xs text-emerald-400 bg-emerald-500/10 py-2 px-3 rounded-lg border border-emerald-500/20 mb-6">
              {t('confirmedOnNetwork')}
            </p>
            <button
              onClick={handleResetAndClose}
              className="w-full bg-emerald-500 hover:bg-emerald-400 text-slate-950 font-bold py-3 rounded-xl transition-all"
            >
              {t('done')}
            </button>
          </div>
        ) : (
          <form onSubmit={handleSubmit}>
            <div className="flex items-center gap-2 mb-6">
              {type === 'deposit' ? (
                <div className="h-10 w-10 bg-emerald-500/20 text-emerald-400 rounded-xl flex items-center justify-center">
                  <ArrowDownLeft size={22} />
                </div>
              ) : (
                <div className="h-10 w-10 bg-indigo-500/20 text-indigo-400 rounded-xl flex items-center justify-center">
                  <ArrowUpRight size={22} />
                </div>
              )}
              <div>
                <h3 id="transaction-modal-title" className="text-lg font-bold text-slate-900 dark:text-white capitalize">
                  {type === 'deposit' ? t('depositUsdc') : t('withdrawUsdc')}
                </h3>
                <p id="transaction-modal-description" className="text-xs text-slate-600 dark:text-slate-400">
                  {type === 'deposit' ? t('mintShares') : t('burnShares')}
                </p>
              </div>
            </div>

            <div className="mb-4">
              <div className="flex justify-between text-xs text-slate-600 dark:text-slate-400 mb-1.5 font-medium">
                <label htmlFor="transaction-amount">{t('amountLabel')}</label>
                <span>{t('available')} {balance.toFixed(2)} USDC</span>
              </div>
              <div className="relative">
                <input
                  ref={amountRef}
                  id="transaction-amount"
                  type="number"
                  step="0.01"
                  min="1"
                  max={type === 'withdraw' ? balance : 10000}
                  value={amount}
                  onChange={(e) => setAmount(e.target.value)}
                  placeholder="0.00"
                  aria-invalid={Boolean(txError)}
                  aria-describedby={txError ? 'transaction-amount-error' : undefined}
                  className="w-full bg-slate-100 dark:bg-slate-900 border border-slate-300 dark:border-slate-700 rounded-xl py-3 px-4 text-slate-900 dark:text-white font-mono text-lg focus:outline-none focus:border-emerald-500 transition-colors"
                  required
                />
                <button
                  type="button"
                  onClick={() => setAmount(type === 'withdraw' ? balance.toString() : '100')}
                  className="absolute right-3 top-3 text-xs font-semibold text-emerald-600 dark:text-emerald-400 hover:text-emerald-500 dark:hover:text-emerald-300 bg-emerald-500/10 px-2 py-1 rounded"
                >
                  {t('max')}
                </button>
              </div>
            </div>

            <div className="bg-slate-100 dark:bg-slate-900/80 p-3 rounded-xl border border-slate-200 dark:border-slate-800 text-xs space-y-2 mb-6 font-mono text-slate-700 dark:text-slate-300">
              <div className="flex justify-between">
                <span className="text-slate-600 dark:text-slate-400">{t('exchangeRate')}</span>
                <span>{exchangeRate.toFixed(4)} {t('usdcPerShare')}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-slate-600 dark:text-slate-400">{t('estimatedShares')}</span>
                <span className="text-emerald-600 dark:text-emerald-400 font-semibold">{estimatedShares} {t('nvShares')}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-slate-600 dark:text-slate-400">{t('networkFee')}</span>
                <span className="text-slate-600 dark:text-slate-400">&lt; 0.00001 XLM</span>
              </div>
            </div>

            {txError && (
              <div
                role="alert"
                className="flex items-start gap-2 bg-rose-500/10 border border-rose-500/20 text-rose-300 text-xs rounded-xl p-3 mb-4"
              >
                <ShieldAlert size={16} className="shrink-0 mt-0.5" />
                <div>
                  <p id="transaction-amount-error">{txError.message}</p>
                  {txHash && (
                    <p className="mt-1 font-mono text-rose-400/80">
                      Transaction Hash: {txHash.substring(0, 12)}...{txHash.substring(txHash.length - 8)}
                    </p>
                  )}
                </div>
              </div>
            )}

            {loading && submission.phase === 'confirming' && txHash && (
              <div role="status" className="text-xs text-amber-700 dark:text-amber-300 bg-amber-500/10 border border-amber-500/20 rounded-xl p-3 mb-4 space-y-2">
                <div>
                  <p className="font-semibold">{t('transactionSubmitted')}</p>
                  <p className="mt-1 font-mono text-amber-800 dark:text-amber-200/80">
                    {t('transactionHash')} {txHash.substring(0, 12)}...{txHash.substring(txHash.length - 8)}
                  </p>
                </div>
                <div className="flex items-center justify-between pt-2 border-t border-amber-500/20">
                  <a
                    href={`https://stellar.expert/explorer/testnet/tx/${txHash}`}
                    target="_blank"
                    rel="noreferrer"
                    className="text-[11px] text-amber-700 dark:text-amber-200 hover:underline flex items-center gap-1"
                  >
                    <span>View on Explorer</span>
                    <ExternalLink size={10} />
                  </a>
                  <button
                    type="button"
                    onClick={handleDismissPending}
                    className="px-2.5 py-1 text-[11px] font-semibold bg-amber-500/20 hover:bg-amber-500/30 text-amber-800 dark:text-amber-200 rounded-lg transition-colors"
                  >
                    {t('checkStatusLater')}
                  </button>
                </div>
              </div>
            )}

            <p className="sr-only" aria-live="polite" aria-atomic="true">
              {loading ? `Transaction ${submission.phase}.` : txSuccess ? 'Transaction confirmed.' : ''}
            </p>

            <button
              type="submit"
              disabled={loading || numAmount <= 0}
              data-client-request-id={submission.requestId ?? undefined}
              className="w-full flex items-center justify-center gap-2 bg-gradient-to-r from-emerald-500 to-teal-600 hover:from-emerald-400 hover:to-teal-500 text-slate-950 font-bold py-3.5 rounded-xl transition-all shadow-glow-emerald disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {loading ? (
                <>
                  <Loader2 className="animate-spin" size={18} />
                  <span>
                    {submission.phase === 'preparing' && t('preparingTransaction')}
                    {submission.phase === 'signing' && t('signingWithFreighter')}
                    {submission.phase === 'confirming' && t('confirmingTransaction')}
                  </span>
                </>
              ) : (
                <span>{type === 'deposit' ? t('confirmDeposit') : t('confirmWithdrawal')}</span>
              )}
            </button>
          </form>
        )}
        </ErrorBoundary>
      </div>
    </div>
  );

  return portalRoot ? createPortal(modal, portalRoot) : null;
};
