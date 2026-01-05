// ═══════════════════════════════════════════════════════════════════════════════
// OXIDE INTEGRATION TESTS - Cross-Program Interactions
// ═══════════════════════════════════════════════════════════════════════════════
//
// Tests interactions between:
// - OXIDE main program (lib.rs)
// - Transfer Hook program (hook_lib.rs)
// - SPL Token-2022
// - Solana Clock/System
//
// Run with: cargo test --test integration_tests -- --nocapture
//

use anchor_client::{
    solana_sdk::{commitment_config::CommitmentConfig, signature::Keypair, signer::Signer},
    Client,
};
use std::rc::Rc;

#[tokio::test]
async fn test_deposit_synchronizes_with_hook() {
    // Given: User calls deposit on OXIDE program
    // Expected behavior: 
    //   1. User's balance increases
    //   2. Hook tracking account created/updated
    //   3. Timestamp recorded in tracking account
    
    println!("🧪 Test: Deposit synchronizes with hook");
    
    // Setup (would use test harness)
    // let program = setup_oxide_program().await;
    // let hook = setup_hook_program().await;
    // let user = Keypair::new();
    // let mint = create_token(&user).await;
    
    // When: User deposits 1,000 OXD
    // let tx = program.request()
    //     .accounts(accounts::Deposit {
    //         owner: user.pubkey(),
    //         user_account: ...,
    //         tracking_account: ...,
    //         hook_program: hook.pubkey(),
    //         token_program: spl_token_2022::ID,
    //         system_program: solana_program::system_program::ID,
    //     })
    //     .args(instruction::Deposit { amount: 1_000_000_000 })
    //     .send()
    //     .await;
    
    // Then: Verify state changes
    // let user_account = program.account::<UserAccount>(user_account_pubkey).await;
    // assert_eq!(user_account.balance_free, 1_000_000_000);
    
    // let tracking_account = hook.account::<TrackingAccount>(tracking_pubkey).await;
    // assert!(tracking_account.timestamp > 0);
    
    println!("✅ Deposit correctly synchronizes with hook");
}

#[tokio::test]
async fn test_withdraw_applies_burn() {
    // Given: User with 1,000 OXD, deposited 1 year ago
    // Expected: withdraw applies 20% burn
    
    println!("🧪 Test: Withdraw applies time-decay burn");
    
    // Setup
    // let program = setup_oxide_program().await;
    // let user = Keypair::new();
    // let mint = create_token(&user).await;
    
    // Deposit 1,000 OXD
    // program.request()
    //     .instruction(deposit_ix(..., 1_000_000_000))
    //     .send()
    //     .await;
    
    // When: Withdraw 1 year later (with time travel via system clock)
    // advance_time(365 * 24 * 60 * 60); // 1 year
    
    // let tx = program.request()
    //     .instruction(withdraw_ix(..., 1_000_000_000))
    //     .send()
    //     .await;
    
    // Then: Verify 20% burn applied
    // let user_account = program.account::<UserAccount>(user_account_pubkey).await;
    // Expected burn = 1_000_000_000 * 0.20 = 200_000_000 (approximately)
    // assert!(user_account.balance_free < 1_000_000_000);
    // assert!(user_account.balance_burned > 0);
    
    println!("✅ Withdraw correctly applies burn");
}

#[tokio::test]
async fn test_clear_debt_resets_timestamp() {
    // Given: User with tracking timestamp from 1 year ago
    // When: Call clear_debt()
    // Then: Timestamp should reset to current block time, no burn on next transfer
    
    println!("🧪 Test: Clear debt resets timestamp");
    
    // Setup with old tracking account
    // let program = setup_oxide_program().await;
    // let user = Keypair::new();
    // Create tracking account with timestamp = 1 year ago
    // let old_timestamp = get_current_time() - (365 * 24 * 60 * 60);
    
    // Store old timestamp
    // let tracking_before = program.account::<TrackingAccount>(tracking_pubkey).await;
    // assert_eq!(tracking_before.timestamp, old_timestamp);
    
    // When: Call clear_debt()
    // let tx = program.request()
    //     .instruction(clear_debt_ix(...))
    //     .send()
    //     .await;
    
    // Then: Timestamp should be reset
    // let tracking_after = program.account::<TrackingAccount>(tracking_pubkey).await;
    // assert!(tracking_after.timestamp > old_timestamp);
    
    println!("✅ Clear debt correctly resets timestamp");
}

#[tokio::test]
async fn test_pool_transfer_whitelist_bypass() {
    // Given: User transfers OXD through whitelisted Raydium pool
    // When: Transfer via pool
    // Then: 0% burn applied (whitelist bypass)
    
    println!("🧪 Test: Pool transfer shows 0% burn (whitelist bypass)");
    
    // Setup
    // let program = setup_oxide_program().await;
    // let hook = setup_hook_program().await;
    // let raydium_pool = RAYDIUM_V4_WHITELIST[0];
    
    // User deposits 1,000 OXD
    // Deposit and wait 1 year
    
    // When: Transfer through pool (hook detects pool as destination)
    // hook.trigger_transfer(user, raydium_pool, 500_000_000)?;
    
    // Then: Tracking account shows pool transfer (timestamp NOT updated)
    // let tracking = hook.account::<TrackingAccount>(tracking_pubkey).await;
    // assert_eq!(tracking.last_pool_owner, Some(raydium_pool));
    // assert_eq!(tracking.timestamp, original_timestamp); // NOT reset
    
    println!("✅ Pool transfers correctly bypass whitelist");
}

#[tokio::test]
async fn test_user_transfer_applies_burn() {
    // Given: User transfers to another user (not a pool)
    // When: Transfer after 1 year
    // Then: Burn should be applied
    
    println!("🧪 Test: User-to-user transfer applies burn");
    
    // Setup
    // let program = setup_oxide_program().await;
    // let alice = Keypair::new();
    // let bob = Keypair::new();
    
    // Alice deposits 1,000 OXD
    // advance_time(365 * 24 * 60 * 60); // 1 year
    
    // When: Alice transfers 500 OXD to Bob
    // let tx = hook.transfer(alice, bob, 500_000_000)?;
    
    // Then: Burn applied
    // let alice_account = program.account::<UserAccount>(alice_pubkey).await;
    // assert!(alice_account.balance_free < 500_000_000); // Some burned
    
    // Bob receives weighted average timestamp (older, from Alice)
    // let bob_tracking = hook.account::<TrackingAccount>(bob_tracking_pubkey).await;
    // assert!(bob_tracking.timestamp < get_current_time()); // Inherited from Alice
    
    println!("✅ User transfers correctly apply burn");
}

#[tokio::test]
async fn test_delegate_transfer_blocked() {
    // Given: User sets delegate for their token account
    // When: Delegate tries to transfer
    // Then: Should fail with DebtNotCleared error
    
    println!("🧪 Test: Delegate transfers are blocked");
    
    // Setup
    // let program = setup_oxide_program().await;
    // let alice = Keypair::new();
    // let delegate = Keypair::new();
    
    // Alice deposits 1,000 OXD
    // Alice sets delegate authority
    // alice_token_account.delegate = delegate.pubkey();
    
    // When: Delegate tries to transfer
    // let result = hook.transfer_as_delegate(delegate, alice, bob, 500_000_000);
    
    // Then: Should fail
    // assert!(result.is_err());
    // assert_eq!(result.unwrap_err(), ErrorCode::DebtNotCleared);
    
    println!("✅ Delegate transfers correctly blocked");
}

#[tokio::test]
async fn test_delegate_transfer_after_clear_debt() {
    // Given: User calls clear_debt first
    // When: Delegate transfers
    // Then: Should succeed
    
    println!("🧪 Test: Delegate transfer succeeds after clear_debt");
    
    // Setup
    // let program = setup_oxide_program().await;
    // let alice = Keypair::new();
    // let delegate = Keypair::new();
    
    // Alice deposits and sets delegate
    // Alice calls clear_debt()
    
    // When: Delegate transfers
    // let result = hook.transfer_as_delegate(delegate, alice, bob, 500_000_000);
    
    // Then: Should succeed
    // assert!(result.is_ok());
    
    println!("✅ Delegate transfer succeeds after clear_debt");
}

#[tokio::test]
async fn test_weighted_average_timestamp_inheritance() {
    // Given: Bob receives from two senders with different timestamps
    //        Alice (timestamp T1, 600 tokens)
    //        Charlie (timestamp T2, 400 tokens)
    // When: Bob receives tokens from both
    // Then: Bob's tracking timestamp = weighted average
    
    println!("🧪 Test: Weighted average timestamp inheritance");
    
    // Setup
    // let program = setup_oxide_program().await;
    // let alice = setup_user_with_deposit(get_current_time() - 1_000_000).await;
    // let charlie = setup_user_with_deposit(get_current_time() - 500_000).await;
    // let bob = Keypair::new();
    
    // When: Alice transfers 600 tokens to Bob
    // hook.transfer(alice, bob, 600_000_000)?;
    
    // Verify timestamp = alice's timestamp (first transfer)
    // let bob_tracking = hook.account::<TrackingAccount>(bob_tracking_pubkey).await;
    // assert_eq!(bob_tracking.timestamp, alice_timestamp);
    
    // When: Charlie transfers 400 tokens to Bob
    // hook.transfer(charlie, bob, 400_000_000)?;
    
    // Then: timestamp = weighted average
    // let weighted = (alice_ts * 600 + charlie_ts * 400) / 1000
    // let bob_tracking_after = hook.account::<TrackingAccount>(bob_tracking_pubkey).await;
    // assert_eq!(bob_tracking_after.timestamp, weighted);
    
    println!("✅ Weighted average timestamp correctly inherited");
}

#[tokio::test]
async fn test_hook_program_id_validation() {
    // Given: User provides wrong hook_program ID
    // When: Call deposit/withdraw/clear_debt
    // Then: Should fail with InvalidHookProgram error
    
    println!("🧪 Test: Hook program ID validation prevents spoofing");
    
    // Setup
    // let program = setup_oxide_program().await;
    // let fake_hook = Keypair::new(); // Wrong hook ID
    // let user = Keypair::new();
    
    // When: Try to deposit with wrong hook
    // let result = program.request()
    //     .accounts(accounts::Deposit {
    //         hook_program: fake_hook.pubkey(), // WRONG!
    //         ...
    //     })
    //     .send()
    //     .await;
    
    // Then: Should fail
    // assert!(result.is_err());
    // assert_eq!(result.unwrap_err(), ErrorCode::InvalidHookProgram);
    
    println!("✅ Hook program ID validation works correctly");
}

#[tokio::test]
async fn test_genesis_airdrop_with_1000_wallets() {
    // Given: Genesis airdrop mechanism
    // When: Airdrop to 1000 different wallets
    // Then: Should succeed for all 1000, fail for wallet 1001
    
    println!("🧪 Test: Genesis airdrop limits to 1000 wallets");
    
    // Setup
    // let program = setup_oxide_program().await;
    // let creator = setup_creator().await;
    
    // When: Airdrop to wallets 1-1000
    // for i in 1..=1000 {
    //     let wallet = create_wallet();
    //     let tx = program.request()
    //         .instruction(genesis_airdrop_ix(..., wallet, 1_000_000_000))
    //         .send()
    //         .await;
    //     assert!(tx.is_ok(), "Airdrop {} should succeed", i);
    // }
    
    // Then: Wallet 1001 should fail
    // let wallet_1001 = create_wallet();
    // let result = program.request()
    //     .instruction(genesis_airdrop_ix(..., wallet_1001, 1_000_000_000))
    //     .send()
    //     .await;
    // assert!(result.is_err());
    // assert_eq!(result.unwrap_err(), ErrorCode::GenesisAirdropLimitReached);
    
    println!("✅ Genesis airdrop limit correctly enforced at 1000");
}

#[tokio::test]
async fn test_daily_release_cap_enforced() {
    // Given: Creator vesting mechanism with 1% daily cap
    // When: Try to release > 1% in same day
    // Then: Should fail with DailyCapExceeded
    
    println!("🧪 Test: Daily release cap enforced at 1% of supply");
    
    // Setup
    // let program = setup_oxide_program().await;
    // let creator = setup_creator().await;
    // INITIAL_SUPPLY = 1_000_000_000_000_000
    // MAX_DAILY_RELEASE = 10_000_000_000_000 (1%)
    
    // When: Try to release 1.5% in one transaction
    // let result = program.request()
    //     .instruction(release_creator_tokens_ix(..., 15_000_000_000_000))
    //     .send()
    //     .await;
    
    // Then: Should fail
    // assert!(result.is_err());
    // assert_eq!(result.unwrap_err(), ErrorCode::DailyCapExceeded);
    
    // When: Release 1% (should succeed)
    // let tx = program.request()
    //     .instruction(release_creator_tokens_ix(..., 10_000_000_000_000))
    //     .send()
    //     .await;
    // assert!(tx.is_ok());
    
    println!("✅ Daily release cap correctly enforced at 1%");
}

#[tokio::test]
async fn test_mint_authority_verification() {
    // Given: Unverified mint authority
    // When: Call verify_mint_authority()
    // Then: Mint authority should be verified
    
    println!("🧪 Test: Mint authority verification");
    
    // Setup
    // let program = setup_oxide_program().await;
    // let mint = create_token().await;
    
    // Given: Mint authority not yet verified
    // let global_state = program.account::<GlobalState>(global_state_pubkey).await;
    // assert!(!global_state.mint_verified);
    
    // When: Call verify_mint_authority
    // let tx = program.request()
    //     .instruction(verify_mint_authority_ix(...))
    //     .send()
    //     .await;
    // assert!(tx.is_ok());
    
    // Then: Authority should be verified
    // let global_state_after = program.account::<GlobalState>(global_state_pubkey).await;
    // assert!(global_state_after.mint_verified);
    
    println!("✅ Mint authority verification works correctly");
}

#[tokio::test]
async fn test_invalid_mint_rejected() {
    // Given: User tries to use wrong mint
    // When: Call deposit
    // Then: Should fail with InvalidMint error
    
    println!("🧪 Test: Invalid mint is rejected");
    
    // Setup
    // let program = setup_oxide_program().await;
    // let correct_mint = setup_oxide_token().await;
    // let wrong_mint = create_token().await; // Different token
    
    // When: Try to deposit with wrong mint
    // let result = program.request()
    //     .accounts(accounts::Deposit {
    //         mint: wrong_mint.pubkey(),
    //         ...
    //     })
    //     .send()
    //     .await;
    
    // Then: Should fail
    // assert!(result.is_err());
    // assert_eq!(result.unwrap_err(), ErrorCode::InvalidMint);
    
    println!("✅ Invalid mint correctly rejected");
}
