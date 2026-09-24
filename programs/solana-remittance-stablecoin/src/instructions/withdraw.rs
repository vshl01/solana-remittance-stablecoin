use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use anchor_spl::token_2022::spl_token_2022::extension::confidential_transfer::{
    instruction::inner_withdraw, DecryptableBalance,
};
use anchor_spl::token_interface::{Mint, Token2022, TokenAccount};
use spl_token_confidential_transfer_proof_extraction::instruction::ProofLocation;

#[derive(Accounts)]
pub struct WithdrawConfidential<'info> {
    pub owner: Signer<'info>,

    pub mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        token::mint = mint,
        token::authority = owner,
    )]
    pub token_account: InterfaceAccount<'info, TokenAccount>,

    /// CHECK: Verified ciphertext-commitment equality proof context for the
    /// remaining available balance. Token-2022 checks owner and proof type.
    pub equality_proof: UncheckedAccount<'info>,

    /// CHECK: Verified batched range proof (u64) context showing the
    /// remaining balance is not negative.
    pub range_proof: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token2022>,
}

/// Move `amount` from the *available* confidential balance back to the public
/// balance. Funds still in *pending* cannot be withdrawn: apply the pending
/// balance first, or the proofs will not match the on-chain available balance.
pub fn handle_withdraw_confidential(
    ctx: Context<WithdrawConfidential>,
    amount: u64,
    new_decryptable_available_balance: [u8; 36],
) -> Result<()> {
    let equality = ctx.accounts.equality_proof.key();
    let range = ctx.accounts.range_proof.key();

    let ix = inner_withdraw(
        &ctx.accounts.token_program.key(),
        &ctx.accounts.token_account.key(),
        &ctx.accounts.mint.key(),
        amount,
        ctx.accounts.mint.decimals,
        &DecryptableBalance::from(new_decryptable_available_balance),
        &ctx.accounts.owner.key(),
        &[],
        ProofLocation::ContextStateAccount(&equality),
        ProofLocation::ContextStateAccount(&range),
    )?;

    invoke(
        &ix,
        &[
            ctx.accounts.token_account.to_account_info(),
            ctx.accounts.mint.to_account_info(),
            ctx.accounts.equality_proof.to_account_info(),
            ctx.accounts.range_proof.to_account_info(),
            ctx.accounts.owner.to_account_info(),
            ctx.accounts.token_program.to_account_info(),
        ],
    )?;

    Ok(())
}
