'use client';

import { useState, useEffect, useCallback, useRef } from 'react';

interface PortfolioEvent {
  type: 'deposit' | 'withdrawal' | 'rebalance' | 'yield';
  amount?: string;
  asset?: string;
  timestamp: number;
  ledger?: number;
  txHash?: string;
}

type ConnectionStatus = 'connecting' | 'connected' | 'reconnecting' | 'disconnected';
type SubscriptionStatus = 'idle' | 'pending' | 'active' | 'failed';

interface PortfolioState {
  balance: {
    idle: number;
    deployed: number;
    total: number;
  };
  yieldAccrual: {
    rate: number;
    earned24h: number;
    earnedTotal: number;
  };
  lastUpdate: number;
  isConnected: boolean;
  connectionType: 'websocket' | 'polling' | 'disconnected';
  connectionStatus: ConnectionStatus;
  subscriptionStatus: SubscriptionStatus;
  isStale: boolean;
}

interface UseRealtimePortfolioOptions {
  wsUrl?: string;
  pollIntervalMs?: number;
  contractId?: string;
  accountAddress?: string;
  maxReconnectAttempts?: number;
}

const DEFAULT_POLL_INTERVAL = 10000;
const DEFAULT_MAX_RECONNECT_ATTEMPTS = 5;
const MAX_BACKOFF_MS = 30000;

function eventDedupeKey(event: PortfolioEvent): string {
  return event.txHash ?? `${event.type}-${event.timestamp}-${event.amount ?? ''}-${event.ledger ?? ''}`;
}

export function useRealtimePortfolio(options: UseRealtimePortfolioOptions = {}) {
  const {
    wsUrl = process.env.NEXT_PUBLIC_WS_URL || 'ws://localhost:3001',
    pollIntervalMs = DEFAULT_POLL_INTERVAL,
    contractId,
    accountAddress,
    maxReconnectAttempts = DEFAULT_MAX_RECONNECT_ATTEMPTS,
  } = options;

  const [state, setState] = useState<PortfolioState>({
    balance: { idle: 0, deployed: 0, total: 0 },
    yieldAccrual: { rate: 0, earned24h: 0, earnedTotal: 0 },
    lastUpdate: Date.now(),
    isConnected: false,
    connectionType: 'disconnected',
    connectionStatus: 'connecting',
    subscriptionStatus: 'idle',
    isStale: true,
  });

  const [events, setEvents] = useState<PortfolioEvent[]>([]);
  const wsRef = useRef<WebSocket | null>(null);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const reconnectTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const reconnectAttempts = useRef(0);
  // Guards against a socket that belongs to a previous account/contract subscription
  // (e.g. still connecting, or about to fire a stray onclose) from mutating state
  // after the subscription target has moved on.
  const subscriptionKeyRef = useRef<string | null>(null);
  const intentionalCloseRef = useRef(false);

  const addEvent = useCallback((event: PortfolioEvent) => {
    setEvents(prev => {
      const key = eventDedupeKey(event);
      if (prev.some(existing => eventDedupeKey(existing) === key)) {
        return prev;
      }
      return [event, ...prev].slice(0, 100);
    });
  }, []);

  const fetchAuthoritativeSnapshot = useCallback(async () => {
    try {
      const params = new URLSearchParams();
      if (contractId) params.set('contractId', contractId);
      if (accountAddress) params.set('address', accountAddress);

      const response = await fetch(`/api/portfolio?${params.toString()}`);
      if (!response.ok) throw new Error('Failed to fetch portfolio');

      const data = await response.json();
      setState(prev => ({
        ...prev,
        balance: data.balance || prev.balance,
        yieldAccrual: data.yield || prev.yieldAccrual,
        lastUpdate: Date.now(),
        isStale: false,
      }));
    } catch (error) {
      console.error('[Realtime] Authoritative refresh failed:', error);
    }
  }, [contractId, accountAddress]);

  const stopPolling = useCallback(() => {
    if (pollRef.current) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }
  }, []);

  const startPolling = useCallback(() => {
    if (pollRef.current) return;

    console.log('[Realtime] Falling back to polling');
    setState(prev => ({
      ...prev,
      isConnected: true,
      connectionType: 'polling',
      connectionStatus: 'connected',
      subscriptionStatus: 'active',
    }));

    fetchAuthoritativeSnapshot();
    pollRef.current = setInterval(fetchAuthoritativeSnapshot, pollIntervalMs);
  }, [fetchAuthoritativeSnapshot, pollIntervalMs]);

  const connectWebSocket = useCallback(() => {
    if (wsRef.current?.readyState === WebSocket.OPEN) return;

    const subscriptionKey = `${contractId ?? ''}::${accountAddress ?? ''}`;
    subscriptionKeyRef.current = subscriptionKey;
    intentionalCloseRef.current = false;

    setState(prev => ({
      ...prev,
      connectionStatus: reconnectAttempts.current > 0 ? 'reconnecting' : 'connecting',
      subscriptionStatus: 'pending',
    }));

    try {
      const ws = new WebSocket(wsUrl);
      wsRef.current = ws;

      ws.onopen = () => {
        // The subscription target changed while this socket was still connecting.
        if (subscriptionKeyRef.current !== subscriptionKey) {
          ws.close();
          return;
        }

        console.log('[Realtime] WebSocket connected');
        const wasReconnect = reconnectAttempts.current > 0;
        reconnectAttempts.current = 0;
        stopPolling();

        setState(prev => ({
          ...prev,
          isConnected: true,
          connectionType: 'websocket',
          connectionStatus: 'connected',
          subscriptionStatus: 'active',
        }));

        if (contractId) {
          ws.send(JSON.stringify({ action: 'subscribe', contractId }));
        }
        if (accountAddress) {
          ws.send(JSON.stringify({ action: 'subscribe_account', address: accountAddress }));
        }

        // Events may have been missed while disconnected - resync from source of truth
        // rather than waiting for the next incremental push.
        if (wasReconnect) {
          fetchAuthoritativeSnapshot();
        }
      };

      ws.onmessage = (event) => {
        if (subscriptionKeyRef.current !== subscriptionKey) return;

        try {
          const data = JSON.parse(event.data);

          if (data.type === 'portfolio_update') {
            setState(prev => ({
              ...prev,
              balance: data.balance || prev.balance,
              yieldAccrual: data.yield || prev.yieldAccrual,
              lastUpdate: Date.now(),
              isStale: false,
            }));
          } else if (data.type === 'vault_event') {
            addEvent({
              type: data.eventType,
              amount: data.amount,
              asset: data.asset,
              timestamp: Date.now(),
              ledger: data.ledger,
              txHash: data.txHash,
            });
          }
        } catch (e) {
          console.error('[Realtime] Failed to parse message:', e);
        }
      };

      ws.onclose = () => {
        // Ignore closes from a stale socket, and skip auto-reconnect for closes we
        // triggered ourselves (disconnect() / account switch).
        if (subscriptionKeyRef.current !== subscriptionKey || intentionalCloseRef.current) {
          return;
        }

        console.log('[Realtime] WebSocket disconnected');
        setState(prev => ({
          ...prev,
          isConnected: false,
          connectionType: 'disconnected',
          connectionStatus: 'reconnecting',
          isStale: true,
        }));

        if (reconnectAttempts.current >= maxReconnectAttempts) {
          console.error('[Realtime] Max reconnect attempts reached, falling back to polling');
          setState(prev => ({ ...prev, subscriptionStatus: 'failed' }));
          fetchAuthoritativeSnapshot();
          startPolling();
          return;
        }

        const delay = Math.min(1000 * Math.pow(2, reconnectAttempts.current), MAX_BACKOFF_MS);
        reconnectAttempts.current++;
        reconnectTimeoutRef.current = setTimeout(connectWebSocket, delay);
      };

      ws.onerror = (error) => {
        console.error('[Realtime] WebSocket error:', error);
        ws.close();
      };
    } catch (error) {
      console.error('[Realtime] Failed to connect WebSocket:', error);
      startPolling();
    }
  }, [wsUrl, contractId, accountAddress, addEvent, fetchAuthoritativeSnapshot, startPolling, stopPolling, maxReconnectAttempts]);

  const disconnect = useCallback(() => {
    intentionalCloseRef.current = true;
    subscriptionKeyRef.current = null;
    reconnectAttempts.current = 0;

    if (reconnectTimeoutRef.current) {
      clearTimeout(reconnectTimeoutRef.current);
      reconnectTimeoutRef.current = null;
    }
    if (wsRef.current) {
      wsRef.current.close();
      wsRef.current = null;
    }
    stopPolling();

    setState(prev => ({
      ...prev,
      isConnected: false,
      connectionType: 'disconnected',
      connectionStatus: 'disconnected',
      subscriptionStatus: 'idle',
    }));
  }, [stopPolling]);

  useEffect(() => {
    // A new contract/account means a brand new subscription - drop any events
    // that belonged to the previous one before cancelling it and connecting fresh.
    setEvents([]);
    connectWebSocket();
    return disconnect;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [contractId, accountAddress, wsUrl]);

  return {
    ...state,
    events,
    disconnect,
    reconnect: connectWebSocket,
  };
}
