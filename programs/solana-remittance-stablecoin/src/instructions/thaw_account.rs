use anchor_lang::prelude::*;
// Aliased so it does not clash with the Accounts struct below.
use anchor_spl::token_interface::{
    thaw_account, Mint, ThawAccount as ThawAccountCpi, Token2022, TokenAccount,
};

#[derive(Accounts)]
pub struct ThawAccount<'info> {
    /// The mint's freeze authority (the KYC operator).
    pub authority: Signer<'info>,

    #[account(mint::freeze_authority = authority)]
    pub mint: InterfaceAccount<'info, Mint>,

    /// The KYC'd user's frozen token account.
    #[account(
        mut,
        token::mint = mint,
        token::authority = owner,
    )]
    pub token_account: InterfaceAccount<'info, TokenAccount>,

    /// CHECK: The wallet that passed KYC; only used to bind the thaw to it.
    pub owner: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token2022>,
}

/// Unfreeze one account after KYC. This touches only that account: the mint's
/// DefaultAccountState stays Frozen, so every other new account still starts
/// frozen.
pub fn handle_thaw_account(ctx: Context<ThawAccount>) -> Result<()> {
    thaw_account(CpiContext::new(
        ctx.accounts.token_program.key(),
        ThawAccountCpi {
            account: ctx.accounts.token_account.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
            authority: ctx.accounts.authority.to_account_info(),
        },
    ))
}
