import logger from './logger';
import { AlertEventPayload, parseAlertEventPayload } from './alertEventPayload';

export interface AlertRule {
    name: string;
    description: string;
    check: (event: AlertEventPayload, state: VaultAlertState) => boolean;
    severity: 'LOW' | 'MEDIUM' | 'HIGH' | 'CRITICAL';
}

interface VaultAlertState {
    totalAssets: number;
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

export async function processEventForAlerts(event: unknown): Promise<string[]> {
    const parsed = parseAlertEventPayload(event);
    if (!parsed.ok) {
        logger.warn({ reason: parsed.reason, event }, 'Dropping invalid or unknown alert event');
        return [];
    }

    // Mock fetching vault state
    const state: VaultAlertState = { totalAssets: 1000000 };
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
    console.log(`[ALERT] [${rule.severity}] ${rule.name}: ${rule.description}`);
    
    // Mock channel notifications
    await sendEmailAlert(rule, event);
    await sendTelegramAlert(rule, event);
    await sendDiscordAlert(rule, event);
    
    if (rule.severity === 'CRITICAL') {
        await sendPagerDutyAlert(rule, event);
    }
}

async function sendEmailAlert(_rule: AlertRule, _event: AlertEventPayload) { console.log('Email sent'); }
async function sendTelegramAlert(_rule: AlertRule, _event: AlertEventPayload) { console.log('Telegram message sent'); }
async function sendDiscordAlert(_rule: AlertRule, _event: AlertEventPayload) { console.log('Discord webhook sent'); }
async function sendPagerDutyAlert(_rule: AlertRule, _event: AlertEventPayload) { console.log('PagerDuty incident created'); }
