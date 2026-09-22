use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use anchor_spl::token_2022::spl_token_2022::{
    extension::{
        transfer_fee::instruction::transfer_checked_with_fee,
        transfer_fee::TransferFeeConfig,
        BaseStateWithExtensions,
        StateWithExtensions,
    },
};

use anchor_lang::solana_program::program::invoke;

use crate::error::ErrorCode;

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

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn handle_transfer(
    ctx: Context<Transfer>,
    amount: u64,
) -> Result<()> {

    // Current Solana epoch
    let current_epoch = Clock::get()?.epoch;

    // Read the mint using StateWithExtensions.
    // Scoped so the data borrow ends before the CPI below.
    let mint_info = ctx.accounts.mint.to_account_info();
    let fee = {
        let mint_data = mint_info.try_borrow_data()?;

        let mint = StateWithExtensions::<
            anchor_spl::token_2022::spl_token_2022::state::Mint,
        >::unpack(&mint_data)?;

        // Get TransferFeeConfig extension
        let transfer_fee_config = mint.get_extension::<TransferFeeConfig>()?;

        // Calculate fee using the CURRENT epoch
        transfer_fee_config
            .calculate_epoch_fee(current_epoch, amount)
            .ok_or_else(|| error!(ErrorCode::FeeCalculationFailed))?
    };

    // Create Token-2022 TransferCheckedWithFee instruction
    let ix = transfer_checked_with_fee(
        &ctx.accounts.token_program.key(),
        &ctx.accounts.source.key(),
        &ctx.accounts.mint.key(),
        &ctx.accounts.destination.key(),
        &ctx.accounts.authority.key(),
        &[],
        amount,
        ctx.accounts.mint.decimals,
        fee,
    )?;

    // Execute Token-2022 CPI
    invoke(
        &ix,
        &[
            ctx.accounts.source.to_account_info(),
            ctx.accounts.mint.to_account_info(),
            ctx.accounts.destination.to_account_info(),
            ctx.accounts.authority.to_account_info(),
        ],
    )?;

    Ok(())
}

