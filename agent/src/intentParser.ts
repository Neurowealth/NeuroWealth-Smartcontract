import OpenAI from 'openai';
import { openAiUsage, errorRate } from './metrics';

const openai = new OpenAI({
  apiKey: process.env.OPENAI_API_KEY,
});

export interface ParsedIntent {
  action: 'deposit' | 'withdraw' | 'balance' | 'earnings' | 'switch_strategy' | 'get_apy';
  amount?: number | 'all';
  strategy?: 'conservative' | 'balanced' | 'growth';
}

/**
 * Parses natural language user messages into structured vault operations.
 * Supported intents: deposit, withdraw, balance, earnings, switch_strategy, get_apy
 */
export async function parseIntent(message: string): Promise<ParsedIntent> {
  const completion = await openai.chat.completions.create({
    model: 'gpt-4-turbo',
    messages: [
      {
        role: 'system',
        content: `You are an intent parser for the NeuroWealth DeFi bot.
Parse the user's message into a JSON object with the following schema:
{
  "action": "deposit" | "withdraw" | "balance" | "earnings" | "switch_strategy" | "get_apy",
  "amount": number or the string "all" (optional, for deposit or withdraw),
  "strategy": "conservative" | "balanced" | "growth" (optional, for deposit or switch_strategy)
}
Only output valid JSON matching this exact schema. If the user wants to withdraw everything, set amount to "all".`
      },
      {
        role: 'user',
        content: message,
      },
    ],
    response_format: { type: 'json_object' },
  });

  openAiUsage.inc({ model: 'gpt-4-turbo', action: 'parseIntent' });

  const result = completion.choices[0].message.content;
  if (!result) {
    throw new Error("Failed to parse intent: Empty response from OpenAI");
  }

  try {
    const parsed = JSON.parse(result) as ParsedIntent;
    
    // Basic validations
    if (parsed.amount !== undefined && typeof parsed.amount === 'number' && parsed.amount <= 0) {
      throw new Error("Amount must be greater than 0");
    }

    if (parsed.action === 'deposit' && !parsed.amount) {
      throw new Error("Deposit action requires an amount");
    }

    if (parsed.action === 'withdraw' && !parsed.amount) {
      throw new Error("Withdraw action requires an amount or 'all'");
    }

    return parsed;
  } catch (error) {
    errorRate.inc({ type: 'intent_parse_error' });
    console.error("Error parsing intent:", error);
    throw error;
  }
}
