/**
 * Loads `.env` and validates the environment before anything else runs.
 *
 * Import this module FIRST in every entry point: modules imported after it
 * (vaultRouter reads SOROBAN_RPC_URL / VAULT_CONTRACT_ID at load time) then
 * see an environment that has already been checked, and an insecure
 * configuration fails the boot with one readable report (#945).
 */
import dotenv from 'dotenv';
import { assertEnv } from './envGuard';

dotenv.config();
assertEnv();
