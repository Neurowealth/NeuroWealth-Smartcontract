#!/usr/bin/env python3
"""
Generate Stellar contract spec for NeuroWealth Vault

This script parses the Soroban contract source code and generates a JSON 
specification dynamically that can be used by frontend and agent clients,
preventing spec drift.

Usage:
    python3 scripts/generate-spec.py
    
Output:
    contract-spec.json
"""

import json
import re
from pathlib import Path
from typing import Dict, List, Any, Optional

FALLBACK_TOPICS = {
    "PauseEvent": "pause_event",
    "InitFailedEvent": "init_failed"
}

FUNCTION_METADATA = {
    "initialize": {
        "category": "initialization",
        "access": "once",
        "description": "Initialize the vault contract (can only be called once)",
        "requires_auth": True,
        "state_changing": True,
        "events": ["VaultInitializedEvent"],
        "notes": [
            "Requires valid signature from deployer to prevent front-running",
            "Can only be called once - subsequent calls will panic"
        ]
    },
    "deposit": {
        "category": "liquidity",
        "access": "public",
        "description": "Deposit USDC into the vault and receive vault shares",
        "requires_auth": True,
        "state_changing": True,
        "constraints": [
            "amount must be > 0",
            "amount >= minDeposit",
            "amount <= maxDeposit",
            "user.balance + amount <= userDepositCap",
            "totalDeposits + amount <= tvlCap",
            "vault must not be paused",
            "shares_to_mint > 0 (non-zero mint guard)"
        ],
        "events": ["DepositEvent"],
        "formula": "shares_to_mint = floor(amount * total_shares / total_assets)",
        "security": "Protects against inflation attacks with minimum deposit and non-zero mint guard"
    },
    "withdraw": {
        "category": "liquidity",
        "access": "public",
        "description": "Withdraw USDC from the vault by burning shares",
        "requires_auth": True,
        "state_changing": True,
        "constraints": [
            "user must have sufficient shares to cover withdrawal",
            "amount > 0",
            "vault must not be paused"
        ],
        "events": ["WithdrawEvent"],
        "formula": "shares_to_burn = ceil(amount * total_shares / total_assets)"
    },
    "withdraw_all": {
        "category": "liquidity",
        "access": "public",
        "description": "Withdraw all user funds by burning all shares",
        "requires_auth": True,
        "state_changing": True,
        "events": ["WithdrawEvent"]
    },
    "rebalance": {
        "category": "management",
        "access": "agent-only",
        "description": "AI agent rebalances funds between yield protocols",
        "requires_auth": True,
        "authorized_caller": "agent",
        "state_changing": True,
        "events": ["RebalanceEvent"],
        "security": "Only authorized agent keypair can call this function",
        "supported_protocols": ["blend", "none"],
        "notes": ["Performs slippage check against min_out parameter"]
    },
    "pause": {
        "category": "administration",
        "access": "owner-only",
        "description": "Pause deposits and withdrawals (emergency function)",
        "requires_auth": True,
        "state_changing": True,
        "events": ["VaultPausedEvent"]
    },
    "unpause": {
        "category": "administration",
        "access": "owner-only",
        "description": "Resume normal operations",
        "requires_auth": True,
        "state_changing": True,
        "events": ["VaultUnpausedEvent"]
    },
    "emergency_pause": {
        "category": "administration",
        "access": "owner-only",
        "description": "Emergency pause without signature verification (for critical situations)",
        "requires_auth": True,
        "state_changing": True,
        "events": ["EmergencyPausedEvent"]
    },
    "set_tvl_cap": {
        "category": "administration",
        "access": "owner-only",
        "description": "Set maximum total value locked in vault",
        "requires_auth": True,
        "state_changing": True,
        "events": ["TvlCapUpdatedEvent"]
    },
    "set_user_deposit_cap": {
        "category": "administration",
        "access": "owner-only",
        "description": "Set maximum deposit per user",
        "requires_auth": True,
        "state_changing": True,
        "events": ["UserDepositCapUpdatedEvent"]
    },
    "set_caps": {
        "category": "administration",
        "access": "owner-only",
        "description": "Set both user deposit cap and TVL cap in single atomic transaction",
        "requires_auth": True,
        "state_changing": True,
        "events": ["CapsUpdatedEvent"],
        "notes": ["Preferred method over calling set_tvl_cap and set_user_deposit_cap separately"]
    },
    "set_deposit_limits": {
        "category": "administration",
        "access": "owner-only",
        "description": "Set minimum and maximum per-transaction deposit limits",
        "requires_auth": True,
        "state_changing": True,
        "events": ["LimitsUpdatedEvent"]
    },
    "set_limits": {
        "category": "administration",
        "access": "owner-only",
        "description": "DEPRECATED: Use set_caps or set_deposit_limits instead",
        "deprecated": True,
        "requires_auth": True,
        "state_changing": True,
        "notes": ["This function is deprecated and may be removed in future versions"]
    },
    "update_agent": {
        "category": "administration",
        "access": "owner-only",
        "description": "Update the authorized AI agent address",
        "requires_auth": True,
        "state_changing": True,
        "events": ["AgentUpdatedEvent"]
    },
    "transfer_ownership": {
        "category": "administration",
        "access": "owner-only",
        "description": "Initiate two-step ownership transfer (new owner must call accept_ownership)",
        "requires_auth": True,
        "state_changing": True,
        "events": ["OwnershipTransferInitiatedEvent"],
        "notes": ["Two-step process prevents accidental ownership loss", "New owner must accept within timeframe"]
    },
    "accept_ownership": {
        "category": "administration",
        "access": "pending-owner-only",
        "description": "Accept ownership transfer (must be called by pending owner)",
        "requires_auth": True,
        "state_changing": True,
        "events": ["OwnershipTransferredEvent"]
    },
    "set_blend_pool": {
        "category": "administration",
        "access": "owner-only",
        "description": "Set Blend pool address for yield deployment",
        "requires_auth": True,
        "state_changing": True,
        "events": ["BlendPoolConfiguredEvent"]
    },
    "set_dex_pool": {
        "category": "administration",
        "access": "owner-only",
        "description": "Set DEX pool address for liquidity deployment",
        "requires_auth": True,
        "state_changing": True,
        "events": ["DexPoolConfiguredEvent"]
    },
    "set_blend_approval_ttl": {
        "category": "administration",
        "access": "owner-only",
        "description": "Set the ledger TTL for Blend token approvals",
        "requires_auth": True,
        "state_changing": True
    },
    "set_approval_ttl": {
        "category": "administration",
        "access": "owner-only",
        "description": "Set the ledger TTL for token approvals (alias for set_blend_approval_ttl)",
        "requires_auth": True,
        "state_changing": True
    },
    "batch_deposit": {
        "category": "liquidity",
        "access": "public",
        "description": "Deposit multiple USDC entries atomically and receive vault shares",
        "requires_auth": True,
        "state_changing": True,
        "constraints": [
            "entry count must be <= max batch size (0 = unlimited)",
            "all entries must use the configured USDC token",
            "batch consumes both deposit and batch-deposit rate-limit buckets"
        ],
        "events": ["DepositEvent"]
    },
    "set_rate_limit": {
        "category": "administration",
        "access": "owner-only",
        "description": "Configure a fixed-window call rate limit for a supported function category",
        "requires_auth": True,
        "state_changing": True,
        "events": ["RateLimitConfigUpdatedEvent"],
        "constraints": [
            "max_calls == 0 disables the category",
            "enabled categories require a non-zero window",
            "unknown categories are rejected"
        ]
    },
    "set_rate_limit_config": {
        "category": "administration",
        "access": "owner-only",
        "description": "Alias for set_rate_limit",
        "requires_auth": True,
        "state_changing": True,
        "events": ["RateLimitConfigUpdatedEvent"]
    },
    "get_rate_limit": {
        "category": "queries",
        "access": "public",
        "description": "Get the configured rate-limit allowance for a category",
        "requires_auth": False,
        "state_changing": False,
        "query_only": True
    },
    "get_rate_limit_config": {
        "category": "queries",
        "access": "public",
        "description": "Get the configured rate-limit allowance for a category",
        "requires_auth": False,
        "state_changing": False,
        "query_only": True
    },
    "get_global_rate_limit_state": {
        "category": "queries",
        "access": "public",
        "description": "Get the current global rate-limit usage bucket",
        "requires_auth": False,
        "state_changing": False,
        "query_only": True
    },
    "get_user_rate_limit_state": {
        "category": "queries",
        "access": "public",
        "description": "Get the current per-user rate-limit usage bucket",
        "requires_auth": False,
        "state_changing": False,
        "query_only": True
    },
    "set_max_batch_size": {
        "category": "administration",
        "access": "owner-only",
        "description": "Set the maximum number of entries accepted by batch_deposit",
        "requires_auth": True,
        "state_changing": True,
        "events": ["BatchSizeLimitUpdatedEvent"]
    },
    "set_batch_size_limit": {
        "category": "administration",
        "access": "owner-only",
        "description": "Alias for set_max_batch_size",
        "requires_auth": True,
        "state_changing": True,
        "events": ["BatchSizeLimitUpdatedEvent"]
    },
    "get_max_batch_size": {
        "category": "queries",
        "access": "public",
        "description": "Get the maximum batch_deposit entry count",
        "requires_auth": False,
        "state_changing": False,
        "query_only": True
    },
    "get_batch_size_limit": {
        "category": "queries",
        "access": "public",
        "description": "Get the maximum batch_deposit entry count",
        "requires_auth": False,
        "state_changing": False,
        "query_only": True
    },
    "set_rebalance_cooldown": {
        "category": "administration",
        "access": "owner-only",
        "description": "Set minimum ledgers between rebalance calls (0 = no cooldown)",
        "requires_auth": True,
        "state_changing": True
    },
    "cancel_ownership_transfer": {
        "category": "administration",
        "access": "owner-only",
        "description": "Cancel a pending ownership transfer",
        "requires_auth": True,
        "state_changing": True,
        "events": ["OwnershipTransferCancelledEvent"]
    },
    "update_total_assets": {
        "category": "management",
        "access": "agent-only",
        "description": "Update total assets to reflect realized yield or loss",
        "requires_auth": True,
        "authorized_caller": "agent",
        "state_changing": True,
        "events": ["AssetsUpdatedEvent"],
        "notes": [
            "Only agent can call",
            "Decreases require owner co-signature",
            "Decrease is capped at max_decrease_bps"
        ]
    },
    "confirm_agent_update": {
        "category": "administration",
        "access": "owner-only",
        "description": "Confirm a proposed agent update after the timelock has elapsed",
        "requires_auth": True,
        "state_changing": True,
        "events": ["AgentUpdateConfirmedEvent", "AgentUpdatedEvent"]
    },
    "cancel_agent_update": {
        "category": "administration",
        "access": "owner-only",
        "description": "Cancel a pending proposed agent update",
        "requires_auth": True,
        "state_changing": True,
        "events": ["AgentUpdateCancelledEvent"]
    },
    "get_pending_agent_update": {
        "category": "queries",
        "access": "public",
        "description": "Get the pending proposed agent address and effective ledger",
        "requires_auth": False,
        "state_changing": False,
        "query_only": True
    },
    "schedule_upgrade": {
        "category": "administration",
        "access": "owner-only",
        "description": "Schedule a contract WASM code upgrade by hash, initiating the timelock window",
        "requires_auth": True,
        "state_changing": True,
        "events": ["UpgradeScheduledEvent"]
    },
    "execute_upgrade": {
        "category": "administration",
        "access": "owner-only",
        "description": "Execute a scheduled contract WASM code upgrade after the timelock has elapsed",
        "requires_auth": True,
        "state_changing": True,
        "events": ["UpgradedEvent"]
    },
    "cancel_upgrade": {
        "category": "administration",
        "access": "owner-only",
        "description": "Cancel a pending contract code upgrade proposal",
        "requires_auth": True,
        "state_changing": True,
        "events": ["UpgradeCancelledEvent"]
    },
    "get_pending_upgrade": {
        "category": "queries",
        "access": "public",
        "description": "Get the pending proposed upgrade WASM hash and effective ledger",
        "requires_auth": False,
        "state_changing": False,
        "query_only": True
    },
    "get_balance": {
        "category": "queries",
        "access": "public",
        "description": "Get user's USDC balance",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "persistent",
        "query_only": True
    },
    "get_total_deposits": {
        "category": "queries",
        "access": "public",
        "description": "Get total USDC deposited in vault",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_total_assets": {
        "category": "queries",
        "access": "public",
        "description": "Get total vault assets (principal + yield accumulated)",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_total_shares": {
        "category": "queries",
        "access": "public",
        "description": "Get total vault shares outstanding",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_shares": {
        "category": "queries",
        "access": "public",
        "description": "Get user's vault shares",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "persistent",
        "query_only": True
    },
    "get_exchange_rate": {
        "category": "queries",
        "access": "public",
        "description": "Get current exchange rate (assets per share * 10^7)",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_owner": {
        "category": "queries",
        "access": "public",
        "description": "Get contract owner address",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_agent": {
        "category": "queries",
        "access": "public",
        "description": "Get authorized AI agent address",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_usdc_token": {
        "category": "queries",
        "access": "public",
        "description": "Get USDC token contract address",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_version": {
        "category": "queries",
        "access": "public",
        "description": "Get contract version for upgrade tracking",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_current_protocol": {
        "category": "queries",
        "access": "public",
        "description": "Get current yield protocol (\"blend\" or \"none\")",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_blend_pool": {
        "category": "queries",
        "access": "public",
        "description": "Get Blend pool address if configured",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_tvl_cap": {
        "category": "queries",
        "access": "public",
        "description": "Get current TVL cap",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_user_deposit_cap": {
        "category": "queries",
        "access": "public",
        "description": "Get current per-user deposit cap",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_min_deposit": {
        "category": "queries",
        "access": "public",
        "description": "Get minimum deposit amount per transaction",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_max_deposit": {
        "category": "queries",
        "access": "public",
        "description": "Get maximum deposit amount per transaction",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_pending_owner": {
        "category": "queries",
        "access": "public",
        "description": "Get pending owner address if transfer in progress",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_user_info": {
        "category": "queries",
        "access": "public",
        "description": "Get complete user information and statistics",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "mixed",
        "query_only": True
    },
    "is_paused": {
        "category": "queries",
        "access": "public",
        "description": "Check if vault is paused",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "touch_user_ttl": {
        "category": "maintenance",
        "access": "public",
        "description": "Extend persistent TTL for user shares entry with a per-user rate limit",
        "requires_auth": False,
        "state_changing": True,
        "storage_type": "mixed",
        "query_only": False
    },
    "preview_deposit_to_shares": {
        "category": "queries",
        "access": "public",
        "description": "Preview shares minted for asset amount (floor), subject to the global preview rate limit",
        "requires_auth": False,
        "state_changing": True,
        "query_only": False
    },
    "preview_shares_to_assets": {
        "category": "queries",
        "access": "public",
        "description": "Preview assets returned for share amount (floor), subject to the global preview rate limit",
        "requires_auth": False,
        "state_changing": True,
        "query_only": False
    },
    "preview_withdraw": {
        "category": "queries",
        "access": "public",
        "description": "Preview shares burned for withdrawal amount (ceil), subject to the global preview rate limit",
        "requires_auth": False,
        "state_changing": True,
        "query_only": False
    },
    "convert_to_shares": {
        "category": "queries",
        "access": "public",
        "description": "Convert asset amount to shares (floor), subject to the global preview rate limit",
        "requires_auth": False,
        "state_changing": True,
        "query_only": False
    },
    "convert_to_assets": {
        "category": "queries",
        "access": "public",
        "description": "Convert share amount to assets (floor), subject to the global preview rate limit",
        "requires_auth": False,
        "state_changing": True,
        "query_only": False
    },
    "get_dex_pool": {
        "category": "queries",
        "access": "public",
        "description": "Get DEX pool address if configured",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_blend_approval_ttl": {
        "category": "queries",
        "access": "public",
        "description": "Get the configured Blend approval TTL in ledgers",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_approval_ttl": {
        "category": "queries",
        "access": "public",
        "description": "Get the configured token approval TTL in ledgers",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_rebalance_cooldown": {
        "category": "queries",
        "access": "public",
        "description": "Get the minimum ledger interval between rebalances",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_last_rebalance_ledger": {
        "category": "queries",
        "access": "public",
        "description": "Get the ledger sequence of the last successful rebalance",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_idle_balance": {
        "category": "queries",
        "access": "public",
        "description": "Get the vault's idle USDC balance (funds held in the vault, not deployed to any protocol)",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_deployed_assets": {
        "category": "queries",
        "access": "public",
        "description": "Get the amount of USDC currently deployed to an external yield protocol",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "get_asset_breakdown": {
        "category": "queries",
        "access": "public",
        "description": "Get the vault's asset breakdown as (idle, deployed) in a single call",
        "requires_auth": False,
        "state_changing": False,
        "storage_type": "instance",
        "query_only": True
    },
    "set_user_strategy": {
        "category": "management",
        "access": "public",
        "description": "Set a user's strategy choice",
        "requires_auth": True,
        "state_changing": True,
        "events": ["UserStrategyUpdatedEvent"]
    },
    "get_user_strategy": {
        "category": "queries",
        "access": "public",
        "description": "Get a user's strategy choice",
        "requires_auth": False,
        "state_changing": False,
        "query_only": True
    }
}


class ContractSpecGenerator:
    """Generate contract specification from Soroban contract source dynamically."""
    
    def __init__(self, contract_path: str, topics_path: str):
        self.contract_path = Path(contract_path)
        self.topics_path = Path(topics_path)
        if not self.contract_path.exists():
            raise FileNotFoundError(f"Contract file not found: {contract_path}")
        if not self.topics_path.exists():
            raise FileNotFoundError(f"Topics file not found: {topics_path}")
        
        self.source = self.contract_path.read_text()
        self.topics_source = self.topics_path.read_text()
    
    def generate(self) -> Dict[str, Any]:
        """Generate complete contract specification."""
        event_to_symbol = self._parse_event_topics()
        return {
            "version": "1.0.0",
            "contract": "NeuroWealth Vault",
            "network": "Stellar Soroban",
            "description": "ERC-4626 inspired vault contract for autonomous yield management",
            "decimals": 7,
            "token": "USDC",
            "functions": self._get_functions(),
            "events": self._get_events(event_to_symbol),
            "errors": self._get_errors(),
            "types": self._get_types(),
            "constants": self._get_constants(),
        }
        
    def _parse_event_topics(self) -> Dict[str, str]:
        # 1. Parse topics.rs to map constant name to symbol string
        # e.g., pub const TOPIC_INIT: Symbol = symbol_short!("init");
        topic_const_to_symbol = {}
        for match in re.finditer(r"pub\s+const\s+(\w+):\s*Symbol\s*=\s*symbol_short!\(\"([^\"]+)\"\);", self.topics_source):
            topic_const_to_symbol[match.group(1)] = match.group(2)
            
        # 2. Parse lib.rs to map event name to topic constant name
        # e.g., env.events().publish((TOPIC_INIT,), VaultInitializedEvent { ...
        event_to_symbol = {}
        for match in re.finditer(r"env\.events\(\)\.publish\(\s*\(\s*(\w+)\s*,\s*\)\s*,\s*(\w+Event)", self.source):
            const_name = match.group(1)
            event_name = match.group(2)
            if const_name in topic_const_to_symbol:
                event_to_symbol[event_name] = topic_const_to_symbol[const_name]
                
        return event_to_symbol

    def _get_functions(self) -> List[Dict[str, Any]]:
        """Extract all public functions from contract source."""
        matches = re.finditer(r"pub\s+(?:async\s+)?fn\s+(\w+)", self.source)
        funcs = []
        for match in matches:
            fn_name = match.group(1)
            start_idx = match.end()
            
            # Find matching parentheses for parameters
            open_paren_idx = self.source.find("(", start_idx)
            if open_paren_idx == -1:
                continue
            paren_count = 1
            idx = open_paren_idx + 1
            while paren_count > 0 and idx < len(self.source):
                if self.source[idx] == "(":
                    paren_count += 1
                elif self.source[idx] == ")":
                    paren_count -= 1
                idx += 1
            if paren_count != 0:
                continue
            param_str = self.source[open_paren_idx + 1 : idx - 1]
            
            # Find return type string before opening brace
            body_start_idx = self.source.find("{", idx)
            if body_start_idx == -1:
                return_str = ""
            else:
                return_str = self.source[idx : body_start_idx].strip()
                
            # Parse parameters
            parameters = []
            param_str_clean = re.sub(r"//.*", "", param_str)
            param_str_clean = re.sub(r"\s+", " ", param_str_clean).strip()
            
            params_list = []
            current_param = []
            bracket_level = 0
            for char in param_str_clean:
                if char == "<" or char == "(":
                    bracket_level += 1
                elif char == ">" or char == ")":
                    bracket_level -= 1
                if char == "," and bracket_level == 0:
                    params_list.append("".join(current_param).strip())
                    current_param = []
                else:
                    current_param.append(char)
            if current_param:
                params_list.append("".join(current_param).strip())
                
            for p in params_list:
                if not p or ":" not in p:
                    continue
                p_parts = p.split(":", 1)
                p_name = p_parts[0].strip()
                p_type = p_parts[1].strip()
                
                # Check for documented parameter description in static FUNCTION_METADATA
                p_desc = None
                meta = FUNCTION_METADATA.get(fn_name, {})
                for pm in meta.get("parameters", []):
                    if pm.get("name") == p_name:
                        p_desc = pm.get("description")
                
                p_entry = {"name": p_name, "type": p_type}
                if p_desc:
                    p_entry["description"] = p_desc
                parameters.append(p_entry)
                
            # Parse return type
            returns = None
            if "->" in return_str:
                ret_parts = return_str.split("->", 1)
                returns = ret_parts[1].strip()
                
            # Retrieve static metadata or supply defaults
            meta = FUNCTION_METADATA.get(fn_name, {})
            fn_entry = {
                "name": fn_name,
                "category": meta.get("category", "queries" if fn_name.startswith(("get_", "preview_", "is_", "convert_", "touch_")) else "management"),
                "access": meta.get("access", "public"),
                "description": meta.get("description", f"Function {fn_name}"),
                "parameters": parameters,
                "returns": returns,
                "requires_auth": meta.get("requires_auth", False),
                "state_changing": meta.get("state_changing", not fn_name.startswith(("get_", "preview_", "is_", "convert_", "touch_"))),
            }
            
            # Merge extra keys from metadata
            for key in ["constraints", "events", "formula", "security", "supported_protocols", "authorized_caller", "storage_type", "query_only", "notes", "deprecated"]:
                if key in meta:
                    fn_entry[key] = meta[key]
                    
            funcs.append(fn_entry)
            
        return funcs
    
    def _get_events(self, event_to_symbol: Dict[str, str]) -> List[Dict[str, Any]]:
        """Extract all event definitions from contract source."""
        events = []
        matches = re.finditer(r"pub\s+struct\s+(\w*Event)\s*\{([^}]*)\}", self.source, re.DOTALL)
        for match in matches:
            event_name = match.group(1)
            body = match.group(2)
            
            fields = []
            current_desc = []
            for line in body.split("\n"):
                line = line.strip()
                if line.startswith("///"):
                    current_desc.append(line[3:].strip())
                elif line.startswith("pub ") and ":" in line:
                    parts = line.split(":", 1)
                    name = parts[0].replace("pub ", "").strip()
                    type_part = parts[1].split("//")[0].strip().rstrip(",").strip()
                    desc = " ".join(current_desc) if current_desc else f"Field {name}"
                    fields.append({
                        "name": name,
                        "type": type_part,
                        "description": desc
                    })
                    current_desc = []
                elif line == "" or line.startswith("//"):
                    pass
                else:
                    current_desc = []
            
            topic = event_to_symbol.get(event_name) or FALLBACK_TOPICS.get(event_name) or "unknown"
            
            # Extract doc comments right before the struct definition
            doc_matches = re.findall(
                r"((?:\s*///.*?\n)+)\s*#\[allow\(missing_docs\)\]\s*#\[contracttype\]\s*pub\s+struct\s+" + re.escape(event_name),
                self.source[:match.start()]
            )
            event_desc = "Emitted by the contract."
            if doc_matches:
                lines = [l.strip().replace("///", "").strip() for l in doc_matches[-1].split("\n") if l.strip().startswith("///")]
                event_desc = " ".join(lines)
                
            events.append({
                "name": event_name,
                "topic": topic,
                "description": event_desc,
                "fields": fields
            })
        return events
    
    def _get_errors(self) -> Dict[str, Any]:
        """Extract VaultError enum and descriptions from source."""
        enum_match = re.search(r"pub\s+enum\s+VaultError\s*\{([^}]*)\}", self.source, re.DOTALL)
        if not enum_match:
            return {}
        
        errors = {}
        body = enum_match.group(1)
        
        lines = body.split("\n")
        current_desc = []
        for line in lines:
            line = line.strip()
            if line.startswith("///"):
                current_desc.append(line[3:].strip())
            elif "=" in line and "," in line:
                parts = line.split("=", 1)
                name = parts[0].strip()
                code_part = parts[1].split(",")[0].strip()
                desc = " ".join(current_desc) if current_desc else f"Error code {code_part}"
                errors[f"VaultError::{name}"] = {
                    "code": int(code_part),
                    "description": desc
                }
                current_desc = []
            elif line == "" or line.startswith("//"):
                pass
            else:
                current_desc = []
                
        # Add non-VaultError standard errors
        static_errors = {
            "ValidationError": {"code": 100, "description": "General validation error"},
            "PausedError": {"code": 101, "description": "Vault is paused, deposits and withdrawals disabled"},
            "UnauthorizedAgentError": {"code": 102, "description": "Only authorized AI agent can call this function"},
            "UnauthorizedOwnerError": {"code": 103, "description": "Only contract owner can call this function"},
            "InsufficientBalanceError": {"code": 104, "description": "User has insufficient balance for withdrawal"},
            "InvalidAmountError": {"code": 105, "description": "Amount is invalid (zero, negative, or outside limits)"},
            "DepositCapExceededError": {"code": 106, "description": "User deposit cap exceeded"},
            "TvlCapExceededError": {"code": 107, "description": "Total value locked cap exceeded"},
            "SlippageError": {"code": 108, "description": "Output less than minimum expected (slippage protection)"}
        }
        errors.update(static_errors)
        return errors
    
    def _get_types(self) -> Dict[str, Any]:
        """Get custom type definitions."""
        return {
            "RateLimitConfig": {
                "description": "Owner-configured fixed-window call allowance",
                "fields": [
                    {"name": "max_calls", "type": "u32", "description": "Maximum accepted calls per window"},
                    {"name": "window_ledgers", "type": "u32", "description": "Window length in ledgers"}
                ]
            },
            "RateLimitState": {
                "description": "Current usage of a fixed-window rate-limit bucket",
                "fields": [
                    {"name": "window_start", "type": "u32", "description": "Window start ledger"},
                    {"name": "calls", "type": "u32", "description": "Accepted calls in the current window"}
                ]
            },
            "UserInfo": {
                "description": "Complete user information snapshot",
                "fields": [
                    {
                        "name": "address",
                        "type": "Address",
                        "description": "User wallet address"
                    },
                    {
                        "name": "balance",
                        "type": "i128",
                        "description": "USDC balance in vault (principal)"
                    },
                    {
                        "name": "shares",
                        "type": "i128",
                        "description": "Vault shares owned"
                    },
                    {
                        "name": "deposit_time",
                        "type": "u64",
                        "description": "Timestamp of first deposit"
                    }
                ]
            },
            "Address": {
                "description": "Stellar account address",
                "format": "G..."
            },
            "Symbol": {
                "description": "Fixed-length Soroban symbol",
                "examples": ["blend", "none"]
            },
            "i128": {
                "description": "128-bit signed integer",
                "notes": ["USDC amounts use 7 decimal places"]
            }
        }
    
    def _get_constants(self) -> Dict[str, Any]:
        """Extract constants from contract."""
        return {
            "DEFAULT_USER_DEPOSIT_CAP": {
                "value": "10_000_000_000",
                "description": "Default per-user deposit cap: 10,000 USDC",
                "type": "i128"
            },
            "DEFAULT_MIN_DEPOSIT": {
                "value": "1_000_000",
                "description": "Default minimum deposit: 1 USDC",
                "type": "i128"
            },
            "DEFAULT_MAX_DEPOSIT": {
                "value": "1_000_000_000",
                "description": "Default maximum deposit: 1,000 USDC",
                "type": "i128"
            },
            "DECIMAL_PLACES": {
                "value": 7,
                "description": "USDC has 7 decimal places on Stellar",
                "type": "u32"
            }
        }


def main():
    """Main entry point."""
    contract_path = "neurowealth-vault/contracts/vault/src/lib.rs"
    topics_path = "neurowealth-vault/contracts/vault/src/topics.rs"
    output_path = "contract-spec.json"
    
    try:
        generator = ContractSpecGenerator(contract_path, topics_path)
        spec = generator.generate()
        
        # Write spec to file
        with open(output_path, "w") as f:
            json.dump(spec, f, indent=2)
        
        print(f"✅ Contract specification generated: {output_path}")
        print(f"   - {len(spec['functions'])} functions")
        print(f"   - {len(spec['events'])} events")
        print(f"   - {len(spec['errors'])} error types")
        
    except FileNotFoundError as e:
        print(f"❌ Error: {e}", file=__import__('sys').stderr)
        exit(1)
    except Exception as e:
        print(f"❌ Failed to generate spec: {e}", file=__import__('sys').stderr)
        exit(1)


if __name__ == "__main__":
    main()
