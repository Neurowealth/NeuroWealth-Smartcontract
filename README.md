NeuroWealth 💰

AI-Powered DeFi Yield Platform on Stellar

NeuroWealth is an autonomous AI investment agent that automatically manages and grows your crypto assets on the Stellar blockchain. Deposit once, let the AI find the best yield opportunities across Stellar's DeFi ecosystem — and withdraw anytime with no lock-ups.

## Overview
Traditional savings accounts offer near-zero interest. Traditional DeFi is too complex for most users. NeuroWealth bridges the gap with a simple chat interface on the web, powered by an AI agent that autonomously deploys your funds into the highest-yielding, safest opportunities on Stellar.

## Why Stellar?

- Transaction fees of fractions of a penny — perfect for frequent AI-driven rebalancing
- 3–5 second finality — the AI can act on market changes instantly
- Native DEX + Soroban smart contracts — composable, programmable yield strategies
- Native USDC + XLM — borderless capital movement with no friction
- Growing DeFi ecosystem — Blend (lending), Templar (borrowing), RWA protocols

## Features
| Feature | Description |
|---------|------------|
| 🤖 AI Agent | Autonomous 24/7 yield optimization across Stellar DeFi |
| 💬 Natural Language | Chat to deposit, withdraw, and check balances |
| 📈 Auto-Rebalancing | Agent shifts funds to best opportunities automatically |
| 🔐 Non-Custodial | Your funds live in audited Soroban smart contracts |
| ⚡ Instant Withdrawals | No lock-ups, no penalties, withdraw anytime |
| 📱 WhatsApp Ready | Full functionality through WhatsApp chat interface |
| 🌍 Global Access | No geographic restrictions, no bank account required |
| 🛡️ Security First | Soroban contracts protected by strict CEI ordering and access controls |

## How It Works
1. User deposits USDC via web app
2. Soroban vault contract receives and records the deposit
3. Contract emits a deposit event
4. AI agent detects the event and deploys funds to best protocol (e.g. Blend)
5. Yield accumulates 24/7 — agent rebalances hourly if better opportunities exist
6. User requests withdrawal anytime — agent pulls funds and sends back in seconds

## Three Investment Strategies

Conservative — Stablecoin lending on Blend. Low risk, steady 3–6% APY.
Balanced — Mix of lending + DEX liquidity provision. Medium risk, 6–10% APY.
Growth — Aggressive multi-protocol deployment. Higher risk, 10–15% APY.

> **Note:** A user's strategy preference (`set_user_strategy` / `get_user_strategy`) is
> **storage-only** — it has no on-chain effect on `rebalance()` or `deposit()` targeting.
> The vault pools all funds to a single `CurrentProtocol`. The off-chain AI agent reads
> the preference and uses it when deciding yield allocation. A user's chosen strategy can
> therefore diverge from where their share of the pooled funds is actually deployed.


## Tech Stack

### Smart Contracts

Language: Rust (Soroban SDK 21.0.0)
Standard: ERC-4626 inspired vault architecture
Network: Stellar Mainnet / Testnet
Security: OpenZeppelin-equivalent patterns (Pausable, Access Control) and strict CEI pattern for reentrancy protection

### Backend / AI Agent

Runtime: Node.js or Python
Stellar SDK: @stellar/stellar-sdk
AI: Claude API / OpenAI for natural language intent parsing
Database: PostgreSQL / Supabase for user position tracking
Queue: Bull / Redis for reliable transaction processing

### Frontend

Framework: Next.js 15
Blockchain: Stellar SDK + Freighter wallet integration
Styling: Tailwind CSS
Charts: Recharts for portfolio analytics

### Integrations

Yield Protocols: Blend Protocol (lending), Stellar DEX (liquidity)
Price Feeds: Stellar anchor price feeds


## Project Structure

```text
NeuroWealth-Smartcontract/
├── neurowealth-vault/              # Soroban smart contracts workspace
│   ├── Cargo.toml
│   ├── Cargo.lock
│   ├── contracts/
│   │   └── vault/                  # Core vault contract
│   │       ├── Cargo.toml
│   │       └── src/
│   │           ├── lib.rs          # Contract logic, events, error types
│   │           ├── topics.rs       # Exported event topic constants
│   │           └── tests/          # Test modules (39 files)
│   └── fuzz/                       # Libfuzzer fuzz targets
│       ├── Cargo.toml
│       └── fuzz_targets/
├── packages/
│   └── vault-client/               # Generated TypeScript client
│       ├── README.md
│       ├── tsconfig.json
│       └── src/
│           ├── index.ts
│           └── generated/
├── scripts/                        # Deployment and utility scripts
│   ├── deploy-devnet.sh
│   ├── e2e-devnet.sh
│   ├── verify-deployment.sh
│   ├── generate-spec.py
│   ├── validate-spec.py
│   ├── generate-client.js
│   ├── check-readme.sh
│   ├── check-no-bare-panic.sh
│   ├── e2e-restore.sh
│   ├── README-E2E.md
│   └── README-SPEC.md
├── docs/
│   ├── BLEND_INTEGRATION_RESEARCH.md
│   ├── DEX_INTEGRATION.md
│   ├── UPGRADE_MIGRATION.md
│   ├── MAINNET_CHECKLIST.md
│   ├── PARTIAL_WITHDRAWAL_BEHAVIOR.md
│   ├── REBALANCE_FAILURE_RECOVERY.md
│   ├── WASM_SIZE.md
│   ├── E2E_ARTIFACT_LIFECYCLE.md
│   ├── monitoring.md
│   ├── state-machine.md
│   ├── FORMAL_VERIFICATION.md
│   ├── ACCESSIBILITY.md
│   └── NOTIFICATIONS.md
├── test/                           # Off-chain security tests
│   ├── NotOwnerCompromiseBlastRadius.test.ts
│   └── OwnerCompromiseBlastRadius.test.ts
├── .env.devnet.template
├── .github/
│   ├── ISSUE_TEMPLATE/
│   ├── pull_request_template.md
│   └── workflows/
│       ├── ci.yml
│       └── contract-spec.yml
├── .stellar-version
├── deny.toml
├── ARCHITECTURE.md
├── EVENTS.md
├── SECURITY.md
├── CONTRIBUTING.md
├── CHANGELOG.md
├── contract-spec.json
├── ERROR_STYLE_GUIDE.md
└── README.md
```

### Planned Components

The following are not yet in this repository and will be added as separate
directories once development begins:

| Component | Directory | Status |
|-----------|-----------|--------|
| AI agent backend (Node.js / Python) | `agent/` | Planned |
| Next.js web frontend | `frontend/` | Completed (#471, #472) |
| WhatsApp bot handler | `whatsapp/` | Completed (#469) |
| PostgreSQL / Supabase schema | `db/` / `supabase/` | Completed (#470) |

## Getting Started

### Prerequisites

Install Rust and the WASM target:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add wasm32-unknown-unknown
```

Install the Stellar CLI (version pinned in [`.stellar-version`](.stellar-version)):
```bash
STELLAR_VERSION=$(cat .stellar-version | tr -d '[:space:]')
cargo install --locked stellar-cli --version "$STELLAR_VERSION" --features opt
```

### Environment Variables

Copy the template and add your secret key:
```bash
cp .env.devnet.template .env.devnet
# Edit .env.devnet and set SOROBAN_SECRET_KEY
```

### Build the Contract

```bash
cd neurowealth-vault
stellar contract build
```

The compiled WASM is output to `target/wasm32-unknown-unknown/release/neurowealth_vault.wasm`.

### Run Tests

```bash
cd neurowealth-vault
cargo test
```

### Deploy to Devnet

```bash
./scripts/deploy-devnet.sh
```

See [`scripts/README-E2E.md`](scripts/README-E2E.md) for end-to-end devnet validation.

> For the AI agent, frontend, and WhatsApp bot — see [Planned Components](#planned-components) above.

## Further Reading

| Document | Purpose |
|----------|---------|
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | Storage layout, share accounting math, asset flow diagrams |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | Development setup, CI requirements, PR process |
| [`scripts/README-E2E.md`](scripts/README-E2E.md) | End-to-end devnet test guide |
| [`SECURITY.md`](SECURITY.md) | Trust model, threat analysis, pause-semantics matrix, agent-compromise adversarial tests, and owner-compromise runbook |
| [`docs/BUG_BOUNTY.md`](docs/BUG_BOUNTY.md) | Bug bounty scope, severity rubric, safe-harbor terms, and payout process |
| [`docs/MAINNET_CHECKLIST.md`](docs/MAINNET_CHECKLIST.md) | Pre-mainnet deployment checklist |
| [`docs/DEX_INTEGRATION.md`](docs/DEX_INTEGRATION.md) | DEX strategy behaviour, integration assumptions, and liquidity routing |
| [`docs/BLEND_INTEGRATION_RESEARCH.md`](docs/BLEND_INTEGRATION_RESEARCH.md) | Blend protocol supply/withdraw design and cross-contract call patterns |
| [`docs/LEAST_PRIVILEGE_AGENT.md`](docs/LEAST_PRIVILEGE_AGENT.md) | Least-privilege evaluation: separate rebalancer vs reporter agent roles (Issue #606) |
| [`docs/GUARDIAN_KEY_DESIGN.md`](docs/GUARDIAN_KEY_DESIGN.md) | Guardian-key design: second signature for `execute_upgrade` (Issue #607) |
| [`docs/FORMAL_VERIFICATION.md`](docs/FORMAL_VERIFICATION.md) | Kani proofs and share-accounting properties (Issue #672) |
| [`docs/ACCESSIBILITY.md`](docs/ACCESSIBILITY.md) | WCAG 2.1 AA statement, axe-core CI, and screen-reader checklist (Issue #668) |
| [`docs/NOTIFICATIONS.md`](docs/NOTIFICATIONS.md) | Web Push, email fallback, and notification preferences (Issue #669) |
| [`docs/ERC4626_CONFORMANCE_CHECKLIST.md`](docs/ERC4626_CONFORMANCE_CHECKLIST.md) | ERC-4626 conformance checklist: function-by-function diff against the spec (Issue #602) |
| [`docs/ISSUER_FREEZE_CONTINGENCY.md`](docs/ISSUER_FREEZE_CONTINGENCY.md) | Operational plan if the vault's USDC or agent wallet is frozen by the issuer (Issue #604) |
| [`docs/SECRETS_HYGIENE.md`](docs/SECRETS_HYGIENE.md) | Full-history secret scan results and pre-commit/CI enforcement (Issue #605) |

## Smart Contract
The core Soroban vault contract handles all on-chain fund management.

### Key Functions

#### Core & Administration

| Function | Who Can Call | Description |
| :--- | :--- | :--- |
| `initialize` | Deployer (once) | Authorize via deployer signature and set **separate** owner and agent addresses plus the USDC token |
| `deposit` | Any verified user | Deposit USDC into the vault |
| `withdraw` | User (their own funds) | Withdraw USDC back to wallet |
| `withdraw_all` | User (their own funds) | Withdraw all USDC by burning all shares |
| `rebalance` | AI Agent only | Move funds between yield strategies (`protocol`, `expected_apy`, `min_out`; supported: `blend`, `dex`, `none`) |
| `harvest` | AI Agent only | Withdraw accrued yield from `CurrentProtocol` and re-supply it with `min_out` slippage protection |
| `set_blend_pool` | Owner only | Configure the Blend lending pool address |
| `set_dex_pool` | Owner only | Configure the DEX liquidity pool address |
| `get_balance` | Anyone | Read a user's current balance |
| `get_total_deposits` | Anyone | Read total vault TVL |
| `get_exchange_rate` | Anyone | Read current exchange rate (assets per share * 10,000,000) |
| `transfer_ownership` | Owner only | Initiate two-step ownership transfer |
| `accept_ownership` | Pending owner only | Complete ownership transfer |
| `set_caps` | Owner only | Sets both user deposit cap and TVL cap in a single transaction |
| `set_deposit_limits` | Owner only | Sets minimum and maximum per-transaction deposit limits |
| `set_tvl_cap` | Owner only | Sets the maximum total TVL that can be deposited |
| `set_user_deposit_cap` | Owner only | Sets the maximum deposit amount per user |
| `set_limits` | Owner only | **Deprecated**: Sets user deposit cap and TVL cap (use `set_caps` instead) |

#### Strategy Preference

| Function | Who Can Call | Description |
| :--- | :--- | :--- |
| `set_user_strategy` | User (their own preference) | Store a strategy preference (`conservative`, `balanced`, or `growth`) on-chain for the AI agent to read |
| `get_user_strategy` | Anyone | Read a user's strategy preference (defaults to `balanced` when unset) |

#### Share Math & Previews

| Function | Who Can Call | Description |
| :--- | :--- | :--- |
| `preview_deposit_to_shares` | Anyone | Shares that would be minted for a given asset amount (rounds **down**) |
| `preview_shares_to_assets` | Anyone | Assets that would be returned for a given share amount (rounds **down**) |
| `preview_withdraw` | Anyone | Shares that would be burned to withdraw a given asset amount (rounds **up**, matching `withdraw`) |
| `convert_to_shares` | Anyone | ERC-4626 asset → share conversion at the current rate (rounds **down**) |
| `convert_to_assets` | Anyone | ERC-4626 share → asset conversion at the current rate (rounds **down**) |

#### Asset Tracking

| Function | Who Can Call | Description |
| :--- | :--- | :--- |
| `get_idle_balance` | Anyone | USDC held in the vault and not yet deployed to a protocol |
| `get_deployed_assets` | Anyone | USDC currently supplied to the active protocol (`0` when `CurrentProtocol` is `none`) |
| `get_asset_breakdown` | Anyone | Both figures in one call as `(idle, deployed)` — avoids two RPC round-trips |

#### Storage Maintenance

| Function | Who Can Call | Description |
| :--- | :--- | :--- |
| `touch_user_ttl` | Anyone | Extend the `Shares(user)` persistent entry TTL; returns `false` when no entry exists. Needed because read-only getters have no TTL side effects |

#### Upgrade Timelock (Issue #316)

The instant `upgrade()` entrypoint has been removed — see the *Upgrade Safety* section of [`ARCHITECTURE.md`](ARCHITECTURE.md).

| Function | Who Can Call | Description |
| :--- | :--- | :--- |
| `schedule_upgrade` | Owner only (not paused) | Propose a new WASM hash; unlocks after `UPGRADE_TIMELOCK_LEDGERS` (17,280 ledgers ≈ 24 h) |
| `execute_upgrade` | Owner only (not paused) | Apply the pending WASM once the timelock has elapsed and increment `Version` |
| `cancel_upgrade` | Owner only | Clear the pending upgrade — the recovery path for a malicious or mistaken proposal |
| `get_pending_upgrade` | Anyone | Returns `Some((wasm_hash, effective_ledger))` while an upgrade is pending, else `None` |

#### Agent Timelock (Issue #317)

Rotating the agent is a two-step, timelocked flow — see the *Agent Update Timelock* section of [`ARCHITECTURE.md`](ARCHITECTURE.md).

| Function | Who Can Call | Description |
| :--- | :--- | :--- |
| `update_agent` | Owner only | **Propose** a new agent; unlocks after `AGENT_TIMELOCK_LEDGERS` (17,280 ledgers ≈ 24 h). The active agent is unchanged |
| `confirm_agent_update` | Owner only | Apply the pending agent once the timelock has elapsed |
| `cancel_agent_update` | Owner only | Clear the pending agent update |
| `get_pending_agent_update` | Anyone | Returns `Some((pending_agent, effective_ledger))` while an update is pending, else `None` |

#### Rebalance Throttle & Approvals

| Function | Who Can Call | Description |
| :--- | :--- | :--- |
| `set_rebalance_cooldown` | Owner only | Minimum ledgers between `rebalance()` calls; `0` disables the cooldown |
| `get_rebalance_cooldown` | Anyone | Read the configured cooldown in ledgers (`0` = disabled) |
| `get_last_rebalance_ledger` | Anyone | Ledger of the most recent successful `rebalance()` (`0` if never called) |
| `set_approval_ttl` | Owner only | Ledger lifetime for Blend/DEX token approvals (bounded to 1,000–500,000 ledgers) |
| `get_approval_ttl` | Anyone | Read the configured approval TTL, or the default when unset |

## Security Model

Users can only withdraw their own funds — enforced at the contract level via user.require_auth()
Only the designated AI agent keypair can call rebalance — no other address can move funds between protocols
Minimum deposit: 1 USDC. Maximum per user: 10,000 USDC (configurable)
Emergency pause functionality available to contract owner
Two-step ownership transfer prevents accidental ownership loss
Vault balance verification ensures reported assets match actual holdings
Read-only getters have no TTL side effects; call `touch_user_ttl` to extend user share entry TTL
Strict Checks-Effects-Interactions (CEI) pattern prevents reentrancy without needing explicit locks (see [reentrancy protection tests](neurowealth-vault/contracts/vault/src/tests/test_legacy_inline.rs))

## Secure Deployment Sequence

`initialize()` is protected against front-running: the contract verifies that the `deployer`
argument + `salt` cryptographically reproduce the deployed contract address, **and** requires
a live authorization signature from that deployer keypair. This means no third party can
seize ownership even if they observe the deployment transaction in the mempool.

Follow these steps in order to safely initialize a new vault:

1. **Generate a deployer keypair** (one-time use, only for initialization):
   ```bash
   stellar keys generate deployer --network testnet
   stellar keys address deployer   # note the deployer address
   ```

2. **Choose a salt** (32 bytes; any fixed value works — must be the same across steps):
   ```bash
   # example: all-zero salt
   SALT="0000000000000000000000000000000000000000000000000000000000000000"
   ```

3. **Deploy the contract** using the deployer keypair and the chosen salt:
   ```bash
   stellar contract deploy \
     --wasm target/wasm32-unknown-unknown/release/neurowealth_vault.wasm \
     --source deployer \
     --network testnet \
     --salt $SALT
   # save the output as VAULT_CONTRACT_ID
   ```

4. **Immediately call `initialize()`** from the same deployer keypair:
   ```bash
   stellar contract invoke \
     --id $VAULT_CONTRACT_ID \
     --source deployer \
     --network testnet \
     -- \
     initialize \
     --deployer $(stellar keys address deployer) \
     --owner  $OWNER_ADDRESS \
     --agent  $AGENT_ADDRESS \
     --usdc_token $USDC_TOKEN_ADDRESS \
     --salt   $SALT
   ```
   The contract rejects any caller whose `deployer` argument does not reproduce
   `VAULT_CONTRACT_ID`, and additionally requires a valid signature from that
   address via `deployer.require_auth()`.

5. **Verify initialization** (read-only, no auth needed):
   ```bash
   stellar contract invoke --id $VAULT_CONTRACT_ID --source deployer \
     --network testnet -- get_owner
   stellar contract invoke --id $VAULT_CONTRACT_ID --source deployer \
     --network testnet -- get_agent
   ```

6. **Secure or discard the deployer keypair** — it has no further privileged role
   after initialization. The `owner` keypair is now the administrator.


## AI Agent

The agent runs as a persistent background service with two main loops.

### Decision Loop (runs every hour)
1. Fetch current APY from all integrated protocols (Blend, DEX pools)
2. Compare against each user's current deployed strategy
3. If a better opportunity exists (> 0.5% improvement), rebalance
4. Submit rebalance transaction to vault contract
5. Log results to database

### Intent Parser (real-time, event-driven)
User message: "deposit 50 USDC into balanced strategy"
       ↓
AI parses intent: { action: "deposit", amount: 50, strategy: "balanced" }
       ↓
Agent builds Stellar transaction
       ↓
Returns confirmation: "Deposited 50 USDC. Earning ~8.2% APY in Balanced strategy."
### Supported User Intents

deposit [amount] [optional: strategy]
withdraw [amount or "all"]
balance / how much do I have
earnings / how much have I made
switch to [conservative/balanced/growth]
what is my APY


## WhatsApp Integration

NeuroWealth is designed to be fully operable through WhatsApp, making it accessible to anyone with a smartphone — no wallet app or browser extension needed.

### User Flow
1. User sends "hi" to NeuroWealth WhatsApp number
2. Bot introduces itself and asks for phone number verification (OTP)
3. OTP verified → agent creates a Stellar keypair for this user (custodial)
4. User can now deposit, withdraw, and check balance entirely through chat
5. Funds are secured in the Soroban vault contract under their wallet address

### Setting Up the Webhook
bash# Your webhook endpoint receives WhatsApp messages
POST /api/whatsapp/webhook

# Register your webhook URL with Twilio
# ngrok http 3000  ← for local testing

### Example Conversation
User:    deposit 100 USDC
Agent:   Got it! Depositing 100 USDC into your Balanced strategy.
         This should take about 5 seconds on Stellar... ✅ Done!
         You're now earning ~8.4% APY. I'll optimize automatically.

User:    what's my balance?
Agent:   💰 Your NeuroWealth Portfolio
         Balance: 100.23 USDC
         Earnings today: +$0.23
         Current APY: 8.4%
         Strategy: Balanced

User:    withdraw everything
Agent:   Withdrawing 100.23 USDC... ✅ Done!
         Funds sent to your wallet. Arrived in 4 seconds.

## Deployment

### Quick Start (Devnet)

For testing and development, you can deploy to Stellar devnet in minutes:

1. **Get a funded devnet account**
   ```bash
   # Visit https://laboratory.stellar.org/#account-creator
   # Create an account and copy the secret key
   ```

2. **Set up environment**
   ```bash
   # Copy the template and add your secret key
   cp .env.devnet.template .env.devnet
   # Edit .env.devnet and add your SOROBAN_SECRET_KEY
   ```

3. **Build contracts**
   ```bash
   cd neurowealth-vault
   stellar contract build
   ```

4. **Deploy to devnet**
   ```bash
   ./scripts/deploy-devnet.sh
   ```

5. **Start using the vault**
   ```bash
   # Source the deployed contract addresses
   source scripts/devnet-contracts.env
   
   # Check your balance
   stellar contract invoke \
     --id $VAULT_CONTRACT_ID \
     --source $AGENT_SECRET_KEY \
     --network $SOROBAN_NETWORK_PASSPHRASE \
     --rpc-url $SOROBAN_RPC_URL \
     -- \
     get_balance \
     --user $AGENT_ADDRESS
   
   # Deposit 10 USDC
   stellar contract invoke \
     --id $VAULT_CONTRACT_ID \
     --source $AGENT_SECRET_KEY \
     --network $SOROBAN_NETWORK_PASSPHRASE \
     --rpc-url $SOROBAN_RPC_URL \
     -- \
     deposit \
     --user $AGENT_ADDRESS \
     --amount 10000000
   ```

The deployment script will:
- Deploy the USDC token contract
- Deploy the NeuroWealth vault contract
- Initialize the vault with your account as the AI agent
- Mint 10,000 USDC for testing
- Save all contract addresses to `scripts/devnet-contracts.env`

### Testnet

```bash
# Deploy everything to Stellar testnet
./scripts/deploy.sh --network testnet
```

### Mainnet
⚠️ **CRITICAL:** Before deploying to Stellar mainnet, you must complete and sign off on all items in the [Mainnet Deployment Checklist](docs/MAINNET_CHECKLIST.md) (including separate keys setup, TVL limits, Blend pool verification, pause drills, and multisig governance plans).

```bash
# Ensure all tests pass first
cargo test
npm test

# Deploy to mainnet
./scripts/deploy.sh --network mainnet
```
## Infrastructure (Recommended)

Agent: Railway, Render, or a VPS (needs to run 24/7)
Frontend: Vercel
Database: Supabase (managed PostgreSQL)
Webhook: Same server as agent, or a separate serverless function


## Roadmap

### Phase 1 — Foundation (Current)

 Soroban vault contract (deposit, withdraw, rebalance)
 Basic AI agent with Blend protocol integration
 Natural language intent parsing
 Web frontend with portfolio dashboard
  WhatsApp bot MVP

### Phase 2 — Intelligence

 Multi-protocol yield aggregation (Blend + DEX liquidity pools)
 Strategy backtesting and risk scoring
 Personalized risk profiles per user
  Earnings history and projection charts

### Phase 3 — Scale

 Real-world asset (RWA) yield strategies
 Cross-chain bridging (Stellar ↔ Ethereum via Axelar)
 Social trading — follow top-performing AI strategies
 NeuroWealth token for governance and fee sharing


## Contributing
Contributions are welcome! Please read our [Contributing Guide](CONTRIBUTING.md) for details on our code of conduct, development setup, and the process for submitting pull requests.

### Quick Start for Contributors
1. **Fork the repo**, then:
   ```bash
   git checkout -b feature/your-feature-name
   git commit -m "feat: add your feature"
   git push origin feature/your-feature-name
   ```
2. **Open a Pull Request** against the `main` branch.
3. Please make sure to run `cargo test` and `npm test` before submitting.

