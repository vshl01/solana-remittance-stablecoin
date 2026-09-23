use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use anchor_spl::token_2022::spl_token_2022::extension::confidential_transfer::instruction::approve_account;

#[derive(Accounts)]
pub struct ApproveAccount<'info> {
    /// Confidential-transfer authority on the mint (the issuer).
    /// Needed because the mint was created with approve_policy = manual.
    pub authority: Signer<'info>,

    /// Token-2022 mint
    pub mint: InterfaceAccount<'info, Mint>,

    /// The already-configured token account awaiting approval
    #[account(
        mut,
        token::mint = mint,
    )]
    pub token_account: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn handle_approve_account(ctx: Context<ApproveAccount>) -> Result<()> {
    let ix = approve_account(
        &ctx.accounts.token_program.key(),
        &ctx.accounts.token_account.key(),
        &ctx.accounts.mint.key(),
        &ctx.accounts.authority.key(),
        &[],
    )?;

    invoke(
        &ix,
        &[
            ctx.accounts.token_account.to_account_info(),
            ctx.accounts.mint.to_account_info(),
            ctx.accounts.authority.to_account_info(),
        ],
    )?;

    Ok(())
}
