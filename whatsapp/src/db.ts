import { Pool } from 'pg';
import dotenv from 'dotenv';

dotenv.config();

let pool: Pool | undefined;

// The pool is created on first use so that importing this module never needs
// a database, and so a missing DATABASE_URL fails closed instead of silently
// falling back to a well-known local credential (see envGuard.ts).
function getPool(): Pool {
  if (!pool) {
    const connectionString = process.env.DATABASE_URL;
    if (!connectionString) {
      throw new Error('DATABASE_URL must be set; refusing to use an unsafe database default');
    }
    pool = new Pool({ connectionString });
  }
  return pool;
}

export const query = (text: string, params?: any[]) => getPool().query(text, params);
