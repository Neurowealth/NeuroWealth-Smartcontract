/// A partial `withdraw()` call that happens to drain the entire Blend
/// position (i.e. the amount pulled from Blend exactly matches the
/// outstanding Blend balance) must update CurrentProtocol to "none",
/// even though the user only requested part of their own shares.
///
/// This is distinct from `test_integration_withdraw_all_updates_current_protocol_to_none`:
/// it exercises the `withdraw` entry point (not `withdraw_all`), which goes
/// through a different reconciliation path before delegating to
/// `withdraw_from_blend`.
#[test]
fn test_integration_withdraw_updates_current_protocol_to_none() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let deposit_amount = 25_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Move all funds into Blend so the vault itself holds zero idle USDC.
    client.rebalance(&symbol_short!("blend"), &800_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));
    assert_eq!(blend_client.supplied(&usdc_token), deposit_amount);
    assert_eq!(vault_usdc_balance(&env, &usdc_token, &contract_id), 0);

    // User withdraws their entire entitled amount via `withdraw` (not
    // `withdraw_all`). Since the vault has zero idle balance, the full
    // amount must be pulled from Blend, leaving exactly zero remaining.
    client.withdraw(&user, &deposit_amount, &None);

    // Vault should have pulled everything out of Blend.
    assert_eq!(blend_client.supplied(&usdc_token), 0);

    // CurrentProtocol must reset to "none" as part of the same withdrawal,
    // with no separate rebalance call required.
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("none"),
        "CurrentProtocol should switch to 'none' when a partial withdraw() fully drains Blend"
    );
}

/// A `withdraw()` that only partially drains the Blend position must leave
/// CurrentProtocol untouched ("blend"), since funds are still deployed.
/// This guards against an overly-eager reset that flips CurrentProtocol to
/// "none" any time `withdraw_from_blend` is invoked, regardless of whether
/// the position was actually fully exited.
#[test]
fn test_integration_withdraw_partial_blend_drain_keeps_current_protocol() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let deposit_amount = 25_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Move all funds into Blend so the vault itself holds zero idle USDC.
    client.rebalance(&symbol_short!("blend"), &800_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));

    // Withdraw less than the full position; Blend should still hold a
    // residual balance afterward.
    let partial_amount = 10_000_000_i128;
    client.withdraw(&user, &partial_amount, &None);

    assert!(
        blend_client.supplied(&usdc_token) > 0,
        "Blend should still hold a residual position after a partial withdrawal"
    );
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("blend"),
        "CurrentProtocol must remain 'blend' while funds are still deployed"
    );
}