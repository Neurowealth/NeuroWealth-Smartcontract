import { Request, Response } from 'express';
import twilio from 'twilio';
import pino from 'pino';
import { hashPhoneNumber } from './cryptoUtils';
import { runExclusive } from './commandLock';

const logger = pino({ name: 'whatsapp-webhook' });

import { getSession, updateState, checkRateLimit, UserState } from './stateManager';
import { generateOTP, verifyOTP } from './otpService';
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
  await runExclusive(phoneHash, () => processWhatsAppMessage(phoneHash, commandCheck.value, res));
}

async function processWhatsAppMessage(phoneHash: string, messageBody: string, res: Response): Promise<void> {
  const twiml = new MessagingResponse();

  // Enforce per-phone rate limiting
  if (!checkRateLimit(phoneHash)) {
    reply(twiml, '⚠️ Rate limit exceeded. Please wait a minute before sending another message.');
    res.type('text/xml').send(twiml.toString());
    return;
  }

  // Get or restore user session
  const session = getSession(phoneHash);
  const intent = parseIntent(messageBody);

  if (intent.strategy) {
    const strategyCheck = boundText(intent.strategy, INPUT_LIMITS.strategy);
    if (!strategyCheck.ok) {
      reply(twiml, SAFE_STRATEGY_REPLY);
      res.type('text/xml').send(twiml.toString());
      return;
    }
  }

  try {
    // STATE MACHINE FLOW:
    // Flow 1: UNVERIFIED User sends "hi" -> Request OTP verification
    if (session.state === UserState.UNVERIFIED) {
      if (intent.type === 'GREETING' || intent.type === 'UNKNOWN') {
        const otpCode = generateOTP(phoneHash);
        updateState(phoneHash, UserState.AWAITING_OTP);

        // In production, Twilio SMS/WhatsApp sends this OTP code.
        // For testing/mocking, we include instructions in response.
        reply(twiml, 
          `👋 Welcome to NeuroWealth AI!\n\nTo secure your wallet, please enter your 6-digit OTP verification code.\n\n🔑 Your OTP code is: ${otpCode}\n(Code expires in 5 minutes)`
        );
      } else {
        reply(twiml, 'Welcome to NeuroWealth! Send "hi" to begin account verification.');
      }
      res.type('text/xml').send(twiml.toString());
      return;
    }

    // Flow 2: AWAITING_OTP -> Verify OTP input code
    if (session.state === UserState.AWAITING_OTP) {
      if (intent.type === 'OTP_CODE' && intent.otpCode) {
        const verification = verifyOTP(phoneHash, intent.otpCode);

        if (verification.success) {
          updateState(phoneHash, UserState.VERIFIED);
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
          reply(twiml, `❌ ${verification.message}`);
        }
      } else {
        reply(twiml, 'Please enter the 6-digit verification code sent to your phone (valid for 5 minutes).');
      }
      res.type('text/xml').send(twiml.toString());
      return;
    }

    // Flow 3: VERIFIED User -> Handle Chat Intents
    if (session.state === UserState.VERIFIED) {
      const wallet = await getWallet(phoneHash);
      if (!wallet) {
        // Fallback state sync
        updateState(phoneHash, UserState.UNVERIFIED);
        reply(twiml, 'Session expired. Please send "hi" to start verification.');
        res.type('text/xml').send(twiml.toString());
        return;
      }

      // Reject amounts and strategy names the vault contract would refuse
      // before building any transaction.
      const validation = validateIntent(intent);
      if (!validation.ok) {
        reply(twiml, `⚠️ ${validation.error}`);
        res.type('text/xml').send(twiml.toString());
        return;
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
          const result = await handleDeposit(phoneHash, intent.amount as number, intent.strategy);
          reply(twiml, `🤖 ${result.message}`);
          break;
        }

        case 'WITHDRAW': {
          const result = await handleWithdraw(phoneHash, intent.amount, intent.withdrawAll);
          reply(twiml, `🤖 ${result.message}`);
          break;
        }

        case 'STRATEGY': {
          if (!intent.strategy) {
            reply(twiml, '❌ Please specify a valid strategy.');
            break;
          }
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
    const reqId = `req-${Math.random().toString(36).substring(2, 9)}`;
    logger.error(
      { error, phoneHash, intent, state: session.state, reqId },
      'Error processing webhook request'
    );
    reply(twiml, `❌ An error occurred processing your request. Please try again in a few moments. (Ref: ${reqId})`);
  }

  res.type('text/xml').send(twiml.toString());
}
