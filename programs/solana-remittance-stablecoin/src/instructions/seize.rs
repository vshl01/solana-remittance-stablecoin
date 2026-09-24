use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    freeze_account, thaw_account, transfer_checked_with_fee, FreezeAccount, Mint, ThawAccount,
    Token2022, TokenAccount, TransferCheckedWithFee,
};

use crate::error::ErrorCode;
use crate::utils::{expected_transfer_fee, permanent_delegate};

#[derive(Accounts)]
pub struct Seize<'info> {
    /// Issuer acting as both permanent delegate and freeze authority.
    pub authority: Signer<'info>,

    #[account(mint::freeze_authority = authority)]
    pub mint: InterfaceAccount<'info, Mint>,

    /// The sanctioned wallet's token account (usually already frozen).
    #[account(mut, token::mint = mint)]
    pub sanctioned_account: InterfaceAccount<'info, TokenAccount>,

    /// Issuer-controlled account that receives the seized funds.
    #[account(mut, token::mint = mint)]
    pub treasury: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Program<'info, Token2022>,
}

/// Move `amount` out of a sanctioned account without the owner's signature.
///
/// Token-2022 blocks transfers from frozen accounts even for the permanent
/// delegate, so this thaws, transfers and re-freezes in one instruction. The
/// account is always left frozen.
///
/// This only reaches the *public* balance. Tokens the owner has moved into
/// their confidential balance cannot be moved by anyone but the owner.
pub fn handle_seize(ctx: Context<Seize>, amount: u64) -> Result<()> {
    let mint = ctx.accounts.mint.to_account_info();
    let sanctioned = ctx.accounts.sanctioned_account.to_account_info();
    let authority = ctx.accounts.authority.to_account_info();
    let token_program = ctx.accounts.token_program.key();

    require!(
        permanent_delegate(&mint)? == Some(authority.key()),
        ErrorCode::NotPermanentDelegate
    );

    if ctx.accounts.sanctioned_account.is_frozen() {
        thaw_account(CpiContext::new(
            token_program,
            ThawAccount {
                account: sanctioned.clone(),
                mint: mint.clone(),
                authority: authority.clone(),
            },
        ))?;
    }

    let fee = expected_transfer_fee(&mint, amount)?;
    transfer_checked_with_fee(
        CpiContext::new(
            token_program,
            TransferCheckedWithFee {
                token_program_id: ctx.accounts.token_program.to_account_info(),
                source: sanctioned.clone(),
                mint: mint.clone(),
                destination: ctx.accounts.treasury.to_account_info(),
                authority: authority.clone(),
            },
        ),
        amount,
        ctx.accounts.mint.decimals,
        fee,
    )?;

    freeze_account(CpiContext::new(
        token_program,
        FreezeAccount {
            account: sanctioned,
            mint,
            authority,
        },
    ))
}
