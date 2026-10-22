import crypto from 'crypto';
import { getStateStore } from './stateStore';

const OTP_TTL_MS = parseInt(process.env.WHATSAPP_OTP_TTL_MS || '300000', 10); // 5 minutes default
const MAX_ATTEMPTS = parseInt(process.env.WHATSAPP_OTP_MAX_ATTEMPTS || '3', 10);

// Challenges are persisted in Postgres (see stateStore.ts) so they survive
// restarts and are shared by every replica. Only a hash is stored, bound to
// the phone hash so identical codes for different users never collide.
function hashOTP(phoneHash: string, code: string): string {
  return crypto.createHash('sha256').update(`${phoneHash}:${code}`).digest('hex');
}

function isOTPMatch(phoneHash: string, storedHash: string, inputCode: string): boolean {
  const inputHash = hashOTP(phoneHash, inputCode.trim());
  const stored = Buffer.from(storedHash, 'hex');
  const input = Buffer.from(inputHash, 'hex');

  return stored.length === input.length && crypto.timingSafeEqual(stored, input);
}

/**
 * Generates a 6-digit numeric OTP that expires after 5 minutes.
 */
export async function generateOTP(phoneHash: string): Promise<string> {
  const code = crypto.randomInt(100000, 999999).toString();
  await getStateStore().putOtp(phoneHash, hashOTP(phoneHash, code), Date.now() + OTP_TTL_MS);
  return code;
}

export function clearOTP(phoneHash: string): void {
  otpStore.delete(phoneHash);
}

/**
 * Verifies an OTP code for a user.
 * Enforces 5-minute expiration and max attempt limits. Each call counts as an
 * attempt atomically in the database, so replicas cannot exceed the limit by
 * guessing in parallel, and a correct code can be redeemed only once.
 */
export async function verifyOTP(phoneHash: string, inputCode: string): Promise<{ success: boolean; message: string; shouldResetSession: boolean }> {
  const store = getStateStore();
  const attempt = await store.attemptOtp(phoneHash, Date.now(), MAX_ATTEMPTS);

  switch (attempt.kind) {
    case 'none':
      return { success: false, message: 'No OTP request found. Please send "hi" to request a new code.', shouldResetSession: false };
    case 'expired':
      return { success: false, message: 'OTP has expired (valid for 5 minutes). Please send "hi" to request a new code.', shouldResetSession: true };
    case 'locked':
      return { success: false, message: 'Too many invalid attempts. Please send "hi" to request a new OTP code.', shouldResetSession: true };
  }

  if (!isOTPMatch(phoneHash, attempt.codeHash, inputCode)) {
    return { success: false, message: `Invalid OTP code. ${Math.max(MAX_ATTEMPTS - attempt.attempts, 0)} attempts remaining.`, shouldResetSession: false };
  }

  // Verification successful: consume the code. Losing this race means another
  // replica already redeemed it.
  if (!(await store.consumeOtp(phoneHash, attempt.codeHash))) {
    return { success: false, message: 'No OTP request found. Please send "hi" to request a new code.', shouldResetSession: false };
  }
  return { success: true, message: 'OTP verified successfully!', shouldResetSession: false };
}
