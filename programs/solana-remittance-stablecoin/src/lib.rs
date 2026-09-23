pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("EP4wQ8HCQZM8Jx8PhSn8YUNXWUb3Yx8XNFwmiSFoF3kU");

#[program]
pub mod solana_remittance_stablecoin {
    use super::*;

    pub fn initialize_mint(
        ctx: Context<InitializeMint>,
        decimals: u8,
        transfer_fee_basis_points: u16,
        maximum_fee: u64,
    ) -> Result<()> {
        handle_initialize_mint(ctx, decimals, transfer_fee_basis_points, maximum_fee)
    }

    pub fn thaw_account(ctx: Context<ThawAccount>) -> Result<()> {
        handle_thaw_account(ctx)
    }

    pub fn transfer(ctx: Context<Transfer>, amount: u64) -> Result<()> {
        transfer::handle_transfer(ctx, amount)
    }

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
}
