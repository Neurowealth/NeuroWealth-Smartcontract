import { Request, Response } from 'express';
import twilio from 'twilio';
import pino from 'pino';
import { hashPhoneNumber } from './cryptoUtils';
import { runExclusive } from './commandLock';
import {
  deriveIdempotencyKey,
  getIdempotencyStore,
  IdempotencyStore,
  isTransientError,
  LeaseLostError,
  RetryableWebhookError,
  SideEffectGuard,
  WEBHOOK_LEASE_MS,
} from './webhookIdempotency';

const logger = pino({ name: 'whatsapp-webhook' });

import { getSession, updateState, checkRateLimit, UserState } from './stateManager';
import { generateOTP, verifyOTP } from './otpService';
import { sendOtpSms } from './smsService';
import { createCustodialWallet, getWallet } from './walletService';
import { parseIntent } from './intentParser';
import { validateIntent } from './contractLimits';
import { getPortfolio, handleDeposit, handleWithdraw, handleStrategyUpdate } from './vaultRouter';
import {
  INPUT_LIMITS,
  SAFE_MALFORMED_REPLY,
  SAFE_OVERSIZE_REPLY,
  SAFE_STRATEGY_REPLY,
  boundOutbound,
  boundText,
} from './inputBounds';

function reply(twiml: { message: (body: string) => unknown }, text: string): void {
  twiml.message(boundOutbound(text));
}

function rejectInbound(res: Response, reason: 'oversize' | 'malformed' | 'type'): void {
  const twiml = new MessagingResponse();
  reply(twiml, reason === 'oversize' ? SAFE_OVERSIZE_REPLY : SAFE_MALFORMED_REPLY);
  res.type('text/xml').send(twiml.toString());
}

const MessagingResponse = twilio.twiml.MessagingResponse;

/** Acknowledges a webhook without sending any reply message to the user. */
const EMPTY_TWIML = '<?xml version="1.0" encoding="UTF-8"?><Response></Response>';

/**
 * Main Webhook Handler for WhatsApp Messages (Twilio HTTP POST)
 */
export async function handleWhatsAppWebhook(req: Request, res: Response): Promise<void> {
  const fromNumber = req.body?.From;
  const messageBody = req.body?.Body;

  if (fromNumber === undefined || fromNumber === null || fromNumber === '') {
    res.status(400).send('Missing sender phone number');
    return;
  }

  const fromCheck = boundText(fromNumber, INPUT_LIMITS.address);
  if (!fromCheck.ok) {
    rejectInbound(res, fromCheck.reason);
    return;
  }

  const bodyCheck = boundText(messageBody ?? '', INPUT_LIMITS.inboundBody);
  if (!bodyCheck.ok) {
    rejectInbound(res, bodyCheck.reason);
    return;
  }

  const commandCheck = boundText(bodyCheck.value.trim(), INPUT_LIMITS.commandText);
  if (!commandCheck.ok) {
    rejectInbound(res, commandCheck.reason);
    return;
  }

  // Hash PII (phone number) at rest
  const phoneHash = hashPhoneNumber(fromCheck.value);

  // Every command below reads and mutates this user's shared session, OTP,
  // strategy, and transaction state. Serialize per phone number so
  // concurrent messages from the same user apply in arrival order instead
  // of racing; messages from different users still process in parallel.
  // The idempotency claim happens inside the lock, so an in-process
  // duplicate observes the first delivery's final state.
  const idempotencyKey = deriveIdempotencyKey(req.body, req.headers?.['i-twilio-idempotency-token']);
  const messageSid = typeof req.body?.MessageSid === 'string' && req.body.MessageSid !== '' ? req.body.MessageSid : null;
  // The raw number is only used in-flight to deliver the OTP via SMS (#942);
  // it is never persisted, only the hash is.
  await runExclusive(phoneHash, () =>
    processIdempotently(idempotencyKey, messageSid, phoneHash, fromCheck.value, commandCheck.value, res)
  );
}

/**
 * Runs a webhook delivery at most once per idempotency key. Twilio may
 * redeliver the same message after a timeout; duplicates of a finished
 * delivery are acknowledged without re-running the command or sending a
 * second reply, and a retryable failure leaves the key open for the next
 * delivery to resume.
 */
async function processIdempotently(
  idempotencyKey: string | null,
  messageSid: string | null,
  phoneHash: string,
  rawPhoneNumber: string,
  messageBody: string,
  res: Response
): Promise<void> {
  if (!idempotencyKey) {
    logger.warn({ phoneHash }, 'Inbound webhook has no MessageSid or idempotency token; processing without deduplication');
    await respondWithCommand(null, null, 0, phoneHash, rawPhoneNumber, messageBody, res);
    return;
  }

  const store = getIdempotencyStore();
  let claim;
  try {
    claim = await store.claim(idempotencyKey, { messageSid, phoneHash }, WEBHOOK_LEASE_MS);
  } catch (error) {
    // Nothing has run yet, so asking Twilio to retry is always safe.
    logger.error({ error, phoneHash, idempotencyKey }, 'Failed to claim inbound webhook idempotency key');
    res.status(503).send('Temporarily unable to process message');
    return;
  }

  switch (claim.kind) {
    case 'completed':
      logger.info({ phoneHash, idempotencyKey }, 'Duplicate inbound webhook after completion; acknowledging without reply');
      res.status(claim.responseStatus).type('text/xml').send(EMPTY_TWIML);
      return;
    case 'abandoned':
      logger.error(
        { phoneHash, idempotencyKey },
        'Duplicate inbound webhook for an abandoned attempt with unknown outcome; not re-running command'
      );
      res.status(200).type('text/xml').send(EMPTY_TWIML);
      return;
    case 'in_progress':
      // Another worker holds the lease. Signal a retryable conflict so a
      // later redelivery can either see the result or resume a failure.
      logger.info({ phoneHash, idempotencyKey }, 'Duplicate inbound webhook while original is still processing');
      res.status(409).send('Message is already being processed');
      return;
    case 'acquired':
      await respondWithCommand(idempotencyKey, store, claim.attempt, phoneHash, rawPhoneNumber, messageBody, res);
      return;
  }
}

async function respondWithCommand(
  idempotencyKey: string | null,
  store: IdempotencyStore | null,
  attempt: number,
  phoneHash: string,
  rawPhoneNumber: string,
  messageBody: string,
  res: Response
): Promise<void> {
  const guard = new SideEffectGuard(store, idempotencyKey, attempt);

  let replyXml: string;
  try {
    replyXml = await processWhatsAppMessage(phoneHash, rawPhoneNumber, messageBody, guard);
  } catch (error) {
    if (error instanceof LeaseLostError) {
      logger.warn({ phoneHash, idempotencyKey, attempt }, 'Lost idempotency lease before side effects; aborting attempt');
      res.status(409).send('Message is already being processed');
      return;
    }

    // Only retryable failures escape processWhatsAppMessage: anything
    // irreversible would already have been converted to a reply.
    logger.warn({ error, phoneHash, idempotencyKey, attempt }, 'Retryable failure processing inbound webhook');
    if (store && idempotencyKey) {
      await store
        .fail(idempotencyKey, attempt, error instanceof Error ? error.message : String(error))
        .catch((storeError) =>
          logger.error({ error: storeError, idempotencyKey }, 'Failed to record retryable webhook failure')
        );
    }
    res.status(503).send('Temporarily unable to process message');
    return;
  }

  if (store && idempotencyKey) {
    // If this write fails the record stays `processing`; once its lease
    // expires a duplicate is either re-run (no side effects started, e.g. a
    // balance read) or marked abandoned - never blindly replayed.
    await store
      .complete(idempotencyKey, attempt, 200)
      .catch((storeError) =>
        logger.error({ error: storeError, idempotencyKey }, 'Failed to record completed webhook processing')
      );
  }
  res.status(200).type('text/xml').send(replyXml);
}

/**
 * Executes one inbound command and returns the TwiML reply. Callers must
 * invoke `guard.begin()` before any state-changing step: a transient error
 * raised before that point is rethrown as RetryableWebhookError so the
 * delivery can be retried, while any error after it becomes a terminal
 * user-facing reply, because the command may already have partially applied.
 */
async function processWhatsAppMessage(
  phoneHash: string,
  rawPhoneNumber: string,
  messageBody: string,
  guard: SideEffectGuard
): Promise<string> {
  const twiml = new MessagingResponse();

  // Enforce per-phone rate limiting
  if (!(await checkRateLimit(phoneHash))) {
    reply(twiml, '⚠️ Rate limit exceeded. Please wait a minute before sending another message.');
    return twiml.toString();
  }

  // Get or restore user session
  const session = await getSession(phoneHash);
  const intent = parseIntent(messageBody);

  if (intent.strategy) {
    const strategyCheck = boundText(intent.strategy, INPUT_LIMITS.strategy);
    if (!strategyCheck.ok) {
      reply(twiml, SAFE_STRATEGY_REPLY);
      return twiml.toString();
    }
  }

  try {
    // STATE MACHINE FLOW:
    // Flow 1: UNVERIFIED User sends "hi" -> Request OTP verification
    if (session.state === UserState.UNVERIFIED) {
      if (intent.type === 'GREETING' || intent.type === 'UNKNOWN') {
        await guard.begin();
        const otpCode = await generateOTP(phoneHash);

        try {
          // The OTP is delivered via SMS only; it must never appear in the
          // WhatsApp chat reply (#942).
          await sendOtpSms(rawPhoneNumber, otpCode);
          await updateState(phoneHash, UserState.AWAITING_OTP);
          reply(twiml,
            `👋 Welcome to NeuroWealth AI!\n\nTo secure your wallet, we sent a 6-digit verification code to your phone via SMS. Please reply with that code.\n(Code expires in 5 minutes)`
          );
        } catch (error) {
          logger.error({ error: error instanceof Error ? error.message : error }, 'Failed to send OTP SMS');
          reply(twiml, '❌ We could not send your verification code. Please try again in a few minutes.');
        }
      } else {
        reply(twiml, 'Welcome to NeuroWealth! Send "hi" to begin account verification.');
      }
      return twiml.toString();
    }

    // Flow 2: AWAITING_OTP -> Verify OTP input code or allow "hi" to request new code
    if (session.state === UserState.AWAITING_OTP) {
      // Allow "hi" to request a new OTP code (helps users recover from lockout/expiry)
      if (intent.type === 'GREETING' || intent.type === 'UNKNOWN') {
        await guard.begin();
        const otpCode = await generateOTP(phoneHash);

        try {
          await sendOtpSms(rawPhoneNumber, otpCode);
          reply(twiml,
            `🔑 A new verification code was sent to your phone via SMS.\n\n(Code expires in 5 minutes)`
          );
        } catch (error) {
          logger.error({ error: error instanceof Error ? error.message : error }, 'Failed to send OTP SMS');
          reply(twiml, '❌ We could not send your verification code. Please try again in a few minutes.');
        }
        return twiml.toString();
      }

      if (intent.type === 'OTP_CODE' && intent.otpCode) {
        // verifyOTP consumes attempts / the code itself.
        await guard.begin();
        const verification = await verifyOTP(phoneHash, intent.otpCode);

        if (verification.success) {
          await updateState(phoneHash, UserState.VERIFIED);
          const { wallet, isNew } = await createCustodialWallet(phoneHash);
          const shortKey = `${wallet.publicKey.substring(0, 8)}...${wallet.publicKey.substring(wallet.publicKey.length - 8)}`;
          const walletLine = isNew
            ? `🔒 Created your secure custodial Stellar wallet:\nPublic Key: ${shortKey}`
            : `👋 Welcome back! Your existing wallet:\nPublic Key: ${shortKey}`;

          reply(twiml,
            `✅ Phone number verified!\n\n` +
            `${walletLine}\n\n` +
            `You can now interact with NeuroWealth entirely through WhatsApp:\n` +
            `• "deposit 100 USDC"\n` +
            `• "what's my balance"\n` +
            `• "withdraw 50"\n` +
            `• "switch to growth"`
          );
        } else {
          // Reset session to UNVERIFIED on lockout or expiry to unblock the user
          if (verification.shouldResetSession) {
            await updateState(phoneHash, UserState.UNVERIFIED);
          }
          reply(twiml, `❌ ${verification.message}`);
        }
      } else {
        reply(twiml, 'Please enter the 6-digit verification code sent to your phone (valid for 5 minutes), or send "hi" to request a new code.');
      }
      return twiml.toString();
    }

    // Flow 3: VERIFIED User -> Handle Chat Intents
    if (session.state === UserState.VERIFIED) {
      const wallet = await getWallet(phoneHash);
      if (!wallet) {
        // Fallback state sync
        await guard.begin();
        await updateState(phoneHash, UserState.UNVERIFIED);
        reply(twiml, 'Session expired. Please send "hi" to start verification.');
        return twiml.toString();
      }

      // Reject amounts and strategy names the vault contract would refuse
      // before building any transaction.
      const validation = validateIntent(intent);
      if (!validation.ok) {
        reply(twiml, `⚠️ ${validation.error}`);
        return twiml.toString();
      }

      switch (intent.type) {
        case 'GREETING': {
          reply(twiml, 
            `🤖 Hi! I'm your NeuroWealth AI Agent.\n\n` +
            `How can I assist your portfolio today?\n` +
            `1. "balance" - View current portfolio\n` +
            `2. "deposit 100 USDC" - Deposit funds\n` +
            `3. "withdraw 50" - Cash out\n` +
            `4. "switch to growth" - Update strategy`
          );
          break;
        }

        case 'BALANCE':
        case 'EARNINGS':
        case 'APY': {
          const portfolio = await getPortfolio(phoneHash);
          reply(twiml, 
            `💰 Your NeuroWealth Portfolio\n\n` +
            `Balance: ${portfolio.balance.toFixed(2)} USDC ($${portfolio.usdEquivalent.toFixed(2)})\n` +
            `Earnings today: +$${portfolio.dailyEarnings.toFixed(2)}\n` +
            `Current APY: ${portfolio.apy}%\n` +
            `Strategy: ${portfolio.strategy}`
          );
          break;
        }

        case 'DEPOSIT': {
          await guard.begin();
          const result = await handleDeposit(phoneHash, intent.amount as number, intent.strategy);
          reply(twiml, `🤖 ${result.message}`);
          break;
        }

        case 'WITHDRAW': {
          await guard.begin();
          const result = await handleWithdraw(phoneHash, intent.amount, intent.withdrawAll);
          reply(twiml, `🤖 ${result.message}`);
          break;
        }

        case 'STRATEGY': {
          if (!intent.strategy) {
            reply(twiml, '❌ Please specify a valid strategy.');
            break;
          }
          await guard.begin();
          const result = await handleStrategyUpdate(phoneHash, intent.strategy);
          reply(twiml, result.success ? `✅ ${result.message}` : `❌ ${result.message}`);
          break;
        }

        default: {
          reply(twiml, 
            `I didn't quite catch that. Try commands like:\n` +
            `• "deposit 50 USDC"\n` +
            `• "what's my balance"\n` +
            `• "withdraw all"\n` +
            `• "switch to conservative"`
          );
          break;
        }
      }
    }
  } catch (error) {
    if (error instanceof LeaseLostError) throw error;
    if (!guard.hasStarted && isTransientError(error)) {
      throw error instanceof RetryableWebhookError
        ? error
        : new RetryableWebhookError('Transient failure before any side effect', { cause: error });
    }

    const reqId = `req-${Math.random().toString(36).substring(2, 9)}`;
    logger.error(
      { error, phoneHash, intent, state: session.state, reqId },
      'Error processing webhook request'
    );
    reply(twiml, `❌ An error occurred processing your request. Please try again in a few moments. (Ref: ${reqId})`);
  }

  return twiml.toString();
}
