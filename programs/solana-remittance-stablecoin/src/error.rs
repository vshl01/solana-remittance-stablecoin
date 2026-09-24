use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
    #[msg("Failed to calculate transfer fee")]
    FeeCalculationFailed,
    #[msg("Signer is not the mint's permanent delegate")]
    NotPermanentDelegate,
    #[msg("Mint still has tokens in circulation")]
    MintHasSupply,
}
