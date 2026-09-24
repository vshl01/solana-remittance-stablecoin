//! Client side of confidential transfers: keys, balance decryption, proof
//! generation, and proof context accounts.
//!
//! Each proof is verified by the ZK ElGamal proof program into its own
//! context state account; the program's instructions then point Token-2022 at
//! those accounts.

use std::mem::size_of;

use anchor_lang::prelude::Pubkey;
use anchor_spl::token_2022::spl_token_2022::{
    extension::{
        confidential_transfer::{ConfidentialTransferAccount, ConfidentialTransferMint},
        confidential_transfer_fee::ConfidentialTransferFeeConfig,
        transfer_fee::TransferFeeConfig,
        BaseStateWithExtensions, StateWithExtensions,
    },
    solana_zk_sdk::{
        encryption::{
            auth_encryption::{AeCiphertext, AeKey},
            elgamal::{ElGamalCiphertext, ElGamalKeypair, ElGamalPubkey},
            pod::{
                auth_encryption::PodAeCiphertext,
                elgamal::{PodElGamalCiphertext, PodElGamalPubkey},
            },
        },
        zk_elgamal_proof_program::{
            self,
            instruction::{close_context_state, ContextStateInfo, ProofInstruction},
            proof_data::{PubkeyValidityProofData, ZkProofData},
            state::ProofContextState,
        },
    },
    state::{Account, Mint},
};
use bytemuck::Pod;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_system_interface::instruction::create_account;
use spl_token_confidential_transfer_proof_generation::{
    transfer_with_fee::transfer_with_fee_split_proof_data, try_combine_lo_hi_ciphertexts,
    withdraw::withdraw_proof_data,
};

use crate::harness::{ix, Env, TxResult};

/// Pending balances and transfer amounts are split into a 16-bit low part and
/// a high part so they can be decrypted quickly.
const LO_BITS: usize = 16;

/// Token-2022 CLI default.
pub const MAX_PENDING_CREDITS: u64 = 65_536;

pub fn pod_pubkey_bytes(keypair: &ElGamalKeypair) -> [u8; 32] {
    bytemuck::bytes_of(&PodElGamalPubkey::from(*keypair.pubkey()))
        .try_into()
        .unwrap()
}

fn ae_bytes(ciphertext: AeCiphertext) -> [u8; 36] {
    bytemuck::bytes_of(&PodAeCiphertext::from(ciphertext))
        .try_into()
        .unwrap()
}

fn elgamal_bytes(ciphertext: &PodElGamalCiphertext) -> [u8; 64] {
    bytemuck::bytes_of(ciphertext).try_into().unwrap()
}

/// Decrypts a ciphertext split into lo/hi halves. Combining first keeps this
/// correct even when a half went "negative" (e.g. fee subtracted from lo).
pub fn decrypt_lo_hi(
    keypair: &ElGamalKeypair,
    lo: PodElGamalCiphertext,
    hi: PodElGamalCiphertext,
) -> u64 {
    try_decrypt_lo_hi(keypair, lo, hi).expect("decrypt")
}

pub fn try_decrypt_lo_hi(
    keypair: &ElGamalKeypair,
    lo: PodElGamalCiphertext,
    hi: PodElGamalCiphertext,
) -> Option<u64> {
    let lo: ElGamalCiphertext = lo.try_into().unwrap();
    let hi: ElGamalCiphertext = hi.try_into().unwrap();
    let total = try_combine_lo_hi_ciphertexts(&lo, &hi, LO_BITS).unwrap();
    keypair.secret().decrypt_u32(&total)
}

pub fn decrypt(keypair: &ElGamalKeypair, ciphertext: PodElGamalCiphertext) -> u64 {
    let ciphertext: ElGamalCiphertext = ciphertext.try_into().unwrap();
    keypair.secret().decrypt_u32(&ciphertext).expect("decrypt")
}

/// Issuer-side ElGamal keys for the confidential mint.
pub struct IssuerKeys {
    /// Can decrypt every confidential transfer amount.
    pub auditor: ElGamalKeypair,
    /// Confidential transfer fees are encrypted to this key.
    pub withdraw_withheld: ElGamalKeypair,
}

impl IssuerKeys {
    pub fn new() -> Self {
        Self {
            auditor: ElGamalKeypair::new_rand(),
            withdraw_withheld: ElGamalKeypair::new_rand(),
        }
    }

    pub fn params(&self) -> solana_remittance_stablecoin::ConfidentialParams {
        solana_remittance_stablecoin::ConfidentialParams {
            auditor_elgamal_pubkey: Some(pod_pubkey_bytes(&self.auditor)),
            withdraw_withheld_authority_elgamal_pubkey: pod_pubkey_bytes(&self.withdraw_withheld),
        }
    }
}

/// A holder's confidential keys. Wallets usually derive these from a
/// signature; random keys are enough for tests.
pub struct UserKeys {
    pub elgamal: ElGamalKeypair,
    pub aes: AeKey,
}

impl UserKeys {
    pub fn new() -> Self {
        Self {
            elgamal: ElGamalKeypair::new_rand(),
            aes: AeKey::new_rand(),
        }
    }

    pub fn encrypt_balance(&self, amount: u64) -> [u8; 36] {
        ae_bytes(self.aes.encrypt(amount))
    }

    /// Available balance, read from the owner-only AES ciphertext.
    pub fn available(&self, account: &ConfidentialTransferAccount) -> u64 {
        let ciphertext: AeCiphertext = account.decryptable_available_balance.try_into().unwrap();
        self.aes.decrypt(&ciphertext).expect("AES decrypt")
    }

    /// Available balance, read from the ElGamal ciphertext Token-2022 updates.
    pub fn available_elgamal(&self, account: &ConfidentialTransferAccount) -> u64 {
        decrypt(&self.elgamal, account.available_balance)
    }

    pub fn pending(&self, account: &ConfidentialTransferAccount) -> u64 {
        decrypt_lo_hi(
            &self.elgamal,
            account.pending_balance_lo,
            account.pending_balance_hi,
        )
    }
}

pub fn confidential_account(env: &Env, address: &Pubkey) -> ConfidentialTransferAccount {
    let data = env.data(address);
    let account = StateWithExtensions::<Account>::unpack(&data).unwrap();
    *account
        .get_extension::<ConfidentialTransferAccount>()
        .unwrap()
}

/// Verifies `proof` into a fresh context state account owned by the ZK
/// ElGamal proof program. Returns the context account address.
pub fn verify_proof<T, U>(
    env: &mut Env,
    payer: &Keypair,
    instruction: ProofInstruction,
    proof: &T,
) -> Pubkey
where
    T: Pod + ZkProofData<U>,
    U: Pod,
{
    let context = Keypair::new();
    let space = size_of::<ProofContextState<U>>();
    let lamports = env.svm.minimum_balance_for_rent_exemption(space);

    let create = create_account(
        &payer.pubkey(),
        &context.pubkey(),
        lamports,
        space as u64,
        &zk_elgamal_proof_program::id(),
    );
    let verify = instruction.encode_verify_proof(
        Some(ContextStateInfo {
            context_state_account: &context.pubkey(),
            context_state_authority: &payer.pubkey(),
        }),
        proof,
    );

    env.send(&[create, verify], payer, &[&context])
        .expect("proof verification");
    context.pubkey()
}

/// Closes proof context accounts and returns their rent to `payer`.
pub fn close_proofs(env: &mut Env, payer: &Keypair, contexts: &[Pubkey]) {
    let ixs: Vec<_> = contexts
        .iter()
        .map(|context| {
            close_context_state(
                ContextStateInfo {
                    context_state_account: context,
                    context_state_authority: &payer.pubkey(),
                },
                &payer.pubkey(),
            )
        })
        .collect();
    env.send(&ixs, payer, &[]).expect("close proof contexts");
}

// ------------------------------------------------------------ lifecycle

/// Owner signs `configure_account` with a verified pubkey-validity proof for
/// `keys` (the proof shows the owner knows the ElGamal secret key).
pub fn configure(
    env: &mut Env,
    owner: &Keypair,
    keys: &UserKeys,
    mint: &Pubkey,
    account: &Pubkey,
) -> TxResult {
    let proof = PubkeyValidityProofData::new(&keys.elgamal).unwrap();
    let context = verify_proof(env, owner, ProofInstruction::VerifyPubkeyValidity, &proof);

    let ix = ix::configure_account(
        &owner.pubkey(),
        mint,
        account,
        &context,
        keys.encrypt_balance(0),
        MAX_PENDING_CREDITS,
    );
    let result = env.send(&[ix], owner, &[]);
    close_proofs(env, owner, &[context]);
    result
}

pub fn approve(env: &mut Env, mint: &Pubkey, account: &Pubkey) -> TxResult {
    let ix = ix::approve_account(&env.issuer.pubkey(), mint, account);
    env.send_as_issuer(&[ix], &[])
}

pub fn deposit(
    env: &mut Env,
    owner: &Keypair,
    mint: &Pubkey,
    account: &Pubkey,
    amount: u64,
) -> TxResult {
    let ix = ix::deposit_confidential(&owner.pubkey(), mint, account, amount);
    env.send(&[ix], owner, &[])
}

/// Folds pending into available, re-encrypting the new total under the
/// owner's AES key.
pub fn apply_pending(
    env: &mut Env,
    owner: &Keypair,
    keys: &UserKeys,
    account: &Pubkey,
) -> TxResult {
    let state = confidential_account(env, account);
    let new_available = keys.available(&state) + keys.pending(&state);

    let ix = ix::apply_pending_balance(
        &owner.pubkey(),
        account,
        u64::from(state.pending_balance_credit_counter),
        keys.encrypt_balance(new_available),
    );
    env.send(&[ix], owner, &[])
}

/// Generates the five transfer-with-fee proofs, verifies them into context
/// accounts, sends `confidential_transfer`, then closes the contexts.
pub fn transfer(
    env: &mut Env,
    sender: &Keypair,
    keys: &UserKeys,
    mint: &Pubkey,
    source: &Pubkey,
    destination: &Pubkey,
    amount: u64,
) -> TxResult {
    transfer_with_audit(env, sender, keys, mint, source, destination, amount).0
}

/// Same as [`transfer`], also returning the transfer amount encrypted to the
/// auditor (lo, hi). These ciphertexts are part of the public instruction
/// data; only the auditor's key opens them.
pub fn transfer_with_audit(
    env: &mut Env,
    sender: &Keypair,
    keys: &UserKeys,
    mint: &Pubkey,
    source: &Pubkey,
    destination: &Pubkey,
    amount: u64,
) -> (TxResult, (PodElGamalCiphertext, PodElGamalCiphertext)) {
    let source_state = confidential_account(env, source);
    let destination_state = confidential_account(env, destination);

    let mint_data = env.data(mint);
    let mint_state = StateWithExtensions::<Mint>::unpack(&mint_data).unwrap();
    let auditor = Option::<PodElGamalPubkey>::from(
        mint_state
            .get_extension::<ConfidentialTransferMint>()
            .unwrap()
            .auditor_elgamal_pubkey,
    )
    .map(|pubkey| ElGamalPubkey::try_from(pubkey).unwrap());
    let withdraw_withheld: ElGamalPubkey = mint_state
        .get_extension::<ConfidentialTransferFeeConfig>()
        .unwrap()
        .withdraw_withheld_authority_elgamal_pubkey
        .try_into()
        .unwrap();
    let fee = *mint_state
        .get_extension::<TransferFeeConfig>()
        .unwrap()
        .get_epoch_fee(env.epoch());

    let current_available = keys.available(&source_state);
    let proofs = transfer_with_fee_split_proof_data(
        &source_state.available_balance.try_into().unwrap(),
        &source_state
            .decryptable_available_balance
            .try_into()
            .unwrap(),
        amount,
        &keys.elgamal,
        &keys.aes,
        &destination_state.elgamal_pubkey.try_into().unwrap(),
        auditor.as_ref(),
        &withdraw_withheld,
        u16::from(fee.transfer_fee_basis_points),
        u64::from(fee.maximum_fee),
    )
    .expect("transfer proof generation");

    let amount_validity = &proofs.transfer_amount_ciphertext_validity_proof_data_with_ciphertext;
    let contexts = [
        verify_proof(
            env,
            sender,
            ProofInstruction::VerifyCiphertextCommitmentEquality,
            &proofs.equality_proof_data,
        ),
        verify_proof(
            env,
            sender,
            ProofInstruction::VerifyBatchedGroupedCiphertext3HandlesValidity,
            &amount_validity.proof_data,
        ),
        verify_proof(
            env,
            sender,
            ProofInstruction::VerifyPercentageWithCap,
            &proofs.percentage_with_cap_proof_data,
        ),
        verify_proof(
            env,
            sender,
            ProofInstruction::VerifyBatchedGroupedCiphertext2HandlesValidity,
            &proofs.fee_ciphertext_validity_proof_data,
        ),
        verify_proof(
            env,
            sender,
            ProofInstruction::VerifyBatchedRangeProofU256,
            &proofs.range_proof_data,
        ),
    ];

    let ix = ix::confidential_transfer(
        &sender.pubkey(),
        mint,
        source,
        destination,
        contexts,
        keys.encrypt_balance(current_available - amount),
        elgamal_bytes(&amount_validity.ciphertext_lo),
        elgamal_bytes(&amount_validity.ciphertext_hi),
    );
    let result = env.send(&[ix], sender, &[]);
    close_proofs(env, sender, &contexts);
    (
        result,
        (amount_validity.ciphertext_lo, amount_validity.ciphertext_hi),
    )
}

/// Withdraws `amount` from the available balance back to the public balance.
pub fn withdraw(
    env: &mut Env,
    owner: &Keypair,
    keys: &UserKeys,
    mint: &Pubkey,
    account: &Pubkey,
    amount: u64,
) -> TxResult {
    let state = confidential_account(env, account);
    let current = keys.available(&state);
    let available: ElGamalCiphertext = state.available_balance.try_into().unwrap();
    withdraw_against(env, owner, keys, mint, account, &available, current, amount)
}

/// Withdraw with proofs built against an arbitrary `claimed_balance`
/// ciphertext. Used to show Token-2022 rejects proofs that do not match the
/// on-chain available balance.
#[allow(clippy::too_many_arguments)]
pub fn withdraw_against(
    env: &mut Env,
    owner: &Keypair,
    keys: &UserKeys,
    mint: &Pubkey,
    account: &Pubkey,
    claimed_balance: &ElGamalCiphertext,
    claimed_amount: u64,
    amount: u64,
) -> TxResult {
    let proofs = withdraw_proof_data(claimed_balance, claimed_amount, amount, &keys.elgamal)
        .expect("withdraw proof generation");
    let contexts = [
        verify_proof(
            env,
            owner,
            ProofInstruction::VerifyCiphertextCommitmentEquality,
            &proofs.equality_proof_data,
        ),
        verify_proof(
            env,
            owner,
            ProofInstruction::VerifyBatchedRangeProofU64,
            &proofs.range_proof_data,
        ),
    ];

    let ix = ix::withdraw_confidential(
        &owner.pubkey(),
        mint,
        account,
        contexts,
        amount,
        keys.encrypt_balance(claimed_amount - amount),
    );
    let result = env.send(&[ix], owner, &[]);
    close_proofs(env, owner, &contexts);
    result
}

/// Full opt-in for a KYC'd holder: configure (owner) + approve (issuer).
pub fn onboard(env: &mut Env, owner: &Keypair, keys: &UserKeys, mint: &Pubkey, account: &Pubkey) {
    configure(env, owner, keys, mint, account).expect("configure_account");
    approve(env, mint, account).expect("approve_account");
}

/// The whole pending balance as one ciphertext (lo + hi << 16).
pub fn pending_ciphertext(state: &ConfidentialTransferAccount) -> ElGamalCiphertext {
    let lo: ElGamalCiphertext = state.pending_balance_lo.try_into().unwrap();
    let hi: ElGamalCiphertext = state.pending_balance_hi.try_into().unwrap();
    try_combine_lo_hi_ciphertexts(&lo, &hi, LO_BITS).unwrap()
}
