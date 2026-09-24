//! Remittance stablecoin on Token-2022.
//!
//! Mint #1 (`initialize_mint`): protocol fee, self-pointing on-chain metadata,
//! accounts frozen until KYC, and a close authority.
//! Mint #2 (`reissue_confidential_mint`): the same, plus a permanent delegate
//! for seizure and manually approved confidential transfers.

pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;
pub mod utils;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("EP4wQ8HCQZM8Jx8PhSn8YUNXWUb3Yx8XNFwmiSFoF3kU");

#[program]
pub mod solana_remittance_stablecoin {
    use super::*;

    // ---------------------------------------------------------------- mints

    pub fn initialize_mint(ctx: Context<InitializeMint>, params: MintParams) -> Result<()> {
        handle_initialize_mint(ctx, params)
    }

    pub fn reissue_confidential_mint(
        ctx: Context<ReissueConfidentialMint>,
        params: MintParams,
        confidential: ConfidentialParams,
    ) -> Result<()> {
        handle_reissue_confidential_mint(ctx, params, confidential)
    }

    pub fn close_mint(ctx: Context<CloseMint>) -> Result<()> {
        handle_close_mint(ctx)
    }

    // ----------------------------------------------------------- compliance

    pub fn thaw_account(ctx: Context<ThawAccount>) -> Result<()> {
        handle_thaw_account(ctx)
    }

    pub fn freeze_account(ctx: Context<FreezeAccount>) -> Result<()> {
        handle_freeze_account(ctx)
    }

    pub fn seize(ctx: Context<Seize>, amount: u64) -> Result<()> {
        handle_seize(ctx, amount)
    }

    // ------------------------------------------------------ public transfer

    pub fn transfer(ctx: Context<Transfer>, amount: u64) -> Result<()> {
        handle_transfer(ctx, amount)
    }

    // ------------------------------------------------ confidential transfer

    pub fn configure_account(
        ctx: Context<ConfigureAccount>,
        decryptable_zero_balance: [u8; 36],
        maximum_pending_balance_credit_counter: u64,
    ) -> Result<()> {
        handle_configure_account(
            ctx,
            decryptable_zero_balance,
            maximum_pending_balance_credit_counter,
        )
    }

    pub fn approve_account(ctx: Context<ApproveAccount>) -> Result<()> {
        handle_approve_account(ctx)
    }

    pub fn deposit_confidential(ctx: Context<DepositConfidential>, amount: u64) -> Result<()> {
        handle_deposit_confidential(ctx, amount)
    }

    pub fn apply_pending_balance(
        ctx: Context<ApplyPendingBalance>,
        expected_pending_balance_credit_counter: u64,
        new_decryptable_available_balance: [u8; 36],
    ) -> Result<()> {
        handle_apply_pending_balance(
            ctx,
            expected_pending_balance_credit_counter,
            new_decryptable_available_balance,
        )
    }

    pub fn confidential_transfer(
        ctx: Context<ConfidentialTransfer>,
        new_source_decryptable_available_balance: [u8; 36],
        transfer_amount_auditor_ciphertext_lo: [u8; 64],
        transfer_amount_auditor_ciphertext_hi: [u8; 64],
    ) -> Result<()> {
        handle_confidential_transfer(
            ctx,
            new_source_decryptable_available_balance,
            transfer_amount_auditor_ciphertext_lo,
            transfer_amount_auditor_ciphertext_hi,
        )
    }

    pub fn withdraw_confidential(
        ctx: Context<WithdrawConfidential>,
        amount: u64,
        new_decryptable_available_balance: [u8; 36],
    ) -> Result<()> {
        handle_withdraw_confidential(ctx, amount, new_decryptable_available_balance)
    }
}
