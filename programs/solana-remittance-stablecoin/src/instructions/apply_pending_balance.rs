use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use anchor_spl::token_2022::spl_token_2022::extension::confidential_transfer::{
    instruction::inner_apply_pending_balance, DecryptableBalance,
};
use anchor_spl::token_interface::{Token2022, TokenAccount};

#[derive(Accounts)]
pub struct ApplyPendingBalance<'info> {
    pub owner: Signer<'info>,

    #[account(mut, token::authority = owner)]
    pub token_account: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Program<'info, Token2022>,
}

/// Fold the pending balance into the available balance. Deposits and incoming
/// confidential transfers land in *pending*; only *available* funds can be
/// transferred or withdrawn.
///
/// `expected_pending_balance_credit_counter` is the number of credits the
/// client decrypted, and `new_decryptable_available_balance` is the new total
/// encrypted under the owner's AES key.
pub fn handle_apply_pending_balance(
    ctx: Context<ApplyPendingBalance>,
    expected_pending_balance_credit_counter: u64,
    new_decryptable_available_balance: [u8; 36],
) -> Result<()> {
    let ix = inner_apply_pending_balance(
        &ctx.accounts.token_program.key(),
        &ctx.accounts.token_account.key(),
        expected_pending_balance_credit_counter,
        &DecryptableBalance::from(new_decryptable_available_balance),
        &ctx.accounts.owner.key(),
        &[],
    )?;

    invoke(
        &ix,
        &[
            ctx.accounts.token_account.to_account_info(),
            ctx.accounts.owner.to_account_info(),
            ctx.accounts.token_program.to_account_info(),
        ],
    )?;

    Ok(())
}
