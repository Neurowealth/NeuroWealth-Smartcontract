import { getStateStore } from './stateStore';

export enum UserState {
  UNVERIFIED = 'UNVERIFIED',
  AWAITING_OTP = 'AWAITING_OTP',
  VERIFIED = 'VERIFIED'
}

export interface ConversationSession {
  state: UserState;
  lastMessageTime: number;
  messageCount: number;
  rateLimitWindowStart: number;
}

const SESSION_TIMEOUT_MS = parseInt(process.env.WHATSAPP_SESSION_TIMEOUT_MS || '900000', 10); // 15 minutes default
const RATE_LIMIT_MAX_MESSAGES = parseInt(process.env.WHATSAPP_RATE_LIMIT_MAX_MESSAGES || '10', 10);
const RATE_LIMIT_WINDOW_MS = parseInt(process.env.WHATSAPP_RATE_LIMIT_WINDOW_MS || '60000', 10); // 1 minute default

// Sessions are persisted in Postgres (see stateStore.ts) so a restart or a
// request landing on another replica sees the same conversation state.

/**
 * Retrieves session for phone hash, resetting if timed out.
 */
export async function getSession(phoneHash: string): Promise<ConversationSession> {
  const session = await getStateStore().touchSession(phoneHash, Date.now(), SESSION_TIMEOUT_MS);
  return { ...session, state: session.state as UserState };
}

/**
 * Updates user state in session.
 */
export async function updateState(phoneHash: string, newState: UserState): Promise<void> {
  await getStateStore().setSessionState(phoneHash, newState, Date.now());
}

/**
 * Enforces per-phone rate limiting.
 */
export async function checkRateLimit(phoneHash: string): Promise<boolean> {
  const now = Date.now();
  // Touching first applies the inactivity reset, as before.
  await getStateStore().touchSession(phoneHash, now, SESSION_TIMEOUT_MS);
  const count = await getStateStore().bumpMessageCount(phoneHash, now, RATE_LIMIT_WINDOW_MS);
  return count <= RATE_LIMIT_MAX_MESSAGES;
}

/**
 * Deletes sessions idle past the timeout and expired OTP challenges.
 * Replaces the old in-memory size cap; safe to run from every replica.
 */
export async function purgeExpiredState(): Promise<void> {
  await getStateStore().purgeExpired(Date.now(), SESSION_TIMEOUT_MS);
}
