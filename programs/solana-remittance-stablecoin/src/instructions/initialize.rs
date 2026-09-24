use anchor_lang::prelude::*;
use anchor_spl::token_interface::Token2022;

use crate::constants::BASE_MINT_EXTENSIONS;
use crate::state::MintParams;
use crate::utils::MintSetup;

#[derive(Accounts)]
pub struct InitializeMint<'info> {
    /// Issuer. Pays rent and holds every mint authority.
    #[account(mut)]
    pub authority: Signer<'info>,

    /// CHECK: Fresh keypair; created and initialized as a Token-2022 mint here.
    #[account(mut)]
    pub mint: Signer<'info>,

    pub token_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
}

/// Mint #1: TransferFeeConfig + MetadataPointer (-> self) +
/// DefaultAccountState (Frozen) + MintCloseAuthority.
pub fn handle_initialize_mint(ctx: Context<InitializeMint>, params: MintParams) -> Result<()> {
    let setup = MintSetup {
        authority: &ctx.accounts.authority.to_account_info(),
        mint: &ctx.accounts.mint.to_account_info(),
        token_program: &ctx.accounts.token_program.to_account_info(),
        system_program: &ctx.accounts.system_program.to_account_info(),
    };

    setup.create_account(&BASE_MINT_EXTENSIONS, &params)?;
    setup.init_base_extensions(&params)?;
    setup.init_mint_and_metadata(&params)
}
