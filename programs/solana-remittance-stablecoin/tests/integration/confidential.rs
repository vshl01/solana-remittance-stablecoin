//! Tasks 5 and 6: the re-issued mint and the full confidential lifecycle.

use anchor_lang::error::ErrorCode as AnchorError;
use anchor_spl::token_2022::spl_token_2022::{
    self,
    error::TokenError,
    extension::{
        confidential_transfer::{self, ConfidentialTransferMint},
        confidential_transfer_fee::ConfidentialTransferFeeAmount,
        confidential_transfer_fee::ConfidentialTransferFeeConfig,
        default_account_state::DefaultAccountState,
        metadata_pointer::MetadataPointer,
        permanent_delegate::PermanentDelegate,
        transfer_fee, BaseStateWithExtensions, ExtensionType, StateWithExtensions,
    },
    solana_zk_sdk::encryption::pod::elgamal::PodElGamalPubkey,
    state::{Account, AccountState, Mint},
};
use anchor_spl::token_interface::spl_token_metadata_interface::state::TokenMetadata;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_system_interface::instruction::create_account;

use solana_remittance_stablecoin::{BASE_MINT_EXTENSIONS, CONFIDENTIAL_MINT_EXTENSIONS};

use crate::harness::*;
use crate::zk::{self, IssuerKeys, UserKeys};

#[test]
fn reissued_mint_carries_forward_extensions_and_adds_seizure_and_confidentiality() {
    let mut env = Env::new();
    let keys = IssuerKeys::new();
    let mint = env.create_confidential_mint(keys.params());
    let issuer = env.issuer.pubkey();

    let data = env.data(&mint);
    let state = StateWithExtensions::<Mint>::unpack(&data).unwrap();

    let mut types = state.get_extension_types().unwrap();
    types.sort_by_key(|t| *t as u16);
    let mut expected = [
        BASE_MINT_EXTENSIONS.as_slice(),
        &CONFIDENTIAL_MINT_EXTENSIONS,
        &[ExtensionType::TokenMetadata],
    ]
    .concat();
    expected.sort_by_key(|t| *t as u16);
    assert_eq!(types, expected);

    // Carried forward from mint #1.
    let pointer = state.get_extension::<MetadataPointer>().unwrap();
    assert_eq!(Option::from(pointer.metadata_address), Some(mint));
    let default_state = state.get_extension::<DefaultAccountState>().unwrap();
    assert_eq!(default_state.state, AccountState::Frozen as u8);
    let metadata = state.get_variable_len_extension::<TokenMetadata>().unwrap();
    assert_eq!(metadata.symbol, mint_params().symbol);

    // Seizure authority.
    let delegate = state.get_extension::<PermanentDelegate>().unwrap();
    assert_eq!(Option::from(delegate.delegate), Some(issuer));

    // Confidential transfers with manual approval and an auditor.
    let ct_mint = state.get_extension::<ConfidentialTransferMint>().unwrap();
    assert!(
        !bool::from(ct_mint.auto_approve_new_accounts),
        "approve_policy = manual"
    );
    assert_eq!(Option::from(ct_mint.authority), Some(issuer));
    assert_eq!(
        Option::<PodElGamalPubkey>::from(ct_mint.auditor_elgamal_pubkey),
        Some(PodElGamalPubkey::from(*keys.auditor.pubkey()))
    );

    let ct_fee = state
        .get_extension::<ConfidentialTransferFeeConfig>()
        .unwrap();
    assert_eq!(
        ct_fee.withdraw_withheld_authority_elgamal_pubkey,
        PodElGamalPubkey::from(*keys.withdraw_withheld.pubkey())
    );

    let fixed = [
        BASE_MINT_EXTENSIONS.as_slice(),
        &CONFIDENTIAL_MINT_EXTENSIONS,
    ]
    .concat();
    let fixed_len = ExtensionType::try_calculate_account_len::<Mint>(&fixed).unwrap();
    assert_eq!(data.len(), fixed_len + metadata_tlv_len(&metadata));
    let lamports = env.svm.get_account(&mint).unwrap().lamports;
    assert_eq!(
        lamports,
        env.svm.minimum_balance_for_rent_exemption(data.len())
    );
}

/// The gap between "keep the fee" and "add confidentiality": Token-2022
/// refuses TransferFeeConfig + ConfidentialTransferMint on their own, because
/// an encrypted transfer needs an encrypted fee. The re-issue therefore also
/// carries ConfidentialTransferFeeConfig.
#[test]
fn fee_plus_confidentiality_is_rejected_without_confidential_fee_config() {
    let mut env = Env::new();
    let issuer = env.issuer.pubkey();
    let mint = Keypair::new();

    let extensions = [
        ExtensionType::TransferFeeConfig,
        ExtensionType::ConfidentialTransferMint,
    ];
    let space = ExtensionType::try_calculate_account_len::<Mint>(&extensions).unwrap();
    let lamports = env.svm.minimum_balance_for_rent_exemption(space);

    let ixs = [
        create_account(&issuer, &mint.pubkey(), lamports, space as u64, &TOKEN_2022),
        transfer_fee::instruction::initialize_transfer_fee_config(
            &TOKEN_2022,
            &mint.pubkey(),
            Some(&issuer),
            Some(&issuer),
            FEE_BPS,
            MAX_FEE,
        )
        .unwrap(),
        confidential_transfer::instruction::initialize_mint(
            &TOKEN_2022,
            &mint.pubkey(),
            Some(issuer),
            false,
            None,
        )
        .unwrap(),
        spl_token_2022::instruction::initialize_mint2(
            &TOKEN_2022,
            &mint.pubkey(),
            &issuer,
            Some(&issuer),
            DECIMALS,
        )
        .unwrap(),
    ];

    let result = env.send_as_issuer(&ixs, &[&mint]);
    assert_err(result, token_err(TokenError::InvalidExtensionCombination));
}

#[test]
fn configure_account_is_owner_only_though_ata_creation_is_permissionless() {
    let mut env = Env::new();
    let mint = env.create_confidential_mint(IssuerKeys::new().params());
    let alice = env.user();
    let stranger = env.user();

    // Anyone can create Alice's ATA...
    let alice_ata = env.create_ata_paid_by(&stranger, &alice.pubkey(), &mint);
    env.thaw(&mint, &alice.pubkey()).unwrap();

    // ...but not opt it into confidential transfers.
    let stranger_keys = UserKeys::new();
    let result = zk::configure(&mut env, &stranger, &stranger_keys, &mint, &alice_ata);
    assert_err(result, anchor_err(AnchorError::ConstraintTokenOwner));

    let alice_keys = UserKeys::new();
    zk::configure(&mut env, &alice, &alice_keys, &mint, &alice_ata).expect("owner configures");

    let data = env.data(&alice_ata);
    let account = StateWithExtensions::<Account>::unpack(&data).unwrap();
    assert!(account
        .get_extension::<ConfidentialTransferFeeAmount>()
        .is_ok());
    let ct = zk::confidential_account(&env, &alice_ata);
    assert_eq!(
        ct.elgamal_pubkey,
        PodElGamalPubkey::from(*alice_keys.elgamal.pubkey())
    );
    assert!(
        !bool::from(ct.approved),
        "manual approval: not approved yet"
    );
    assert_eq!(alice_keys.available(&ct), 0);
}

#[test]
fn manual_approval_gates_confidential_deposits() {
    let mut env = Env::new();
    let mint = env.create_confidential_mint(IssuerKeys::new().params());
    let (alice, alice_ata) = env.kyc_user(&mint);
    let alice_keys = UserKeys::new();
    env.mint_to(&mint, &alice_ata, 100 * TOKEN).unwrap();
    zk::configure(&mut env, &alice, &alice_keys, &mint, &alice_ata).unwrap();

    // Not approved yet.
    let result = zk::deposit(&mut env, &alice, &mint, &alice_ata, 10 * TOKEN);
    assert_err(
        result,
        token_err(TokenError::ConfidentialTransferAccountNotApproved),
    );

    // Only the confidential-transfer authority can approve (Token-2022
    // requires that exact key's signature).
    let ix = ix::approve_account(&alice.pubkey(), &mint, &alice_ata);
    assert_failed_with(env.send(&[ix], &alice, &[]), "MissingRequiredSignature");

    zk::approve(&mut env, &mint, &alice_ata).expect("issuer approves");
    assert!(bool::from(
        zk::confidential_account(&env, &alice_ata).approved
    ));

    zk::deposit(&mut env, &alice, &mint, &alice_ata, 10 * TOKEN).expect("deposit after approval");
}

#[test]
fn full_confidential_lifecycle() {
    let mut env = Env::new();
    let issuer_keys = IssuerKeys::new();
    let mint = env.create_confidential_mint(issuer_keys.params());

    let (alice, alice_ata) = env.kyc_user(&mint);
    let (bob, bob_ata) = env.kyc_user(&mint);
    let alice_keys = UserKeys::new();
    let bob_keys = UserKeys::new();
    zk::onboard(&mut env, &alice, &alice_keys, &mint, &alice_ata);
    zk::onboard(&mut env, &bob, &bob_keys, &mint, &bob_ata);
    env.mint_to(&mint, &alice_ata, 1_000 * TOKEN).unwrap();

    // 1. Deposit: public -> pending.
    zk::deposit(&mut env, &alice, &mint, &alice_ata, 600 * TOKEN).expect("deposit");
    assert_eq!(env.token(&alice_ata).amount, 400 * TOKEN);
    let ct = zk::confidential_account(&env, &alice_ata);
    assert_eq!(alice_keys.pending(&ct), 600 * TOKEN);
    assert_eq!(alice_keys.available(&ct), 0, "deposits land in pending");

    // 2. Apply pending: pending -> available.
    zk::apply_pending(&mut env, &alice, &alice_keys, &alice_ata).expect("apply pending");
    let ct = zk::confidential_account(&env, &alice_ata);
    assert_eq!(alice_keys.pending(&ct), 0);
    assert_eq!(alice_keys.available(&ct), 600 * TOKEN);
    assert_eq!(alice_keys.available_elgamal(&ct), 600 * TOKEN);

    // 3. Confidential transfer with the protocol fee.
    let amount = 200 * TOKEN;
    let fee = env.current_fee(&mint, amount);
    assert_eq!(fee, TOKEN);
    let public_before = (env.token(&alice_ata).amount, env.token(&bob_ata).amount);

    zk::transfer(
        &mut env,
        &alice,
        &alice_keys,
        &mint,
        &alice_ata,
        &bob_ata,
        amount,
    )
    .expect("confidential transfer");

    // Public balances do not move, so observers learn nothing.
    assert_eq!(
        (env.token(&alice_ata).amount, env.token(&bob_ata).amount),
        public_before
    );

    let alice_ct = zk::confidential_account(&env, &alice_ata);
    assert_eq!(alice_keys.available(&alice_ct), 400 * TOKEN);
    assert_eq!(alice_keys.available_elgamal(&alice_ct), 400 * TOKEN);

    let bob_ct = zk::confidential_account(&env, &bob_ata);
    assert_eq!(
        bob_keys.pending(&bob_ct),
        amount - fee,
        "bob receives amount minus fee"
    );
    assert_eq!(bob_keys.available(&bob_ct), 0);

    // The encrypted fee is withheld for the issuer's withdraw-withheld key.
    let data = env.data(&bob_ata);
    let account = StateWithExtensions::<Account>::unpack(&data).unwrap();
    let withheld = account
        .get_extension::<ConfidentialTransferFeeAmount>()
        .unwrap();
    assert_eq!(
        zk::decrypt(&issuer_keys.withdraw_withheld, withheld.withheld_amount),
        fee
    );

    // 4. Bob must apply pending before he can withdraw.
    zk::apply_pending(&mut env, &bob, &bob_keys, &bob_ata).expect("bob applies pending");
    let bob_ct = zk::confidential_account(&env, &bob_ata);
    assert_eq!(bob_keys.available(&bob_ct), amount - fee);

    // 5. Withdraw: available -> public.
    zk::withdraw(&mut env, &bob, &bob_keys, &mint, &bob_ata, amount - fee).expect("withdraw");
    assert_eq!(env.token(&bob_ata).amount, amount - fee);
    let bob_ct = zk::confidential_account(&env, &bob_ata);
    assert_eq!(bob_keys.available(&bob_ct), 0);

    // Supply is conserved end to end.
    assert_eq!(env.supply(&mint), 1_000 * TOKEN);
}

#[test]
fn auditor_can_read_the_hidden_transfer_amount() {
    let mut env = Env::new();
    let issuer_keys = IssuerKeys::new();
    let mint = env.create_confidential_mint(issuer_keys.params());
    let (alice, alice_ata) = env.kyc_user(&mint);
    let (bob, bob_ata) = env.kyc_user(&mint);
    let alice_keys = UserKeys::new();
    zk::onboard(&mut env, &alice, &alice_keys, &mint, &alice_ata);
    zk::onboard(&mut env, &bob, &UserKeys::new(), &mint, &bob_ata);
    env.mint_to(&mint, &alice_ata, 100 * TOKEN).unwrap();
    zk::deposit(&mut env, &alice, &mint, &alice_ata, 100 * TOKEN).unwrap();
    zk::apply_pending(&mut env, &alice, &alice_keys, &alice_ata).unwrap();

    let amount = 42 * TOKEN;
    let (result, (lo, hi)) = zk::transfer_with_audit(
        &mut env,
        &alice,
        &alice_keys,
        &mint,
        &alice_ata,
        &bob_ata,
        amount,
    );
    result.expect("confidential transfer");

    // The auditor ciphertexts sit in the public instruction data, but only
    // the auditor key opens them.
    assert_eq!(zk::decrypt_lo_hi(&issuer_keys.auditor, lo, hi), amount);
    assert_ne!(
        zk::try_decrypt_lo_hi(&UserKeys::new().elgamal, lo, hi),
        Some(amount),
        "any other key learns nothing"
    );
}

#[test]
fn withdraw_needs_the_pending_balance_applied_first() {
    let mut env = Env::new();
    let mint = env.create_confidential_mint(IssuerKeys::new().params());
    let (alice, alice_ata) = env.kyc_user(&mint);
    let keys = UserKeys::new();
    zk::onboard(&mut env, &alice, &keys, &mint, &alice_ata);
    env.mint_to(&mint, &alice_ata, 50 * TOKEN).unwrap();
    zk::deposit(&mut env, &alice, &mint, &alice_ata, 50 * TOKEN).unwrap();

    // Try to withdraw the deposit while it is still pending: valid proofs
    // about the pending ciphertext do not match the on-chain available
    // balance (still zero), so Token-2022 refuses.
    let ct = zk::confidential_account(&env, &alice_ata);
    let pending = zk::pending_ciphertext(&ct);
    let result = zk::withdraw_against(
        &mut env,
        &alice,
        &keys,
        &mint,
        &alice_ata,
        &pending,
        50 * TOKEN,
        10 * TOKEN,
    );
    assert_err(
        result,
        token_err(TokenError::ConfidentialTransferBalanceMismatch),
    );
    assert_eq!(env.token(&alice_ata).amount, 0);

    // Apply first, then the same withdrawal works.
    zk::apply_pending(&mut env, &alice, &keys, &alice_ata).unwrap();
    zk::withdraw(&mut env, &alice, &keys, &mint, &alice_ata, 10 * TOKEN)
        .expect("withdraw after apply");
    assert_eq!(env.token(&alice_ata).amount, 10 * TOKEN);
    let ct = zk::confidential_account(&env, &alice_ata);
    assert_eq!(keys.available(&ct), 40 * TOKEN);
}
