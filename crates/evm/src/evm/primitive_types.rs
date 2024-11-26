use std::ops::Range;

use alloy_consensus::serde_bincode_compat as alloy_serde_bincode_compat;
use alloy_primitives::{Address, Sealable, B256};
use reth_primitives::transaction::serde_bincode_compat as reth_tx_serde_bincode_compat;
use reth_primitives::{Header, SealedHeader, TransactionSigned};
use reth_primitives_traits::serde_bincode_compat as reth_serde_bincode_compat;
use serde_with::serde_as;

/// Rlp encoded evm transaction.
#[derive(
    borsh::BorshDeserialize,
    borsh::BorshSerialize,
    Debug,
    PartialEq,
    Clone,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct RlpEvmTransaction {
    /// Rlp data.
    pub rlp: Vec<u8>,
}

#[serde_as]
#[derive(Debug, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct TransactionSignedAndRecovered {
    /// Signer of the transaction
    pub(crate) signer: Address,
    /// Signed transaction
    #[serde_as(as = "reth_tx_serde_bincode_compat::TransactionSigned")]
    pub(crate) signed_transaction: TransactionSigned,
    /// Block the transaction was added to
    pub(crate) block_number: u64,
}

#[serde_as]
#[derive(Debug, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Block {
    /// Block header.
    #[serde_as(as = "alloy_serde_bincode_compat::Header")]
    pub(crate) header: Header,

    /// L1 fee rate.
    pub(crate) l1_fee_rate: u128,

    /// The hash of L1 block that the L2 block corresponds to.  
    pub(crate) l1_hash: B256,

    /// Transactions in this block.
    pub(crate) transactions: Range<u64>,
}

impl Block {
    pub(crate) fn seal(self) -> SealedBlock {
        let sealed = self.header.seal_slow();
        let (header, seal) = sealed.into_parts();
        SealedBlock {
            header: SealedHeader::new(header, seal),
            l1_fee_rate: self.l1_fee_rate,
            l1_hash: self.l1_hash,
            transactions: self.transactions,
        }
    }
}

#[serde_as]
#[derive(Debug, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SealedBlock {
    /// Block header.
    #[serde_as(as = "reth_serde_bincode_compat::SealedHeader")]
    pub(crate) header: SealedHeader,

    /// L1 fee rate.
    pub(crate) l1_fee_rate: u128,

    /// The hash of L1 block that the L2 block corresponds to.  
    pub(crate) l1_hash: B256,

    /// Transactions in this block.
    pub(crate) transactions: Range<u64>,
}

#[derive(Debug, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Receipt {
    pub(crate) receipt: reth_primitives::Receipt,
    pub(crate) gas_used: u128,
    pub(crate) log_index_start: u64,
    pub(crate) l1_diff_size: u64,
}
