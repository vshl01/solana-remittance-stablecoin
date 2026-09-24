use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use anchor_spl::token_2022::spl_token_2022::{
    extension::{
        confidential_transfer::instruction::initialize_mint as confidential_transfer_initialize_mint,
        confidential_transfer_fee::instruction::initialize_confidential_transfer_fee_config,
    },
    solana_zk_sdk::encryption::pod::elgamal::PodElGamalPubkey,
};
use anchor_spl::token_interface::{
    permanent_delegate_initialize, PermanentDelegateInitialize, Token2022,
};

use crate::constants::{BASE_MINT_EXTENSIONS, CONFIDENTIAL_MINT_EXTENSIONS};
use crate::state::{ConfidentialParams, MintParams};
use crate::utils::MintSetup;

#[derive(Accounts)]
pub struct ReissueConfidentialMint<'info> {
    /// Issuer. Also becomes the permanent delegate (seizure authority) and the
    /// confidential-transfer authority that approves accounts.
    #[account(mut)]
    pub authority: Signer<'info>,

    /// CHECK: Fresh keypair; created and initialized as a Token-2022 mint here.
    #[account(mut)]
    pub mint: Signer<'info>,

    pub token_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
}

/// Mint #2. Confidential transfers cannot be added to an existing mint, so the
/// stablecoin is re-issued with the same four extensions plus
/// PermanentDelegate, ConfidentialTransferMint (manual approval) and the
/// ConfidentialTransferFeeConfig that Token-2022 requires when fees and
/// confidentiality are combined.
pub fn handle_reissue_confidential_mint(
    ctx: Context<ReissueConfidentialMint>,
    params: MintParams,
    confidential: ConfidentialParams,
) -> Result<()> {
    let authority = ctx.accounts.authority.key();
    let mint = ctx.accounts.mint.to_account_info();
    let token_program = ctx.accounts.token_program.to_account_info();

    let setup = MintSetup {
        authority: &ctx.accounts.authority.to_account_info(),
        mint: &mint,
        token_program: &token_program,
        system_program: &ctx.accounts.system_program.to_account_info(),
    };

    let extensions = [
        BASE_MINT_EXTENSIONS.as_slice(),
        &CONFIDENTIAL_MINT_EXTENSIONS,
    ]
    .concat();
    setup.create_account(&extensions, &params)?;
    setup.init_base_extensions(&params)?;

    permanent_delegate_initialize(
        CpiContext::new(
            token_program.key(),
            PermanentDelegateInitialize {
                token_program_id: token_program.clone(),
                mint: mint.clone(),
            },
        ),
        &authority,
    )?;

    // approve_policy = manual: new confidential accounts stay unapproved until
    // the issuer calls ApproveAccount.
    let ix = confidential_transfer_initialize_mint(
        &token_program.key(),
        &mint.key(),
        Some(authority),
        false,
        confidential
            .auditor_elgamal_pubkey
            .map(PodElGamalPubkey::from),
    )?;
    invoke(&ix, &[mint.clone(), token_program.clone()])?;

    let ix = initialize_confidential_transfer_fee_config(
        &token_program.key(),
        &mint.key(),
        Some(authority),
        &PodElGamalPubkey::from(confidential.withdraw_withheld_authority_elgamal_pubkey),
    )?;
    invoke(&ix, &[mint.clone(), token_program.clone()])?;

    setup.init_mint_and_metadata(&params)
}
