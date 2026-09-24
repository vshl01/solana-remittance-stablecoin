use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use anchor_spl::token_2022::spl_token_2022::{
    extension::confidential_transfer::{instruction::inner_transfer_with_fee, DecryptableBalance},
    solana_zk_sdk::encryption::pod::elgamal::PodElGamalCiphertext,
};
use anchor_spl::token_interface::{Mint, Token2022, TokenAccount};
use spl_token_confidential_transfer_proof_extraction::instruction::ProofLocation;

#[derive(Accounts)]
pub struct ConfidentialTransfer<'info> {
    pub owner: Signer<'info>,

    pub mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        token::mint = mint,
        token::authority = owner,
    )]
    pub source: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = mint,
    )]
    pub destination: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: Verified ciphertext-commitment equality proof context
    /// (sender's remaining balance). Token-2022 checks owner and proof type.
    pub equality_proof: UncheckedAccount<'info>,

    /// CHECK: Verified grouped-ciphertext (3 handles) validity proof context
    /// for the transfer amount.
    pub transfer_amount_ciphertext_validity_proof: UncheckedAccount<'info>,

    /// CHECK: Verified percentage-with-cap proof context showing the
    /// encrypted fee matches the mint's current fee rate.
    pub fee_sigma_proof: UncheckedAccount<'info>,

    /// CHECK: Verified grouped-ciphertext (2 handles) validity proof context
    /// for the encrypted fee.
    pub fee_ciphertext_validity_proof: UncheckedAccount<'info>,

    /// CHECK: Verified batched range proof (u256) context.
    pub range_proof: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token2022>,
}

/// Confidential transfer with the protocol fee. Because the mint has
/// TransferFeeConfig, Token-2022 requires `TransferWithFee`: the amount and
/// the fee both stay encrypted, and five ZK proofs (verified beforehand into
/// context state accounts) show the transfer is valid.
pub fn handle_confidential_transfer(
    ctx: Context<ConfidentialTransfer>,
    new_source_decryptable_available_balance: [u8; 36],
    transfer_amount_auditor_ciphertext_lo: [u8; 64],
    transfer_amount_auditor_ciphertext_hi: [u8; 64],
) -> Result<()> {
    let accounts = &ctx.accounts;
    let equality = accounts.equality_proof.key();
    let amount_validity = accounts.transfer_amount_ciphertext_validity_proof.key();
    let fee_sigma = accounts.fee_sigma_proof.key();
    let fee_validity = accounts.fee_ciphertext_validity_proof.key();
    let range = accounts.range_proof.key();

    let ix = inner_transfer_with_fee(
        &accounts.token_program.key(),
        &accounts.source.key(),
        &accounts.mint.key(),
        &accounts.destination.key(),
        &DecryptableBalance::from(new_source_decryptable_available_balance),
        &PodElGamalCiphertext::from(transfer_amount_auditor_ciphertext_lo),
        &PodElGamalCiphertext::from(transfer_amount_auditor_ciphertext_hi),
        &accounts.owner.key(),
        &[],
        ProofLocation::ContextStateAccount(&equality),
        ProofLocation::ContextStateAccount(&amount_validity),
        ProofLocation::ContextStateAccount(&fee_sigma),
        ProofLocation::ContextStateAccount(&fee_validity),
        ProofLocation::ContextStateAccount(&range),
    )?;

    invoke(
        &ix,
        &[
            accounts.source.to_account_info(),
            accounts.mint.to_account_info(),
            accounts.destination.to_account_info(),
            accounts.equality_proof.to_account_info(),
            accounts
                .transfer_amount_ciphertext_validity_proof
                .to_account_info(),
            accounts.fee_sigma_proof.to_account_info(),
            accounts.fee_ciphertext_validity_proof.to_account_info(),
            accounts.range_proof.to_account_info(),
            accounts.owner.to_account_info(),
            accounts.token_program.to_account_info(),
        ],
    )?;

    Ok(())
}
