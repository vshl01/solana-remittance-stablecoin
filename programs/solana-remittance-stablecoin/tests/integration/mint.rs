//! Task 1: mint #1 extension stack, sizing and ordering. Plus the close
//! authority that lets the issuer decommission the mint.

use anchor_spl::token_2022::spl_token_2022::{
    error::TokenError,
    extension::{
        default_account_state::DefaultAccountState, metadata_pointer::MetadataPointer,
        mint_close_authority::MintCloseAuthority, transfer_fee::TransferFeeConfig,
        BaseStateWithExtensions, ExtensionType, StateWithExtensions,
    },
    state::{AccountState, Mint},
};
use anchor_spl::token_interface::spl_token_metadata_interface::state::TokenMetadata;
use solana_signer::Signer;

use solana_remittance_stablecoin::{error::ErrorCode, BASE_MINT_EXTENSIONS};

use crate::harness::*;

#[test]
fn initialize_mint_stacks_fee_metadata_frozen_default_and_close_authority() {
    let mut env = Env::new();
    let mint = env.create_mint();
    let issuer = env.issuer.pubkey();

    let data = env.data(&mint);
    let state = StateWithExtensions::<Mint>::unpack(&data).unwrap();

    // Base mint
    assert!(state.base.is_initialized);
    assert_eq!(state.base.decimals, DECIMALS);
    assert_eq!(Option::from(state.base.mint_authority), Some(issuer));
    assert_eq!(Option::from(state.base.freeze_authority), Some(issuer));

    // Exactly the four fixed extensions plus the metadata they point to.
    let mut types = state.get_extension_types().unwrap();
    types.sort_by_key(|t| *t as u16);
    let mut expected = [
        BASE_MINT_EXTENSIONS.as_slice(),
        &[ExtensionType::TokenMetadata],
    ]
    .concat();
    expected.sort_by_key(|t| *t as u16);
    assert_eq!(types, expected);

    let fee = state.get_extension::<TransferFeeConfig>().unwrap();
    let current = fee.get_epoch_fee(env.epoch());
    assert_eq!(u16::from(current.transfer_fee_basis_points), FEE_BPS);
    assert_eq!(u64::from(current.maximum_fee), MAX_FEE);
    assert_eq!(
        Option::from(fee.transfer_fee_config_authority),
        Some(issuer)
    );
    assert_eq!(Option::from(fee.withdraw_withheld_authority), Some(issuer));

    let pointer = state.get_extension::<MetadataPointer>().unwrap();
    assert_eq!(
        Option::from(pointer.metadata_address),
        Some(mint),
        "points at the mint itself"
    );
    assert_eq!(Option::from(pointer.authority), Some(issuer));

    let default_state = state.get_extension::<DefaultAccountState>().unwrap();
    assert_eq!(default_state.state, AccountState::Frozen as u8);

    let close = state.get_extension::<MintCloseAuthority>().unwrap();
    assert_eq!(Option::from(close.close_authority), Some(issuer));

    // Metadata lives on-chain inside the mint.
    let metadata = state.get_variable_len_extension::<TokenMetadata>().unwrap();
    let params = mint_params();
    assert_eq!(metadata.mint, mint);
    assert_eq!(metadata.name, params.name);
    assert_eq!(metadata.symbol, params.symbol);
    assert_eq!(metadata.uri, params.uri);

    // Sized with try_calculate_account_len, plus the metadata TLV that
    // Token-2022 reallocates in after InitializeMint. Rent was pre-funded
    // for exactly the final size.
    let fixed_len =
        ExtensionType::try_calculate_account_len::<Mint>(&BASE_MINT_EXTENSIONS).unwrap();
    assert_eq!(data.len(), fixed_len + metadata_tlv_len(&metadata));
    let lamports = env.svm.get_account(&mint).unwrap().lamports;
    assert_eq!(
        lamports,
        env.svm.minimum_balance_for_rent_exemption(data.len())
    );
}

#[test]
fn close_authority_can_decommission_an_empty_mint() {
    let mut env = Env::new();
    let mint = env.create_mint();
    let issuer = env.issuer.pubkey();

    let ix = ix::close_mint(&issuer, &mint, &issuer);
    env.send_as_issuer(&[ix], &[]).expect("close_mint");

    assert!(!env.exists(&mint), "mint account is gone");
}

#[test]
fn mint_cannot_be_closed_while_tokens_are_in_circulation() {
    let mut env = Env::new();
    let mint = env.create_mint();
    let (_alice, alice_ata) = env.kyc_user(&mint);
    env.mint_to(&mint, &alice_ata, TOKEN).unwrap();

    let issuer = env.issuer.pubkey();
    let result = env.send_as_issuer(&[ix::close_mint(&issuer, &mint, &issuer)], &[]);

    assert_err(result, program_err(ErrorCode::MintHasSupply));
    assert!(env.exists(&mint));
}

#[test]
fn only_the_close_authority_can_close_the_mint() {
    let mut env = Env::new();
    let mint = env.create_mint();
    let attacker = env.user();

    let ix = ix::close_mint(&attacker.pubkey(), &mint, &attacker.pubkey());
    let result = env.send(&[ix], &attacker, &[]);

    assert_err(result, token_err(TokenError::OwnerMismatch));
    assert!(env.exists(&mint));
}
