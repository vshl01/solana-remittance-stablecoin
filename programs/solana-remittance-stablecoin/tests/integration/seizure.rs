//! PermanentDelegate seizure, and the gap it leaves once funds are
//! confidential.

use anchor_spl::token_2022::spl_token_2022::error::TokenError;
use solana_signer::Signer;

use solana_remittance_stablecoin::error::ErrorCode;

use crate::harness::*;
use crate::zk::{self, IssuerKeys, UserKeys};

/// Issuer's own thawed account that receives seized funds.
fn treasury(env: &mut Env, mint: &anchor_lang::prelude::Pubkey) -> anchor_lang::prelude::Pubkey {
    let issuer = env.issuer.pubkey();
    let treasury = env.create_ata(&issuer, mint);
    env.thaw(mint, &issuer).unwrap();
    treasury
}

#[test]
fn permanent_delegate_seizes_public_balance_from_a_frozen_account() {
    let mut env = Env::new();
    let mint = env.create_confidential_mint(IssuerKeys::new().params());
    let treasury = treasury(&mut env, &mint);
    let (_mallory, mallory_ata) = env.kyc_user(&mint);
    env.mint_to(&mint, &mallory_ata, 300 * TOKEN).unwrap();

    // Sanction: freeze first so nothing can move while the case is handled.
    env.freeze(&mint, &mallory_ata).unwrap();
    assert!(env.token(&mallory_ata).frozen);

    // Seize without Mallory's signature.
    let amount = 300 * TOKEN;
    let fee = env.current_fee(&mint, amount);
    let issuer = env.issuer.pubkey();
    let ix = ix::seize(&issuer, &mint, &mallory_ata, &treasury, amount);
    env.send_as_issuer(&[ix], &[]).expect("seize");

    let mallory = env.token(&mallory_ata);
    assert_eq!(mallory.amount, 0);
    assert!(mallory.frozen, "account is left frozen");

    let treasury = env.token(&treasury);
    assert_eq!(treasury.amount, amount - fee);
    assert_eq!(treasury.withheld, fee);
}

#[test]
fn mint_without_a_permanent_delegate_cannot_seize() {
    let mut env = Env::new();
    let mint = env.create_mint(); // mint #1 has no PermanentDelegate
    let treasury = treasury(&mut env, &mint);
    let (_mallory, mallory_ata) = env.kyc_user(&mint);
    env.mint_to(&mint, &mallory_ata, 10 * TOKEN).unwrap();

    let issuer = env.issuer.pubkey();
    let ix = ix::seize(&issuer, &mint, &mallory_ata, &treasury, 10 * TOKEN);
    let result = env.send_as_issuer(&[ix], &[]);

    assert_err(result, program_err(ErrorCode::NotPermanentDelegate));
    assert_eq!(env.token(&mallory_ata).amount, 10 * TOKEN);
}

/// The written finding, as a test: a sanctioned user who moves funds into
/// the confidential balance before the permanent delegate acts puts them out
/// of reach. The issuer can freeze them, but not take them.
#[test]
fn confidential_balance_is_beyond_the_permanent_delegate() {
    let mut env = Env::new();
    let mint = env.create_confidential_mint(IssuerKeys::new().params());
    let treasury = treasury(&mut env, &mint);
    let (mallory, mallory_ata) = env.kyc_user(&mint);
    let keys = UserKeys::new();
    zk::onboard(&mut env, &mallory, &keys, &mint, &mallory_ata);
    env.mint_to(&mint, &mallory_ata, 500 * TOKEN).unwrap();

    // Mallory sees the sanction coming and moves everything in first.
    zk::deposit(&mut env, &mallory, &mint, &mallory_ata, 500 * TOKEN).unwrap();
    zk::apply_pending(&mut env, &mallory, &keys, &mallory_ata).unwrap();
    assert_eq!(
        env.token(&mallory_ata).amount,
        0,
        "nothing left in public view"
    );

    // The issuer freezes and tries to seize.
    env.freeze(&mint, &mallory_ata).unwrap();
    let issuer = env.issuer.pubkey();
    let ix = ix::seize(&issuer, &mint, &mallory_ata, &treasury, 500 * TOKEN);
    let result = env.send_as_issuer(&[ix], &[]);

    // The permanent delegate only moves the public `amount` field.
    assert_err(result, token_err(TokenError::InsufficientFunds));
    assert_eq!(env.token(&treasury).amount, 0);

    // Freezing does stop Mallory from moving the funds on...
    let result = zk::withdraw(&mut env, &mallory, &keys, &mint, &mallory_ata, TOKEN);
    assert_err(result, token_err(TokenError::AccountFrozen));

    // ...but the 500 tokens still exist, still count in supply, and only
    // Mallory's ElGamal key can ever move them.
    let ct = zk::confidential_account(&env, &mallory_ata);
    assert_eq!(keys.available(&ct), 500 * TOKEN);
    assert_eq!(env.supply(&mint), 500 * TOKEN);
    assert!(env.token(&mallory_ata).frozen);
}
