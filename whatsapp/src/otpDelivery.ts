import twilio from 'twilio';

export async function deliverOTP(phoneNumber: string, code: string): Promise<void> {
  const accountSid = process.env.TWILIO_ACCOUNT_SID;
  const authToken = process.env.TWILIO_AUTH_TOKEN;
  const fromNumber = process.env.TWILIO_SMS_FROM;

  if (!accountSid || !authToken || !fromNumber) {
    throw new Error('OTP SMS delivery is not configured');
  }

  const client = twilio(accountSid, authToken);
  await client.messages.create({
    body: `Your NeuroWealth verification code is ${code}. It expires in 5 minutes.`,
    from: fromNumber,
    to: phoneNumber,
  });
}