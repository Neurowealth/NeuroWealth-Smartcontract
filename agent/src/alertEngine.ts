/** Shape of the event the listener forwards to the rules. */
export interface VaultEvent {
    type: string;
    amount?: number;
}

/** Vault state a rule may consult when deciding. */
export interface VaultState {
    totalAssets: number;
}

export interface AlertRule {
    name: string;
    description: string;
    check: (event: VaultEvent, state: VaultState) => boolean;
    severity: 'LOW' | 'MEDIUM' | 'HIGH' | 'CRITICAL';
}

export const alertRules: AlertRule[] = [
    {
        name: 'TVL_ANOMALY',
        description: 'Sudden drop in TVL detected.',
        check: (event, state) => {
            return event.type === 'withdraw' && event.amount > state.totalAssets * 0.2;
        },
        severity: 'CRITICAL',
    },
    {
        name: 'LARGE_WITHDRAWAL',
        description: 'Large withdrawal detected.',
        check: (event, _state) => {
            return event.type === 'withdraw' && event.amount > 100000;
        },
        severity: 'HIGH',
    },
    {
        name: 'AUTH_FAILURE',
        description: 'Authentication failure on vault operations.',
        check: (event, _state) => {
            return event.type === 'auth_failure';
        },
        severity: 'MEDIUM',
    },
];

export async function processEventForAlerts(event: VaultEvent) {
    // Mock fetching vault state
    const state = { totalAssets: 1000000 }; 
    
    for (const rule of alertRules) {
        if (rule.check(event, state)) {
            await triggerAlert(rule, event);
        }
    }
}

async function triggerAlert(rule: AlertRule, event: VaultEvent) {
    console.log(`[ALERT] [${rule.severity}] ${rule.name}: ${rule.description}`);
    
    // Mock channel notifications
    await sendEmailAlert(rule, event);
    await sendTelegramAlert(rule, event);
    await sendDiscordAlert(rule, event);
    
    if (rule.severity === 'CRITICAL') {
        await sendPagerDutyAlert(rule, event);
    }
}

async function sendEmailAlert(_rule: AlertRule, _event: VaultEvent) { console.log('Email sent'); }
async function sendTelegramAlert(_rule: AlertRule, _event: VaultEvent) { console.log('Telegram message sent'); }
async function sendDiscordAlert(_rule: AlertRule, _event: VaultEvent) { console.log('Discord webhook sent'); }
async function sendPagerDutyAlert(_rule: AlertRule, _event: VaultEvent) { console.log('PagerDuty incident created'); }
