import twilio from 'twilio';
import pino from 'pino';

const logger = pino({ name: 'whatsapp-sms' });

/**
 * Thin wrapper around the Twilio REST client, exposed as a mutable object so
 * tests can monkey-patch `send` instead of hitting the real Twilio API
 * (mirrors the `server`/`pool` monkey-patch pattern used in agent tests).
 */
export const smsClient = {
  async send(toNumber: string, body: string): Promise<void> {
    const accountSid = process.env.TWILIO_ACCOUNT_SID;
    const authToken = process.env.TWILIO_AUTH_TOKEN;
    const from = process.env.TWILIO_SMS_FROM_NUMBER;

    if (!accountSid || !authToken || !from) {
      throw new Error('Twilio SMS is not configured (TWILIO_ACCOUNT_SID/TWILIO_AUTH_TOKEN/TWILIO_SMS_FROM_NUMBER)');
    }

    const client = twilio(accountSid, authToken);
    await client.messages.create({ to: stripWhatsAppPrefix(toNumber), from, body });
  },
};

function stripWhatsAppPrefix(number: string): string {
  return number.startsWith('whatsapp:') ? number.slice('whatsapp:'.length) : number;
}

/**
 * Delivers an OTP code to the user's phone via SMS. The code must never be
 * echoed back in the WhatsApp chat reply (#942), so this is the only place
 * the plaintext code is transmitted to the user.
 */
export async function sendOtpSms(toNumber: string, code: string): Promise<void> {
  try {
    await smsClient.send(toNumber, `Your NeuroWealth verification code is ${code}. It expires in 5 minutes.`);
  } catch (error) {
    logger.error({ error: error instanceof Error ? error.message : error }, 'Failed to send OTP SMS');
    throw error;
  }
}
