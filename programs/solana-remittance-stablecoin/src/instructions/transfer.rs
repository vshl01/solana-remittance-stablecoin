use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    transfer_checked_with_fee, Mint, Token2022, TokenAccount, TransferCheckedWithFee,
};

use crate::utils::expected_transfer_fee;

#[derive(Accounts)]
pub struct Transfer<'info> {
    /// User sending the tokens
    pub authority: Signer<'info>,

    /// Token-2022 mint
    pub mint: InterfaceAccount<'info, Mint>,

    /// User's token account
    #[account(
        mut,
        token::mint = mint,
        token::authority = authority,
    )]
    pub source: InterfaceAccount<'info, TokenAccount>,

    /// Receiver's token account
    #[account(
        mut,
        token::mint = mint,
    )]
    pub destination: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Program<'info, Token2022>,
}

/// Public transfer that always pays the protocol fee. The fee is computed from
/// the rate active in the current epoch and passed to TransferCheckedWithFee,
/// which makes Token-2022 reject the transfer if the sender expected a
/// different fee.
pub fn handle_transfer(ctx: Context<Transfer>, amount: u64) -> Result<()> {
    let fee = expected_transfer_fee(&ctx.accounts.mint.to_account_info(), amount)?;

    transfer_checked_with_fee(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            TransferCheckedWithFee {
                token_program_id: ctx.accounts.token_program.to_account_info(),
                source: ctx.accounts.source.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
                destination: ctx.accounts.destination.to_account_info(),
                authority: ctx.accounts.authority.to_account_info(),
            },
        ),
        amount,
        ctx.accounts.mint.decimals,
        fee,
    )
}
