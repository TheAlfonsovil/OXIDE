═══════════════════════════════════════════════════════════════════════════════
  OXIDE TEST SUITE DOCUMENTATION
═══════════════════════════════════════════════════════════════════════════════

OVERVIEW
────────────────────────────────────────────────────────────────────────────────

Four test files covering different scopes:

1. unit_tests.rs           - Core logic verification (no blockchain)
2. integration_tests.rs    - Cross-program interactions (local validator)
3. edge_case_tests.rs      - Boundary conditions & extreme values (CRITICAL)
4. devnet_tests.rs         - Live DEVNET cluster testing (external)


═══════════════════════════════════════════════════════════════════════════════
  1. UNIT TESTS (unit_tests.rs)
═══════════════════════════════════════════════════════════════════════════════

Purpose: Test pure Rust logic without Solana blockchain
Scope: Burn calculations, timestamps, fixed-point math, error conditions
Speed: Very fast (< 1 second for all tests)

Command:
  $ cargo test --test unit_tests -- --nocapture

Coverage:
  ✅ BURN CALCULATION TESTS
     - test_burn_calculation_basic()              → 1,000 OXD, 1 year, 20% burn
     - test_burn_calculation_zero_elapsed()       → Zero burn with zero time
     - test_burn_calculation_one_day()            → Fractional burn (1/365)
     - test_burn_remainder_accumulation()         → Fixed-point remainder handling
     - test_burn_with_max_u64_balance()           → u64::MAX balance safety
     - test_burn_with_fractional_seconds()        → Sub-minute precision

  ✅ TIMESTAMP TRACKING TESTS
     - test_weighted_average_timestamp_single_transfer()
     - test_weighted_average_timestamp_two_senders()
     - test_weighted_average_timestamp_unequal_split()
     - test_timestamp_reset_after_clear_debt()

  ✅ FIXED-POINT ARITHMETIC TESTS
     - test_fixed_point_scale_conversion()
     - test_fixed_point_remainder_overflow()
     - test_fixed_point_precision_loss()

  ✅ GRACE PERIOD TESTS
     - test_grace_period_not_elapsed()            → 5 min < 15 min grace
     - test_grace_period_at_boundary()            → Exactly 15 min
     - test_grace_period_exceeded()               → 20 min > 15 min grace

  ✅ ERROR CONDITION TESTS
     - test_insufficient_funds_detection()
     - test_mint_authority_validation()
     - test_invalid_hook_program_id()
     - test_owner_mismatch_detection()

  ✅ SUPPLY CAP TESTS
     - test_initial_supply_constant()             → 1 trillion OXD
     - test_daily_release_cap_calculation()       → 1% per day
     - test_daily_release_cap_exceeded()
     - test_genesis_airdrop_limit()               → 1000 wallets max
     - test_genesis_airdrop_limit_exceeded()

  ✅ CLOCK/TIME TESTS
     - test_seconds_per_year_constant()
     - test_seconds_per_day_constant()
     - test_clock_regression_handling()           → Clock goes backward (rare)
     - test_timestamp_in_future()


═══════════════════════════════════════════════════════════════════════════════
  2. INTEGRATION TESTS (integration_tests.rs)
═══════════════════════════════════════════════════════════════════════════════

Purpose: Test full program interactions via anchor_client (local validator)
Scope: Multi-program calls, state consistency, hook synchronization
Speed: Medium (5-10 seconds per test)

Prerequisites:
  - Run local validator: $ solana-test-validator
  - Or use: $ anchor test (includes auto-start)

Command:
  $ anchor test --skip-build

Coverage:
  ✅ DEPOSIT SYNCHRONIZATION
     - test_deposit_synchronizes_with_hook()
       → Deposit updates user balance AND hook tracking account

  ✅ BURN ON WITHDRAWAL
     - test_withdraw_applies_burn()
       → Withdrawing 1 year later = 20% burn applied

  ✅ CLEAR DEBT FUNCTIONALITY
     - test_clear_debt_resets_timestamp()
       → Timestamp reset prevents future burns

  ✅ POOL TRANSFER WHITELIST
     - test_pool_transfer_whitelist_bypass()
       → Raydium/Orca/Meteora transfers = 0% burn
     - test_user_transfer_applies_burn()
       → User-to-user transfers = burn applied

  ✅ DELEGATE TRANSFER BLOCKING
     - test_delegate_transfer_blocked()
       → Delegates fail with DebtNotCleared (unless > 15 min grace)
     - test_delegate_transfer_after_clear_debt()
       → Works after clear_debt() called

  ✅ WEIGHTED AVERAGE TIMESTAMP
     - test_weighted_average_timestamp_inheritance()
       → Bob receives from Alice (600) + Charlie (400)
       → Bob's timestamp = weighted average

  ✅ SECURITY: HOOK PROGRAM VALIDATION
     - test_hook_program_id_validation()
       → Wrong hook_program ID rejected with InvalidHookProgram

  ✅ GENESIS AIRDROP LIMITS
     - test_genesis_airdrop_with_1000_wallets()
       → Succeeds 1-1000, fails at 1001

  ✅ VESTING CAPS
     - test_daily_release_cap_enforced()
       → Creator can only release 1% per day

  ✅ MINT AUTHORITY
     - test_mint_authority_verification()
       → verify_mint_authority() correctly marks verified
     - test_invalid_mint_rejected()
       → Wrong mint rejected with InvalidMint error


═══════════════════════════════════════════════════════════════════════════════
  3. EDGE CASE TESTS (edge_case_tests.rs) 🔴 MOST CRITICAL
═══════════════════════════════════════════════════════════════════════════════

Purpose: Verify behavior at extreme boundaries and rare conditions
Scope: Overflow/underflow, boundary values, rare edge cases
Speed: Fast (< 1 second each)
Priority: RUN THESE FIRST - Most likely to find bugs

Command:
  $ cargo test --test edge_case_tests -- --nocapture --test-threads=1

Critical Edge Cases:

  🔴 ARITHMETIC OVERFLOW TESTS
     - edge_case_max_u64_balance()
       → u64::MAX balance with 1-year burn calculation
       → Verify u128 intermediate prevents overflow
       → Confirm result < balance

     - edge_case_elapsed_negative_clock_regression()
       → Clock goes backward (Solana can regress during issues)
       → Elapsed becomes NEGATIVE
       → Must handle gracefully (no panic, elapsed = 0)
       → This is CRITICAL - could crash system

  🟠 BOUNDARY VALUE TESTS
     - edge_case_elapsed_time_zero()
       → Same-block transfer (elapsed = 0)
       → Must be zero burn

     - edge_case_one_second_elapsed()
       → Minimal time (1 second)
       → Verify fractional burn calculated

     - edge_case_ten_years_elapsed()
       → Extreme elapsed time
       → Verify burn compounds (but check capping)

     - edge_case_grace_period_boundary()
       → Exactly at 15-minute mark
       → Verify boundary is exclusive: elapsed < 900 (not <=)

     - edge_case_timestamp_overflow()
       → Tracking timestamp near u64::MAX
       → Current time = u64::MAX
       → Subtraction must not overflow

     - edge_case_genesis_airdrop_exactly_1000()
       → Wallet 1000: succeeds
       → Wallet 1001: rejected
       → Boundary enforcement

     - edge_case_daily_release_cap_exactly_1_percent()
       → Release exactly 1%: succeeds
       → Release 1% + 1 wei: rejected

  🟡 ZERO/MINIMUM VALUE TESTS
     - edge_case_zero_balance_burn()
       → Balance = 0
       → Burn = 0 (safe)

     - edge_case_one_unit_balance()
       → Balance = 1 (0.000001 OXD with 6 decimals)
       → Verify fixed-point handles minimal values

     - edge_case_all_balance_transferred_zero_remaining()
       → Transfer 100% of balance
       → No dust remains

  🟡 ACCUMULATION TESTS
     - edge_case_remainder_accumulation_overflow()
       → Simulate 1M fractional burns
       → Verify u128 scale is sufficient
       → Verify no loss of precision

     - edge_case_weighted_average_one_wei_difference()
       → Weighted average with 1 unit of difference
       → Verify fixed-point precision

  🔴 IMMUTABILITY & SECURITY TESTS
     - edge_case_hook_program_id_immutable_validation()
       → HOOK_PROGRAM_ID cannot be changed
       → Hardcoded constant checked at tx time
       → No bypass possible (enforced by Anchor)

     - edge_case_mint_authority_pda_derivation()
       → Mint authority MUST be program's PDA
       → Random pubkey rejected

     - edge_case_transfer_hook_invocation_consistency()
       → Transfer hook is MANDATORY for ALL transfers
       → Cannot be bypassed
       → Enforced by SPL-2022 runtime


Expected Results for All Edge Case Tests:
✅ No panics
✅ No overflows/underflows
✅ Boundary conditions handled correctly
✅ Results within expected ranges
✅ Security checks enforced


═══════════════════════════════════════════════════════════════════════════════
  4. DEVNET TESTS (devnet_tests.rs)
═══════════════════════════════════════════════════════════════════════════════

Purpose: Test against LIVE DEVNET cluster (external validation)
Scope: RPC connectivity, actual program execution, real token behavior
Speed: Medium (10-60 seconds per test, depends on network)
Importance: Final validation before mainnet

Prerequisites:
  - Programs deployed to DEVNET (solana-keygen + anchor deploy)
  - Token created with transfer hook enabled
  - Sufficient DEVNET SOL in test wallets

Command (run specific test):
  $ CLUSTER=devnet cargo test devnet_cluster_connectivity -- --nocapture --ignored

Command (run all DEVNET tests):
  $ CLUSTER=devnet cargo test --test devnet_tests -- --nocapture --ignored --test-threads=1

Test Categories:

  🔗 CONNECTIVITY TESTS
     - devnet_cluster_connectivity()
       → Can connect to https://api.devnet.solana.com
       → Get latest blockhash

     - devnet_program_account_exists()
       → OXIDE program deployed and executable
       → Check Owner and Executable flags

     - devnet_hook_program_account_exists()
       → Hook program deployed and executable

     - devnet_token_created_with_hook()
       → OXD token exists
       → Transfer hook extension enabled

  📋 INSTRUCTION EXECUTION TESTS
     - devnet_deposit_instruction()
       → User can deposit OXD

     - devnet_withdraw_instruction()
       → User can withdraw OXD

     - devnet_clear_debt_instruction()
       → User can clear debt and reset timestamp

     - devnet_invalid_hook_program_rejected()
       → Wrong hook_program ID rejected

  🪝 TRANSFER HOOK TESTS
     - devnet_transfer_triggers_hook()
       → Every OXD transfer triggers hook

     - devnet_tracking_account_created_on_deposit()
       → Hook creates TrackingAccount with timestamp

     - devnet_pool_transfer_shows_zero_decay()
       → Raydium/Orca/Meteora = 0% decay

     - devnet_user_transfer_applies_burn()
       → User-to-user = burn applied

     - devnet_delegate_transfer_blocked()
       → Delegates fail (unless clear_debt called)

  🏊 POOL INTEGRATION TESTS
     - devnet_raydium_pool_integration()
     - devnet_orca_pool_integration()
     - devnet_meteora_pool_integration()
     - devnet_non_whitelisted_pool_applies_decay()

  ⚙️ COMPUTE UNIT TESTS
     - devnet_monitor_deposit_cu_usage()        → Target: ~170k CU
     - devnet_monitor_withdraw_cu_usage()       → Target: ~180k CU
     - devnet_monitor_clear_debt_cu_usage()     → Target: ~160k CU
     - devnet_monitor_transfer_hook_cu_usage()  → Target: ~150k CU

  ⚡ LOAD TESTS
     - devnet_concurrent_deposits()             → 10 users parallel
     - devnet_sequential_transfers_consistency() → 100 sequential transfers

  🎁 VESTING & GOVERNANCE TESTS
     - devnet_genesis_airdrop_limit_enforced()
     - devnet_daily_release_cap_enforced()
     - devnet_creator_cannot_unstake()

  💯 STATE CONSISTENCY TESTS
     - devnet_verify_no_balance_loss()          → Conservation law
     - devnet_verify_no_duplicate_tracking_accounts()
     - devnet_verify_remainder_accumulation()

  🧪 FULL SMOKE TEST
     - devnet_full_smoke_test()
       → Runs all critical tests
       → Reports overall readiness


═══════════════════════════════════════════════════════════════════════════════
  TEST EXECUTION PLAN
═══════════════════════════════════════════════════════════════════════════════

PHASE 1: LOCAL VALIDATION (Before DEVNET)
────────────────────────────────────────────────────────────────────────────────
1. Unit tests
   $ cargo test --test unit_tests -- --nocapture
   Expected: All pass (< 1 second)

2. Edge case tests
   $ cargo test --test edge_case_tests -- --nocapture --test-threads=1
   Expected: All pass (< 10 seconds), CRITICAL validation

3. Integration tests (with local validator)
   $ anchor test --skip-build
   Expected: All pass (10-30 seconds)


PHASE 2: DEVNET VALIDATION (After deployment)
────────────────────────────────────────────────────────────────────────────────
1. Setup DEVNET
   $ solana config set --url https://api.devnet.solana.com
   $ solana airdrop 10 [YOUR_WALLET]

2. Deploy programs
   $ solana program deploy --program-id hook-program.json target/deploy/oxide_transfer_hook.so
   $ solana program deploy --program-id oxide-program.json target/deploy/oxide.so

3. Create token with hook
   $ spl-token create-token --program-id TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPvLZ8 --transfer-hook [HOOK_ID]

4. Run DEVNET smoke tests
   $ CLUSTER=devnet cargo test devnet_full_smoke_test -- --ignored --nocapture

5. Run full DEVNET suite
   $ CLUSTER=devnet cargo test --test devnet_tests -- --ignored --nocapture --test-threads=1


PHASE 3: PRODUCTION READINESS
────────────────────────────────────────────────────────────────────────────────
If all tests pass:
✅ OXIDE is ready for mainnet deployment

Required documentation:
- OXIDE_PROGRAM_ID
- HOOK_PROGRAM_ID
- OXD_MINT_ID
- Build timestamp
- Git commit hash
- Test results summary


═══════════════════════════════════════════════════════════════════════════════
  TESTING CHECKLIST
═══════════════════════════════════════════════════════════════════════════════

Before DEVNET deployment, verify:

UNIT TESTS:
☐ All burn calculations correct (basic, zero, one day, max u64, etc.)
☐ All timestamp logic correct (weighted average, reset, tracking)
☐ Grace period boundary handling (14:59, 15:00, 15:01)
☐ Fixed-point arithmetic safe (remainder accumulation)
☐ Supply caps enforced (genesis, daily release)
☐ Error detection working (insufficient funds, invalid auth, invalid mint)

EDGE CASE TESTS:
☐ No overflow on max u64 balance
☐ Clock regression handled (elapsed < 0)
☐ Zero elapsed time = zero burn
☐ Timestamp overflow safe
☐ Genesis airdrop boundary (1000 exact, 1001 rejected)
☐ Daily release boundary (1% exact, 1%+1 rejected)

INTEGRATION TESTS:
☐ Deposit syncs with hook
☐ Withdraw applies burn correctly
☐ Clear debt resets timestamp
☐ Pool transfers bypass (0% burn)
☐ User transfers apply burn
☐ Delegate transfers blocked
☐ Weighted average timestamps inherit correctly
☐ Hook program ID validation working
☐ Mint authority verification working

DEVNET TESTS:
☐ Programs deployed and executable
☐ Token created with hook enabled
☐ All transactions executable
☐ CU usage < 200k per instruction
☐ Pool integrations working (Raydium, Orca, Meteora)
☐ Concurrent and sequential operations consistent
☐ Vesting caps enforced
☐ No balance loss (conservation law)


═══════════════════════════════════════════════════════════════════════════════
  DEBUGGING COMMON TEST FAILURES
═══════════════════════════════════════════════════════════════════════════════

❌ "test_burn_with_max_u64_balance panics"
→ Fix: Ensure u128 intermediate in burn calculation
→ Code: let balance_u128 = balance as u128;

❌ "test_clock_regression_handling fails"
→ Fix: Handle negative elapsed (if elapsed < 0 { elapsed = 0 })
→ This edge case kills mainnet if not handled

❌ "test_delegate_transfer_blocked fails"
→ Fix: Verify hook checks elapsed time before allowing delegate transfer

❌ "devnet_monitor_deposit_cu_usage exceeds 200k"
→ Fix: Optimize code, consider splitting complex operations

❌ "devnet_pool_transfer_shows_zero_decay fails"
→ Fix: Verify pool address is in whitelist
→ Check: is_whitelisted_owner_program() implementation


═══════════════════════════════════════════════════════════════════════════════
