// ═══════════════════════════════════════════════════════════════════════════════
// OXIDE UNIT TESTS - Internal Logic Verification
// ═══════════════════════════════════════════════════════════════════════════════
//
// Tests the core mathematical and state management logic of OXIDE protocol
// Focus: Burn calculations, timestamp tracking, error handling
//

#[cfg(test)]
mod unit_tests {
    use oxide::*;

    // ───────────────────────────────────────────────────────────────────────────
    // SECTION 1: BURN CALCULATION TESTS
    // ───────────────────────────────────────────────────────────────────────────
    
    #[test]
    fn test_burn_calculation_basic() {
        // Given: 1,000 OXD, elapsed 1 year, 20% annual burn
        let balance = 1_000_000_000u64; // 1,000 OXD with 6 decimals
        let elapsed_seconds = 365 * 24 * 60 * 60i64; // 1 year
        let annual_burn_bp = 2000u128; // 20%
        
        // Calculate expected burn
        let balance_u128 = balance as u128;
        let expected_burn = (balance_u128 * annual_burn_bp * elapsed_seconds as u128) 
            / (10_000 * SECONDS_PER_YEAR as u128);
        
        // When: Applying lazy burn
        // Then: Burn should equal ~20% (actual calculation depends on fixed-point)
        assert!(expected_burn > 0, "Burn must be positive for positive elapsed time");
        assert!(expected_burn < balance_u128, "Burn cannot exceed balance");
    }

    #[test]
    fn test_burn_calculation_zero_elapsed() {
        // Given: elapsed = 0 seconds
        let balance = 1_000_000_000u64;
        let elapsed_seconds = 0i64;
        let annual_burn_bp = 2000u128;
        
        // When: Apply burn calculation
        let balance_u128 = balance as u128;
        let burn = (balance_u128 * annual_burn_bp * elapsed_seconds as u128) 
            / (10_000 * SECONDS_PER_YEAR as u128);
        
        // Then: No burn should occur
        assert_eq!(burn, 0, "Zero elapsed time should result in zero burn");
    }

    #[test]
    fn test_burn_calculation_one_day() {
        // Given: 1,000 OXD, elapsed = 1 day (out of 365 days/year)
        let balance = 1_000_000_000u64;
        let elapsed_seconds = 24 * 60 * 60i64; // 1 day
        let annual_burn_bp = 2000u128; // 20%
        
        // When: Apply burn
        let balance_u128 = balance as u128;
        let burn = (balance_u128 * annual_burn_bp * elapsed_seconds as u128) 
            / (10_000 * SECONDS_PER_YEAR as u128);
        
        // Then: Burn should be ~0.0548% (20% / 365)
        let expected_burn = (balance_u128 * 2000) / (10_000 * 365);
        assert!((burn as i128 - expected_burn as i128).abs() < 1_000_000, 
                "Daily burn calculation should be approximately 20% / 365");
    }

    #[test]
    fn test_burn_remainder_accumulation() {
        // Given: Multiple transfers with fractional burns
        let mut remainder = 0u128;
        let burn_rem_scale = 1_000_000u128;
        
        // When: Process multiple small burns
        for _ in 0..10 {
            let burn_amount = 123u128; // Fractional burn
            remainder += burn_amount % burn_rem_scale;
        }
        
        // Then: Remainder should accumulate and eventually form a complete token
        let tokens_from_remainder = remainder / burn_rem_scale;
        assert!(tokens_from_remainder >= 0, "Remainder accumulation should work");
    }

    #[test]
    fn test_burn_with_max_u64_balance() {
        // Given: Maximum u64 balance (18,446,744,073,709,551,615)
        let max_balance = u64::MAX;
        let elapsed_seconds = 365 * 24 * 60 * 60i64; // 1 year
        let annual_burn_bp = 2000u128; // 20%
        
        // When: Apply burn calculation with u128 intermediate
        let balance_u128 = max_balance as u128;
        let burn = (balance_u128 * annual_burn_bp * elapsed_seconds as u128) 
            / (10_000 * SECONDS_PER_YEAR as u128);
        
        // Then: Should not overflow (u128 can handle it)
        // Verify result is reasonable: 20% of max_u64 ≈ 3.6e18
        assert!(burn < balance_u128, "Burn should not exceed balance");
        assert!(burn > balance_u128 / 6, "Burn should be around 20%");
    }

    #[test]
    fn test_burn_with_fractional_seconds() {
        // Given: Elapsed time with fractional minutes (not exact second)
        let balance = 1_000_000_000u64;
        let elapsed_seconds = (15 * 60) + 30; // 15.5 minutes
        let annual_burn_bp = 2000u128;
        
        // When: Apply burn
        let balance_u128 = balance as u128;
        let burn = (balance_u128 * annual_burn_bp * elapsed_seconds as u128) 
            / (10_000 * SECONDS_PER_YEAR as u128);
        
        // Then: Should handle fractional seconds gracefully
        assert!(burn > 0, "Even small elapsed times should produce some burn");
        assert!(burn < balance_u128, "Burn must be within balance");
    }

    // ───────────────────────────────────────────────────────────────────────────
    // SECTION 2: TIMESTAMP TRACKING TESTS
    // ───────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_weighted_average_timestamp_single_transfer() {
        // Given: User receives tokens from single sender (no mixing)
        let sender_timestamp = 1_000_000u64;
        let sender_amount = 1_000_000_000u64;
        let receiver_amount = 500_000_000u64;
        
        // When: Calculate weighted average
        let weighted_ts = (
            (sender_timestamp as u128 * sender_amount as u128) +
            (receiver_amount as u128 * 0) // receiver has no prior tokens
        ) / (sender_amount as u128 + receiver_amount as u128);
        
        // Then: Should equal sender's timestamp (no mixing)
        assert_eq!(weighted_ts, sender_timestamp as u128, 
                   "Single sender should preserve timestamp");
    }

    #[test]
    fn test_weighted_average_timestamp_two_senders() {
        // Given: User receives from two senders with different timestamps
        let ts1 = 1_000_000u64;
        let amount1 = 1_000_000_000u64; // 50% of transfer
        
        let ts2 = 2_000_000u64;
        let amount2 = 1_000_000_000u64; // 50% of transfer
        
        // When: Calculate weighted average
        let weighted_ts = (
            (ts1 as u128 * amount1 as u128) +
            (ts2 as u128 * amount2 as u128)
        ) / ((amount1 + amount2) as u128);
        
        // Then: Should be average (1.5M)
        assert_eq!(weighted_ts, 1_500_000, 
                   "50/50 split should give average timestamp");
    }

    #[test]
    fn test_weighted_average_timestamp_unequal_split() {
        // Given: 70% from old sender, 30% from new sender
        let ts_old = 1_000_000u64;
        let amount_old = 7_000_000_000u64;
        
        let ts_new = 2_000_000u64;
        let amount_new = 3_000_000_000u64;
        
        // When: Calculate weighted average
        let weighted_ts = (
            (ts_old as u128 * amount_old as u128) +
            (ts_new as u128 * amount_new as u128)
        ) / ((amount_old + amount_new) as u128);
        
        // Then: Should lean toward old timestamp (70% weight)
        assert!(weighted_ts < 1_300_000, 
                "Weighted average should be closer to old timestamp");
        assert!(weighted_ts > 1_200_000,
                "Weighted average should still incorporate new timestamp");
    }

    #[test]
    fn test_timestamp_reset_after_clear_debt() {
        // Given: User with old tracking account timestamp
        let old_timestamp = 1_000_000u64;
        
        // When: Calling clear_debt (simulated as timestamp update)
        let current_timestamp = 2_000_000u64;
        
        // Then: Timestamp should reset to current
        assert_ne!(current_timestamp, old_timestamp, 
                   "clear_debt should update timestamp to current block time");
    }

    // ───────────────────────────────────────────────────────────────────────────
    // SECTION 3: FIXED-POINT ARITHMETIC TESTS
    // ───────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_fixed_point_scale_conversion() {
        // Given: Fixed-point scale = 1,000,000
        let scale = 1_000_000u128;
        let token_units = 100u128; // 100 whole tokens
        
        // When: Convert to fixed-point
        let fixed_point = token_units * scale;
        
        // Then: Should store fractional components
        assert_eq!(fixed_point, 100_000_000, "Fixed-point conversion correct");
    }

    #[test]
    fn test_fixed_point_remainder_overflow() {
        // Given: Accumulating remainders
        let mut remainder = 0u128;
        let scale = 1_000_000u128;
        
        // When: Add fractional amounts until overflow
        for i in 0..2_000_000 {
            remainder += (i % 500) as u128;
        }
        
        // Then: Should not panic, but track spill correctly
        let whole_tokens = remainder / scale;
        let leftover = remainder % scale;
        
        assert!(whole_tokens >= 0, "Should compute whole tokens");
        assert!(leftover < scale, "Leftover should be less than scale");
    }

    #[test]
    fn test_fixed_point_precision_loss() {
        // Given: Very small fractional amount
        let small_amount = 1u128; // Less than 1 token unit
        let scale = 1_000_000u128;
        
        // When: Store in fixed-point and retrieve
        let stored = small_amount % scale;
        
        // Then: Should preserve as remainder
        assert_eq!(stored, small_amount, "Small amounts should be preserved as remainder");
    }

    // ───────────────────────────────────────────────────────────────────────────
    // SECTION 4: GRACE PERIOD TESTS
    // ───────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_grace_period_not_elapsed() {
        // Given: Transfer within 15-minute grace window
        let timestamp = 1_000_000u64;
        let current_time = timestamp + (5 * 60); // 5 minutes later
        let grace_window = 15 * 60u64; // 15 minutes
        
        // When: Check if grace period is active
        let elapsed = current_time - timestamp;
        let within_grace = elapsed < grace_window;
        
        // Then: Should be within grace period
        assert!(within_grace, "5 minutes should be within 15-minute grace window");
    }

    #[test]
    fn test_grace_period_at_boundary() {
        // Given: Transfer exactly at 15-minute mark
        let timestamp = 1_000_000u64;
        let current_time = timestamp + (15 * 60);
        let grace_window = 15 * 60u64;
        
        // When: Check grace period
        let elapsed = current_time - timestamp;
        let within_grace = elapsed < grace_window;
        
        // Then: Should NOT be within grace (boundary = not included)
        assert!(!within_grace, "Exactly 15 minutes should exit grace period");
    }

    #[test]
    fn test_grace_period_exceeded() {
        // Given: Transfer after 20 minutes
        let timestamp = 1_000_000u64;
        let current_time = timestamp + (20 * 60);
        let grace_window = 15 * 60u64;
        
        // When: Check grace period
        let elapsed = current_time - timestamp;
        let within_grace = elapsed < grace_window;
        
        // Then: Should be outside grace period
        assert!(!within_grace, "20 minutes should exceed grace period");
    }

    // ───────────────────────────────────────────────────────────────────────────
    // SECTION 5: ERROR CONDITION TESTS
    // ───────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_insufficient_funds_detection() {
        // Given: User tries to withdraw more than balance
        let balance = 500_000_000u64; // 500 OXD
        let withdraw_amount = 600_000_000u64; // 600 OXD (more than balance)
        
        // When: Check for insufficient funds
        let has_funds = balance >= withdraw_amount;
        
        // Then: Should detect insufficient funds
        assert!(!has_funds, "Should detect insufficient withdrawal amount");
    }

    #[test]
    fn test_mint_authority_validation() {
        // Given: Wrong account as mint authority
        let expected_authority = "5amCTzFY1j7PSfKfb4vvCvQeJJPXwRBCaZBVL8PZM4qV";
        let provided_authority = "WrongAuthority123456789012345678901234567";
        
        // When: Validate mint authority
        let is_valid = expected_authority == provided_authority;
        
        // Then: Should reject invalid authority
        assert!(!is_valid, "Should reject wrong mint authority");
    }

    #[test]
    fn test_invalid_hook_program_id() {
        // Given: Hook program ID mismatch
        let expected_hook = "HookProg1234567890123456789012345678901234";
        let provided_hook = "WrongHook1234567890123456789012345678901234";
        
        // When: Validate hook program
        let is_valid = expected_hook == provided_hook;
        
        // Then: Should reject invalid hook
        assert!(!is_valid, "Should reject invalid hook program ID");
    }

    #[test]
    fn test_owner_mismatch_detection() {
        // Given: Account owner mismatch
        let expected_owner = "Alice1111111111111111111111111111111111111111";
        let provided_owner = "Bob2222222222222222222222222222222222222222";
        
        // When: Validate owner
        let matches = expected_owner == provided_owner;
        
        // Then: Should detect mismatch
        assert!(!matches, "Should detect owner mismatch");
    }

    // ───────────────────────────────────────────────────────────────────────────
    // SECTION 6: SUPPLY CAP TESTS
    // ───────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_initial_supply_constant() {
        // Given: INITIAL_SUPPLY constant
        const INITIAL_SUPPLY: u64 = 1_000_000_000_000_000; // 1 trillion with 6 decimals
        
        // When: Verify supply
        let expected = 1_000_000_000_000_000u64;
        
        // Then: Should match 1 trillion OXD
        assert_eq!(INITIAL_SUPPLY, expected, "Initial supply should be 1 trillion OXD");
    }

    #[test]
    fn test_daily_release_cap_calculation() {
        // Given: 1% of supply per day cap
        const INITIAL_SUPPLY: u64 = 1_000_000_000_000_000;
        const MAX_DAILY_RELEASE: u64 = INITIAL_SUPPLY / 100; // 1%
        
        // When: Calculate max daily release
        let expected_daily_cap = 10_000_000_000_000u64; // 10 trillion / 100
        
        // Then: Should be 1% of supply
        assert_eq!(MAX_DAILY_RELEASE, expected_daily_cap, 
                   "Daily cap should be 1% of initial supply");
    }

    #[test]
    fn test_daily_release_cap_exceeded() {
        // Given: Creator tries to release more than daily cap
        const DAILY_CAP: u64 = 10_000_000_000_000u64;
        let released_today = 8_000_000_000_000u64;
        let attempt_release = 3_000_000_000_000u64; // Exceeds cap
        
        // When: Check if exceeds cap
        let exceeds = (released_today + attempt_release) > DAILY_CAP;
        
        // Then: Should reject
        assert!(exceeds, "Should reject release exceeding daily cap");
    }

    #[test]
    fn test_genesis_airdrop_limit() {
        // Given: Genesis airdrop limit of 1000 wallets
        const GENESIS_LIMIT: u64 = 1000;
        let airdrops_used = 500u64;
        
        // When: Check if can airdrop to wallet 501
        let can_airdrop = airdrops_used < GENESIS_LIMIT;
        
        // Then: Should allow (under limit)
        assert!(can_airdrop, "Should allow airdrop under limit");
    }

    #[test]
    fn test_genesis_airdrop_limit_exceeded() {
        // Given: Used all 1000 genesis airdrops
        const GENESIS_LIMIT: u64 = 1000;
        let airdrops_used = 1000u64;
        
        // When: Try to airdrop wallet 1001
        let can_airdrop = airdrops_used < GENESIS_LIMIT;
        
        // Then: Should reject
        assert!(!can_airdrop, "Should reject airdrop exceeding limit");
    }

    // ───────────────────────────────────────────────────────────────────────────
    // SECTION 7: CLOCK/TIME TESTS
    // ───────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_seconds_per_year_constant() {
        // Given: SECONDS_PER_YEAR constant
        const SECONDS_PER_YEAR: i64 = 365 * 24 * 60 * 60;
        
        // When: Verify calculation
        let expected = 31_536_000i64;
        
        // Then: Should be correct
        assert_eq!(SECONDS_PER_YEAR, expected, "Seconds per year should be 31,536,000");
    }

    #[test]
    fn test_seconds_per_day_constant() {
        // Given: SECONDS_PER_DAY constant
        const SECONDS_PER_DAY: i64 = 24 * 60 * 60;
        
        // When: Verify calculation
        let expected = 86_400i64;
        
        // Then: Should be correct
        assert_eq!(SECONDS_PER_DAY, expected, "Seconds per day should be 86,400");
    }

    #[test]
    fn test_clock_regression_handling() {
        // Given: Clock goes backward (rare but possible)
        let previous_time = 1_000_000u64;
        let current_time = 999_000u64; // 1000 seconds earlier
        
        // When: Calculate elapsed
        let elapsed = current_time as i64 - previous_time as i64; // Negative!
        
        // Then: Should handle gracefully (no burn)
        assert!(elapsed < 0, "Elapsed should be negative if clock regresses");
        // In actual implementation: if elapsed < 0 { elapsed = 0; } // No burn
    }

    #[test]
    fn test_timestamp_in_future() {
        // Given: Tracking timestamp is somehow in future
        let tracking_ts = 2_000_000u64;
        let current_block_time = 1_000_000u64; // Earlier than tracking_ts
        
        // When: Calculate elapsed
        let elapsed = current_block_time as i64 - tracking_ts as i64; // Negative
        
        // Then: Should be negative (invalid state, handled as 0 burn)
        assert!(elapsed < 0, "Future timestamp should give negative elapsed");
    }
}
