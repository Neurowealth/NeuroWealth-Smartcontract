import logger from './logger';
import { AlertEventPayload, parseAlertEventPayload } from './alertEventPayload';

export interface AlertRule {
    name: string;
    description: string;
    check: (event: AlertEventPayload, state: VaultAlertState) => boolean;
    severity: 'LOW' | 'MEDIUM' | 'HIGH' | 'CRITICAL';
}

export interface VaultAlertState {
    /** Vault total assets in whole USDC, or null when it could not be read from chain. */
    totalAssets: number | null;
}

/** Reads live vault state for alert evaluation. Injectable for tests. */
export type VaultAlertStateReader = () => Promise<VaultAlertState>;

async function readVaultAlertStateFromChain(): Promise<VaultAlertState> {
    // Loaded lazily: sorobanTx -> eventListener -> alertEngine would otherwise be a load-time cycle.
    const { fetchVaultTotalAssets } = await import('./sorobanTx');
    return { totalAssets: await fetchVaultTotalAssets() };
}

export interface MonitoringAlertThresholds {
    tvlDropPercentage: number;
    largeWithdrawalThreshold: number;
}

export function getAlertThresholds(env: Record<string, string | undefined> = process.env): MonitoringAlertThresholds {
    const rawTvl = env.ALERT_TVL_DROP_PERCENTAGE ?? env.TVL_DROP_PERCENTAGE;
    const rawLarge = env.ALERT_LARGE_WITHDRAWAL_THRESHOLD ?? env.LARGE_WITHDRAWAL_THRESHOLD;

    const parsedTvl = rawTvl !== undefined && rawTvl !== '' ? Number(rawTvl) : NaN;
    const parsedLarge = rawLarge !== undefined && rawLarge !== '' ? Number(rawLarge) : NaN;

    return {
        tvlDropPercentage: Number.isFinite(parsedTvl) && parsedTvl > 0 ? parsedTvl : 20,
        largeWithdrawalThreshold: Number.isFinite(parsedLarge) && parsedLarge > 0 ? parsedLarge : 100_000,
    };
}

export const alertRules: AlertRule[] = [
    {
        name: 'TVL_ANOMALY',
        description: 'Sudden drop in TVL detected.',
        check: (event, state) => {
            // Without real on-chain TVL the rule cannot be evaluated; never guess it.
            if (state.totalAssets === null || state.totalAssets <= 0) return false;
            const thresholds = getAlertThresholds();
            return event.type === 'withdraw' && event.amount > state.totalAssets * (thresholds.tvlDropPercentage / 100);
        },
        severity: 'CRITICAL',
    },
    {
        name: 'LARGE_WITHDRAWAL',
        description: 'Large withdrawal detected.',
        check: (event, _state) => {
            const thresholds = getAlertThresholds();
            return event.type === 'withdraw' && event.amount > thresholds.largeWithdrawalThreshold;
        },
        severity: 'HIGH',
    },
    {
        name: 'AUTH_FAILURE',
        description: 'Authentication failure on vault operations.',
        check: (event, state) => {
            return event.type === 'auth_failure';
        },
        severity: 'MEDIUM',
    },
];

export async function processEventForAlerts(
    event: unknown,
    readVaultState: VaultAlertStateReader = readVaultAlertStateFromChain,
): Promise<string[]> {
    const parsed = parseAlertEventPayload(event);
    if (!parsed.ok) {
        logger.warn({ reason: parsed.reason, event }, 'Dropping invalid or unknown alert event');
        return [];
    }

    let state: VaultAlertState;
    try {
        state = await readVaultState();
    } catch (error) {
        logger.warn({ error: error instanceof Error ? error.message : error }, 'Failed to read vault state for alerts');
        state = { totalAssets: null };
    }
    if (state.totalAssets === null) {
        logger.warn('Vault total assets unavailable; skipping TVL-relative alert rules');
    }
    const triggered: string[] = [];
    
    for (const rule of alertRules) {
        if (rule.check(parsed.payload, state)) {
            triggered.push(rule.name);
            await triggerAlert(rule, parsed.payload);
        }
    }

    return triggered;
}

async function triggerAlert(rule: AlertRule, event: AlertEventPayload) {
    logger.info({ severity: rule.severity, alert: rule.name }, 'Vault alert triggered');

    await Promise.all([
        sendEmailAlert(rule, event),
        sendTelegramAlert(rule, event),
        sendDiscordAlert(rule, event),
    ]);

    if (rule.severity === 'CRITICAL') {
        await sendPagerDutyAlert(rule, event);
    }
}

function alertMessage(rule: AlertRule, event: AlertEventPayload): string {
    return `[${rule.severity}] ${rule.name}: ${rule.description} (type=${event.type}, amount=${event.amount}, user=${event.user ?? 'unknown'}, ledger=${event.ledger})`;
}

async function sendEmailAlert(rule: AlertRule, event: AlertEventPayload) {
    const apiKey = process.env.SENDGRID_API_KEY;
    const to = process.env.ALERT_EMAIL_TO;
    const from = process.env.ALERT_EMAIL_FROM;
    if (!apiKey || !to || !from) {
        logger.debug('Email alert channel not configured; skipping');
        return;
    }

    try {
        const response = await fetch('https://api.sendgrid.com/v3/mail/send', {
            method: 'POST',
            headers: {
                Authorization: `Bearer ${apiKey}`,
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({
                personalizations: [{ to: [{ email: to }] }],
                from: { email: from },
                subject: `NeuroWealth alert: ${rule.name}`,
                content: [{ type: 'text/plain', value: alertMessage(rule, event) }],
            }),
        });
        if (!response.ok) {
            logger.error({ status: response.status }, 'Email alert send failed');
            return;
        }
        logger.info('Email alert sent');
    } catch (error) {
        logger.error({ error: error instanceof Error ? error.message : error }, 'Email alert send failed');
    }
}

async function sendTelegramAlert(rule: AlertRule, event: AlertEventPayload) {
    const botToken = process.env.TELEGRAM_BOT_TOKEN;
    const chatId = process.env.TELEGRAM_CHAT_ID;
    if (!botToken || !chatId) {
        logger.debug('Telegram alert channel not configured; skipping');
        return;
    }

    try {
        const response = await fetch(`https://api.telegram.org/bot${botToken}/sendMessage`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ chat_id: chatId, text: alertMessage(rule, event) }),
        });
        if (!response.ok) {
            logger.error({ status: response.status }, 'Telegram alert send failed');
            return;
        }
        logger.info('Telegram alert sent');
    } catch (error) {
        logger.error({ error: error instanceof Error ? error.message : error }, 'Telegram alert send failed');
    }
}

async function sendDiscordAlert(rule: AlertRule, event: AlertEventPayload) {
    const webhookUrl = process.env.DISCORD_WEBHOOK_URL;
    if (!webhookUrl) {
        logger.debug('Discord alert channel not configured; skipping');
        return;
    }

    try {
        const response = await fetch(webhookUrl, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ content: alertMessage(rule, event) }),
        });
        if (!response.ok) {
            logger.error({ status: response.status }, 'Discord alert send failed');
            return;
        }
        logger.info('Discord alert sent');
    } catch (error) {
        logger.error({ error: error instanceof Error ? error.message : error }, 'Discord alert send failed');
    }
}

async function sendPagerDutyAlert(rule: AlertRule, event: AlertEventPayload) {
    const routingKey = process.env.PAGERDUTY_ROUTING_KEY;
    if (!routingKey) {
        logger.debug('PagerDuty alert channel not configured; skipping');
        return;
    }

    try {
        const response = await fetch('https://events.pagerduty.com/v2/enqueue', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                routing_key: routingKey,
                event_action: 'trigger',
                payload: {
                    summary: alertMessage(rule, event),
                    source: 'neurowealth-agent',
                    severity: rule.severity.toLowerCase(),
                },
            }),
        });
        if (!response.ok) {
            logger.error({ status: response.status }, 'PagerDuty incident creation failed');
            return;
        }
        logger.info('PagerDuty incident created');
    } catch (error) {
        logger.error({ error: error instanceof Error ? error.message : error }, 'PagerDuty incident creation failed');
    }
}
