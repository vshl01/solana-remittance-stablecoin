use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use anchor_spl::token_2022::spl_token_2022::extension::confidential_transfer::{
    instruction::inner_configure_account, DecryptableBalance,
};
use spl_token_confidential_transfer_proof_extraction::instruction::ProofLocation;

#[derive(Accounts)]
pub struct ConfigureAccount<'info> {
    /// Owner of the token account. Creating an ATA is permissionless,
    /// but only the owner may configure it for confidential transfers.
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
    /// client already verified with the ZK ElGamal proof program.
    pub proof_context_state: UncheckedAccount<'info>,

    pub token_program: Interface<'info, TokenInterface>,
}

pub fn handle_configure_account(
    ctx: Context<ConfigureAccount>,
    decryptable_zero_balance: [u8; 36],
    maximum_pending_balance_credit_counter: u64,
) -> Result<()> {
    let proof_context_state = ctx.accounts.proof_context_state.key();

    let ix = inner_configure_account(
        &ctx.accounts.token_program.key(),
        &ctx.accounts.token_account.key(),
        &ctx.accounts.mint.key(),
        &DecryptableBalance::from(decryptable_zero_balance),
        maximum_pending_balance_credit_counter,
        &ctx.accounts.owner.key(),
        &[],
        // Proof is pre-verified into a context state account by the client.
        ProofLocation::ContextStateAccount(&proof_context_state),
    )?;

    invoke(
        &ix,
        &[
            ctx.accounts.token_account.to_account_info(),
            ctx.accounts.mint.to_account_info(),
            ctx.accounts.proof_context_state.to_account_info(),
            ctx.accounts.owner.to_account_info(),
        ],
    )?;

    Ok(())
}
