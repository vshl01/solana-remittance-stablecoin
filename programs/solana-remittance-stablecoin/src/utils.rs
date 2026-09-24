use anchor_lang::prelude::*;
use anchor_lang::system_program::{create_account, CreateAccount};
use anchor_spl::token_2022::spl_token_2022::{
    extension::{
        permanent_delegate::PermanentDelegate, transfer_fee::TransferFeeConfig,
        BaseStateWithExtensions, ExtensionType, StateWithExtensions,
    },
    state::{AccountState, Mint},
};
use anchor_spl::token_interface::{
    default_account_state_initialize, initialize_mint2, metadata_pointer_initialize,
    mint_close_authority_initialize, spl_pod::optional_keys::OptionalNonZeroPubkey,
    spl_token_metadata_interface::state::TokenMetadata, token_metadata_initialize,
    transfer_fee_initialize, DefaultAccountStateInitialize, InitializeMint2,
    MetadataPointerInitialize, MintCloseAuthorityInitialize, TokenMetadataInitialize,
    TransferFeeInitialize,
};

use crate::error::ErrorCode;
use crate::state::MintParams;

/// Expected Token-2022 fee for `amount`, using the rate active in the *current*
/// epoch. The mint is read through `StateWithExtensions`, never a raw unpack,
/// and the rate is never cached: a fee change scheduled with `SetTransferFee`
/// is picked up as soon as its epoch arrives.
pub fn expected_transfer_fee(mint: &AccountInfo, amount: u64) -> Result<u64> {
    let current_epoch = Clock::get()?.epoch;
    let data = mint.try_borrow_data()?;
    let mint = StateWithExtensions::<Mint>::unpack(&data)?;

    mint.get_extension::<TransferFeeConfig>()?
        .calculate_epoch_fee(current_epoch, amount)
        .ok_or_else(|| error!(ErrorCode::FeeCalculationFailed))
}

/// The mint's permanent delegate, if it has one.
pub fn permanent_delegate(mint: &AccountInfo) -> Result<Option<Pubkey>> {
    let data = mint.try_borrow_data()?;
    let mint = StateWithExtensions::<Mint>::unpack(&data)?;

    Ok(mint
        .get_extension::<PermanentDelegate>()
        .ok()
        .and_then(|extension| Option::<Pubkey>::from(extension.delegate)))
}

/// Bytes the TokenMetadata entry takes inside a Token-2022 mint: a 2-byte
/// extension type, a 2-byte length, then the borsh-encoded metadata.
/// (`TokenMetadata::tlv_size_of` assumes a standalone TLV account's 12-byte
/// header, which over-counts by 8.)
pub fn metadata_extension_len(metadata: &TokenMetadata) -> Result<usize> {
    const TLV_HEADER_LEN: usize = 4;
    let data = borsh::to_vec(metadata).map_err(|_| ProgramError::InvalidAccountData)?;
    Ok(TLV_HEADER_LEN + data.len())
}

/// Shared steps for creating a remittance mint.
///
/// Order matters in Token-2022: allocate the account, run every
/// extension-init instruction, then `InitializeMint`. TokenMetadata is the one
/// exception — it is variable-length and its initialize instruction needs the
/// mint authority's signature, so Token-2022 only accepts it after
/// `InitializeMint`.
pub struct MintSetup<'a, 'info> {
    pub authority: &'a AccountInfo<'info>,
    pub mint: &'a AccountInfo<'info>,
    pub token_program: &'a AccountInfo<'info>,
    pub system_program: &'a AccountInfo<'info>,
}

impl<'info> MintSetup<'_, 'info> {
    /// Allocate the mint sized for exactly `extensions`, and pre-fund rent for
    /// the TokenMetadata that Token-2022 reallocates into place later.
    pub fn create_account(&self, extensions: &[ExtensionType], params: &MintParams) -> Result<()> {
        let space = ExtensionType::try_calculate_account_len::<Mint>(extensions)?;
        let metadata_space = metadata_extension_len(&self.token_metadata(params)?)?;
        let lamports = Rent::get()?.minimum_balance(space + metadata_space);

        create_account(
            CpiContext::new(
                *self.system_program.key,
                CreateAccount {
                    from: self.authority.clone(),
                    to: self.mint.clone(),
                },
            ),
            lamports,
            space as u64,
            self.token_program.key,
        )
    }

    /// TransferFeeConfig, MetadataPointer (-> the mint itself),
    /// DefaultAccountState (Frozen) and MintCloseAuthority.
    pub fn init_base_extensions(&self, params: &MintParams) -> Result<()> {
        let authority = self.authority.key;

        transfer_fee_initialize(
            CpiContext::new(
                *self.token_program.key,
                TransferFeeInitialize {
                    token_program_id: self.token_program.clone(),
                    mint: self.mint.clone(),
                },
            ),
            Some(authority),
            Some(authority),
            params.transfer_fee_basis_points,
            params.maximum_fee,
        )?;

        // Metadata lives inside the mint, so wallets can read it without
        // trusting an off-chain registry.
        metadata_pointer_initialize(
            CpiContext::new(
                *self.token_program.key,
                MetadataPointerInitialize {
                    token_program_id: self.token_program.clone(),
                    mint: self.mint.clone(),
                },
            ),
            Some(*authority),
            Some(*self.mint.key),
        )?;

        // Every new token account starts frozen until KYC clears.
        default_account_state_initialize(
            CpiContext::new(
                *self.token_program.key,
                DefaultAccountStateInitialize {
                    token_program_id: self.token_program.clone(),
                    mint: self.mint.clone(),
                },
            ),
            &AccountState::Frozen,
        )?;

        mint_close_authority_initialize(
            CpiContext::new(
                *self.token_program.key,
                MintCloseAuthorityInitialize {
                    token_program_id: self.token_program.clone(),
                    mint: self.mint.clone(),
                },
            ),
            Some(authority),
        )
    }

    /// `InitializeMint2`, then the TokenMetadata the pointer refers to.
    pub fn init_mint_and_metadata(&self, params: &MintParams) -> Result<()> {
        let authority = self.authority.key;

        initialize_mint2(
            CpiContext::new(
                *self.token_program.key,
                InitializeMint2 {
                    mint: self.mint.clone(),
                },
            ),
            params.decimals,
            authority,
            Some(authority),
        )?;

        token_metadata_initialize(
            CpiContext::new(
                *self.token_program.key,
                TokenMetadataInitialize {
                    program_id: self.token_program.clone(),
                    metadata: self.mint.clone(),
                    update_authority: self.authority.clone(),
                    mint_authority: self.authority.clone(),
                    mint: self.mint.clone(),
                },
            ),
            params.name.clone(),
            params.symbol.clone(),
            params.uri.clone(),
        )
    }

    fn token_metadata(&self, params: &MintParams) -> Result<TokenMetadata> {
        Ok(TokenMetadata {
            update_authority: OptionalNonZeroPubkey::try_from(Some(*self.authority.key))?,
            mint: *self.mint.key,
            name: params.name.clone(),
            symbol: params.symbol.clone(),
            uri: params.uri.clone(),
            additional_metadata: vec![],
        })
    }
}
