/**
 * This file is auto-generated from supabase/migrations/*.sql.
 * Do not edit manually. Run `python3 scripts/generate-db-types.py` to regenerate.
 */

export interface AgentRebalanceOperationRecord {
  operation_id: string;
  decision_id: string;
  attempt: number;
  execution_mode: 'live' | 'dry-run';
  status: 'prepared' | 'submitting' | 'submitted' | 'reconciling' | 'confirmed' | 'rejected' | 'dry_run';
  tx_hash?: string | null;
  input_snapshot: Record<string, any>;
  simulation_result?: Record<string, any> | null;
  retry_reason?: string | null;
  last_error?: string | null;
  created_at: string;
  updated_at: string;
}

export interface AgentStateRecord {
  key: string;
  value: Record<string, any>;
  updated_at: string;
}

export interface AuditLogRecord {
  id: string;
  table_name: string;
  action: 'INSERT' | 'UPDATE' | 'DELETE';
  record_id?: string | null;
  details?: Record<string, any> | null;
  created_at: string;
}

export interface DepositRecord {
  id: string;
  user_id: string;
  amount: number;
  shares: number;
  tx_hash: string;
  timestamp: string;
}

export interface EarningsHistoryRecord {
  id: string;
  user_id: string;
  daily_earnings: number;
  date: string;
  created_at: string;
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

export interface UserRecord {
  id: string;
  stellar_address: string;
  phone_hash?: string | null;
  strategy_preference: 'conservative' | 'balanced' | 'growth';
  created_at: string;
  updated_at: string;
}

export interface VaultEventRecord {
  id: string;
  event_id: string;
  event_type: string;
  ledger_sequence: number;
  created_at: string;
}

export interface WhatsappMessageDeliveryRecord {
  message_sid: string;
  phone_hash?: string | null;
  to_encrypted?: string | null;
  to_iv?: string | null;
  to_tag?: string | null;
  status: string;
  error_code?: string | null;
  error_reason?: string | null;
  retry_count: number;
  escalated_at?: string | null;
  created_at: string;
  updated_at: string;
}

export interface WhatsappWalletRecord {
  phone_hash: string;
  public_key: string;
  encrypted_data: string;
  iv: string;
  tag: string;
  created_at: string;
}

export interface WithdrawalRecord {
  id: string;
  user_id: string;
  amount: number;
  shares: number;
  tx_hash: string;
  timestamp: string;
}

export interface YieldSnapshotRecord {
  id: string;
  user_id: string;
  total_assets: number;
  timestamp: string;
}

export interface Database {
  public: {
    Tables: {
      agent_rebalance_operations: {
        Row: AgentRebalanceOperationRecord;
        Insert: {
          operation_id?: string;
          decision_id: string;
          attempt: number;
          execution_mode: 'live' | 'dry-run';
          status: 'prepared' | 'submitting' | 'submitted' | 'reconciling' | 'confirmed' | 'rejected' | 'dry_run';
          tx_hash?: string | null;
          input_snapshot: Record<string, any>;
          simulation_result?: Record<string, any> | null;
          retry_reason?: string | null;
          last_error?: string | null;
          created_at?: string;
          updated_at?: string;
        };
        Update: {
          operation_id?: string;
          decision_id?: string;
          attempt?: number;
          execution_mode?: 'live' | 'dry-run';
          status?: 'prepared' | 'submitting' | 'submitted' | 'reconciling' | 'confirmed' | 'rejected' | 'dry_run';
          tx_hash?: string | null;
          input_snapshot?: Record<string, any>;
          simulation_result?: Record<string, any> | null;
          retry_reason?: string | null;
          last_error?: string | null;
          created_at?: string;
          updated_at?: string;
        };
      };
      agent_state: {
        Row: AgentStateRecord;
        Insert: {
          key?: string;
          value: Record<string, any>;
          updated_at?: string;
        };
        Update: {
          key?: string;
          value?: Record<string, any>;
          updated_at?: string;
        };
      };
      audit_logs: {
        Row: AuditLogRecord;
        Insert: {
          id?: string;
          table_name: string;
          action: 'INSERT' | 'UPDATE' | 'DELETE';
          record_id?: string | null;
          details?: Record<string, any> | null;
          created_at?: string;
        };
        Update: {
          id?: string;
          table_name?: string;
          action?: 'INSERT' | 'UPDATE' | 'DELETE';
          record_id?: string | null;
          details?: Record<string, any> | null;
          created_at?: string;
        };
      };
      deposits: {
        Row: DepositRecord;
        Insert: {
          id?: string;
          user_id: string;
          amount: number;
          shares: number;
          tx_hash: string;
          timestamp?: string;
        };
        Update: {
          id?: string;
          user_id?: string;
          amount?: number;
          shares?: number;
          tx_hash?: string;
          timestamp?: string;
        };
      };
      earnings_history: {
        Row: EarningsHistoryRecord;
        Insert: {
          id?: string;
          user_id: string;
          daily_earnings: number;
          date: string;
          created_at?: string;
        };
        Update: {
          id?: string;
          user_id?: string;
          daily_earnings?: number;
          date?: string;
          created_at?: string;
        };
      };
      rebalances: {
        Row: RebalanceRecord;
        Insert: {
          id?: string;
          protocol: 'blend' | 'dex' | 'none';
          amount_moved: number;
          apy_before: number;
          apy_after: number;
          tx_hash: string;
          timestamp?: string;
        };
        Update: {
          id?: string;
          protocol?: 'blend' | 'dex' | 'none';
          amount_moved?: number;
          apy_before?: number;
          apy_after?: number;
          tx_hash?: string;
          timestamp?: string;
        };
      };
      users: {
        Row: UserRecord;
        Insert: {
          id?: string;
          stellar_address: string;
          phone_hash?: string | null;
          strategy_preference?: 'conservative' | 'balanced' | 'growth';
          created_at?: string;
          updated_at?: string;
        };
        Update: {
          id?: string;
          stellar_address?: string;
          phone_hash?: string | null;
          strategy_preference?: 'conservative' | 'balanced' | 'growth';
          created_at?: string;
          updated_at?: string;
        };
      };
      vault_events: {
        Row: VaultEventRecord;
        Insert: {
          id?: string;
          event_id: string;
          event_type: string;
          ledger_sequence: number;
          created_at?: string;
        };
        Update: {
          id?: string;
          event_id?: string;
          event_type?: string;
          ledger_sequence?: number;
          created_at?: string;
        };
      };
      whatsapp_message_deliveries: {
        Row: WhatsappMessageDeliveryRecord;
        Insert: {
          message_sid?: string;
          phone_hash?: string | null;
          to_encrypted?: string | null;
          to_iv?: string | null;
          to_tag?: string | null;
          status: string;
          error_code?: string | null;
          error_reason?: string | null;
          retry_count?: number;
          escalated_at?: string | null;
          created_at?: string;
          updated_at?: string;
        };
        Update: {
          message_sid?: string;
          phone_hash?: string | null;
          to_encrypted?: string | null;
          to_iv?: string | null;
          to_tag?: string | null;
          status?: string;
          error_code?: string | null;
          error_reason?: string | null;
          retry_count?: number;
          escalated_at?: string | null;
          created_at?: string;
          updated_at?: string;
        };
      };
      whatsapp_wallets: {
        Row: WhatsappWalletRecord;
        Insert: {
          phone_hash?: string;
          public_key: string;
          encrypted_data: string;
          iv: string;
          tag: string;
          created_at?: string;
        };
        Update: {
          phone_hash?: string;
          public_key?: string;
          encrypted_data?: string;
          iv?: string;
          tag?: string;
          created_at?: string;
        };
      };
      withdrawals: {
        Row: WithdrawalRecord;
        Insert: {
          id?: string;
          user_id: string;
          amount: number;
          shares: number;
          tx_hash: string;
          timestamp?: string;
        };
        Update: {
          id?: string;
          user_id?: string;
          amount?: number;
          shares?: number;
          tx_hash?: string;
          timestamp?: string;
        };
      };
      yield_snapshots: {
        Row: YieldSnapshotRecord;
        Insert: {
          id?: string;
          user_id: string;
          total_assets: number;
          timestamp?: string;
        };
        Update: {
          id?: string;
          user_id?: string;
          total_assets?: number;
          timestamp?: string;
        };
      };
    };
    Views: Record<string, never>;
    Functions: Record<string, never>;
  };
}
