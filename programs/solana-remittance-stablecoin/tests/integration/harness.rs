//! Test environment: LiteSVM with the program loaded, plus helpers for the
//! issuer, users, token accounts and reading state.
//!
//! All account and mint state is read through `StateWithExtensions`.

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};
use anchor_lang::{InstructionData, ToAccountMetas};
use anchor_spl::token_2022::spl_token_2022::{
    self,
    extension::{
        transfer_fee::{TransferFeeAmount, TransferFeeConfig},
        BaseStateWithExtensions, StateWithExtensions,
    },
    state::{Account, AccountState, Mint},
};
use litesvm::types::{FailedTransactionMetadata, TransactionMetadata};
use litesvm::LiteSVM;
use solana_compute_budget_interface::ComputeBudgetInstruction;
use solana_keypair::Keypair;
use solana_message::Message;
use solana_signer::Signer;
use solana_transaction::Transaction;

use solana_remittance_stablecoin::{accounts, instruction, ConfidentialParams, MintParams};

pub type TxResult = Result<TransactionMetadata, FailedTransactionMetadata>;

pub const TOKEN_2022: Pubkey = spl_token_2022::ID;
pub const PROGRAM_ID: Pubkey = solana_remittance_stablecoin::ID;

pub const DECIMALS: u8 = 6;
/// One whole token in base units.
pub const TOKEN: u64 = 1_000_000;
/// 0.5% protocol fee.
pub const FEE_BPS: u16 = 50;
/// Fee cap: 5 tokens per transfer.
pub const MAX_FEE: u64 = 5 * TOKEN;

const PROGRAM_SO: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/deploy/solana_remittance_stablecoin.so"
);

/// Token-2022 v10.0.0 built with `zk-ops`. LiteSVM's bundled copy is the
/// mainnet build, where confidential balance operations are compiled out.
/// Rebuild with `scripts/build-token-2022-zk-ops.sh`.
const TOKEN_2022_ZK_OPS_SO: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/spl_token_2022_zk_ops.so"
);

pub fn mint_params() -> MintParams {
    MintParams {
        decimals: DECIMALS,
        transfer_fee_basis_points: FEE_BPS,
        maximum_fee: MAX_FEE,
        name: "Remit USD".to_string(),
        symbol: "rUSD".to_string(),
        uri: "https://example.com/rusd.json".to_string(),
    }
}

/// Fee Token-2022 charges at `bps`, capped at `MAX_FEE` (rounded up).
pub fn fee_for(amount: u64, bps: u16) -> u64 {
    let fee = (amount as u128 * bps as u128).div_ceil(10_000) as u64;
    fee.min(MAX_FEE)
}

pub struct Env {
    pub svm: LiteSVM,
    /// Holds every mint authority: mint, freeze, fee, close, metadata,
    /// permanent delegate and confidential-transfer approval.
    pub issuer: Keypair,
}

/// Public view of a token account.
#[derive(Debug)]
pub struct TokenView {
    pub amount: u64,
    pub frozen: bool,
    /// Protocol fee withheld on this account (issuer revenue).
    pub withheld: u64,
}

impl Env {
    pub fn new() -> Self {
        let mut svm = LiteSVM::new();
        svm.add_program_from_file(TOKEN_2022, TOKEN_2022_ZK_OPS_SO)
            .expect("Token-2022 fixture missing: run scripts/build-token-2022-zk-ops.sh");
        svm.add_program_from_file(PROGRAM_ID, PROGRAM_SO)
            .expect("program binary missing: run `anchor build` first");

        let issuer = Keypair::new();
        svm.airdrop(&issuer.pubkey(), 100_000_000_000).unwrap();
        Self { svm, issuer }
    }

    /// Sends `ixs` with a raised compute limit (ZK proof verification needs
    /// it), then expires the blockhash so identical txs never collide.
    pub fn send(&mut self, ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair]) -> TxResult {
        let mut instructions = vec![ComputeBudgetInstruction::set_compute_unit_limit(1_400_000)];
        instructions.extend_from_slice(ixs);

        let mut keys: Vec<&Keypair> = vec![payer];
        for signer in signers {
            if !keys.iter().any(|k| k.pubkey() == signer.pubkey()) {
                keys.push(signer);
            }
        }

        let message = Message::new(&instructions, Some(&payer.pubkey()));
        let tx = Transaction::new(&keys[..], message, self.svm.latest_blockhash());
        let result = self.svm.send_transaction(tx);
        self.svm.expire_blockhash();
        result
    }

    /// Sends as the issuer.
    pub fn send_as_issuer(&mut self, ixs: &[Instruction], signers: &[&Keypair]) -> TxResult {
        let issuer = self.issuer.insecure_clone();
        self.send(ixs, &issuer, signers)
    }

    pub fn user(&mut self) -> Keypair {
        let user = Keypair::new();
        self.svm.airdrop(&user.pubkey(), 10_000_000_000).unwrap();
        user
    }

    // ------------------------------------------------------------ mints

    /// Mint #1 via `initialize_mint`.
    pub fn create_mint(&mut self) -> Pubkey {
        let mint = Keypair::new();
        let ix = ix::initialize_mint(&self.issuer.pubkey(), &mint.pubkey(), mint_params());
        self.send_as_issuer(&[ix], &[&mint])
            .expect("initialize_mint");
        mint.pubkey()
    }

    /// Mint #2 via `reissue_confidential_mint`.
    pub fn create_confidential_mint(&mut self, confidential: ConfidentialParams) -> Pubkey {
        let mint = Keypair::new();
        let ix = ix::reissue_confidential_mint(
            &self.issuer.pubkey(),
            &mint.pubkey(),
            mint_params(),
            confidential,
        );
        self.send_as_issuer(&[ix], &[&mint])
            .expect("reissue_confidential_mint");
        mint.pubkey()
    }

    // --------------------------------------------------- token accounts

    /// Creates `owner`'s ATA. Permissionless: `payer` can be anyone.
    pub fn create_ata_paid_by(&mut self, payer: &Keypair, owner: &Pubkey, mint: &Pubkey) -> Pubkey {
        let ata = ata_address(owner, mint);
        let ix = Instruction {
            program_id: anchor_spl::associated_token::ID,
            accounts: vec![
                AccountMeta::new(payer.pubkey(), true),
                AccountMeta::new(ata, false),
                AccountMeta::new_readonly(*owner, false),
                AccountMeta::new_readonly(*mint, false),
                AccountMeta::new_readonly(anchor_lang::system_program::ID, false),
                AccountMeta::new_readonly(TOKEN_2022, false),
            ],
            data: vec![0], // Create
        };
        self.send(&[ix], payer, &[]).expect("create ATA");
        ata
    }

    pub fn create_ata(&mut self, owner: &Pubkey, mint: &Pubkey) -> Pubkey {
        let issuer = self.issuer.insecure_clone();
        self.create_ata_paid_by(&issuer, owner, mint)
    }

    /// KYC: freeze authority thaws `owner`'s account.
    pub fn thaw(&mut self, mint: &Pubkey, owner: &Pubkey) -> TxResult {
        let ix = ix::thaw_account(
            &self.issuer.pubkey(),
            mint,
            &ata_address(owner, mint),
            owner,
        );
        self.send_as_issuer(&[ix], &[])
    }

    pub fn freeze(&mut self, mint: &Pubkey, account: &Pubkey) -> TxResult {
        let ix = ix::freeze_account(&self.issuer.pubkey(), mint, account);
        self.send_as_issuer(&[ix], &[])
    }

    /// New funded user with a created and KYC-thawed ATA.
    pub fn kyc_user(&mut self, mint: &Pubkey) -> (Keypair, Pubkey) {
        let user = self.user();
        let ata = self.create_ata(&user.pubkey(), mint);
        self.thaw(mint, &user.pubkey()).expect("thaw");
        (user, ata)
    }

    pub fn mint_to(&mut self, mint: &Pubkey, account: &Pubkey, amount: u64) -> TxResult {
        let ix = spl_token_2022::instruction::mint_to_checked(
            &TOKEN_2022,
            mint,
            account,
            &self.issuer.pubkey(),
            &[],
            amount,
            DECIMALS,
        )
        .unwrap();
        self.send_as_issuer(&[ix], &[])
    }

    // ------------------------------------------------------------ reads

    pub fn data(&self, address: &Pubkey) -> Vec<u8> {
        self.svm
            .get_account(address)
            .unwrap_or_else(|| panic!("account {address} not found"))
            .data
    }

    pub fn exists(&self, address: &Pubkey) -> bool {
        self.svm
            .get_account(address)
            .is_some_and(|account| account.lamports > 0)
    }

    pub fn token(&self, address: &Pubkey) -> TokenView {
        let data = self.data(address);
        let account = StateWithExtensions::<Account>::unpack(&data).unwrap();
        TokenView {
            amount: account.base.amount,
            frozen: account.base.state == AccountState::Frozen,
            withheld: account
                .get_extension::<TransferFeeAmount>()
                .map(|fee| u64::from(fee.withheld_amount))
                .unwrap_or(0),
        }
    }

    pub fn supply(&self, mint: &Pubkey) -> u64 {
        let data = self.data(mint);
        StateWithExtensions::<Mint>::unpack(&data)
            .unwrap()
            .base
            .supply
    }

    /// Fee Token-2022 will charge right now, from the mint's TransferFeeConfig.
    pub fn current_fee(&self, mint: &Pubkey, amount: u64) -> u64 {
        let data = self.data(mint);
        let mint = StateWithExtensions::<Mint>::unpack(&data).unwrap();
        mint.get_extension::<TransferFeeConfig>()
            .unwrap()
            .calculate_epoch_fee(self.epoch(), amount)
            .unwrap()
    }

    pub fn epoch(&self) -> u64 {
        self.svm.get_sysvar::<anchor_lang::prelude::Clock>().epoch
    }

    pub fn warp_epochs(&mut self, epochs: u64) {
        let mut clock = self.svm.get_sysvar::<anchor_lang::prelude::Clock>();
        clock.epoch += epochs;
        clock.slot += epochs * 432_000;
        self.svm.set_sysvar(&clock);
    }
}

pub fn ata_address(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[owner.as_ref(), TOKEN_2022.as_ref(), mint.as_ref()],
        &anchor_spl::associated_token::ID,
    )
    .0
}

/// Asserts the tx failed with custom error `code` (from our program, Anchor
/// or Token-2022 through CPI).
#[track_caller]
pub fn assert_err(result: TxResult, code: u32) {
    match result {
        Ok(meta) => panic!(
            "expected error {code}, tx succeeded. logs: {:#?}",
            meta.logs
        ),
        Err(failed) => {
            let err = format!("{:?}", failed.err);
            assert!(
                err.contains(&format!("Custom({code})")),
                "expected Custom({code}), got {err}\nlogs: {:#?}",
                failed.meta.logs
            );
        }
    }
}

/// Asserts the tx failed with a built-in error, e.g. `MissingRequiredSignature`.
#[track_caller]
pub fn assert_failed_with(result: TxResult, error: &str) {
    match result {
        Ok(meta) => panic!("expected {error}, tx succeeded. logs: {:#?}", meta.logs),
        Err(failed) => {
            let err = format!("{:?}", failed.err);
            assert!(
                err.contains(error),
                "expected {error}, got {err}\nlogs: {:#?}",
                failed.meta.logs
            );
        }
    }
}

/// Size of a TokenMetadata entry inside a Token-2022 mint:
/// u16 extension type + u16 length + borsh-encoded metadata.
pub fn metadata_tlv_len(
    metadata: &anchor_spl::token_interface::spl_token_metadata_interface::state::TokenMetadata,
) -> usize {
    2 + 2 + anchor_lang::prelude::borsh::to_vec(metadata).unwrap().len()
}

/// Anchor custom error code for our program's `ErrorCode`.
pub fn program_err(code: solana_remittance_stablecoin::error::ErrorCode) -> u32 {
    anchor_lang::error::ERROR_CODE_OFFSET + code as u32
}

pub fn anchor_err(code: anchor_lang::error::ErrorCode) -> u32 {
    code as u32
}

pub fn token_err(code: spl_token_2022::error::TokenError) -> u32 {
    code as u32
}

/// Instruction builders for every program instruction.
pub mod ix {
    use super::*;

    fn build(accounts: impl ToAccountMetas, data: impl InstructionData) -> Instruction {
        Instruction {
            program_id: PROGRAM_ID,
            accounts: accounts.to_account_metas(None),
            data: data.data(),
        }
    }

    pub fn initialize_mint(authority: &Pubkey, mint: &Pubkey, params: MintParams) -> Instruction {
        build(
            accounts::InitializeMint {
                authority: *authority,
                mint: *mint,
                token_program: TOKEN_2022,
                system_program: anchor_lang::system_program::ID,
            },
            instruction::InitializeMint { params },
        )
    }

    pub fn reissue_confidential_mint(
        authority: &Pubkey,
        mint: &Pubkey,
        params: MintParams,
        confidential: ConfidentialParams,
    ) -> Instruction {
        build(
            accounts::ReissueConfidentialMint {
                authority: *authority,
                mint: *mint,
                token_program: TOKEN_2022,
                system_program: anchor_lang::system_program::ID,
            },
            instruction::ReissueConfidentialMint {
                params,
                confidential,
            },
        )
    }

    pub fn close_mint(authority: &Pubkey, mint: &Pubkey, destination: &Pubkey) -> Instruction {
        build(
            accounts::CloseMint {
                authority: *authority,
                mint: *mint,
                destination: *destination,
                token_program: TOKEN_2022,
            },
            instruction::CloseMint {},
        )
    }

    pub fn thaw_account(
        authority: &Pubkey,
        mint: &Pubkey,
        token_account: &Pubkey,
        owner: &Pubkey,
    ) -> Instruction {
        build(
            accounts::ThawAccount {
                authority: *authority,
                mint: *mint,
                token_account: *token_account,
                owner: *owner,
                token_program: TOKEN_2022,
            },
            instruction::ThawAccount {},
        )
    }

    pub fn freeze_account(
        authority: &Pubkey,
        mint: &Pubkey,
        token_account: &Pubkey,
    ) -> Instruction {
        build(
            accounts::FreezeAccount {
                authority: *authority,
                mint: *mint,
                token_account: *token_account,
                token_program: TOKEN_2022,
            },
            instruction::FreezeAccount {},
        )
    }

    pub fn seize(
        authority: &Pubkey,
        mint: &Pubkey,
        sanctioned_account: &Pubkey,
        treasury: &Pubkey,
        amount: u64,
    ) -> Instruction {
        build(
            accounts::Seize {
                authority: *authority,
                mint: *mint,
                sanctioned_account: *sanctioned_account,
                treasury: *treasury,
                token_program: TOKEN_2022,
            },
            instruction::Seize { amount },
        )
    }

    pub fn transfer(
        authority: &Pubkey,
        mint: &Pubkey,
        source: &Pubkey,
        destination: &Pubkey,
        amount: u64,
    ) -> Instruction {
        build(
            accounts::Transfer {
                authority: *authority,
                mint: *mint,
                source: *source,
                destination: *destination,
                token_program: TOKEN_2022,
            },
            instruction::Transfer { amount },
        )
    }

    pub fn configure_account(
        owner: &Pubkey,
        mint: &Pubkey,
        token_account: &Pubkey,
        proof_context_state: &Pubkey,
        decryptable_zero_balance: [u8; 36],
        maximum_pending_balance_credit_counter: u64,
    ) -> Instruction {
        build(
            accounts::ConfigureAccount {
                owner: *owner,
                mint: *mint,
                token_account: *token_account,
                proof_context_state: *proof_context_state,
                token_program: TOKEN_2022,
                system_program: anchor_lang::system_program::ID,
            },
            instruction::ConfigureAccount {
                decryptable_zero_balance,
                maximum_pending_balance_credit_counter,
            },
        )
    }

    pub fn approve_account(
        authority: &Pubkey,
        mint: &Pubkey,
        token_account: &Pubkey,
    ) -> Instruction {
        build(
            accounts::ApproveAccount {
                authority: *authority,
                mint: *mint,
                token_account: *token_account,
                token_program: TOKEN_2022,
            },
            instruction::ApproveAccount {},
        )
    }

    pub fn deposit_confidential(
        owner: &Pubkey,
        mint: &Pubkey,
        token_account: &Pubkey,
        amount: u64,
    ) -> Instruction {
        build(
            accounts::DepositConfidential {
                owner: *owner,
                mint: *mint,
                token_account: *token_account,
                token_program: TOKEN_2022,
            },
            instruction::DepositConfidential { amount },
        )
    }

    pub fn apply_pending_balance(
        owner: &Pubkey,
        token_account: &Pubkey,
        expected_pending_balance_credit_counter: u64,
        new_decryptable_available_balance: [u8; 36],
    ) -> Instruction {
        build(
            accounts::ApplyPendingBalance {
                owner: *owner,
                token_account: *token_account,
                token_program: TOKEN_2022,
            },
            instruction::ApplyPendingBalance {
                expected_pending_balance_credit_counter,
                new_decryptable_available_balance,
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn confidential_transfer(
        owner: &Pubkey,
        mint: &Pubkey,
        source: &Pubkey,
        destination: &Pubkey,
        proofs: [Pubkey; 5],
        new_source_decryptable_available_balance: [u8; 36],
        transfer_amount_auditor_ciphertext_lo: [u8; 64],
        transfer_amount_auditor_ciphertext_hi: [u8; 64],
    ) -> Instruction {
        build(
            accounts::ConfidentialTransfer {
                owner: *owner,
                mint: *mint,
                source: *source,
                destination: *destination,
                equality_proof: proofs[0],
                transfer_amount_ciphertext_validity_proof: proofs[1],
                fee_sigma_proof: proofs[2],
                fee_ciphertext_validity_proof: proofs[3],
                range_proof: proofs[4],
                token_program: TOKEN_2022,
            },
            instruction::ConfidentialTransfer {
                new_source_decryptable_available_balance,
                transfer_amount_auditor_ciphertext_lo,
                transfer_amount_auditor_ciphertext_hi,
            },
        )
    }

    pub fn withdraw_confidential(
        owner: &Pubkey,
        mint: &Pubkey,
        token_account: &Pubkey,
        proofs: [Pubkey; 2],
        amount: u64,
        new_decryptable_available_balance: [u8; 36],
    ) -> Instruction {
        build(
            accounts::WithdrawConfidential {
                owner: *owner,
                mint: *mint,
                token_account: *token_account,
                equality_proof: proofs[0],
                range_proof: proofs[1],
                token_program: TOKEN_2022,
            },
            instruction::WithdrawConfidential {
                amount,
                new_decryptable_available_balance,
            },
        )
    }
}
