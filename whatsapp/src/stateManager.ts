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

const MAX_IN_MEMORY_SESSIONS = parseInt(process.env.WHATSAPP_MAX_SESSIONS || '10000', 10);
const SESSION_TIMEOUT_MS = parseInt(process.env.WHATSAPP_SESSION_TIMEOUT_MS || '900000', 10); // 15 minutes default
const RATE_LIMIT_MAX_MESSAGES = parseInt(process.env.WHATSAPP_RATE_LIMIT_MAX_MESSAGES || '10', 10);
const RATE_LIMIT_WINDOW_MS = parseInt(process.env.WHATSAPP_RATE_LIMIT_WINDOW_MS || '60000', 10); // 1 minute default

const sessions = new Map<string, ConversationSession>();

function sweepStaleSessions(): void {
  const now = Date.now();
  for (const [hash, session] of sessions.entries()) {
    if (now - session.lastMessageTime > SESSION_TIMEOUT_MS) {
      sessions.delete(hash);
    }
  }
}

/**
 * Retrieves session for phone hash, resetting if timed out.
 */
export function getSession(phoneHash: string): ConversationSession {
  const now = Date.now();
  let session = sessions.get(phoneHash);

  // Session timeout check
  if (session && now - session.lastMessageTime > SESSION_TIMEOUT_MS) {
    sessions.delete(phoneHash);
    session = undefined;
  }

  if (!session) {
    if (sessions.size >= MAX_IN_MEMORY_SESSIONS) {
      sweepStaleSessions();
      if (sessions.size >= MAX_IN_MEMORY_SESSIONS) {
        const firstKey = sessions.keys().next().value;
        if (firstKey) {
          sessions.delete(firstKey);
        }
      }
    }

    session = {
      state: UserState.UNVERIFIED,
      lastMessageTime: now,
      messageCount: 0,
      rateLimitWindowStart: now
    };
    sessions.set(phoneHash, session);
    return session;
  }

  session.lastMessageTime = now;
  
  // Maintain LRU order
  sessions.delete(phoneHash);
  sessions.set(phoneHash, session);
  
  return session;
}

/**
 * Updates user state in session.
 */
export function updateState(phoneHash: string, newState: UserState): void {
  const session = getSession(phoneHash);
  session.state = newState;
  sessions.set(phoneHash, session);
}

/**
 * Enforces per-phone rate limiting.
 */
export function checkRateLimit(phoneHash: string): boolean {
  const now = Date.now();
  const session = getSession(phoneHash);

  if (now - session.rateLimitWindowStart > RATE_LIMIT_WINDOW_MS) {
    session.rateLimitWindowStart = now;
    session.messageCount = 1;
    return true;
  }

  session.messageCount += 1;
  return session.messageCount <= RATE_LIMIT_MAX_MESSAGES;
}
