//! Task 4: accounts start frozen; the freeze authority thaws one account
//! after KYC without touching the mint-level default state.

use anchor_lang::error::ErrorCode as AnchorError;
use anchor_spl::token_2022::spl_token_2022::{
    error::TokenError,
    extension::{
        default_account_state::DefaultAccountState, BaseStateWithExtensions, StateWithExtensions,
    },
    state::{AccountState, Mint},
};
use solana_signer::Signer;

use crate::harness::*;

#[test]
fn new_token_accounts_start_frozen() {
    let mut env = Env::new();
    let mint = env.create_mint();
    let alice = env.user();
    let alice_ata = env.create_ata(&alice.pubkey(), &mint);

    assert!(env.token(&alice_ata).frozen);

    // Nothing can be minted into an account that has not cleared KYC.
    let result = env.mint_to(&mint, &alice_ata, TOKEN);
    assert_err(result, token_err(TokenError::AccountFrozen));
}

#[test]
fn kyc_thaw_unfreezes_only_that_account_and_keeps_the_frozen_default() {
    let mut env = Env::new();
    let mint = env.create_mint();
    let alice = env.user();
    let bob = env.user();
    let alice_ata = env.create_ata(&alice.pubkey(), &mint);
    let bob_ata = env.create_ata(&bob.pubkey(), &mint);

    env.thaw(&mint, &alice.pubkey())
        .expect("thaw alice after KYC");

    assert!(!env.token(&alice_ata).frozen, "alice is active");
    assert!(env.token(&bob_ata).frozen, "bob has not passed KYC");

    // The mint-level default is untouched...
    let data = env.data(&mint);
    let state = StateWithExtensions::<Mint>::unpack(&data).unwrap();
    let default_state = state.get_extension::<DefaultAccountState>().unwrap();
    assert_eq!(default_state.state, AccountState::Frozen as u8);

    // ...so the next new account is still frozen.
    let carol = env.user();
    let carol_ata = env.create_ata(&carol.pubkey(), &mint);
    assert!(env.token(&carol_ata).frozen);

    // Alice can now receive funds.
    env.mint_to(&mint, &alice_ata, 10 * TOKEN).unwrap();
    assert_eq!(env.token(&alice_ata).amount, 10 * TOKEN);
}

#[test]
fn only_the_freeze_authority_can_thaw() {
    let mut env = Env::new();
    let mint = env.create_mint();
    let alice = env.user();
    let alice_ata = env.create_ata(&alice.pubkey(), &mint);

    // Alice tries to thaw herself.
    let ix = ix::thaw_account(&alice.pubkey(), &mint, &alice_ata, &alice.pubkey());
    let result = env.send(&[ix], &alice, &[]);

    assert_err(
        result,
        anchor_err(AnchorError::ConstraintMintFreezeAuthority),
    );
    assert!(env.token(&alice_ata).frozen);
}

#[test]
fn thaw_is_bound_to_the_wallet_that_passed_kyc() {
    let mut env = Env::new();
    let mint = env.create_mint();
    let alice = env.user();
    let mallory = env.user();
    env.create_ata(&alice.pubkey(), &mint);
    let mallory_ata = env.create_ata(&mallory.pubkey(), &mint);

    // Alice passed KYC, but the account passed in is Mallory's.
    let issuer = env.issuer.pubkey();
    let ix = ix::thaw_account(&issuer, &mint, &mallory_ata, &alice.pubkey());
    let result = env.send_as_issuer(&[ix], &[]);

    assert_err(result, anchor_err(AnchorError::ConstraintTokenOwner));
    assert!(env.token(&mallory_ata).frozen);
}
