use anchor_lang::prelude::*;
use anchor_spl::token_interface::{close_account, CloseAccount, Mint, Token2022};

use crate::error::ErrorCode;

#[derive(Accounts)]
pub struct CloseMint<'info> {
    /// MintCloseAuthority.
    pub authority: Signer<'info>,

    #[account(mut, constraint = mint.supply == 0 @ ErrorCode::MintHasSupply)]
    pub mint: InterfaceAccount<'info, Mint>,

    /// CHECK: Only receives the mint's rent lamports.
    #[account(mut)]
    pub destination: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token2022>,
}

/// Decommission the mint. Only possible because of MintCloseAuthority, and
/// only once every token has been burned.
pub fn handle_close_mint(ctx: Context<CloseMint>) -> Result<()> {
    close_account(CpiContext::new(
        ctx.accounts.token_program.key(),
        CloseAccount {
            account: ctx.accounts.mint.to_account_info(),
            destination: ctx.accounts.destination.to_account_info(),
            authority: ctx.accounts.authority.to_account_info(),
        },
    ))
}
