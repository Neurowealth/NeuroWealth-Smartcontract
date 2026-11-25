import {
  UserRecord,
  DepositRecord,
  WithdrawalRecord,
  EarningsHistoryRecord,
  YieldSnapshotRecord,
} from "./types";

/**
 * Database access helper module for User Positions & History.
 * Wraps Supabase/PostgreSQL queries with strict parameterization.
 */
export class DatabaseClient {
  private connectionString: string;

  constructor(connectionString?: string) {
    this.connectionString = connectionString || process.env.DATABASE_URL;
    if (!this.connectionString) {
      throw new Error("DATABASE_URL must be set");
    }
  }

  async getUserByStellarAddress(
    stellarAddress: string,
  ): Promise<UserRecord | null> {
    throw new Error("Not implemented");
  }

  async getUserDeposits(userId: string): Promise<DepositRecord[]> {
    throw new Error("Not implemented");
  }

  async getUserEarningsHistory(
    userId: string,
  ): Promise<EarningsHistoryRecord[]> {
    throw new Error("Not implemented");
  }

  async getUserYieldSnapshots(userId: string): Promise<YieldSnapshotRecord[]> {
    throw new Error("Not implemented");
  }
}

export const dbClient = new DatabaseClient();


