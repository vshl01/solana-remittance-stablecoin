# Remittance Stablecoin on Token-2022

An Anchor program that issues a remittance stablecoin on Token-2022. It charges a protocol
fee on every transfer, keeps new accounts frozen until KYC, stores its metadata inside the
mint, and can be closed if it is ever decommissioned. A second, re-issued mint adds a
seizure authority and confidential (encrypted) transfers.

![All tests passing](docs/tests-passing.png)

```
anchor test   →   test result: ok. 23 passed; 0 failed
```

---

## Contents

- [Quick start](#quick-start)
- [The two mints](#the-two-mints)
- [Instructions](#instructions)
- [How each task is met](#how-each-task-is-met)
- [The gap between seizure and privacy](#the-gap-between-seizure-and-privacy)
- [Written finding: moving funds into confidential before seizure](#written-finding-what-happens-if-a-sanctioned-user-moves-their-balance-into-the-confidential-system-before-the-permanent-delegate-acts)
- [Tests](#tests)
- [Testing note: Token-2022 with `zk-ops`](#testing-note-token-2022-with-zk-ops)
- [Project layout](#project-layout)

---

## Quick start

Requirements: Rust 1.89 (pinned in `rust-toolchain.toml`), Solana CLI 3.0.x, Anchor CLI 1.1+.

```bash
anchor test          # builds the program, then runs `cargo test`
```

or step by step:

```bash
anchor build
cargo test -p solana-remittance-stablecoin
```

The tests run on [LiteSVM](https://github.com/LiteSVM/litesvm) against the compiled program,
so no local validator is needed (`skip_local_validator = true` in `Anchor.toml`).

---

## The two mints

| Extension | Mint #1 `initialize_mint` | Mint #2 `reissue_confidential_mint` | Why |
|---|:-:|:-:|---|
| `TransferFeeConfig` | ✅ | ✅ | Protocol fee on every transfer (issuer revenue) |
| `MetadataPointer` → the mint itself | ✅ | ✅ | Wallets read metadata on-chain, no off-chain registry |
| `TokenMetadata` (name, symbol, uri) | ✅ | ✅ | The data the pointer points to |
| `DefaultAccountState = Frozen` | ✅ | ✅ | Every new account starts frozen until KYC |
| `MintCloseAuthority` | ✅ | ✅ | The mint can be closed once supply is zero |
| `PermanentDelegate` | | ✅ | Seizure authority for sanctioned wallets |
| `ConfidentialTransferMint` (manual approval, auditor key) | | ✅ | Hidden transfer amounts |
| `ConfidentialTransferFeeConfig` | | ✅ | **Required** by Token-2022 when fees and confidentiality are combined |

Confidential transfers cannot be added to an existing mint, so mint #2 is a new mint. It
carries forward the full extension set of mint #1.

**Authorities.** For clarity, one issuer key holds every role: mint, freeze, fee config,
withheld-fee withdrawal, metadata update, close, permanent delegate and confidential-transfer
approval. In production these should be separate keys, ideally multisigs.

---

## Instructions

| Instruction | Signer | What it does |
|---|---|---|
| `initialize_mint` | issuer | Creates mint #1 |
| `reissue_confidential_mint` | issuer | Creates mint #2 |
| `close_mint` | close authority | Closes an empty mint |
| `thaw_account` | freeze authority | KYC: unfreezes one user's account |
| `freeze_account` | freeze authority | Sanction: freezes an account |
| `seize` | permanent delegate + freeze authority | Thaw → transfer to treasury → re-freeze, in one instruction |
| `transfer` | owner | Public transfer via `TransferCheckedWithFee` |
| `configure_account` | owner | Reallocate + `ConfigureAccount` (register ElGamal key) |
| `approve_account` | confidential-transfer authority | Manual approval of a configured account |
| `deposit_confidential` | owner | Public balance → pending confidential balance |
| `apply_pending_balance` | owner | Pending → available confidential balance |
| `confidential_transfer` | owner | `TransferWithFee`: encrypted amount and encrypted fee |
| `withdraw_confidential` | owner | Available confidential balance → public balance |

Every confidential instruction takes proofs that were verified beforehand into **context state
accounts** by the ZK ElGamal Proof program. This keeps each transaction small and makes the
CPI simple.

---

## How each task is met

### 1. Mint with four stacked extensions, sized correctly, ordered correctly

[`utils.rs`](programs/solana-remittance-stablecoin/src/utils.rs) → `MintSetup`, used by
[`initialize.rs`](programs/solana-remittance-stablecoin/src/instructions/initialize.rs):

1. `create_account` with `space = ExtensionType::try_calculate_account_len::<Mint>(&BASE_MINT_EXTENSIONS)`.
2. `TransferFeeConfig` → `MetadataPointer` (address = the mint) → `DefaultAccountState(Frozen)` → `MintCloseAuthority`.
3. `InitializeMint2`, after every extension init.
4. `TokenMetadata` initialize. This is the one step that must come *after* `InitializeMint`,
   because it needs the mint authority's signature. It is variable-length, so rent for it is
   pre-funded in step 1 and Token-2022 reallocates it into place.

The test checks the account is **exactly** `try_calculate_account_len + metadata TLV` bytes and
holds exactly rent-exempt lamports for that size.

> Small detail found here: `TokenMetadata::tlv_size_of()` counts a 12-byte standalone-TLV
> header, but inside a Token-2022 mint the header is 4 bytes (u16 type + u16 length). Using it
> would over-fund rent by 8 bytes. The program computes the embedded size instead
> (`metadata_extension_len`).

### 2. Transfer with `transfer_checked_with_fee` and an epoch-accurate fee

[`transfer.rs`](programs/solana-remittance-stablecoin/src/instructions/transfer.rs) calls
`transfer_checked_with_fee`, never `transfer` or `transfer_checked`. The fee comes from
[`expected_transfer_fee`](programs/solana-remittance-stablecoin/src/utils.rs):

```rust
let current_epoch = Clock::get()?.epoch;
let mint = StateWithExtensions::<Mint>::unpack(&data)?;
mint.get_extension::<TransferFeeConfig>()?
    .calculate_epoch_fee(current_epoch, amount)
```

Nothing is cached. The test `fee_follows_the_current_epoch_not_a_cached_rate` schedules a new
rate with `SetTransferFee` and warps two epochs forward. A client using the old rate is then
rejected with `FeeMismatch`, while the program picks up the new rate and succeeds.

### 3. State read only through `StateWithExtensions`

- Program: `expected_transfer_fee` and `permanent_delegate` in `utils.rs` use
  `StateWithExtensions::<Mint>::unpack`. Anchor's `InterfaceAccount<Mint | TokenAccount>`
  also deserializes through `StateWithExtensions` inside anchor-spl.
- Tests: every read (`Env::token`, `Env::supply`, `Env::current_fee`, `zk::confidential_account`,
  and the mint layout checks) uses `StateWithExtensions`. There is no raw `unpack` anywhere.

### 4. KYC thaw, separate from the mint default

[`thaw_account.rs`](programs/solana-remittance-stablecoin/src/instructions/thaw_account.rs):
the freeze authority (enforced by `mint::freeze_authority = authority`) thaws **one** account,
bound to the wallet that passed KYC (`token::authority = owner`). The mint's
`DefaultAccountState` is never changed. The program has no instruction that updates it, and the
test shows the next new account is still frozen.

### 5. Re-issued mint with seizure and confidential transfers (manual approval)

[`reissue_mint.rs`](programs/solana-remittance-stablecoin/src/instructions/reissue_mint.rs):
the same four extensions plus `PermanentDelegate`, `ConfidentialTransferMint` with
`auto_approve_new_accounts = false` (approve_policy = manual) and an optional auditor ElGamal
key, plus `ConfidentialTransferFeeConfig`. See [the gap](#the-gap-between-seizure-and-privacy) for
why that last one is required.

### 6. Full confidential lifecycle

| Step | Instruction | Notes |
|---|---|---|
| Configure | `configure_account` | **Owner-only.** Anyone can create an ATA for a wallet (the test has a stranger pay for it), but only the owner can configure it: `token::authority = owner` plus the owner's signature. It reallocates for `ConfidentialTransferAccount` + `ConfidentialTransferFeeAmount`, then registers the ElGamal key with a pubkey-validity proof. |
| Approve | `approve_account` | Issuer approves after KYC. Deposits fail with `ConfidentialTransferAccountNotApproved` until then. |
| Deposit | `deposit_confidential` | Public → **pending**. |
| Apply | `apply_pending_balance` | Pending → **available**. The client decrypts pending and re-encrypts the new total under its AES key. |
| Transfer | `confidential_transfer` | `TransferWithFee` with 5 proofs: equality, amount validity, fee sigma, fee validity, range (u256). |
| Apply | `apply_pending_balance` | The receiver applies before spending. |
| Withdraw | `withdraw_confidential` | Available → public. Uses an equality proof and a range proof (u64). |

**Applying pending balance before withdrawal** is enforced on-chain.
`withdraw_needs_the_pending_balance_applied_first` builds *valid* proofs about the pending
ciphertext and tries to withdraw it. Token-2022 rejects the withdrawal with
`ConfidentialTransferBalanceMismatch` because the available balance is still zero. After
`apply_pending_balance`, the same withdrawal succeeds.

---

## The gap between seizure and privacy

Regulators want the issuer to take funds from sanctioned wallets. Users want amounts hidden.
Putting both on one mint leaves two gaps.

**1. Extension gap: the fee needs its own confidential config.** Token-2022 rejects
`TransferFeeConfig` + `ConfidentialTransferMint` on their own (`InvalidExtensionCombination`),
because an encrypted transfer needs an encrypted fee. The re-issue must add
`ConfidentialTransferFeeConfig` (an ElGamal key the withheld fees are encrypted to). Confidential
transfers then must use `TransferWithFee`, which needs five ZK proofs instead of three. The test
`fee_plus_confidentiality_is_rejected_without_confidential_fee_config` shows the rejection.

**2. Authority gap: the permanent delegate cannot reach confidential balances.** A permanent
delegate can move or burn the public `amount` field of any account. A confidential balance is
not in that field. It is an ElGamal ciphertext, and moving it requires ZK proofs that only the
holder of the account's ElGamal secret key can produce. Token-2022's confidential `Transfer`,
`Withdraw` and `EmptyAccount` accept only the account owner as authority. So on this mint,
**seizure covers the public balance only**. The confidential balance can be frozen, but only its
owner can move it.

What the issuer can still do:

- **Freeze.** A frozen account cannot deposit, withdraw, or send or receive confidential transfers.
- **Audit.** The mint's auditor key decrypts every confidential transfer amount (see
  `auditor_can_read_the_hidden_transfer_amount`).
- **Gate entry.** With manual approval, only accounts the issuer approves can use confidential
  transfers at all.

---

## Written finding: what happens if a sanctioned user moves their balance into the confidential system before the permanent delegate acts?

**Short answer: the funds become frozen-at-best, never seizable. From that point the permanent
delegate is a freeze-only power over them.**

The test `confidential_balance_is_beyond_the_permanent_delegate` replays this exactly:

1. Mallory sees the sanction coming and calls `DepositConfidentialTokens` for her whole balance,
   then `ApplyPendingBalance`. Her public `amount` is now `0`. Her 500 tokens exist only as a
   ciphertext.
2. The issuer freezes her account and calls `seize`. It fails with **`InsufficientFunds`**. The
   permanent delegate can only debit `amount`, and `amount` is zero.
3. Freezing does work as containment. Her `WithdrawConfidentialTokens` fails with `AccountFrozen`,
   and so would a confidential transfer in or out.
4. But the 500 tokens are still there, still counted in `supply`, and only Mallory's ElGamal
   key can ever move them.

Why this happens:

- **Seizure is a public-balance operation.** `PermanentDelegate` makes the delegate an accepted
  authority for `Transfer`/`Burn`, which act on `amount`. Nothing in Token-2022 lets the
  delegate act on `ConfidentialTransferAccount.available_balance`.
- **Moving an encrypted balance needs the secret key.** A confidential transfer or withdrawal
  must prove, in zero knowledge, that the new balance ciphertext is correct and not negative. Those
  proofs cannot be made without the account's ElGamal secret key. The auditor key does not help:
  it can *read* transfer amounts, but it cannot *prove* anything for the account.
- **Deposit is a single instruction.** A user can do it in the same slot they learn of a
  sanction. From then on, the race is lost.

Consequences for an issuer:

- **Reserves become irreconcilable.** Frozen confidential tokens still count as circulating
  supply, backed by real reserves, but the issuer can neither burn nor recover them. Unless
  Mallory cooperates, that backing is stuck forever.
- **Without an auditor key it is worse.** If Mallory sends confidential transfers before the
  freeze, the regulator sees which accounts she paid but not how much. This project sets an
  auditor key for that reason.
- **Approval cannot be taken back.** Token-2022 has no "un-approve". Once an account is
  approved for confidential transfers, freezing is the only lever left.

Mitigations, in order of strength:

1. **Freeze first, and freeze fast.** Freeze before any public signal of a sanction. The
   `seize` instruction already thaws → transfers → re-freezes atomically so the account is never
   left unfrozen, but that only helps while the funds are public.
2. **Always configure an auditor key.** It keeps confidential amounts visible to the regulator,
   so enforcement can follow the money off-chain.
3. **Use manual approval as a second KYC gate.** Only approve accounts whose owners have passed
   enhanced due diligence. Anyone who could be sanctioned soon should never get confidential
   access.
4. **Off-chain legal leverage.** A frozen confidential balance can still be recovered by
   compelling the owner to withdraw it to the issuer. On-chain, nothing else will.
5. **Know the limit.** If regulators need guaranteed seizure of *every* token, Token-2022
   confidential transfers do not provide it today. The honest statement for this mint is:
   *"public balances are seizable; confidential balances are freezable and auditable."*

---

## Tests

23 integration tests in [`tests/integration/`](programs/solana-remittance-stablecoin/tests/integration).
Each builds a fresh LiteSVM, loads the compiled program, and sends real transactions.

| Module | Test | Shows |
|---|---|---|
| `mint` | `initialize_mint_stacks_fee_metadata_frozen_default_and_close_authority` | Exact extension set, values, self-pointer, on-chain metadata, exact size and rent (task 1) |
| | `close_authority_can_decommission_an_empty_mint` | MintCloseAuthority closes the mint |
| | `mint_cannot_be_closed_while_tokens_are_in_circulation` | `MintHasSupply` |
| | `only_the_close_authority_can_close_the_mint` | `OwnerMismatch` |
| `kyc` | `new_token_accounts_start_frozen` | Default frozen; minting to it fails (task 4) |
| | `kyc_thaw_unfreezes_only_that_account_and_keeps_the_frozen_default` | Per-account thaw; mint default unchanged; next account still frozen |
| | `only_the_freeze_authority_can_thaw` | Self-thaw rejected |
| | `thaw_is_bound_to_the_wallet_that_passed_kyc` | Can't thaw a different wallet's account |
| `transfer` | `transfer_pays_the_protocol_fee` | `TransferCheckedWithFee` used; receiver gets amount − fee; fee withheld (task 2) |
| | `fee_is_capped_at_the_maximum` | Max fee cap |
| | `fee_follows_the_current_epoch_not_a_cached_rate` | New rate after 2 epochs; a stale fee is rejected (tasks 2, 3) |
| | `cannot_send_to_an_account_that_has_not_passed_kyc` | `AccountFrozen` |
| | `only_the_owner_can_send` | `ConstraintTokenOwner` |
| `confidential` | `reissued_mint_carries_forward_extensions_and_adds_seizure_and_confidentiality` | Full mint #2 layout, manual approval, auditor, exact size (task 5) |
| | `fee_plus_confidentiality_is_rejected_without_confidential_fee_config` | The extension gap |
| | `configure_account_is_owner_only_though_ata_creation_is_permissionless` | Stranger creates the ATA but can't configure it (task 6) |
| | `manual_approval_gates_confidential_deposits` | Unapproved deposit fails; only the authority approves |
| | `full_confidential_lifecycle` | Configure → approve → deposit → apply → transfer → apply → withdraw, with balances decrypted at each step, fee withheld (encrypted), public balances unchanged by the transfer, supply conserved (task 6) |
| | `auditor_can_read_the_hidden_transfer_amount` | Auditor decrypts; other keys can't |
| | `withdraw_needs_the_pending_balance_applied_first` | `ConfidentialTransferBalanceMismatch` until applied |
| `seizure` | `permanent_delegate_seizes_public_balance_from_a_frozen_account` | Atomic thaw → seize → re-freeze |
| | `mint_without_a_permanent_delegate_cannot_seize` | `NotPermanentDelegate` on mint #1 |
| | `confidential_balance_is_beyond_the_permanent_delegate` | The written finding, as a test |

Client-side confidential logic (key generation, lo/hi balance decryption, the five
transfer-with-fee proofs, withdraw proofs, and proof context accounts) lives in
[`zk.rs`](programs/solana-remittance-stablecoin/tests/integration/zk.rs). It uses
`spl-token-confidential-transfer-proof-generation`.

---

## Testing note: Token-2022 with `zk-ops`

LiteSVM ships the **mainnet** Token-2022 v10.0.0 binary. That build is compiled *without* the
`zk-ops` feature, so `Deposit`, `ApplyPendingBalance`, `Withdraw` and `Transfer`/`TransferWithFee`
all return `InvalidInstructionData`. `ConfigureAccount` and `ApproveAccount` still work, because
they need no ciphertext math.

To run the confidential lifecycle, the tests load Token-2022 **v10.0.0 built from the published
crate source with its default features** (which include `zk-ops`):
[`tests/fixtures/spl_token_2022_zk_ops.so`](programs/solana-remittance-stablecoin/tests/fixtures).
It is the same program and version, with only that feature flag different. Rebuild it with:

```bash
./scripts/build-token-2022-zk-ops.sh
```

The script downloads `spl-token-2022-10.0.0` from crates.io, keeps only the on-chain program,
reuses the pinned [`scripts/token-2022-zk-ops.Cargo.lock`](scripts/token-2022-zk-ops.Cargo.lock),
and builds with platform-tools v1.52.

---

## Project layout

```
programs/solana-remittance-stablecoin/
├── src/
│   ├── lib.rs                  # instruction entry points
│   ├── constants.rs            # extension sets for mint #1, mint #2, confidential accounts
│   ├── state.rs                # MintParams, ConfidentialParams
│   ├── error.rs
│   ├── utils.rs                # MintSetup, expected_transfer_fee, permanent_delegate
│   └── instructions/
│       ├── initialize.rs       # mint #1
│       ├── reissue_mint.rs     # mint #2
│       ├── close_mint.rs
│       ├── thaw_account.rs     # KYC
│       ├── freeze_account.rs   # sanction
│       ├── seize.rs            # permanent delegate
│       ├── transfer.rs         # transfer_checked_with_fee
│       ├── configure_account.rs
│       ├── approve_account.rs
│       ├── deposit.rs
│       ├── apply_pending_balance.rs
│       ├── confidential_transfer.rs
│       └── withdraw.rs
└── tests/
    ├── fixtures/spl_token_2022_zk_ops.so
    └── integration/
        ├── main.rs
        ├── harness.rs          # LiteSVM env, instruction builders, state readers
        ├── zk.rs               # confidential client: keys, proofs, context accounts
        ├── mint.rs  kyc.rs  transfer.rs  confidential.rs  seizure.rs
scripts/build-token-2022-zk-ops.sh
docs/programOverview.md         # original design notes
```

Out of scope: the optional extension challenge (delegated-authority agent program + CPI Guard).
