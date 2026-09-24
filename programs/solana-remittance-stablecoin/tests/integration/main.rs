//! Integration tests on LiteSVM against the compiled program.
//!
//! Build the program first: `anchor build` (or run everything with `anchor test`).

// LiteSVM's FailedTransactionMetadata is large; tests return it as-is.
#![allow(clippy::result_large_err)]

mod harness;
mod zk;

mod confidential;
mod kyc;
mod mint;
mod seizure;
mod transfer;
