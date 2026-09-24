use anchor_lang::prelude::*;

/// Settings shared by both mint generations.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct MintParams {
    pub decimals: u8,
    /// Protocol fee in basis points (1 bp = 0.01%).
    pub transfer_fee_basis_points: u16,
    /// Fee cap per transfer, in base units.
    pub maximum_fee: u64,
    /// On-chain TokenMetadata written into the mint itself.
    pub name: String,
    pub symbol: String,
    pub uri: String,
}

/// Extra settings for the confidential re-issue.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct ConfidentialParams {
    /// Optional auditor ElGamal key. Every confidential transfer amount is also
    /// encrypted to it, so a regulator can read amounts the public cannot.
    pub auditor_elgamal_pubkey: Option<[u8; 32]>,
    /// ElGamal key that confidential transfer fees are encrypted to.
    pub withdraw_withheld_authority_elgamal_pubkey: [u8; 32],
}
