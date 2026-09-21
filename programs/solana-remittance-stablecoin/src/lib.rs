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
}
