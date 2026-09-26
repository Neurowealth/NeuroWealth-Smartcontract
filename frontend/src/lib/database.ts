import { supabase } from './supabase';

export interface EarningsSummary {
  today: number;
  week: number;
  month: number;
}

export interface ChartDataPoint {
  date: string;
  value: number;
  yield: number;
}

export interface TransactionRecord {
  id: string;
  type: 'deposit' | 'withdrawal' | 'rebalance';
  amount: number;
  asset?: string;
  txHash: string;
  timestamp: string;
  status: 'confirmed' | 'pending';
}

export async function getEarningsSummary(userAddress?: string): Promise<EarningsSummary> {
  if (!userAddress || !supabaseUrl()) return { today: 0, week: 0, month: 0 };

  const { data: user } = await supabase
    .from('users')
    .select('id')
    .eq('stellar_address', userAddress)
    .single();

  if (!user) return { today: 0, week: 0, month: 0 };

  const now = new Date();
  const todayStart = new Date(now.getFullYear(), now.getMonth(), now.getDate()).toISOString();
  const weekStart = new Date(now.getTime() - 7 * 24 * 60 * 60 * 1000).toISOString();
  const monthStart = new Date(now.getFullYear(), now.getMonth(), 1).toISOString();

  const { data: earnings } = await supabase
    .from('earnings_history')
    .select('daily_earnings, date')
    .eq('user_id', user.id)
    .gte('date', monthStart.split('T')[0])
    .order('date', { ascending: false });

  if (!earnings || earnings.length === 0) return { today: 0, week: 0, month: 0 };

  let today = 0;
  let week = 0;
  let month = 0;

  for (const entry of earnings) {
    const amount = parseFloat(String(entry.daily_earnings));
    const entryDate = new Date(entry.date);
    month += amount;
    if (entryDate >= new Date(weekStart)) week += amount;
    if (entryDate >= new Date(todayStart)) today += amount;
  }

  return {
    today: Number(today.toFixed(2)),
    week: Number(week.toFixed(2)),
    month: Number(month.toFixed(2)),
  };
}

export async function getPortfolioValueHistory(userAddress?: string): Promise<ChartDataPoint[]> {
  if (!userAddress || !supabaseUrl()) return [];

  const { data: user } = await supabase
    .from('users')
    .select('id')
    .eq('stellar_address', userAddress)
    .single();

  if (!user) return [];

  const { data: snapshots } = await supabase
    .from('yield_snapshots')
    .select('total_assets, timestamp')
    .eq('user_id', user.id)
    .order('timestamp', { ascending: true })
    .limit(30);

  if (!snapshots || snapshots.length === 0) return [];

  let prevAssets = 0;
  return snapshots.map((s) => {
    const assets = parseFloat(String(s.total_assets));
    const date = new Date(s.timestamp);
    const label = `${date.toLocaleString('en-US', { month: 'short' })} ${date.getDate()}`;
    const yieldVal = Number((assets - prevAssets).toFixed(2));
    prevAssets = assets;
    return { date: label, value: Number(assets.toFixed(2)), yield: yieldVal };
  });
}

export async function getRecentTransactions(userAddress?: string): Promise<TransactionRecord[]> {
  if (!userAddress) return [];
  
  if (!supabaseUrl()) {
    return getRecentTransactionsFromRpc(userAddress);
  }

  const { data: user } = await supabase
    .from('users')
    .select('id')
    .eq('stellar_address', userAddress)
    .single();

  if (!user) return [];

  const [depositsRes, withdrawalsRes] = await Promise.all([
    supabase
      .from('deposits')
      .select('id, amount, tx_hash, timestamp')
      .eq('user_id', user.id)
      .order('timestamp', { ascending: false })
      .limit(10),
    supabase
      .from('withdrawals')
      .select('id, amount, tx_hash, timestamp')
      .eq('user_id', user.id)
      .order('timestamp', { ascending: false })
      .limit(10),
  ]);

  const deposits = (depositsRes.data || []).map((d) => ({
    id: d.id,
    type: 'deposit' as const,
    amount: parseFloat(String(d.amount)),
    txHash: d.tx_hash,
    timestamp: new Date(d.timestamp).toLocaleString('en-US', {
      year: 'numeric', month: '2-digit', day: '2-digit',
      hour: '2-digit', minute: '2-digit',
    }),
    status: 'confirmed' as const,
  }));

  const withdrawals = (withdrawalsRes.data || []).map((w) => ({
    id: w.id,
    type: 'withdrawal' as const,
    amount: parseFloat(String(w.amount)),
    txHash: w.tx_hash,
    timestamp: new Date(w.timestamp).toLocaleString('en-US', {
      year: 'numeric', month: '2-digit', day: '2-digit',
      hour: '2-digit', minute: '2-digit',
    }),
    status: 'confirmed' as const,
  }));

  return [...deposits, ...withdrawals]
    .sort((a, b) => new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime())
    .slice(0, 10);
}

function supabaseUrl(): boolean {
  return !!process.env.NEXT_PUBLIC_SUPABASE_URL;
}

import { server } from './stellar';
import { rpc, xdr, scValToNative } from '@stellar/stellar-sdk';

/**
 * Fallback to fetch transactions directly from Soroban RPC events
 * Note: 'mock mode' as requested by the issue, although this fetches real on-chain events.
 */
async function getRecentTransactionsFromRpc(userAddress: string): Promise<TransactionRecord[]> {
  try {
    const contractId = process.env.NEXT_PUBLIC_VAULT_CONTRACT_ID;
    if (!contractId) return [];

    const latestLedger = await server.getLatestLedger();
    const startLedger = Math.max(1, latestLedger.sequence - 10000); // look back ~10000 ledgers

    const request: Parameters<typeof server.getEvents>[0] = {
      startLedger,
      filters: [{ type: 'contract', contractIds: [contractId] }],
      limit: 100,
    };
    
    const response = await server.getEvents(request);
    
    const records: TransactionRecord[] = [];
    
    for (const event of response.events) {
      if (event.type !== 'contract') continue;
      
      const topic1 = event.topic[0];
      if (!topic1) continue;
      
      let eventType = '';
      try {
        const nativeTopic = scValToNative(topic1);
        if (nativeTopic === 'deposit' || nativeTopic === 'withdraw') {
          eventType = nativeTopic;
        } else {
          continue;
        }
      } catch (e) {
        continue;
      }
      
      try {
        const val = event.value;
        const decoded = scValToNative(val);
        // decode event structure { caller, amount, ... } or similar depending on contract
        // assuming standard map or struct where 'amount' and 'user' exist
        
        let amount = 0;
        let user = '';
        if (typeof decoded === 'object' && decoded !== null) {
          if ('amount' in decoded) amount = Number(decoded.amount) / 1e7;
          if ('user' in decoded) user = String(decoded.user);
          if ('caller' in decoded) user = String(decoded.caller);
          // if array format: [user, amount]
          if (Array.isArray(decoded) && decoded.length >= 2) {
            user = String(decoded[0]);
            amount = Number(decoded[1]) / 1e7;
          }
        }
        
        if (user !== userAddress) continue;
        
        records.push({
          id: event.id,
          type: eventType === 'deposit' ? 'deposit' : 'withdrawal',
          amount: amount || 0,
          txHash: event.txHash,
          timestamp: new Date().toLocaleString('en-US', { // Mocked timestamp since RPC doesn't provide it
            year: 'numeric', month: '2-digit', day: '2-digit',
            hour: '2-digit', minute: '2-digit',
          }) + ' (RPC Fallback)',
          status: 'confirmed'
        });
      } catch (e) {
        // ignore parsing errors
      }
    }
    
    return records.reverse().slice(0, 10);
  } catch (error) {
    console.warn('Failed to fetch fallback transactions from RPC', error);
    return [];
  }
}

