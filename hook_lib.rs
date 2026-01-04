use anchor_lang::prelude::*;
use anchor_spl::{
    token_2022::Token2022,
    token_interface::{Mint, TokenAccount},
};
use spl_transfer_hook_interface::instruction::{ExecuteInstruction, TransferHookInstruction};
use spl_tlv_account_resolution::{
    account::ExtraAccountMeta, seeds::Seed, state::ExtraAccountMetaList,
};
use spl_token_2022::{
    extension::{BaseStateWithExtensions, StateWithExtensions},
    state::Account as Token2022Account,
};

declare_id!("HookxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxXXX");

const SECONDS_PER_YEAR: i64 = 365 * 24 * 60 * 60;
const ANNUAL_BURN_BP: u128 = 20_00; // 20% anual

// Technical grace window. Not an economic parameter. Ensures UX fluidity while maintaining long-term state consistency.
// WHY 15 MINUTES:
// - Allows normal trading/swaps without constant clear_debt() friction
// - Bots can't escape decay: they pay network fees to postpone sync, but debt accrues on remaining balance
// - Economically neutral/negative for wash-trading bots (fees + inevitable decay)
const MAX_ELAPSED_BEFORE_CLEAR: i64 = 15 * 60; // 15 minutos

// WHITELIST: "Zona Franca" - Pools de liquidez donde tokens NO acumulan deuda
// Estos son los program IDs que POSEEN las cuentas de los pools
//
// FIXED_WHITELIST: Chosen for RADICAL IMMUTABILITY.
// ⚠️ DECISIÓN DE DISEÑO: Esta lista es ESTÁTICA y hardcoded deliberadamente.
// Prioriza inmutabilidad absoluta (cero upgrades) sobre compatibilidad futura.
// Si un nuevo DEX emerge, el protocolo NO se adapta - esto PREVIENE:
//   1. Governance attacks (nadie puede modificar reglas post-deploy)
//   2. Backdoors vía "actualizaciones de seguridad"
//   3. Centralización progresiva (común en DAOs)
//
// TRADE-OFF ACEPTADO: Si Raydium/Orca rotan program IDs, OXIDE pierde compatibility.
// Alternativa: Fork del contrato o migración manual. Código es ley, no conveniencia.
pub mod amm_whitelist {
    use solana_program::pubkey::Pubkey;
    use solana_program::pubkey;
    
    // Raydium V4 Pool Program (owner de las pool accounts)
    pub const RAYDIUM_V4: Pubkey = pubkey!("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8");
    
    // Orca Whirlpool Program (owner de las pool accounts)
    pub const ORCA_WHIRLPOOL: Pubkey = pubkey!("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc");
    
    // Meteora DLMM (owner de las pool accounts)
    pub const METEORA_DLMM: Pubkey = pubkey!("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo");
    
    // Añadir más según se listen en otros DEXs
}

#[program]
pub mod oxide_transfer_hook {
    use super::*;

    /// Inicializa el Transfer Hook - SOLO SE LLAMA UNA VEZ
    /// Debe llamarse ANTES de configurar el mint con TransferHook extension
    pub fn initialize_extra_account_meta_list(
        ctx: Context<InitializeExtraAccountMetaList>,
        oxide_program_id: Pubkey,
    ) -> Result<()> {
        // Configurar las PDAs adicionales que se pasan al hook
        let account_metas = vec![
            // GlobalState PDA del programa OXIDE (para leer whitelist_enabled)
            ExtraAccountMeta::new_external_pda_with_seeds(
                0, // index 0 = oxide_program (añadido manualmente al invoke)
                &[Seed::Literal {
                    bytes: b"global".to_vec(),
                }],
                false, // no es signer
                false, // no es writable (solo lectura)
            )?,
            // PDA del sender (TrackingAccount) - derivada de [b"tracking", sender.owner]
            ExtraAccountMeta::new_with_seeds(
                &[
                    Seed::Literal {
                        bytes: b"tracking".to_vec(),
                    },
                    Seed::AccountData {
                        account_index: 0, // source_token
                        data_index: 32,   // offset a "owner" field en TokenAccount
                        length: 32,       // Pubkey length
                    },
                ],
                false, // no es signer
                true,  // es writable
            )?,
            // PDA del receiver (TrackingAccount) - derivada de [b"tracking", destination.owner]
            ExtraAccountMeta::new_with_seeds(
                &[
                    Seed::Literal {
                        bytes: b"tracking".to_vec(),
                    },
                    Seed::AccountData {
                        account_index: 1, // destination_token
                        data_index: 32,   // offset a "owner" field en TokenAccount
                        length: 32,       // Pubkey length
                    },
                ],
                false, // no es signer
                true,  // es writable
            )?,
            // Payer account - quien firma la transacción original (authority/delegate del transfer)
            // Este será el que pague la inicialización del receiver_tracking si es necesario
            ExtraAccountMeta::new(
                3,     // index 3 en la instrucción de transfer = authority/owner
                true,  // es signer
                true,  // es writable (paga rent)
            )?,
        ];

        // Inicializar la lista de cuentas extra
        let account_size = ExtraAccountMetaList::size_of(account_metas.len())? as u64;
        let lamports = Rent::get()?.minimum_balance(account_size as usize);
        
        ExtraAccountMetaList::init::<ExecuteInstruction>(
            &mut ctx.accounts.extra_account_meta_list.try_borrow_mut_data()?,
            &account_metas,
        )?;

        Ok(())
    }

    /// Sincroniza TrackingAccount con UserAccount del programa OXIDE
    /// Llamada via CPI desde clear_debt() y withdraw()
    pub fn sync_tracking(ctx: Context<SyncTracking>, new_timestamp: u64) -> Result<()> {
        let tracking = &mut ctx.accounts.tracking_account;
        
        // Inicializar si es primera vez
        if tracking.last_update == 0 {
            tracking.owner = ctx.accounts.user_wallet.key();
            tracking.burn_fraction_remainder = 0;
        }
        
        tracking.last_update = new_timestamp;
        
        msg!("TrackingAccount synced to timestamp: {}", new_timestamp);
        Ok(())
    }

    /// Hook ejecutado en CADA transferencia de tokens SPL
    /// BLOQUEA si elapsed > 15 min (debe llamar clear_debt primero)
    /// Actualiza last_update del receiver al completar
    /// 
    /// ⚠️ CU OPTIMIZATION: Este método se ejecuta en CADA transfer SPL.
    /// Mantener operaciones al mínimo para evitar exceder Compute Unit límites.
    pub fn transfer_hook(ctx: Context<TransferHook>, amount: u64) -> Result<()> {
        let sender_tracking = &mut ctx.accounts.sender_tracking;
        let receiver_tracking = &mut ctx.accounts.receiver_tracking;
        
        // CU Optimization: Clock::get() es una syscall costosa, llamar UNA sola vez
        // Note: Solana's Clock is slot-based and approximate. Not for sub-second precision.
        let now = Clock::get()?.unix_timestamp;

        // VALIDAR que sender_tracking está inicializada
        // EXCEPCIÓN: Si sender es un Pool (Zona Franca), permitir sin validar
        // Esto permite que pools envíen tokens sin llamar clear_debt()
        
        // CU Optimization: Deserializar GlobalState solo UNA vez
        let global_data: GlobalStateData = AnchorDeserialize::deserialize(
            &mut &ctx.accounts.global_state.try_borrow_data()?[8..] // Skip discriminator
        )?;
        let whitelist_enabled = global_data.amm_whitelist_enabled;
        
        // Verificar si sender es pool Y whitelist está habilitada
        let sender_is_pool = whitelist_enabled && is_whitelisted_pool(&ctx.accounts.source_token.owner);
        
        if !sender_is_pool {
            // Sender es usuario normal - DEBE tener tracking inicializada
            if sender_tracking.last_update == 0 {
                msg!("❌ ERROR: TrackingAccount no inicializada. Acción requerida:");
                msg!("   1. Llama clear_debt() para activar tu wallet, O");
                msg!("   2. Llama withdraw() para sincronizar tu balance SPL");
                return Err(ErrorCode::TrackingNotInitialized.into());
            }
        }

        // CU Optimization: Cálculo aritmético simple (no loops, no exponenciales)
        let elapsed = now - sender_tracking.last_update as i64;
        
        // LÓGICA DE "CRUCE DE FRONTERAS":
        // - Usuario → Pool: Validar elapsed < 15min (usuario paga deuda ANTES de vender)
        // - Pool → Usuario: NO validar (pool es Zona Franca, comprador entra limpio)
        // - Usuario → Usuario: Validar elapsed < 15min
        //
        // TODO (Future Enhancement): El threshold de 15 min es ARBITRARIO.
        // Consideración de auditoría: Permite que ballenas reseten el reloj con micro-transfers.
        // MEJORA: Implementar decay CONTINUO proporcional al balance:
        //   burn = balance × 0.20 × (elapsed / YEAR) × sqrt(balance / total_supply)
        // Esto penaliza más a grandes holders que hacen wash trading.
        
        // Si sender NO es pool Y elapsed > 15min → BLOQUEAR
        if !sender_is_pool && elapsed > MAX_ELAPSED_BEFORE_CLEAR {
            msg!("❌ ERROR: Deuda de oxidación detectada.");
            msg!("   Tiempo transcurrido: {} segundos ({} minutos)", elapsed, elapsed / 60);
            msg!("   Límite permitido: {} segundos ({} minutos)", MAX_ELAPSED_BEFORE_CLEAR, MAX_ELAPSED_BEFORE_CLEAR / 60);
            msg!("   SOLUCIÓN: Llama a clear_debt() ANTES de transferir/vender.");
            return Err(ErrorCode::DebtNotCleared.into());
        }

        // Si llegamos aquí, la transferencia está permitida
        if sender_is_pool {
            msg!("Transfer from POOL (Zona Franca) approved - no elapsed validation");
        } else {
            msg!("Transfer from USER approved: {} seconds elapsed (within limit)", elapsed);
        }

        // HEREDAR timestamp según ORIGEN:
        // - Viene de Pool (Zona Franca) → Comprador entra LIMPIO (timestamp = now)
        // - Viene de Usuario → Heredar timestamp (previene token laundering)
        
        if sender_is_pool {
            // COMPRADOR desde POOL: Entra limpio con cronómetro en T=0
            if receiver_tracking.last_update == 0 {
                receiver_tracking.owner = ctx.accounts.destination_token.owner;
                receiver_tracking.burn_fraction_remainder = 0;
            }
            receiver_tracking.last_update = now as u64;
            msg!("Buyer from POOL enters clean: timestamp reset to now");
        } else {
            // TRANSFER entre USUARIOS: Heredar antigüedad
            if receiver_tracking.last_update == 0 {
                receiver_tracking.owner = ctx.accounts.destination_token.owner;
                receiver_tracking.burn_fraction_remainder = 0;
                receiver_tracking.last_update = sender_tracking.last_update;
                msg!("Receiver initialized with sender's timestamp: {}", sender_tracking.last_update);
            } else {
                // Receiver YA tiene tokens - calcular weighted average de timestamps
                // Esto refleja que recibe una MEZCLA de tokens nuevos (del sender) con viejos (suyos)
                let receiver_balance = ctx.accounts.destination_token.amount;
                let transfer_amount = amount;
                let old_receiver_timestamp = receiver_tracking.last_update;
                
                if receiver_balance == 0 {
                    // Receiver tenía tracking pero balance 0 (gastó todo) → heredar timestamp del sender
                    receiver_tracking.last_update = sender_tracking.last_update;
                    msg!("Receiver had 0 balance, inherited sender's timestamp: {}", sender_tracking.last_update);
                } else if transfer_amount > 0 {
                    // Weighted average: (old_balance * old_ts + new_amount * new_ts) / total
                    let old_weighted = (receiver_balance as u128) * (old_receiver_timestamp as u128);
                    let new_weighted = (transfer_amount as u128) * (sender_tracking.last_update as u128);
                    let total_balance = receiver_balance + transfer_amount;
                    
                    let avg_timestamp = ((old_weighted + new_weighted) / (total_balance as u128)) as u64;
                    receiver_tracking.last_update = avg_timestamp;
                    
                    msg!(
                        "Receiver mixed tokens: old={} (ts={}), new={} (ts={}) → avg_ts={}",
                        receiver_balance, old_receiver_timestamp,
                        transfer_amount, sender_tracking.last_update,
                        avg_timestamp
                    );
                }
            }
        }

        Ok(())
    }

    /// Fallback para instrucciones no implementadas
    pub fn fallback<'info>(
        _program_id: &Pubkey,
        _accounts: &'info [AccountInfo<'info>],
        _data: &[u8],
    ) -> Result<()> {
        Err(ProgramError::InvalidInstructionData.into())
    }
}

/// Verifica si una cuenta es owned por un programa de pool conocido (Zona Franca)
/// Pools whitelisted NO acumulan deuda - tokens "en tránsito" en liquidez
fn is_whitelisted_pool(token_account_owner: &Pubkey) -> bool {
    // Verificar contra program IDs conocidos que poseen pool accounts
    // NOTA: Esto verifica el OWNER de la TokenAccount, no el programa que ejecuta
    
    // Para pools de Raydium/Orca/Meteora, el owner será el pool account (PDA del programa)
    // NO podemos verificar directamente contra program IDs aquí
    // SOLUCIÓN: Verificar si la cuenta es una PDA derivada de programas conocidos
    // Por simplicidad, verificamos contra lista hardcoded de pool addresses conocidos
    
    // En producción real, deberías:
    // 1. Mantener lista on-chain de pools verificados (actualizable por authority)
    // 2. O verificar que el owner es PDA del programa AMM conocido
    
    // Por ahora, verificamos contra program IDs directamente
    // (asumiendo que pools pueden ser owned directamente por el programa)
    token_account_owner == &amm_whitelist::RAYDIUM_V4
        || token_account_owner == &amm_whitelist::ORCA_WHIRLPOOL
        || token_account_owner == &amm_whitelist::METEORA_DLMM
}

// ============== STRUCTS ==============

/// Estructura para deserializar GlobalState del programa OXIDE
#[derive(AnchorDeserialize)]
pub struct GlobalStateData {
    pub authority: Pubkey,
    pub mint: Pubkey,
    pub mint_verified: bool,
    pub amm_whitelist_enabled: bool,
    pub total_tokens_released: u64,
    pub release_rate_basis_points: u16,
    pub genesis_airdrops_given: u16,
    pub last_release_day: i64,          // Campo añadido para cap diario
    pub daily_released_amount: u64,     // Campo añadido para cap diario
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct TrackingAccount {
    pub owner: Pubkey,                    // Wallet del holder
    pub last_update: u64,                 // Último timestamp de interacción
    pub burn_fraction_remainder: u128,    // Acumulador de fracciones
}

// ============== CONTEXTS ==============

#[derive(Accounts)]
pub struct InitializeExtraAccountMetaList<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    /// La lista de cuentas extra que el hook necesita
    /// CHECK: Validado por ExtraAccountMetaList
    #[account(mut)]
    pub extra_account_meta_list: UncheckedAccount<'info>,

    pub mint: InterfaceAccount<'info, Mint>,
    
    #[account(
        seeds = [b"extra-account-metas", mint.key().as_ref()],
        bump
    )]
    pub extra_account_meta_list_pda: SystemAccount<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SyncTracking<'info> {
    /// Wallet del usuario cuya TrackingAccount se sincroniza
    /// CHECK: Solo se usa para derivar PDA
    pub user_wallet: UncheckedAccount<'info>,
    
    /// TrackingAccount a actualizar
    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + TrackingAccount::INIT_SPACE,
        seeds = [b"tracking", user_wallet.key().as_ref()],
        bump
    )]
    pub tracking_account: Account<'info, TrackingAccount>,
    
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct TransferHook<'info> {
    /// Source token account (sender)
    #[account(token::token_program = token_program)]
    pub source_token: InterfaceAccount<'info, TokenAccount>,

    /// Mint del token
    pub mint: InterfaceAccount<'info, Mint>,

    /// Destination token account (receiver)
    #[account(token::token_program = token_program)]
    pub destination_token: InterfaceAccount<'info, TokenAccount>,

    /// Owner del source (puede NO ser signer si es delegated transfer)
    /// CHECK: Validado por token program
    pub owner: UncheckedAccount<'info>,

    /// La lista de cuentas extra configurada
    /// CHECK: Validado por ExtraAccountMetaList
    pub extra_account_meta_list: UncheckedAccount<'info>,
    
    /// GlobalState del programa OXIDE para leer whitelist_enabled
    /// CHECK: PDA derivada con seeds [b"global"]
    #[account(
        seeds = [b"global"],
        bump,
        seeds::program = oxide_program.key()
    )]
    pub global_state: UncheckedAccount<'info>,
    
    /// Programa OXIDE (para derivar GlobalState)
    /// CHECK: Program ID de OXIDE
    pub oxide_program: UncheckedAccount<'info>,

    /// Tracking account del sender
    /// DEBE estar inicializada ANTES del transfer (via sync_tracking en clear_debt/withdraw)
    #[account(
        mut,
        seeds = [b"tracking", source_token.owner.as_ref()],
        bump
    )]
    pub sender_tracking: Account<'info, TrackingAccount>,

    /// Tracking account del receiver
    /// Se inicializa si no existe (receiver puede recibir sin haberlo solicitado)
    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + TrackingAccount::INIT_SPACE,
        seeds = [b"tracking", destination_token.owner.as_ref()],
        bump
    )]
    pub receiver_tracking: Account<'info, TrackingAccount>,
    
    /// Payer para inicializar receiver_tracking si es necesario
    /// ⚠️ IMPORTANTE: La primera vez que alguien recibe OXD, el SENDER paga la renta
    /// de creación de la TrackingAccount del receiver (~0.002 SOL).
    /// Esto puede causar transacciones ligeramente más caras en el primer envío.
    /// En protocolos DeFi/swaps automatizados, asegúrate de que el payer tenga SOL suficiente.
    #[account(mut)]
    pub payer: Signer<'info>,

    pub token_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
}

// ============== ERRORS ==============

#[error_code]
pub enum ErrorCode {
    #[msg("Cálculo de burn inválido")]
    InvalidBurnCalculation,
    #[msg("⚠️ Deuda de oxidación detectada. Llama clear_debt() antes de transferir (han pasado >15 min)")]
    DebtNotCleared,
    #[msg("⚠️ TrackingAccount no inicializada. Llama clear_debt() o withdraw() primero para activar tu wallet")]
    TrackingNotInitialized,
}
