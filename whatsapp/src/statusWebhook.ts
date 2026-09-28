import { Request, Response } from 'express';
import pino from 'pino';
import { applyDeliveryStatus } from './messageDeliveryService';

const logger = pino({ name: 'whatsapp-status-webhook' });

/**
 * Twilio calls this URL asynchronously as an outbound WhatsApp message moves
 * through queued -> sent -> delivered (or failed / undelivered). We always
 * acknowledge with 200 so Twilio does not retry-storm us - even a callback
 * for an identifier we can't reconcile is logged and dropped, not failed.
 */
export async function handleDeliveryStatusCallback(req: Request, res: Response): Promise<void> {
  const body = req.body ?? {};
  const messageSid = body.MessageSid || body.SmsSid || '';
  const status = typeof body.MessageStatus === 'string' ? body.MessageStatus.toLowerCase() : '';

  try {
    await applyDeliveryStatus({
      messageSid,
      status,
      to: body.To,
      errorCode: body.ErrorCode,
      errorMessage: body.ErrorMessage,
    });
  } catch (error) {
    logger.error({ error, messageSid }, 'Failed to persist delivery status callback');
  }

  res.status(200).send();
}
