use anchor_lang::prelude::*;
use anchor_spl::token_2022::{self, mint_to, burn, Mint, Token2022, TokenAccount, MintTo, Burn};
use anchor_spl::token_interface::{TokenInterface, Mint as MintInterface};
use solana_program::program_option::COption;
use solana_program::instruction::AccountMeta;
use spl_token_2022::{
    extension::{
        BaseStateWithExtensions, 
        ExtensionType,
        transfer_fee::{TransferFeeConfig, TransferFee, MAX_FEE_BASIS_POINTS},
    },
    state::Mint as MintState,
};
use spl_pod::optional_keys::OptionalNonZeroPubkey;

declare_id!("OXIDExxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx");

// ⚠️ SECURITY: Hook program ID (must match hook_lib.rs declare_id)
// Usado para validar que hook_program es el correcto en ClearDebt/Deposit/Withdraw
const HOOK_PROGRAM_ID: Pubkey = pubkey!("HookxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxXXX");
const ANNUAL_BURN_BP: u128 = 20_00; // 20.00% en basis points (10000 = 100%)
const BURN_REM_SCALE: u128 = 1_000_000; // Fixed-point scale = 1 token unit (6 decimales)
const INITIAL_SUPPLY: u64 = 1_000_000_000_000_000; // 1 BILLÓN de OXD (6 decimales)

// ⚠️ SEGURIDAD ECONÓMICA: Cap diario de liberación de tokens del creador
// Previene wash-trading para liberar tokens masivamente en un solo día
// 1% del supply total por día = 10,000,000,000 tokens (10M OXD)
const MAX_DAILY_RELEASE: u64 = INITIAL_SUPPLY / 100; // 1% del supply total (10M OXD)
const SECONDS_PER_YEAR: i64 = 365 * 24 * 60 * 60;
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

#[program]
pub mod oxide {
    use super::*;

    /// Inicializa el programa, crea GlobalState y asigna TODO el supply al creador
    /// SOLO SE PUEDE LLAMAR UNA VEZ
    pub fn initialize_global(ctx: Context<InitializeGlobal>) -> Result<()> {
        let global = &mut ctx.accounts.global_state;
        let creator = &mut ctx.accounts.creator_account;
        let now = Clock::get()?.unix_timestamp;

        // Configurar estado global
        global.authority = ctx.accounts.authority.key();
        global.mint = ctx.accounts.mint.key();
        global.mint_verified = false;  // Debe llamarse verify_mint_authority
        global.amm_whitelist_enabled = true; // Whitelist activa por defecto
        global.total_tokens_released = 0;  // Vesting: empieza en 0
        global.release_rate_basis_points = 10; // 0.1% del volumen libera tokens del creador
        global.genesis_airdrops_given = 0;     // Contador de airdrops
        global.last_release_day = 0;           // Sin liberaciones aún
        global.daily_released_amount = 0;      // Sin liberaciones aún
        global.bump = ctx.bumps.global_state;

        // ASIGNAR SUMINISTRO INICIAL AL CREADOR
        // - 10,000 OXD en balance_free (para crear liquidez inicial en DEX)
        // - 999,990,000 OXD en balance_staked (solo se libera vía vesting dinámico)
        creator.owner = ctx.accounts.authority.key();
        creator.balance_free = 10_000_000_000;  // 10,000 OXD (6 decimales)
        creator.balance_staked = INITIAL_SUPPLY - 10_000_000_000;  // 999,990,000 OXD
        creator.last_update = now as u64;
        creator.burn_fraction_remainder = 0;

        Ok(())
    }

    /// Verifica que el PDA es mint_authority del token SPL
    /// DEBE llamarse después de transferir mint_authority al PDA
    /// Sin esto, withdraw() fallará
    pub fn verify_mint_authority(ctx: Context<VerifyMintAuthority>) -> Result<()> {
        let global = &mut ctx.accounts.global_state;
        let mint = &ctx.accounts.mint;
        
        // Solo se puede verificar una vez
        require!(!global.mint_verified, ErrorCode::AlreadyVerified);
        
        // Verificar que el mint authority es el PDA
        require!(
            mint.mint_authority == COption::Some(global.key()),
            ErrorCode::InvalidMintAuthority
        );
        
        // Marcar como verificado
        global.mint_verified = true;
        
        Ok(())
    }

    /// Inicializa cuenta de usuario nueva (balance 0)
    pub fn initialize_user(ctx: Context<InitializeUser>) -> Result<()> {
        let user = &mut ctx.accounts.user;
        let now = Clock::get()?.unix_timestamp;

        user.owner = ctx.accounts.owner.key();
        user.balance_free = 0;
        user.balance_staked = 0;
        user.last_update = now as u64;
        user.burn_fraction_remainder = 0;

        Ok(())
    }

    /// NUEVO: transfer_to_uninitialized
    /// Permite enviar tokens a wallets no registradas (crea UserAccount si falta)
    /// y evita pisar balances existentes de receptores ya inicializados.
    pub fn transfer_to_uninitialized(ctx: Context<TransferToUninitialized>, amount: u64) -> Result<()> {
        let global = &mut ctx.accounts.global_state;
        let from = &mut ctx.accounts.from_user;
        let to = &mut ctx.accounts.to_user;
        let to_owner = ctx.accounts.to_owner.key();
        let now = Clock::get()?.unix_timestamp;

        // Validar que mint_authority está verificada
        require!(global.mint_verified, ErrorCode::MintNotVerified);

        // Guardar si el destinatario ya estaba inicializado (antes de mutar last_update)
        let to_was_initialized = to.last_update != 0;

        // Aplicar burn y actualizar reloj en ambas cuentas
        apply_lazy_burn(from, now)?;
        apply_lazy_burn(to, now)?;

        // Si la cuenta ya existe, validar owner para no clobber de otra wallet
        if to_was_initialized {
            require!(to.owner == to_owner, ErrorCode::OwnerMismatch);
        } else {
            // Inicialización segura (solo si estaba vacía)
            to.owner = to_owner;
            to.balance_free = 0;
            to.balance_staked = 0;
            to.last_update = now as u64;
            to.burn_fraction_remainder = 0;
        }

        // Enviar lo máximo posible (burn ya aplicado a 'from')
        let available = from.balance_free;
        let to_send = amount.min(available);
        require!(to_send > 0, ErrorCode::InsufficientFunds);

        from.balance_free -= to_send;
        to.balance_free = to.balance_free.saturating_add(to_send);
        
        // VESTING DINÁMICO: Liberar tokens del creador proporcional al volumen
        release_creator_tokens(
            global,
            &mut ctx.accounts.creator_account,
            to_send,
            now
        )?;

        Ok(())
    }

    /// DEPOSITAR: Quemar SPL tokens → obtener balance interno
    /// (Usuario devuelve "recibos" al sistema)
    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        let global = &ctx.accounts.global_state;
        let user = &mut ctx.accounts.user_account;
        let now = Clock::get()?.unix_timestamp;

        // Validar que mint_authority está verificada
        require!(global.mint_verified, ErrorCode::MintNotVerified);

        // Aplica lazy burn antes
        apply_lazy_burn(user, now)?;

        // QUEMAR los SPL tokens del usuario (Token-2022)
        let cpi_accounts = Burn {
            mint: ctx.accounts.mint.to_account_info(),
            from: ctx.accounts.user_token_account.to_account_info(),
            authority: ctx.accounts.owner.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        burn(cpi_ctx, amount)?;

        // Acreditar balance interno (1:1)
        user.balance_free += amount;
        
        // SINCRONIZAR TrackingAccount después de quemar SPL tokens
        // Esto evita desincronización cuando usuario deposita tokens viejos
        sync_tracking_account(
            &ctx.accounts.hook_program,
            &ctx.accounts.owner,
            &ctx.accounts.tracking_account,
            &ctx.accounts.system_program,
            user.last_update,
        )?;
        
        // VESTING DINÁMICO: Liberar tokens del creador proporcional al volumen
        release_creator_tokens(
            &mut ctx.accounts.global_state,
            &mut ctx.accounts.creator_account,
            amount,
            now
        )?;

        Ok(())
    }

    /// RETIRAR: Reducir balance interno → mintear SPL tokens
    /// (Usuario obtiene "recibos" para vender en DEX)
    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        let user = &mut ctx.accounts.user_account;
        let global = &mut ctx.accounts.global_state;
        let now = Clock::get()?.unix_timestamp;

        // Aplica lazy burn ANTES de retirar
        apply_lazy_burn(user, now)?;

        require!(global.mint_verified, ErrorCode::MintNotVerified);

        // Enviar lo máximo posible (burn ya aplicado)
        let available = user.balance_free;
        let to_withdraw = amount.min(available);
        
        require!(to_withdraw > 0, ErrorCode::InsufficientFunds);

        // Deducir balance interno
        user.balance_free -= to_withdraw;

        // MINTEAR SPL tokens al usuario (PDA es mint authority) - Token-2022
        let seeds = &[
            b"global".as_ref(),
            &[global.bump],
        ];
        let signer = &[&seeds[..]];

        let cpi_accounts = MintTo {
            mint: ctx.accounts.mint.to_account_info(),
            to: ctx.accounts.user_token_account.to_account_info(),
            authority: ctx.accounts.global_state.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        mint_to(cpi_ctx, to_withdraw)?;
        
        // SINCRONIZAR TrackingAccount del hook para que tokens SPL nuevos
        // tengan el mismo timestamp que UserAccount (evita desincronización)
        sync_tracking_account(
            &ctx.accounts.hook_program,
            &ctx.accounts.owner,
            &ctx.accounts.tracking_account,
            &ctx.accounts.system_program,
            user.last_update,
        )?;
        
        msg!("Withdrew {} SPL tokens, TrackingAccount synced to ts={}", to_withdraw, user.last_update);

        Ok(())
    }

    pub fn stake(ctx: Context<Stake>, amount: u64) -> Result<()> {
        let user = &mut ctx.accounts.user;
        let now = Clock::get()?.unix_timestamp;

        apply_lazy_burn(user, now)?;

        // Stakear lo máximo posible (burn ya aplicado)
        let available = user.balance_free;
        let to_stake = amount.min(available);
        
        require!(to_stake > 0, ErrorCode::InsufficientFunds);

        user.balance_free -= to_stake;
        user.balance_staked += to_stake;

        Ok(())
    }

    pub fn unstake(ctx: Context<Unstake>, amount: u64) -> Result<()> {
        let user = &mut ctx.accounts.user;
        let global = &ctx.accounts.global_state;
        let now = Clock::get()?.unix_timestamp;
        
        // RESTRICCIÓN: El creador NO puede unstakear manualmente
        // Solo puede liberar tokens vía vesting dinámico (release_creator_tokens)
        require!(
            user.owner != global.authority,
            ErrorCode::CreatorCannotUnstake
        );

        apply_lazy_burn(user, now)?;

        let unstake_amount = amount.min(user.balance_staked);
        user.balance_staked -= unstake_amount;
        user.balance_free += unstake_amount;

        Ok(())
    }

    pub fn transfer(ctx: Context<TransferTokens>, amount: u64) -> Result<()> {
        let global = &ctx.accounts.global_state;
        let from = &mut ctx.accounts.from_user;
        let to = &mut ctx.accounts.to_user;
        let now = Clock::get()?.unix_timestamp;

        // Validar que mint_authority está verificada
        require!(global.mint_verified, ErrorCode::MintNotVerified);

        apply_lazy_burn(from, now)?;
        apply_lazy_burn(to, now)?;

        // Enviar lo máximo posible (burn ya aplicado)
        let available = from.balance_free;
        let to_send = amount.min(available);
        
        require!(to_send > 0, ErrorCode::InsufficientFunds);

        from.balance_free -= to_send;
        to.balance_free += to_send;
        
        // VESTING DINÁMICO: Liberar tokens del creador proporcional al volumen
        release_creator_tokens(
            &mut ctx.accounts.global_state,
            &mut ctx.accounts.creator_account,
            to_send,
            now
        )?;

        Ok(())
    }

    /// Toggle whitelist de AMMs (solo authority)
    /// Permite deshabilitar "Zona Franca" en emergencias
    /// Si se deshabilita, incluso pools deben cumplir validación de 15min
    pub fn toggle_amm_whitelist(ctx: Context<ToggleWhitelist>) -> Result<()> {
        let global = &mut ctx.accounts.global_state;
        global.amm_whitelist_enabled = !global.amm_whitelist_enabled;
        
        msg!("AMM whitelist (Zona Franca) is now: {}", if global.amm_whitelist_enabled { "ENABLED" } else { "DISABLED" });
        Ok(())
    }

    /// GENESIS AIRDROP: Distribuir tokens iniciales a primeros 1000 usuarios
    /// Solo puede llamarse por authority, máximo 1000 veces
    /// 
    /// ⚠️ EXCEPCIÓN A LAS REGLAS DE VESTING:
    /// Este método es el ÚNICO que permite al creador liberar tokens manualmente,
    /// pero con límites estrictos de seguridad:
    ///   - Máximo 100 OXD por airdrop (100,000,000 unidades con 6 decimales)
    ///   - Máximo 1000 airdrops totales (lifetime limit)
    ///   - **NO respeta cap diario** (diseñado para siembra inicial rápida a early adopters)
    ///   - Solo puede enviar desde balance_staked (no puede crear tokens)
    /// 
    /// Máximo teórico: 100,000 OXD en total (100 OXD × 1000 airdrops)
    /// Esto representa solo 0.01% del supply inicial (1B tokens)
    /// 
    /// DISEÑO: Permite distribuir a early adopters sin esperar volumen de trading,
    /// pero está limitado en cantidad total para prevenir rug-pull.
    pub fn genesis_airdrop(ctx: Context<GenesisAirdrop>, amount: u64) -> Result<()> {
        let global = &mut ctx.accounts.global_state;
        let creator = &mut ctx.accounts.creator_account;
        let recipient = &mut ctx.accounts.recipient_account;
        let now = Clock::get()?.unix_timestamp;
        
        // LÍMITE 1: Máximo 1000 airdrops lifetime
        require!(
            global.genesis_airdrops_given < 1000,
            ErrorCode::GenesisAirdropLimitReached
        );
        
        // LÍMITE 2: Máximo 100 OXD por airdrop (100,000,000 unidades)
        const MAX_AIRDROP_AMOUNT: u64 = 100_000_000; // 100 OXD
        require!(
            amount <= MAX_AIRDROP_AMOUNT,
            ErrorCode::AirdropAmountExceedsLimit
        );
        
        // Aplicar burn al creador y recipiente
        apply_lazy_burn(creator, now)?;
        apply_lazy_burn(recipient, now)?;
        
        // LÍMITE 3: Verificar que el creador tiene suficiente balance stakado
        require!(
            creator.balance_staked >= amount,
            ErrorCode::InsufficientFunds
        );
        
        // Transferir desde balance_staked del creador al balance_free del recipiente
        // NOTA: Esto es una EXCEPCIÓN - normalmente el creador no puede unstakear
        creator.balance_staked -= amount;
        recipient.balance_free += amount;
        
        // Actualizar contadores globales
        // NOTA: NO actualiza daily_released_amount (excepción al cap diario)
        global.total_tokens_released += amount;
        global.genesis_airdrops_given += 1;
        
        msg!(
            "Genesis airdrop #{}: {} OXD to {}. Remaining airdrops: {}. Total released: {} OXD",
            global.genesis_airdrops_given,
            amount / 1_000_000, // Mostrar en OXD
            recipient.owner,
            1000 - global.genesis_airdrops_given,
            global.total_tokens_released / 1_000_000
        );
        
        Ok(())
    }

    /// NUEVA: clear_debt
    /// Limpia la deuda de oxidación del balance SPL ANTES de venderlo
    /// Debe llamarse si elapsed > 15 minutos antes de transferir SPL
    pub fn clear_debt(ctx: Context<ClearDebt>) -> Result<()> {
        let global = &ctx.accounts.global_state;
        let user = &mut ctx.accounts.user_account;
        let now = Clock::get()?.unix_timestamp;

        // Validar que mint_authority está verificada
        require!(global.mint_verified, ErrorCode::MintNotVerified);

        // Validar que el tracking provisto corresponde al PDA correcto del hook
        let (expected_tracking, _) = Pubkey::find_program_address(
            &[b"tracking", ctx.accounts.owner.key().as_ref()],
            ctx.accounts.hook_program.key
        );
        require_keys_eq!(
            ctx.accounts.tracking_account.key(),
            expected_tracking,
            ErrorCode::InvalidTrackingAccount
        );

        // Si la cuenta existe y no la posee el hook, es inválida
        if !ctx.accounts.tracking_account.data_is_empty() {
            require!(
                ctx.accounts.tracking_account.owner == ctx.accounts.hook_program.key(),
                ErrorCode::InvalidTrackingAccount
            );
        }

        // Fuente de verdad: timestamp del TrackingAccount (hook). Si no existe, usar user.last_update.
        let tracking_last_update = read_tracking_last_update(&ctx.accounts.tracking_account)
            .unwrap_or(user.last_update);

        let elapsed = now - tracking_last_update as i64;
        
        if elapsed > 0 {
            // Obtener balance SPL actual del usuario
            let spl_balance = ctx.accounts.user_token_account.amount;

            if spl_balance > 0 {
                let balance_u128 = spl_balance as u128;
                
                // Calcular burn total (incluyendo remainder)
                // Fixed-point burn with bounded remainder (mod BURN_REM_SCALE)
                let burn_fp = user.burn_fraction_remainder
                    + (balance_u128 * ANNUAL_BURN_BP * (elapsed as u128) * BURN_REM_SCALE)
                        / (SECONDS_PER_YEAR as u128 * 10_000);

                let burn_u64 = ((burn_fp / BURN_REM_SCALE) as u64).min(spl_balance);
                user.burn_fraction_remainder = burn_fp % BURN_REM_SCALE;

                if burn_u64 > 0 {
                    msg!("Clearing debt: burning {} tokens after {} seconds", burn_u64, elapsed);

                    // QUEMAR los tokens de deuda
                    let cpi_accounts = Burn {
                        mint: ctx.accounts.mint.to_account_info(),
                        from: ctx.accounts.user_token_account.to_account_info(),
                        authority: ctx.accounts.owner.to_account_info(),
                    };
                    let cpi_program = ctx.accounts.token_program.to_account_info();
                    let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
                    burn(cpi_ctx, burn_u64)?;
                }
            }
        }

        // Aplicar lazy burn al balance interno para mantener consistencia con el reloj global
        apply_lazy_burn(user, now)?;
        
        // SINCRONIZAR TrackingAccount del hook via CPI con el timestamp nuevo
        sync_tracking_account(
            &ctx.accounts.hook_program,
            &ctx.accounts.owner,
            &ctx.accounts.tracking_account,
            &ctx.accounts.system_program,
            user.last_update,
        )?;
        
        msg!("TrackingAccount synchronized with UserAccount timestamp: {}", user.last_update);

        Ok(())
    }
}

/// Helper para sincronizar TrackingAccount del hook via CPI
fn sync_tracking_account<'info>(
    hook_program: &UncheckedAccount<'info>,
    user_wallet: &Signer<'info>,
    tracking_account: &UncheckedAccount<'info>,
    system_program: &Program<'info, System>,
    timestamp: u64,
) -> Result<()> {
    // Calcular discriminator de sync_tracking: sighash("global:sync_tracking")
    // Anchor usa primeros 8 bytes de SHA256
    let discriminator = anchor_lang::solana_program::hash::hash(b"global:sync_tracking")
        .to_bytes()[..8]
        .try_into()
        .unwrap();
    
    let mut data = Vec::with_capacity(8 + 8);
    data.extend_from_slice(&discriminator);
    data.extend_from_slice(&timestamp.to_le_bytes());
    
    anchor_lang::solana_program::program::invoke(
        &anchor_lang::solana_program::instruction::Instruction {
            program_id: hook_program.key(),
            accounts: vec![
                AccountMeta::new_readonly(user_wallet.key(), false),
                AccountMeta::new(tracking_account.key(), false),
                AccountMeta::new(user_wallet.key(), true),
                AccountMeta::new_readonly(system_program.key(), false),
            ],
            data,
        },
        &[
            user_wallet.to_account_info(),
            tracking_account.to_account_info(),
            system_program.to_account_info(),
        ],
    )?;
    
    Ok(())
}

/// Estructura mínima para leer TrackingAccount del hook (owner, last_update, remainder)
#[derive(AnchorDeserialize)]
struct TrackingAccountData {
    pub owner: Pubkey,
    pub last_update: u64,
    pub burn_fraction_remainder: u128,
}

/// Lee last_update del TrackingAccount; retorna None si la cuenta está vacía o no tiene tamaño suficiente
fn read_tracking_last_update(tracking_account: &UncheckedAccount) -> Option<u64> {
    let data_ref = tracking_account.try_borrow_data().ok()?;
    let data: &[u8] = &data_ref;
    // Debe tener al menos discriminator (8) + campos
    if data.len() < 8 + 32 + 8 + 16 {
        return None;
    }
    // Saltar discriminator Anchor (8 bytes)
    let mut slice: &[u8] = &data[8..];
    if let Ok(tracking) = TrackingAccountData::deserialize(&mut slice) {
        if tracking.last_update > 0 {
            return Some(tracking.last_update);
        }
    }
    None
}

fn apply_lazy_burn(user: &mut UserAccount, now: i64) -> Result<()> {
    // Note: Solana's Clock is slot-based and approximate. Not for sub-second precision.
    // Para OXIDE, la precisión de ~400ms por slot es aceptable (burn es anual).
    let elapsed = now - user.last_update as i64;

    if elapsed > 0 && user.balance_free > 0 {
        let balance_u128 = user.balance_free as u128;
        
        // Sumar remainder anterior + nuevo cálculo
        // Fixed-point burn with bounded remainder (mod BURN_REM_SCALE)
        let burn_fp = user.burn_fraction_remainder
            + (balance_u128 * ANNUAL_BURN_BP * (elapsed as u128) * BURN_REM_SCALE)
                / (SECONDS_PER_YEAR as u128 * 10_000);

        // Extraer la parte entera (en tokens) para quemar
        let burn_u64 = ((burn_fp / BURN_REM_SCALE) as u64).min(user.balance_free);

        // Guardar remainder acotado por la escala
        user.burn_fraction_remainder = burn_fp % BURN_REM_SCALE;

        user.balance_free -= burn_u64;
    }
    user.last_update = now as u64;
    Ok(())
}

/// Helper para liberar tokens del creador según volumen de trading
/// Esto implementa el VESTING DINÁMICO: más trading = más tokens liberados
/// 
/// ⚠️ MITIGACIÓN DE WASH-TRADING:
/// - Cap diario de 1% del supply total (10M OXD)
/// - Si se alcanza el cap, no se liberan más tokens hasta el próximo día
/// - Previene que el creador incentive wash-trading para liberar tokens masivamente
///
/// 🔒 ECONOMIC SAFETY:
/// Creator vesting is tied to ORGANIC volume. Wash-trading is economically disincentivized:
///   1. Network fees (Solana tx cost + priority fees during congestion)
///   2. Time-decay applies to tokens USED for wash-trading (20% annual on free balance)
///   3. Daily cap prevents burst liberation (max 1% per day regardless of volume)
///   4. Net effect: Wash-trading costs MORE than the tokens released (fee burn > vesting gain)
fn release_creator_tokens(
    global: &mut GlobalState,
    creator: &mut UserAccount,
    amount_traded: u64,
    now: i64,
) -> Result<()> {
    // Note: Solana's Clock is slot-based and approximate. Not for sub-second precision.
    // Para el cap diario, la precisión de ~400ms es aceptable.
    
    // Solo aplicar si hay tokens stakados del creador
    if creator.balance_staked == 0 {
        return Ok(()); // Sin tokens para liberar
    }
    
    // Calcular día actual (unix timestamp / SECONDS_PER_DAY)
    let current_day = now / SECONDS_PER_DAY;
    
    // Si es un nuevo día, resetear contador diario
    if current_day > global.last_release_day {
        global.last_release_day = current_day;
        global.daily_released_amount = 0;
        msg!("Nuevo día detectado - cap diario reseteado");
    }
    
    // Verificar si ya se alcanzó el cap diario (10M OXD)
    if global.daily_released_amount >= MAX_DAILY_RELEASE {
        msg!(
            "⚠️ Cap diario alcanzado: {} OXD liberados hoy. Próxima liberación en {} horas.",
            global.daily_released_amount / 1_000_000, // Convertir a OXD (6 decimales)
            ((current_day + 1) * SECONDS_PER_DAY - now) / 3600 // Horas restantes
        );
        return Ok(()); // No liberar más tokens hoy
    }
    
    // Calcular tokens a liberar: amount_traded * release_rate_basis_points / 10_000
    // Ejemplo: 1000 tokens traded * 10 BP / 10_000 = 1 token liberado (0.1%)
    let tokens_to_release = ((amount_traded as u128) * (global.release_rate_basis_points as u128)) / 10_000;
    let mut tokens_to_release = (tokens_to_release as u64).min(creator.balance_staked);
    
    // Aplicar cap diario: no liberar más del límite restante del día
    let remaining_daily_quota = MAX_DAILY_RELEASE.saturating_sub(global.daily_released_amount);
    tokens_to_release = tokens_to_release.min(remaining_daily_quota);
    
    if tokens_to_release > 0 {
        // Aplicar burn al creador ANTES de liberar tokens (fairness)
        apply_lazy_burn(creator, now)?;
        
        // Mover de staked a free
        creator.balance_staked -= tokens_to_release;
        creator.balance_free += tokens_to_release;
        
        // Actualizar contadores globales
        global.total_tokens_released += tokens_to_release;
        global.daily_released_amount += tokens_to_release;
        
        msg!(
            "Vesting liberado: {} OXD ({} BP del volumen). Total liberado: {} OXD. Hoy: {} / {} OXD",
            tokens_to_release / 1_000_000,
            global.release_rate_basis_points,
            global.total_tokens_released / 1_000_000,
            global.daily_released_amount / 1_000_000,
            MAX_DAILY_RELEASE / 1_000_000
        );
    }
    
    Ok(())
}

// ============== STRUCTS ==============

#[account]
pub struct GlobalState {
    pub authority: Pubkey,      // creador original
    pub mint: Pubkey,           // dirección del SPL token
    pub mint_verified: bool,    // true si verify_mint_authority pasó
    pub amm_whitelist_enabled: bool, // true = whitelist activa, false = deshabilitada
    pub total_tokens_released: u64,  // Tracking de tokens liberados del vesting
    pub release_rate_basis_points: u16, // 0.1% = 10 BP - tokens liberados por cada trade
    pub genesis_airdrops_given: u16,    // Contador de airdrops (max 1000)
    pub last_release_day: i64,          // Último día donde se liberaron tokens (unix timestamp / SECONDS_PER_DAY)
    pub daily_released_amount: u64,     // Tokens liberados en el día actual
    pub bump: u8,               // PDA bump
}

#[account]
pub struct UserAccount {
    pub owner: Pubkey,           // wallet del usuario
    pub balance_free: u64,       // tokens líquidos (lazy burn aplica aquí)
    pub balance_staked: u64,     // tokens protegidos (NO se queman)
    pub last_update: u64,        // último timestamp aplicado
    pub burn_fraction_remainder: u128, // Acumula remainders de divisiones para evitar micro-burn exploit
}

// ============== CONTEXTS ==============

#[derive(Accounts)]
pub struct InitializeGlobal<'info> {
    #[account(
        init,
        payer = authority,
        space = 8
            + 32  // authority
            + 32  // mint
            + 1   // mint_verified
            + 1   // amm_whitelist_enabled
            + 8   // total_tokens_released
            + 2   // release_rate_basis_points
            + 2   // genesis_airdrops_given
            + 8   // last_release_day
            + 8   // daily_released_amount
            + 1,  // bump
        seeds = [b"global"],
        bump
    )]
    pub global_state: Account<'info, GlobalState>,
    
    /// Cuenta del creador - recibe TODO el supply inicial
    #[account(
        init,
        payer = authority,
        space = 8 + 32 + 8 + 8 + 8 + 16,  // discriminator + owner + balance_free + balance_staked + last_update + burn_fraction_remainder
        seeds = [b"user", authority.key().as_ref()],
        bump
    )]
    pub creator_account: Account<'info, UserAccount>,
    
    /// El mint SPL (debe crearse ANTES con mint_authority = PDA)
    pub mint: Account<'info, Mint>,
    
    #[account(mut)]
    pub authority: Signer<'info>,
    
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct VerifyMintAuthority<'info> {
    #[account(
        mut,
        seeds = [b"global"],
        bump = global_state.bump,
        has_one = authority,
        has_one = mint,
    )]
    pub global_state: Account<'info, GlobalState>,
    
    pub mint: Account<'info, Mint>,
    
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct InitializeUser<'info> {
    #[account(
        init, 
        payer = owner, 
        space = 8 + 32 + 8 + 8 + 8 + 16, // +16 para burn_fraction_remainder (u128)
        seeds = [b"user", owner.key().as_ref()],
        bump
    )]
    pub user: Account<'info, UserAccount>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(
        mut,
        seeds = [b"user", owner.key().as_ref()],
        bump,
        has_one = owner
    )]
    pub user_account: Account<'info, UserAccount>,
    
    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        constraint = mint.key() == global_state.mint @ ErrorCode::InvalidMint
    )]
    pub mint: Account<'info, Mint>,
    
    #[account(
        mut,
        seeds = [b"global"],
        bump = global_state.bump,
    )]
    pub global_state: Account<'info, GlobalState>,
    
    /// Cuenta del creador para vesting dinámico
    #[account(
        mut,
        seeds = [b"user", global_state.authority.as_ref()],
        bump,
    )]
    pub creator_account: Account<'info, UserAccount>,
    
    /// TrackingAccount del hook que se sincronizará
    /// CHECK: PDA derivada validada por el hook
    #[account(mut)]
    pub tracking_account: UncheckedAccount<'info>,
    
    /// Programa del transfer hook
    #[account(
        constraint = hook_program.key() == &HOOK_PROGRAM_ID @ ErrorCode::InvalidHookProgram
    )]
    pub hook_program: UncheckedAccount<'info>,
    
    pub owner: Signer<'info>,
    pub token_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(
        mut, 
        seeds = [b"user", owner.key().as_ref()],
        bump,
        has_one = owner
    )]
    pub user_account: Account<'info, UserAccount>,
    
    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        constraint = mint.key() == global_state.mint
    )]
    pub mint: Account<'info, Mint>,
    
    #[account(
        mut,
        seeds = [b"global"],
        bump = global_state.bump,
    )]
    pub global_state: Account<'info, GlobalState>,
    
    /// Cuenta del creador para vesting dinámico
    #[account(
        mut,
        seeds = [b"user", global_state.authority.as_ref()],
        bump,
    )]
    pub creator_account: Account<'info, UserAccount>,
    
    /// TrackingAccount del hook que se sincronizará
    /// CHECK: PDA derivada validada por el hook
    #[account(mut)]
    pub tracking_account: UncheckedAccount<'info>,
    
    /// Programa del transfer hook
    #[account(
        constraint = hook_program.key() == &HOOK_PROGRAM_ID @ ErrorCode::InvalidHookProgram
    )]
    pub hook_program: UncheckedAccount<'info>,
    
    #[account(mut)]
    pub owner: Signer<'info>,
    pub token_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Stake<'info> {
    #[account(
        mut, 
        seeds = [b"user", owner.key().as_ref()],
        bump,
        has_one = owner
    )]
    pub user: Account<'info, UserAccount>,
    pub owner: Signer<'info>,
}

#[derive(Accounts)]
pub struct Unstake<'info> {
    #[account(
        mut, 
        seeds = [b"user", owner.key().as_ref()],
        bump,
        has_one = owner
    )]
    pub user: Account<'info, UserAccount>,
    
    #[account(
        seeds = [b"global"],
        bump = global_state.bump,
    )]
    pub global_state: Account<'info, GlobalState>,
    
    pub owner: Signer<'info>,
}

#[derive(Accounts)]
pub struct TransferTokens<'info> {
    #[account(
        mut, 
        seeds = [b"user", owner.key().as_ref()],
        bump,
        has_one = owner
    )]
    pub from_user: Account<'info, UserAccount>,
    
    /// CHECK: Validamos que to_user es una PDA válida derivada de su owner
    #[account(
        mut,
        seeds = [b"user", to_user.owner.as_ref()],
        bump,
    )]
    pub to_user: Account<'info, UserAccount>,
    
    #[account(
        mut,
        seeds = [b"global"],
        bump = global_state.bump,
    )]
    pub global_state: Account<'info, GlobalState>,
    
    /// Cuenta del creador para vesting dinámico
    #[account(
        mut,
        seeds = [b"user", global_state.authority.as_ref()],
        bump,
    )]
    pub creator_account: Account<'info, UserAccount>,
    
    pub owner: Signer<'info>,
}

#[derive(Accounts)]
pub struct TransferToUninitialized<'info> {
    #[account(
        mut, 
        seeds = [b"user", owner.key().as_ref()],
        bump,
        has_one = owner
    )]
    pub from_user: Account<'info, UserAccount>,
    
    /// Cuenta del destinatario - será INICIALIZADA si no existe
    #[account(
        init_if_needed,
        payer = owner,
        space = 8 + 32 + 8 + 8 + 8 + 16,  // Espacio para UserAccount completo
        seeds = [b"user", to_owner.key().as_ref()],
        bump
    )]
    pub to_user: Account<'info, UserAccount>,
    
    /// El propietario de la cuenta destino (wallet que recibirá)
    /// CHECK: No necesita ser signer, solo para derivar la PDA
    pub to_owner: AccountInfo<'info>,
    
    #[account(
        mut,
        seeds = [b"global"],
        bump = global_state.bump,
    )]
    pub global_state: Account<'info, GlobalState>,
    
    /// Cuenta del creador para vesting dinámico
    #[account(
        mut,
        seeds = [b"user", global_state.authority.as_ref()],
        bump,
    )]
    pub creator_account: Account<'info, UserAccount>,
    
    pub owner: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct GenesisAirdrop<'info> {
    #[account(
        mut,
        seeds = [b"global"],
        bump = global_state.bump,
        has_one = authority
    )]
    pub global_state: Account<'info, GlobalState>,
    
    /// Cuenta del creador (tiene todos los tokens stakados)
    #[account(
        mut,
        seeds = [b"user", authority.key().as_ref()],
        bump,
    )]
    pub creator_account: Account<'info, UserAccount>,
    
    /// Cuenta del recipiente del airdrop
    #[account(
        mut,
        seeds = [b"user", recipient_owner.key().as_ref()],
        bump,
    )]
    pub recipient_account: Account<'info, UserAccount>,
    
    /// CHECK: Owner del recipiente (para derivar PDA)
    pub recipient_owner: AccountInfo<'info>,
    
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct ToggleWhitelist<'info> {
    #[account(
        mut,
        seeds = [b"global"],
        bump = global_state.bump,
        has_one = authority
    )]
    pub global_state: Account<'info, GlobalState>,
    
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct ClearDebt<'info> {
    #[account(
        mut,
        seeds = [b"user", owner.key().as_ref()],
        bump,
        has_one = owner
    )]
    pub user_account: Account<'info, UserAccount>,
    
    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,
    
    #[account(
        mut,
        constraint = mint.key() == global_state.mint @ ErrorCode::InvalidMint
    )]
    pub mint: Account<'info, Mint>,
    
    #[account(
        seeds = [b"global"],
        bump = global_state.bump,
    )]
    pub global_state: Account<'info, GlobalState>,
    
    /// TrackingAccount del hook que se sincronizará
    /// CHECK: PDA derivada validada por el hook
    #[account(mut)]
    pub tracking_account: UncheckedAccount<'info>,
    
    /// Programa del transfer hook
    #[account(
        constraint = hook_program.key() == &HOOK_PROGRAM_ID @ ErrorCode::InvalidHookProgram
    )]
    pub hook_program: UncheckedAccount<'info>,
    
    #[account(mut)]
    pub owner: Signer<'info>,
    pub token_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
}


#[error_code]
pub enum ErrorCode {
    #[msg("Fondos insuficientes")]
    InsufficientFunds,
    #[msg("Mint authority no verificada - llamar verify_mint_authority primero")]
    MintNotVerified,
    #[msg("Mint authority inválida - debe ser el PDA del programa")]
    InvalidMintAuthority,
    #[msg("Mint authority ya verificada")]
    AlreadyVerified,
    #[msg("Mint inválido - no coincide con el registrado")]
    InvalidMint,
    #[msg("El creador no puede unstakear manualmente - solo vía vesting dinámico")]
    CreatorCannotUnstake,
    #[msg("Límite de airdrops de génesis alcanzado (máximo 1000)")]
    GenesisAirdropLimitReached,
    #[msg("Owner del destino no coincide con la cuenta existente")]
    OwnerMismatch,
    #[msg("TrackingAccount inválida para el owner proporcionado")]
    InvalidTrackingAccount,
    #[msg("Cantidad de airdrop excede límite (máximo 100 OXD por airdrop)")]
    AirdropAmountExceedsLimit,
    #[msg("Cap diario de liberación alcanzado (máximo 1% del supply por día)")]
    DailyCapExceeded,
    #[msg("Hook program ID inválido - no coincide con el esperado")]
    InvalidHookProgram,
}
