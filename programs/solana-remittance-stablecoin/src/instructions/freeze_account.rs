use anchor_lang::prelude::*;
// Aliased so it does not clash with the Accounts struct below.
use anchor_spl::token_interface::{
    freeze_account, FreezeAccount as FreezeAccountCpi, Mint, Token2022, TokenAccount,
};

#[derive(Accounts)]
pub struct FreezeAccount<'info> {
    /// The mint's freeze authority.
    pub authority: Signer<'info>,

    #[account(mint::freeze_authority = authority)]
    pub mint: InterfaceAccount<'info, Mint>,

    #[account(mut, token::mint = mint)]
    pub token_account: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Program<'info, Token2022>,
}

/// Sanction an account. A frozen account cannot send, receive, deposit,
/// withdraw or make confidential transfers.
pub fn handle_freeze_account(ctx: Context<FreezeAccount>) -> Result<()> {
    freeze_account(CpiContext::new(
        ctx.accounts.token_program.key(),
        FreezeAccountCpi {
            account: ctx.accounts.token_account.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
            authority: ctx.accounts.authority.to_account_info(),
        },
    ))
}
