```
Anchor Program
│
├── 1. initialize_mint()✅
│ └── Create & configure Token-2022 mint
│
├── 2. transfer()✅
│ └── Transfer tokens with protocol fee
│
├── 3. thaw_account()✅
│ └── Unfreeze user's ATA after mock KYC
│
├── 4. configure_confidential()
│ └── ConfigureAccount for confidential transfers
│
├── 5. deposit_confidential()
│ └── DepositConfidentialTokens
│
├── 6. apply_pending()
│ └── ApplyPendingBalance
│
├── 7. confidential_transfer()
│ └── Confidential Transfer
│
└── 8. withdraw_confidential()
│ └── WithdrawConfidentialTokens
```

**Flow:** `Mint` → `Transfer/KYC` → `Configure` → `Deposit` → `Apply` → `Confidential Transfer` → `Apply` → `Withdraw`

```
Token-2022 Mint Account
│
├── Base Mint data
│    ├── mint_authority
│    ├── freeze_authority
│    ├── decimals
│    └── supply
│
├── TransferFeeConfig extension
│    ├── fee rate
│    ├── max fee
│    └── fee authorities
│
├── MetadataPointer extension
├── DefaultAccountState extension
├── MintCloseAuthority extension
├── PermanentDelegate extension
└── ConfidentialTransferMint extension

```

## Proper Implementation Steps

```
1. Create Token-2022 Mint
   ↓
   Add:
   • TransferFeeConfig
   • MetadataPointer
   • DefaultAccountState = Frozen
   • MintCloseAuthority
   • PermanentDelegate
   • ConfidentialTransfer (manual approval)
   ↓
   Initialize Mint
```

```
2. Create User Token Account / ATA
   ↓
   Account starts FROZEN 🔒
```

```
3. KYC + Thaw
   ↓
   Mock KYC approval
   ↓
   Freeze authority thaws user's ATA
   ↓
   ATA becomes ACTIVE
```

```
4. Normal Transfer
   ↓
   transfer_checked_with_fee()
   ↓
   Protocol fee is applied
```

```
5. Configure Confidential Account
   ↓
   User authorizes ConfigureAccount
   ↓
   ATA becomes ready for confidential transfers
```

```
6. Deposit into Confidential Balance
   ↓
   DepositConfidentialTokens
   ↓
   Tokens → Pending Balance
   ↓
   ApplyPendingBalance
   ↓
   Confidential Balance
```

```
7. Confidential Transfer
   ↓
   Sender's confidential balance
   ↓
   Private transfer
   ↓
   Receiver's Pending Balance
   ↓
   Receiver calls ApplyPendingBalance
   ↓
   Receiver's Confidential Balance
```

```
8. Withdraw
   ↓
   ApplyPendingBalance if needed
   ↓
   WithdrawConfidentialTokens
   ↓
   Confidential Balance → Normal Token Balance
```

## Final mental model

```
MINT
 ↓
User ATA
 ↓
Frozen → KYC → Thaw
 ↓
Normal Transfer + Fee
 ↓
Configure Confidential
 ↓
Deposit → Apply
 ↓
Confidential Transfer → Apply
 ↓
Withdraw
```
