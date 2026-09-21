use anchor_lang::prelude::*;
// Aliased so it does not clash with the Accounts struct below.
use anchor_spl::token_interface::{
    thaw_account, Mint, ThawAccount as ThawAccountCpi, TokenAccount, TokenInterface,
};

#[derive(Accounts)]
pub struct ThawAccount<'info> {
    /// The mint's freeze authority.
    #[account(mut)]
    pub authority: Signer<'info>,

    /// The Token-2022 mint.
    pub mint: InterfaceAccount<'info, Mint>,

    /// The user's frozen ATA.
    #[account(
        mut,
        token::mint = mint,
        token::authority = owner,
    )]
    pub token_account: InterfaceAccount<'info, TokenAccount>,

    /// User who owns the ATA.
    /// CHECK: Only used to verify token account ownership.
    pub owner: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn handle_thaw_account(ctx: Context<ThawAccount>) -> Result<()> {
    thaw_account(CpiContext::new(
        ctx.accounts.token_program.key(),
        ThawAccountCpi {
            account: ctx.accounts.token_account.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
            authority: ctx.accounts.authority.to_account_info(),
        },
    ))?;

    Ok(())
}