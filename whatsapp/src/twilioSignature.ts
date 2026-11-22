import { NextFunction, Request, RequestHandler, Response } from 'express';
import twilio from 'twilio';

export interface TwilioSignatureOptions {
  /** Twilio account auth token used to compute the HMAC. */
  authToken?: string;
  /**
   * Public URL Twilio is configured to call. Set this when the bot runs behind
   * a proxy / load balancer / tunnel, where the URL seen by Express differs
   * from the one Twilio signed.
   */
  webhookUrl?: string;
  /** Explicit opt-out for local development only; ignored in production. */
  skipValidation?: boolean;
  nodeEnv?: string;
}

function optionsFromEnv(): TwilioSignatureOptions {
  return {
    authToken: process.env.TWILIO_AUTH_TOKEN,
    webhookUrl: process.env.TWILIO_WEBHOOK_URL,
    skipValidation: process.env.TWILIO_SKIP_SIGNATURE_VALIDATION === 'true',
    nodeEnv: process.env.NODE_ENV,
  };
}

/** Reconstructs the full URL Twilio signed when no explicit URL is configured. */
function requestUrl(req: Request): string {
  return `${req.protocol}://${req.get('host')}${req.originalUrl}`;
}

/**
 * Rejects any request whose `X-Twilio-Signature` header is missing or does not
 * match the HMAC-SHA1 of the URL + POST params signed with TWILIO_AUTH_TOKEN.
 * Fails closed: without an auth token every request is refused, unless
 * validation is explicitly skipped outside production.
 */
export function verifyTwilioSignature(
  options: TwilioSignatureOptions = optionsFromEnv(),
): RequestHandler {
  const { authToken, webhookUrl, skipValidation, nodeEnv } = options;
  const bypass = skipValidation === true && nodeEnv !== 'production';

  if (bypass) {
    console.warn('⚠️ Twilio signature validation is DISABLED (TWILIO_SKIP_SIGNATURE_VALIDATION=true). Never use this in production.');
  } else if (!authToken) {
    console.error('TWILIO_AUTH_TOKEN is not set; all WhatsApp webhook requests will be rejected.');
  }

  return (req: Request, res: Response, next: NextFunction): void => {
    if (bypass) {
      next();
      return;
    }

    if (!authToken) {
      res.status(500).send('Webhook signature validation is not configured');
      return;
    }

    const isHttps = req.secure || req.headers['x-forwarded-proto'] === 'https' || req.protocol === 'https';
    if (nodeEnv === 'production' && !isHttps) {
      res.status(403).send('HTTPS required in production');
      return;
    }

    const signature = req.get('X-Twilio-Signature');
    if (!signature) {
      res.status(403).send('Missing Twilio signature');
      return;
    }

    const url = webhookUrl || requestUrl(req);
    const params = req.body && typeof req.body === 'object' ? req.body : {};

    if (!twilio.validateRequest(authToken, signature, url, params)) {
      res.status(403).send('Invalid Twilio signature');
      return;
    }

    next();
  };
}
