use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use anchor_spl::token_2022::spl_token_2022::extension::confidential_transfer::{
    instruction::inner_configure_account, DecryptableBalance,
};
use anchor_spl::token_interface::{reallocate, Mint, Reallocate, Token2022, TokenAccount};
use spl_token_confidential_transfer_proof_extraction::instruction::ProofLocation;

use crate::constants::CONFIDENTIAL_ACCOUNT_EXTENSIONS;

#[derive(Accounts)]
pub struct ConfigureAccount<'info> {
    /// Owner of the token account. Anyone can create an ATA for this wallet,
    /// but only the owner can opt it into confidential transfers: they sign
    /// here and pay for the extra space.
    #[account(mut)]
    pub owner: Signer<'info>,

    /// Token-2022 mint
    pub mint: InterfaceAccount<'info, Mint>,

    /// The owner's token account being configured
    #[account(
        mut,
        token::mint = mint,
        token::authority = owner,
    )]
    pub token_account: InterfaceAccount<'info, TokenAccount>,

    /// CHECK: Context state account holding the pubkey-validity proof the
    /// client already verified with the ZK ElGamal proof program. Token-2022
    /// checks its owner and proof type.
    pub proof_context_state: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
}

pub fn handle_configure_account(
    ctx: Context<ConfigureAccount>,
    decryptable_zero_balance: [u8; 36],
    maximum_pending_balance_credit_counter: u64,
) -> Result<()> {
    let token_program = ctx.accounts.token_program.to_account_info();
    let token_account = ctx.accounts.token_account.to_account_info();
    let owner = ctx.accounts.owner.to_account_info();

    // 1. Make room for the confidential account extensions.
    reallocate(
        CpiContext::new(
            token_program.key(),
            Reallocate {
                account: token_account.clone(),
                payer: owner.clone(),
                authority: owner.clone(),
                system_program: ctx.accounts.system_program.to_account_info(),
            },
        ),
        &CONFIDENTIAL_ACCOUNT_EXTENSIONS,
    )?;

    // 2. Register the owner's ElGamal key. The account starts unapproved
    //    because the mint uses manual approval.
    let proof_context_state = ctx.accounts.proof_context_state.key();
    let ix = inner_configure_account(
        &token_program.key(),
        &token_account.key(),
        &ctx.accounts.mint.key(),
        &DecryptableBalance::from(decryptable_zero_balance),
        maximum_pending_balance_credit_counter,
        &owner.key(),
        &[],
        ProofLocation::ContextStateAccount(&proof_context_state),
    )?;

    invoke(
        &ix,
        &[
            token_account,
            ctx.accounts.mint.to_account_info(),
            ctx.accounts.proof_context_state.to_account_info(),
            owner,
            token_program,
        ],
    )?;

    Ok(())
}
