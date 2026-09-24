//! Tasks 2 and 3: transfers go through TransferCheckedWithFee with a fee
//! computed from the current epoch's rate, and state is read through
//! StateWithExtensions (see `Env::token` / `Env::current_fee`).

use anchor_spl::token_2022::spl_token_2022::{
    error::TokenError,
    extension::transfer_fee::instruction::{set_transfer_fee, transfer_checked_with_fee},
};
use solana_signer::Signer;

use crate::harness::*;

#[test]
fn transfer_pays_the_protocol_fee() {
    let mut env = Env::new();
    let mint = env.create_mint();
    let (alice, alice_ata) = env.kyc_user(&mint);
    let (_bob, bob_ata) = env.kyc_user(&mint);
    env.mint_to(&mint, &alice_ata, 1_000 * TOKEN).unwrap();

    let amount = 100 * TOKEN;
    let fee = fee_for(amount, FEE_BPS);
    assert_eq!(fee, TOKEN / 2, "0.5% of 100");

    let ix = ix::transfer(&alice.pubkey(), &mint, &alice_ata, &bob_ata, amount);
    let meta = env.send(&[ix], &alice, &[]).expect("transfer");
    assert!(
        meta.logs
            .iter()
            .any(|log| log.contains("TransferCheckedWithFee")),
        "must use TransferCheckedWithFee: {:#?}",
        meta.logs
    );

    assert_eq!(env.token(&alice_ata).amount, 900 * TOKEN);
    let bob = env.token(&bob_ata);
    assert_eq!(bob.amount, amount - fee, "receiver gets amount minus fee");
    assert_eq!(bob.withheld, fee, "fee is withheld for the issuer");
}

#[test]
fn fee_is_capped_at_the_maximum() {
    let mut env = Env::new();
    let mint = env.create_mint();
    let (alice, alice_ata) = env.kyc_user(&mint);
    let (_bob, bob_ata) = env.kyc_user(&mint);
    env.mint_to(&mint, &alice_ata, 10_000 * TOKEN).unwrap();

    // 0.5% of 5,000 is 25 tokens, above the 5 token cap.
    let amount = 5_000 * TOKEN;
    env.send(
        &[ix::transfer(
            &alice.pubkey(),
            &mint,
            &alice_ata,
            &bob_ata,
            amount,
        )],
        &alice,
        &[],
    )
    .expect("transfer");

    let bob = env.token(&bob_ata);
    assert_eq!(bob.withheld, MAX_FEE);
    assert_eq!(bob.amount, amount - MAX_FEE);
}

#[test]
fn fee_follows_the_current_epoch_not_a_cached_rate() {
    let mut env = Env::new();
    let mint = env.create_mint();
    let (alice, alice_ata) = env.kyc_user(&mint);
    let (_bob, bob_ata) = env.kyc_user(&mint);
    env.mint_to(&mint, &alice_ata, 1_000 * TOKEN).unwrap();
    let amount = 100 * TOKEN;

    // Issuer schedules a new rate: 1%. Token-2022 activates it two epochs later.
    let new_bps = 100;
    let issuer = env.issuer.pubkey();
    let ix = set_transfer_fee(&TOKEN_2022, &mint, &issuer, &[], new_bps, MAX_FEE).unwrap();
    env.send_as_issuer(&[ix], &[]).expect("set_transfer_fee");

    // Same epoch: old rate still applies.
    env.send(
        &[ix::transfer(
            &alice.pubkey(),
            &mint,
            &alice_ata,
            &bob_ata,
            amount,
        )],
        &alice,
        &[],
    )
    .expect("transfer at old rate");
    let old_fee = fee_for(amount, FEE_BPS);
    assert_eq!(env.token(&bob_ata).withheld, old_fee);

    // Two epochs later the new rate is live.
    env.warp_epochs(2);
    let new_fee = fee_for(amount, new_bps);
    assert_eq!(env.current_fee(&mint, amount), new_fee);

    // A caller that cached the old rate is rejected by Token-2022...
    let stale = transfer_checked_with_fee(
        &TOKEN_2022,
        &alice_ata,
        &mint,
        &bob_ata,
        &alice.pubkey(),
        &[],
        amount,
        DECIMALS,
        old_fee,
    )
    .unwrap();
    assert_err(
        env.send(&[stale], &alice, &[]),
        token_err(TokenError::FeeMismatch),
    );

    // ...while the program recomputes it for the current epoch and succeeds.
    env.send(
        &[ix::transfer(
            &alice.pubkey(),
            &mint,
            &alice_ata,
            &bob_ata,
            amount,
        )],
        &alice,
        &[],
    )
    .expect("transfer at new rate");

    let bob = env.token(&bob_ata);
    assert_eq!(bob.withheld, old_fee + new_fee);
    assert_eq!(bob.amount, 2 * amount - old_fee - new_fee);
}

#[test]
fn cannot_send_to_an_account_that_has_not_passed_kyc() {
    let mut env = Env::new();
    let mint = env.create_mint();
    let (alice, alice_ata) = env.kyc_user(&mint);
    let bob = env.user();
    let bob_ata = env.create_ata(&bob.pubkey(), &mint); // still frozen
    env.mint_to(&mint, &alice_ata, 100 * TOKEN).unwrap();

    let ix = ix::transfer(&alice.pubkey(), &mint, &alice_ata, &bob_ata, 10 * TOKEN);
    assert_err(
        env.send(&[ix], &alice, &[]),
        token_err(TokenError::AccountFrozen),
    );
    assert_eq!(env.token(&alice_ata).amount, 100 * TOKEN);
}

#[test]
fn only_the_owner_can_send() {
    let mut env = Env::new();
    let mint = env.create_mint();
    let (_alice, alice_ata) = env.kyc_user(&mint);
    let (mallory, mallory_ata) = env.kyc_user(&mint);
    env.mint_to(&mint, &alice_ata, 100 * TOKEN).unwrap();

    let ix = ix::transfer(
        &mallory.pubkey(),
        &mint,
        &alice_ata,
        &mallory_ata,
        10 * TOKEN,
    );
    let result = env.send(&[ix], &mallory, &[]);

    assert_err(
        result,
        anchor_err(anchor_lang::error::ErrorCode::ConstraintTokenOwner),
    );
}
