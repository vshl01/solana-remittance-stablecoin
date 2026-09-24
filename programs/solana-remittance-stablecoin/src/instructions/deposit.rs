use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use anchor_spl::token_2022::spl_token_2022::extension::confidential_transfer::instruction::deposit;
use anchor_spl::token_interface::{Mint, Token2022, TokenAccount};

#[derive(Accounts)]
pub struct DepositConfidential<'info> {
    pub owner: Signer<'info>,

    pub mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        token::mint = mint,
        token::authority = owner,
    )]
    pub token_account: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Program<'info, Token2022>,
}

/// Move `amount` from the public balance into the *pending* confidential
/// balance. The amount itself is public here; later transfers are not.
pub fn handle_deposit_confidential(ctx: Context<DepositConfidential>, amount: u64) -> Result<()> {
    let ix = deposit(
        &ctx.accounts.token_program.key(),
        &ctx.accounts.token_account.key(),
        &ctx.accounts.mint.key(),
        amount,
        ctx.accounts.mint.decimals,
        &ctx.accounts.owner.key(),
        &[],
    )?;

    invoke(
        &ix,
        &[
            ctx.accounts.token_account.to_account_info(),
            ctx.accounts.mint.to_account_info(),
            ctx.accounts.owner.to_account_info(),
            ctx.accounts.token_program.to_account_info(),
        ],
    )?;

    Ok(())
}
