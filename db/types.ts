export interface UserRecord {
  id: string;
  stellar_address: string;
  phone_hash?: string | null;
  strategy_preference: 'conservative' | 'balanced' | 'growth';
  created_at: string;
  updated_at: string;
}

export interface DepositRecord {
  id: string;
  user_id: string;
  amount: number;
  shares: number;
  tx_hash: string;
  timestamp: string;
}

export interface WithdrawalRecord {
  id: string;
  user_id: string;
  amount: number;
  shares: number;
  tx_hash: string;
  timestamp: string;
}

export interface RebalanceRecord {
  id: string;
  protocol: 'blend' | 'dex' | 'none';
  amount_moved: number;
  apy_before: number;
  apy_after: number;
  tx_hash: string;
  timestamp: string;
}

export interface YieldSnapshotRecord {
  id: string;
  user_id: string;
  total_assets: number;
  timestamp: string;
}

export interface EarningsHistoryRecord {
  id: string;
  user_id: string;
  daily_earnings: number;
  date: string;
  created_at: string;
}

export interface AuditLogRecord {
  id: string;
  table_name: string;
  action: 'INSERT' | 'UPDATE' | 'DELETE';
  record_id?: string;
  details?: Record<string, any>;
  created_at: string;
}

export interface AgentStateRecord {
  key: string;
  value: Record<string, any>;
  updated_at: string;
}
