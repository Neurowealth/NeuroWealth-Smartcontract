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

const MessagingResponse = twilio.twiml.MessagingResponse;

/**
 * Main Webhook Handler for WhatsApp Messages (Twilio HTTP POST)
 */
export async function handleWhatsAppWebhook(req: Request, res: Response): Promise<void> {
  const fromNumber = req.body.From || ''; // E.164 format: whatsapp:+1234567890
  const messageBody = req.body.Body || '';

  if (!fromNumber) {
    res.status(400).send('Missing sender phone number');
    return;
  }

  // Hash PII (phone number) at rest
  const phoneHash = hashPhoneNumber(fromNumber);

  // Every command below reads and mutates this user's shared session, OTP,
  // strategy, and transaction state. Serialize per phone number so
  // concurrent messages from the same user apply in arrival order instead
  // of racing; messages from different users still process in parallel.
  await runExclusive(phoneHash, () => processWhatsAppMessage(phoneHash, messageBody, res));
}

async function processWhatsAppMessage(phoneHash: string, messageBody: string, res: Response): Promise<void> {
  const twiml = new MessagingResponse();

  // Enforce per-phone rate limiting
  if (!checkRateLimit(phoneHash)) {
    twiml.message('⚠️ Rate limit exceeded. Please wait a minute before sending another message.');
    res.type('text/xml').send(twiml.toString());
    return;
  }

  // Get or restore user session
  const session = getSession(phoneHash);
  const intent = parseIntent(messageBody);

  try {
    // STATE MACHINE FLOW:
    // Flow 1: UNVERIFIED User sends "hi" -> Request OTP verification
    if (session.state === UserState.UNVERIFIED) {
      if (intent.type === 'GREETING' || intent.type === 'UNKNOWN') {
        const otpCode = generateOTP(phoneHash);
        updateState(phoneHash, UserState.AWAITING_OTP);

        // In production, Twilio SMS/WhatsApp sends this OTP code.
        // For testing/mocking, we include instructions in response.
        twiml.message(
          `👋 Welcome to NeuroWealth AI!\n\nTo secure your wallet, please enter your 6-digit OTP verification code.\n\n🔑 Your OTP code is: ${otpCode}\n(Code expires in 5 minutes)`
        );
      } else {
        twiml.message('Welcome to NeuroWealth! Send "hi" to begin account verification.');
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
          const wallet = await createCustodialWallet(phoneHash);

          twiml.message(
            `✅ Phone number verified!\n\n` +
            `🔒 Created your secure custodial Stellar wallet:\n` +
            `Public Key: ${wallet.publicKey.substring(0, 8)}...${wallet.publicKey.substring(wallet.publicKey.length - 8)}\n\n` +
            `You can now interact with NeuroWealth entirely through WhatsApp:\n` +
            `• "deposit 100 USDC"\n` +
            `• "what's my balance"\n` +
            `• "withdraw 50"\n` +
            `• "switch to growth"`
          );
        } else {
          twiml.message(`❌ ${verification.message}`);
        }
      } else {
        twiml.message('Please enter the 6-digit verification code sent to your phone (valid for 5 minutes).');
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
        twiml.message('Session expired. Please send "hi" to start verification.');
        res.type('text/xml').send(twiml.toString());
        return;
      }

      // Reject amounts and strategy names the vault contract would refuse
      // before building any transaction.
      const validation = validateIntent(intent);
      if (!validation.ok) {
        twiml.message(`⚠️ ${validation.error}`);
        res.type('text/xml').send(twiml.toString());
        return;
      }

      switch (intent.type) {
        case 'GREETING': {
          twiml.message(
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
          twiml.message(
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
          twiml.message(`🤖 ${result.message}`);
          break;
        }

        case 'WITHDRAW': {
          const result = await handleWithdraw(phoneHash, intent.amount, intent.withdrawAll);
          twiml.message(`🤖 ${result.message}`);
          break;
        }

        case 'STRATEGY': {
          if (!intent.strategy) {
            twiml.message('❌ Please specify a valid strategy.');
            break;
          }
          const result = await handleStrategyUpdate(phoneHash, intent.strategy);
          twiml.message(result.success ? `✅ ${result.message}` : `❌ ${result.message}`);
          break;
        }

        default: {
          twiml.message(
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
    twiml.message(`❌ An error occurred processing your request. Please try again in a few moments. (Ref: ${reqId})`);
  }

  res.type('text/xml').send(twiml.toString());
}
