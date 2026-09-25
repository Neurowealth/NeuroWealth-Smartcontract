import crypto from 'crypto';

interface OTPRecord {
  codeHash: string;
  expiresAt: number; // Unix timestamp ms
  attempts: number;
}

const OTP_TTL_MS = parseInt(process.env.WHATSAPP_OTP_TTL_MS || '300000', 10); // 5 minutes default
const MAX_ATTEMPTS = parseInt(process.env.WHATSAPP_OTP_MAX_ATTEMPTS || '3', 10);

// In-memory OTP storage keyed by phone number hash
const otpStore = new Map<string, OTPRecord>();

function hashOTP(code: string): string {
  return crypto.createHash('sha256').update(code).digest('hex');
}

function isOTPMatch(storedHash: string, inputCode: string): boolean {
  const inputHash = hashOTP(inputCode.trim());
  const stored = Buffer.from(storedHash, 'hex');
  const input = Buffer.from(inputHash, 'hex');

  return stored.length === input.length && crypto.timingSafeEqual(stored, input);
}

/**
 * Generates a 6-digit numeric OTP that expires after 5 minutes.
 */
export function generateOTP(phoneHash: string): string {
  const code = crypto.randomInt(100000, 999999).toString();
  const expiresAt = Date.now() + OTP_TTL_MS;

  otpStore.set(phoneHash, {
    codeHash: hashOTP(code),
    expiresAt,
    attempts: 0
  });

  return code;
}

/**
 * Verifies an OTP code for a user.
 * Enforces 5-minute expiration and max attempt limits.
 */
export function verifyOTP(phoneHash: string, inputCode: string): { success: boolean; message: string } {
  const record = otpStore.get(phoneHash);

  if (!record) {
    return { success: false, message: 'No OTP request found. Please send "hi" to request a new code.' };
  }

  if (Date.now() > record.expiresAt) {
    otpStore.delete(phoneHash);
    return { success: false, message: 'OTP has expired (valid for 5 minutes). Please request a new code.' };
  }

  if (record.attempts >= MAX_ATTEMPTS) {
    otpStore.delete(phoneHash);
    return { success: false, message: 'Too many invalid attempts. Please request a new OTP code.' };
  }

  if (!isOTPMatch(record.codeHash, inputCode)) {
    record.attempts += 1;
    return { success: false, message: `Invalid OTP code. ${MAX_ATTEMPTS - record.attempts} attempts remaining.` };
  }

  // Verification successful: clear OTP code
  otpStore.delete(phoneHash);
  return { success: true, message: 'OTP verified successfully!' };
}