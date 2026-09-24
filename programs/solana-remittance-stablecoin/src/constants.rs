use anchor_spl::token_2022::spl_token_2022::extension::ExtensionType;

/// Fixed-length extensions every remittance mint carries:
/// protocol fee, self-pointing metadata, KYC-frozen default and a close authority.
pub const BASE_MINT_EXTENSIONS: [ExtensionType; 4] = [
    ExtensionType::TransferFeeConfig,
    ExtensionType::MetadataPointer,
    ExtensionType::DefaultAccountState,
    ExtensionType::MintCloseAuthority,
];

/// Extensions the re-issued mint adds on top of [`BASE_MINT_EXTENSIONS`].
///
/// `ConfidentialTransferFeeConfig` is not optional: Token-2022 rejects a mint that
/// has both `TransferFeeConfig` and `ConfidentialTransferMint` without it
/// (`InvalidExtensionCombination`), because confidential transfers need somewhere
/// to put the encrypted fee.
pub const CONFIDENTIAL_MINT_EXTENSIONS: [ExtensionType; 3] = [
    ExtensionType::PermanentDelegate,
    ExtensionType::ConfidentialTransferMint,
    ExtensionType::ConfidentialTransferFeeConfig,
];

/// Account extensions a holder must make room for before `ConfigureAccount`.
/// ATAs are only created with the extensions the mint *requires*, and
/// confidential state is opt-in, so the owner reallocates for it.
pub const CONFIDENTIAL_ACCOUNT_EXTENSIONS: [ExtensionType; 2] = [
    ExtensionType::ConfidentialTransferAccount,
    ExtensionType::ConfidentialTransferFeeAmount,
];
