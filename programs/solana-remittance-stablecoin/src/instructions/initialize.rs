use anchor_lang::prelude::*;
use anchor_lang::system_program::{create_account, CreateAccount};

use anchor_spl::token_interface::{
    default_account_state_initialize,
    initialize_mint2,
    metadata_pointer_initialize,
    mint_close_authority_initialize,
    permanent_delegate_initialize,
    transfer_fee_initialize,
    DefaultAccountStateInitialize,
    InitializeMint2,
    MetadataPointerInitialize,
    MintCloseAuthorityInitialize,
    PermanentDelegateInitialize,
    TransferFeeInitialize,
    Token2022,
};

// anchor-spl has no confidential-transfer CPI helper yet, so this one is
// built as a raw Token-2022 instruction and invoked directly.
use anchor_lang::solana_program::program::invoke;
use anchor_spl::token_2022::spl_token_2022::{
    extension::{
        confidential_transfer::instruction::initialize_mint as confidential_transfer_initialize_mint, 
        ExtensionType
    },
    state::{AccountState, Mint},
};

#[derive(Accounts)]
pub struct InitializeMint<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    /// CHECK: New Token-2022 mint account
    #[account(mut)]
    pub mint: Signer<'info>,

    pub token_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
}

pub fn handle_initialize_mint(
    ctx: Context<InitializeMint>,
    decimals: u8,
    transfer_fee_basis_points: u16,
    maximum_fee: u64,
) -> Result<()> {
    let authority = ctx.accounts.authority.key();

    // 1. Calculate space for ALL mint extensions.
    let extensions = [
        ExtensionType::TransferFeeConfig,
        ExtensionType::MetadataPointer,
        ExtensionType::DefaultAccountState,
        ExtensionType::MintCloseAuthority,
        ExtensionType::PermanentDelegate,
        ExtensionType::ConfidentialTransferMint,
    ];

    // Calculate how much storage the Mint + all extensions need.
    let space =
        ExtensionType::try_calculate_account_len::<Mint>(&extensions)?;

    let lamports = Rent::get()?.minimum_balance(space);

    // 2. Create the Token-2022 mint account.
    create_account(
        /* 
        → Create that Mint account on Solana.
        → Authority pays rent.
        → Mint keypair's address becomes the on-chain account.
        */
        CpiContext::new(
            ctx.accounts.system_program.key(),
            CreateAccount {
                from: ctx.accounts.authority.to_account_info(),
                to: ctx.accounts.mint.to_account_info(),
            },
        ),
        lamports,
        space as u64,
        &ctx.accounts.token_program.key(),
    )?;

    // 3. TransferFeeConfig
    transfer_fee_initialize(
        /*
        → Configure Transfer Fee on the Mint.
        → Set fee authority, fee rate, and max fee.
        */
        CpiContext::new(
            ctx.accounts.token_program.key(),
            TransferFeeInitialize {
                token_program_id: ctx.accounts.token_program.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
            },
        ),
        Some(&authority),
        Some(&authority),
        transfer_fee_basis_points,
        maximum_fee,
    )?;

    // 4. MetadataPointer → points to the mint itself.
    metadata_pointer_initialize(
        /*
         → Configure Metadata Pointer.
         → Tell the Mint where its metadata lives (here: the Mint itself).
        */
        CpiContext::new(
            ctx.accounts.token_program.key(),
            MetadataPointerInitialize {
                token_program_id: ctx.accounts.token_program.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
            },
        ),
        Some(authority),
        Some(ctx.accounts.mint.key()),
    )?;

    // 5. New token accounts start frozen.
    default_account_state_initialize(
        // → Configure new token accounts to start FROZEN.
        CpiContext::new(
            ctx.accounts.token_program.key(),
            DefaultAccountStateInitialize {
                token_program_id: ctx.accounts.token_program.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
            },
        ),
        &AccountState::Frozen,
    )?;

    // 6. MintCloseAuthority
    mint_close_authority_initialize(
        // → Set who can close the Mint later.
        CpiContext::new(
            ctx.accounts.token_program.key(),
            MintCloseAuthorityInitialize {
                token_program_id: ctx.accounts.token_program.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
            },
        ),
        Some(&authority),
    )?;

    // 7. PermanentDelegate
    permanent_delegate_initialize(
        /*
         → Set the permanent delegate.
         → Here, authority gets that role.
         */
        CpiContext::new(
            ctx.accounts.token_program.key(),
            PermanentDelegateInitialize {
                token_program_id: ctx.accounts.token_program.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
            },
        ),
        &authority,
    )?;

    // 8. ConfidentialTransferMint
    let confidential_ix = confidential_transfer_initialize_mint(
        /*
         → Enable Confidential Transfers on the Mint.
         → false = new confidential accounts require manual approval.
         */
        &ctx.accounts.token_program.key(),
        &ctx.accounts.mint.key(),
        Some(authority),
        false, // auto_approve_new_accounts = false → manual approval
        None,  // no auditor
    )?;
    invoke(&confidential_ix, &[ctx.accounts.mint.to_account_info()])?;

    // 9. Initialize the base mint LAST.
    initialize_mint2(
        /*
        → Finally initialize the actual Token-2022 Mint.
        → Set decimals + mint authority + freeze authority.
         */
        CpiContext::new(
            ctx.accounts.token_program.key(),
            InitializeMint2 {
                mint: ctx.accounts.mint.to_account_info(),
            },
        ),
        decimals,
        &authority,
        Some(&authority),
    )?;

    Ok(())
}